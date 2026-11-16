//! SSH profiles: the `Host` entries of the user's OpenSSH client configuration
//! (`~/.ssh/config`), opened as a terminal running the OpenSSH client.
//!
//! ForgeTerm has no SSH or crypto code of its own and never sees a password: the
//! `ssh` program does everything (keys, agent, known hosts, prompts) inside the
//! terminal. The config file is only read, never written. Wildcard patterns
//! (`Host *`), negations and `Match` blocks are not profiles; `Include`d files are
//! not followed.

use std::path::{Path, PathBuf};

use crate::shell::ShellSpec;

/// One connectable `Host` alias with what the config says about it (for display;
/// `ssh` itself applies the full configuration).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshHost {
    pub alias: String,
    pub host_name: Option<String>,
    pub user: Option<String>,
    pub port: Option<String>,
}

impl SshHost {
    /// `user@hostname:port` as far as known, else the alias.
    pub fn target(&self) -> String {
        let host = self.host_name.as_deref().unwrap_or(&self.alias);
        let mut out = match &self.user {
            Some(u) => format!("{u}@{host}"),
            None => host.to_owned(),
        };
        if let Some(p) = &self.port {
            out.push(':');
            out.push_str(p);
        }
        out
    }
}

/// Largest config read (a normal file is a few KB).
const MAX_CONFIG_BYTES: u64 = 256 * 1024;

/// `~/.ssh/config` (Windows: `%USERPROFILE%\.ssh\config`, as OpenSSH for Windows).
pub fn config_path() -> Option<PathBuf> {
    let home = if cfg!(windows) {
        std::env::var_os("USERPROFILE")
    } else {
        std::env::var_os("HOME")
    }?;
    Some(PathBuf::from(home).join(".ssh").join("config"))
}

/// Reads the hosts from `path`; a missing or unreadable file means no hosts.
pub fn read_hosts(path: &Path) -> Vec<SshHost> {
    match std::fs::metadata(path) {
        Ok(m) if m.is_file() && m.len() <= MAX_CONFIG_BYTES => {}
        _ => return Vec::new(),
    }
    std::fs::read_to_string(path)
        .map(|text| parse_config(&text))
        .unwrap_or_default()
}

/// Parses OpenSSH client configuration text into connectable aliases, in file
/// order, each alias once.
pub fn parse_config(text: &str) -> Vec<SshHost> {
    let mut hosts: Vec<SshHost> = Vec::new();
    // Indices of the hosts the current `Host` line declared.
    let mut current: Vec<usize> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // `Keyword value`, `Keyword=value` or `Keyword = value`.
        let (key, value) = match line.find(|c: char| c.is_whitespace() || c == '=') {
            Some(i) => (
                &line[..i],
                line[i..]
                    .trim_start_matches(|c: char| c.is_whitespace() || c == '=')
                    .trim(),
            ),
            None => (line, ""),
        };
        match key.to_ascii_lowercase().as_str() {
            "host" => {
                current.clear();
                for alias in value.split_whitespace().map(|a| a.trim_matches('"')) {
                    if alias.is_empty() || alias.contains(['*', '?', '!']) || alias.starts_with('-')
                    {
                        continue;
                    }
                    let index = match hosts.iter().position(|h| h.alias == alias) {
                        Some(i) => i,
                        None => {
                            hosts.push(SshHost {
                                alias: alias.to_owned(),
                                host_name: None,
                                user: None,
                                port: None,
                            });
                            hosts.len() - 1
                        }
                    };
                    current.push(index);
                }
            }
            // A Match block applies conditionally: nothing in it is shown.
            "match" => current.clear(),
            "hostname" | "user" | "port" if !value.is_empty() => {
                let value = value.trim_matches('"').to_owned();
                for &i in &current {
                    let h = &mut hosts[i];
                    // As ssh: the first value obtained wins.
                    let slot = match key.to_ascii_lowercase().as_str() {
                        "hostname" => &mut h.host_name,
                        "user" => &mut h.user,
                        _ => &mut h.port,
                    };
                    if slot.is_none() {
                        *slot = Some(value.clone());
                    }
                }
            }
            _ => {}
        }
    }
    hosts
}

/// The terminal program for `alias`: the OpenSSH client found on `PATH`. `--` ends
/// the options, so an alias can never be read as one.
pub fn ssh_spec(ssh_program: &Path, host: &SshHost) -> ShellSpec {
    ShellSpec::new(
        format!("SSH: {}", host.alias),
        ssh_program.to_string_lossy(),
        vec!["--".into(), host.alias.clone()],
    )
}

/// The OpenSSH client, if installed (`ssh` on `PATH`; Windows ships it in
/// `System32\OpenSSH`, which is on `PATH`).
pub fn find_ssh() -> Option<PathBuf> {
    crate::shell::find_in_path("ssh")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ShellKind;

    #[test]
    fn hosts_are_read_with_their_details() {
        let hosts = parse_config(
            r#"
# comment
Host *
    User everyone
Host web prod-db
    HostName 10.0.0.5
    User deploy
    Port 2222
Host web
    User ignored-second-value
host=build
  hostname = build.example.com
Host !blocked *.internal jump? "quoted"
    User q
Match host web
    User from-match
Host -oProxyCommand=evil
Include ~/.ssh/other
"#,
        );
        let aliases: Vec<&str> = hosts.iter().map(|h| h.alias.as_str()).collect();
        assert_eq!(aliases, ["web", "prod-db", "build", "quoted"]);
        assert_eq!(hosts[0].host_name.as_deref(), Some("10.0.0.5"));
        assert_eq!(hosts[0].user.as_deref(), Some("deploy"), "first value wins");
        assert_eq!(hosts[0].target(), "deploy@10.0.0.5:2222");
        assert_eq!(hosts[1].target(), "deploy@10.0.0.5:2222");
        assert_eq!(hosts[2].target(), "build.example.com");
        assert_eq!(hosts[3].target(), "q@quoted");
        assert!(parse_config("").is_empty());
    }

    #[test]
    fn the_alias_is_never_an_option() {
        let host = SshHost {
            alias: "web".into(),
            host_name: None,
            user: None,
            port: None,
        };
        let spec = ssh_spec(Path::new("/usr/bin/ssh"), &host);
        assert_eq!(spec.args, ["--", "web"]);
        assert_eq!(spec.name, "SSH: web");
        assert_eq!(spec.kind, ShellKind::Remote);
        assert_eq!(
            ShellKind::from_program(r"C:\Program Files\Docker\Docker\resources\bin\docker.exe"),
            ShellKind::Remote
        );
    }

    #[test]
    fn missing_or_huge_config_means_no_hosts() {
        let dir = std::env::temp_dir().join(format!("forgeterm-ssh-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(read_hosts(&dir.join("config")).is_empty());
        assert!(read_hosts(&dir).is_empty(), "a folder is not a config");
        let big = dir.join("big");
        std::fs::write(&big, "Host a\n".repeat(50_000)).unwrap();
        assert!(read_hosts(&big).is_empty());
        let ok = dir.join("ok");
        std::fs::write(&ok, "Host a\n").unwrap();
        assert_eq!(read_hosts(&ok).len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
