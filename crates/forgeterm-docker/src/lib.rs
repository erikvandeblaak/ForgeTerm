//! Docker through the `docker` CLI: list containers, start and stop one, and the
//! commands for a shell or the logs of a container in a terminal.
//!
//! Optional: without the CLI nothing is offered, and without a running engine the
//! reason is shown. Nothing here runs by itself: the app calls it when the user
//! asks (refresh, start, stop, open). ForgeTerm never removes containers, images
//! or volumes.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

/// One container from `docker ps -a`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Container {
    /// Full container id (hex).
    pub id: String,
    pub name: String,
    pub image: String,
    /// `running`, `exited`, `paused`, `created`, `restarting`, `dead`.
    pub state: String,
    /// Docker's own summary, e.g. "Up 3 hours" or "Exited (0) 2 days ago".
    pub status: String,
    pub ports: String,
}

impl Container {
    pub fn is_running(&self) -> bool {
        self.state == "running"
    }

    /// A stable number for this container (commands carry it, not a list index,
    /// so a list refreshed meanwhile can never redirect an action).
    pub fn key(&self) -> u64 {
        // FNV-1a: stable across runs, no dependency.
        self.id.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
        })
    }
}

/// Fields separated by tabs: names, images and statuses never contain one.
const PS_FORMAT: &str = "{{.ID}}\t{{.Names}}\t{{.Image}}\t{{.State}}\t{{.Status}}\t{{.Ports}}";

/// The `docker` CLI on `PATH` (absolute entries only: never one planted in the
/// current folder).
pub fn find_docker() -> Option<PathBuf> {
    let exe = if cfg!(windows) {
        "docker.exe"
    } else {
        "docker"
    };
    std::env::split_paths(&std::env::var_os("PATH")?)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join(exe))
        .find(|p| p.is_file())
}

/// All containers, running first, then by name.
pub fn list(docker: &Path) -> Result<Vec<Container>, String> {
    let out = run(
        docker,
        &["ps", "-a", "--no-trunc", "--format", PS_FORMAT],
        Duration::from_secs(10),
    )?;
    if out.code != Some(0) {
        return Err(explain(&out.stderr));
    }
    let mut containers = parse_ps(&String::from_utf8_lossy(&out.stdout));
    containers.sort_by(|a, b| {
        b.is_running()
            .cmp(&a.is_running())
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(containers)
}

/// `docker start <id>`.
pub fn start(docker: &Path, id: &str) -> Result<(), String> {
    simple(docker, "start", id)
}

/// `docker stop <id>` (Docker's default: SIGTERM, then SIGKILL after 10 s).
pub fn stop(docker: &Path, id: &str) -> Result<(), String> {
    simple(docker, "stop", id)
}

fn simple(docker: &Path, verb: &str, id: &str) -> Result<(), String> {
    check_id(id)?;
    let out = run(docker, &[verb, id], Duration::from_secs(40))?;
    if out.code == Some(0) {
        Ok(())
    } else {
        Err(explain(&out.stderr))
    }
}

/// Arguments for an interactive shell in the container: bash when it has one,
/// else sh.
pub fn shell_args(id: &str) -> Result<Vec<String>, String> {
    check_id(id)?;
    Ok([
        "exec",
        "-it",
        id,
        "sh",
        "-c",
        "if command -v bash >/dev/null 2>&1; then exec bash; else exec sh; fi",
    ]
    .map(String::from)
    .to_vec())
}

/// Arguments that follow the container's log (the last 500 lines, then live).
pub fn logs_args(id: &str) -> Result<Vec<String>, String> {
    check_id(id)?;
    Ok(["logs", "--follow", "--tail", "500", id]
        .map(String::from)
        .to_vec())
}

/// Ids come from `docker ps`; anything else is refused so it can never be read
/// as an option or a second argument.
fn check_id(id: &str) -> Result<(), String> {
    if !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(format!("not a container id: {id:?}"))
    }
}

/// Parses `docker ps` output in [`PS_FORMAT`]; malformed lines are skipped.
pub fn parse_ps(text: &str) -> Vec<Container> {
    text.lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.trim_end_matches('\r').split('\t').collect();
            if f.len() < 5 || check_id(f[0]).is_err() {
                return None;
            }
            Some(Container {
                id: f[0].to_owned(),
                name: f[1].to_owned(),
                image: f[2].to_owned(),
                state: f[3].to_owned(),
                status: f[4].to_owned(),
                ports: f.get(5).copied().unwrap_or_default().to_owned(),
            })
        })
        .collect()
}

/// A short reason for the user; the engine not running is the common case.
pub fn explain(stderr: &str) -> String {
    let lower = stderr.to_ascii_lowercase();
    if lower.contains("cannot connect to the docker daemon")
        || lower.contains("failed to connect to the docker api")
        || lower.contains("is the docker daemon running")
        || lower.contains("dockerdesktoplinuxengine")
    {
        return "Docker is installed, but its engine is not running (start Docker Desktop or the docker service)".to_owned();
    }
    let first: Vec<&str> = stderr
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .take(3)
        .collect();
    if first.is_empty() {
        "docker failed".to_owned()
    } else {
        first.join(" ")
    }
}

