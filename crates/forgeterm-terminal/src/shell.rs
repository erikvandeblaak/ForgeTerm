use std::env;
use std::path::{Path, PathBuf};

/// A shell program that can be launched in a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellSpec {
    pub name: String,
    pub program: String,
    pub args: Vec<String>,
}

impl ShellSpec {
    pub fn new(name: impl Into<String>, program: impl Into<String>, args: Vec<String>) -> Self {
        Self {
            name: name.into(),
            program: program.into(),
            args,
        }
    }
}

/// Finds an executable on `PATH` (honours `PATHEXT` semantics for `.exe` on Windows).
fn find_in_path(program: &str) -> Option<PathBuf> {
    let candidate = Path::new(program);
    if candidate.is_absolute() {
        return candidate.is_file().then(|| candidate.to_path_buf());
    }
    let exe = if cfg!(windows) && candidate.extension().is_none() {
        format!("{program}.exe")
    } else {
        program.to_owned()
    };
    env::split_paths(&env::var_os("PATH")?)
        .map(|dir| dir.join(&exe))
        .find(|p| p.is_file())
}

/// Lists shells that are actually installed on this machine, preferred first.
pub fn detect_shells() -> Vec<ShellSpec> {
    let mut found = Vec::new();

    #[cfg(windows)]
    {
        let candidates = [
            ("PowerShell 7", "pwsh"),
            ("Windows PowerShell", "powershell"),
            ("Command Prompt", "cmd"),
        ];
        for (name, prog) in candidates {
            if let Some(path) = find_in_path(prog) {
                let args = if prog == "cmd" {
                    Vec::new()
                } else {
                    vec!["-NoLogo".into()]
                };
                found.push(ShellSpec::new(name, path.to_string_lossy(), args));
            }
        }
        let git_bash = [
            r"C:\Program Files\Git\bin\bash.exe",
            r"C:\Program Files (x86)\Git\bin\bash.exe",
        ];
        if let Some(path) = git_bash.iter().find(|p| Path::new(p).is_file()) {
            found.push(ShellSpec::new(
                "Git Bash",
                *path,
                vec!["--login".into(), "-i".into()],
            ));
        }
        if let Some(path) = find_in_path("wsl") {
            found.push(ShellSpec::new("WSL", path.to_string_lossy(), Vec::new()));
        }
    }

    #[cfg(unix)]
    {
        if let Ok(sh) = env::var("SHELL")
            && Path::new(&sh).is_file()
        {
            let name = Path::new(&sh)
                .file_name()
                .map_or("shell".into(), |n| n.to_string_lossy().into_owned());
            found.push(ShellSpec::new(name, sh, vec!["-l".into()]));
        }
        for prog in ["bash", "zsh", "fish"] {
            if let Some(path) = find_in_path(prog) {
                let path = path.to_string_lossy().into_owned();
                if !found.iter().any(|s| s.program == path) {
                    found.push(ShellSpec::new(prog, path, vec!["-l".into()]));
                }
            }
        }
    }

    found
}

/// The shell ForgeTerm opens by default.
pub fn default_shell() -> Option<ShellSpec> {
    detect_shells().into_iter().next()
}
