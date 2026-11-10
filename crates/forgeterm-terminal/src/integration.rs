//! Shell integration: makes shells report their working directory (see [`crate::cwd`]).
//!
//! Nothing is written to the user's profile or rc files. The integration only lives in
//! the started session, through arguments and environment variables:
//!
//! * PowerShell: after the profile has loaded, `-NoExit -Command` wraps the existing
//!   `prompt` function so it also emits OSC 9;9 for file-system locations (the same
//!   technique VS Code's shell integration uses).
//! * cmd: `PROMPT` gets an OSC 9;9 prefix (`$e` = ESC, `$P` = current path).
//! * bash (Git Bash, Linux): `PROMPT_COMMAND` prints OSC 7. An rc file that assigns
//!   `PROMPT_COMMAND` itself replaces it; then directory tracking is simply absent.

use crate::shell::{ShellKind, ShellSpec};

/// Extra arguments and environment for a shell.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Integration {
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

/// Wraps the current prompt; works in Windows PowerShell 5.1 (no `` `e ``) and pwsh 7.
const POWERSHELL: &str = "$global:__ForgeTermPrompt = $function:prompt; \
function global:prompt { \
$p = & $global:__ForgeTermPrompt; \
$l = $executionContext.SessionState.Path.CurrentLocation; \
if ($l.Provider.Name -eq 'FileSystem') { \"$([char]27)]9;9;$($l.ProviderPath)$([char]27)\\\" + $p } else { $p } \
}";

const BASH: &str = r#"printf '\e]7;file://%s%s\e\\' "${HOSTNAME}" "${PWD}""#;

/// Git Bash: MSYS paths such as `/tmp` cannot be mapped to Windows paths by ForgeTerm,
/// so the builtin `pwd -W` prints the Windows path (no subprocess per prompt). Its
/// trailing newline ends up inside the OSC string, where it is ignored and trimmed.
const GIT_BASH: &str = r#"printf '\e]9;9;'; builtin pwd -W; printf '\e\\'"#;

/// Integration for `shell`. `inherited` looks up variables of the environment the
/// shell inherits, so existing values are extended rather than replaced.
pub fn integration(shell: &ShellSpec, inherited: impl Fn(&str) -> Option<String>) -> Integration {
    match shell.kind {
        ShellKind::PowerShell => Integration {
            args: vec!["-NoExit".into(), "-Command".into(), POWERSHELL.into()],
            env: Vec::new(),
        },
        ShellKind::Cmd => {
            let prompt = inherited("PROMPT").unwrap_or_else(|| "$P$G".into());
            Integration {
                args: Vec::new(),
                env: vec![("PROMPT".into(), format!("$e]9;9;$P$e\\{prompt}"))],
            }
        }
        ShellKind::GitBash | ShellKind::Posix if is_bash(&shell.program) => {
            let ours = if shell.kind == ShellKind::GitBash {
                GIT_BASH
            } else {
                BASH
            };
            let command = match inherited("PROMPT_COMMAND") {
                Some(existing) if !existing.trim().is_empty() => format!("{ours}; {existing}"),
                _ => ours.to_owned(),
            };
            Integration {
                args: Vec::new(),
                env: vec![("PROMPT_COMMAND".into(), command)],
            }
        }
        // zsh / fish: fish emits OSC 7 itself in many setups; zsh needs a hook in the
        // user's config. WSL does not pass Windows environment variables through.
        _ => Integration::default(),
    }
}

fn is_bash(program: &str) -> bool {
    let name = program
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    name == "bash" || name == "bash.exe"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn powershell_wraps_prompt_after_profile() {
        let i = integration(&ShellSpec::new("pwsh", r"C:\pwsh\pwsh.exe", vec![]), none);
        assert_eq!(i.args[..2], ["-NoExit", "-Command"]);
        assert!(i.args[2].contains("]9;9;"));
        assert!(i.args[2].contains("ProviderPath"));
        assert!(i.env.is_empty());
    }

    #[test]
    fn cmd_prefixes_existing_prompt() {
        let cmd = ShellSpec::new("cmd", r"C:\Windows\system32\cmd.exe", vec![]);
        assert_eq!(
            integration(&cmd, none).env,
            [("PROMPT".to_owned(), r"$e]9;9;$P$e\$P$G".to_owned())]
        );
        let custom = integration(&cmd, |k| (k == "PROMPT").then(|| "[$T] $P$G".into()));
        assert_eq!(custom.env[0].1, r"$e]9;9;$P$e\[$T] $P$G");
    }

    #[test]
    fn bash_keeps_existing_prompt_command() {
        let bash = ShellSpec::new("Git Bash", r"C:\Program Files\Git\bin\bash.exe", vec![]);
        let plain = integration(&bash, none);
        assert_eq!(plain.env[0].0, "PROMPT_COMMAND");
        assert!(plain.env[0].1.contains("pwd -W"));
        let linux = integration(&ShellSpec::new("bash", "/bin/bash", vec![]), none);
        assert!(linux.env[0].1.contains(r"\e]7;file://"));
        let chained = integration(&bash, |k| {
            (k == "PROMPT_COMMAND").then(|| "history -a".into())
        });
        assert!(chained.env[0].1.ends_with("; history -a"));
    }

    #[test]
    fn other_shells_untouched() {
        for program in [
            "/usr/bin/zsh",
            "/usr/bin/fish",
            r"C:\Windows\system32\wsl.exe",
        ] {
            assert_eq!(
                integration(&ShellSpec::new("x", program, vec![]), none),
                Integration::default(),
                "{program}"
            );
        }
    }
}