struct Output {
    code: Option<i32>,
    stdout: Vec<u8>,
    stderr: String,
}

fn run(docker: &Path, args: &[&str], timeout: Duration) -> Result<Output, String> {
    let mut cmd = Command::new(docker);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    hide_console(&mut cmd);
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("could not run docker: {e}"))?;
    let drain = |stream: Option<Box<dyn Read + Send>>| {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut s) = stream {
                let _ = s.read_to_end(&mut buf);
            }
            let _ = tx.send(buf);
        });
        rx
    };
    let out_rx = drain(
        child
            .stdout
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
    );
    let err_rx = drain(
        child
            .stderr
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
    );
    let stdout = match out_rx.recv_timeout(timeout) {
        Ok(buf) => buf,
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "docker {} gave no answer within {} s",
                args.first().copied().unwrap_or_default(),
                timeout.as_secs()
            ));
        }
    };
    let stderr = err_rx
        .recv_timeout(Duration::from_secs(5))
        .unwrap_or_default();
    let status = child.wait().map_err(|e| e.to_string())?;
    Ok(Output {
        code: status.code(),
        stdout,
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
    })
}

#[cfg(windows)]
fn hide_console(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_console(_: &mut Command) {}

#[cfg(test)]
mod tests {
    use super::*;

    const ID_A: &str = "a5915f0be3fa1111111111111111111111111111111111111111111111111111";
    const ID_B: &str = "6e0bee193ab22222222222222222222222222222222222222222222222222222";

    #[test]
    fn ps_output_is_parsed_and_bad_lines_skipped() {
        let text = format!(
            "{ID_A}\tweb-1\tsite-web\trunning\tUp 3 hours\t0.0.0.0:8080->80/tcp\r\n\
             {ID_B}\told\taf5af113fc5e\texited\tExited (255) 7 weeks ago\t\n\
             not-an-id\tx\ty\trunning\tUp\t\n\
             short line\n"
        );
        let c = parse_ps(&text);
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].name, "web-1");
        assert!(c[0].is_running());
        assert_eq!(c[0].ports, "0.0.0.0:8080->80/tcp");
        assert_eq!(c[1].status, "Exited (255) 7 weeks ago");
        assert!(!c[1].is_running());
        assert_ne!(c[0].key(), c[1].key());
        assert_eq!(c[0].key(), parse_ps(&text)[0].key(), "stable");
    }

    #[test]
    fn ids_are_checked_before_use() {
        assert!(shell_args(ID_A).is_ok());
        for bad in ["", "-it", "abc;rm", "a b", &"a".repeat(65)] {
            assert!(shell_args(bad).is_err(), "{bad:?}");
            assert!(logs_args(bad).is_err(), "{bad:?}");
        }
        let args = logs_args(ID_A).unwrap();
        assert_eq!(args.last().map(String::as_str), Some(ID_A));
    }

    #[test]
    fn engine_not_running_is_explained() {
        let e = explain(
            "failed to connect to the docker API at npipe:////./pipe/dockerDesktopLinuxEngine; check if the path is correct",
        );
        assert!(e.contains("engine is not running"), "{e}");
        assert_eq!(
            explain("Error: No such container: x\n"),
            "Error: No such container: x"
        );
        assert_eq!(explain(""), "docker failed");
    }

    /// Stop and start against a real, throwaway container named in
    /// `FORGETERM_TEST_CONTAINER` (skipped otherwise: never someone's own container).
    #[test]
    fn stop_and_start_a_test_container() {
        let (Some(docker), Ok(name)) = (find_docker(), std::env::var("FORGETERM_TEST_CONTAINER"))
        else {
            println!("no FORGETERM_TEST_CONTAINER: skipped");
            return;
        };
        let find = || {
            list(&docker)
                .unwrap()
                .into_iter()
                .find(|c| c.name == name)
                .expect("test container exists")
        };
        let c = find();
        assert!(c.is_running(), "start it first: {c:?}");
        stop(&docker, &c.id).unwrap();
        assert!(!find().is_running());
        start(&docker, &c.id).unwrap();
        assert!(find().is_running());
        assert!(stop(&docker, "0123456789ab").is_err(), "unknown id");
    }

    /// Whatever this machine has: a list or a reason, never a panic.
    #[test]
    fn list_works_or_says_why() {
        let Some(docker) = find_docker() else {
            println!("docker not installed: skipped");
            return;
        };
        match list(&docker) {
            Ok(c) => println!("{} containers", c.len()),
            Err(e) => {
                assert!(!e.is_empty());
                println!("not available: {e}");
            }
        }
    }
}
