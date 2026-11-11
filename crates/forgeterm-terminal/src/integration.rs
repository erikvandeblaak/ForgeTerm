//! Shell integration: makes shells report their working directory (see [`crate::cwd`])
//! and mark their prompts and commands (see [`crate::marks`]).
//!
//! Nothing is written to the user's profile or rc files. The integration only lives in
//! the started session, through arguments and environment variables:
//!
//! * PowerShell: after the profile has loaded, `-NoExit -Command` wraps the existing
//!   `prompt` function. It emits OSC 9;9 for file-system locations, `133;D;<code>`
//!   for the previous command, `133;A` + a numbered `forgeterm-prompt:` hyperlink
//!   around the prompt text, then `133;B`. On the first prompt it also wraps
//!   PSReadLine's `PSConsoleHostReadLine` (the same technique VS Code's shell
//!   integration uses) to emit the command line (`633;E`) and `133;C` when a command
//!   is accepted. Without PSReadLine the next prompt sees a new `Get-History` entry
//!   and sends the command line, `C` and `D` afterwards (exit code and output still
//!   work; only the moment the output started is not marked).
//! * cmd: `PROMPT` gets OSC 9;9, `133;A`, an unnumbered prompt link and `133;B`
//!   (`$e` = ESC, `$P` = current path). cmd cannot report commands or exit codes.
//! * bash (Git Bash, Linux): `PROMPT_COMMAND` prints `133;D;$?` (only after a command
//!   ran), the directory (OSC 7, Git Bash OSC 9;9), `133;A` + a numbered prompt link,
//!   and once appends the link end + `133;B` to `PS1` and prefixes `PS0` with
//!   `133;C` (bash ≥ 4.4 prints `PS0` before running a command). `$?` is restored
//!   for a `PROMPT_COMMAND` that was already set. An rc file that assigns
//!   `PROMPT_COMMAND` itself replaces all of this; then integration is simply absent.

use crate::shell::{ShellKind, ShellSpec};

/// Extra arguments and environment for a shell.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Integration {
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

/// Wraps the current prompt; works in Windows PowerShell 5.1 (no `` `e ``) and pwsh 7.
/// One statement per line; the lines are joined with spaces.
const POWERSHELL: &str = r#"$global:__ForgeTermPrompt = $function:prompt;
$global:__ForgeTermN = 0;
$global:__ForgeTermRan = $false;
$global:__ForgeTermHist = [int](Get-History -Count 1 | % Id);
function global:__ForgeTermEscape([string]$s) {
[regex]::Replace($s, '[\\\x00-\x1f\x7f;]', { param($m) if ($m.Value -eq '\') { '\\' } else { '\x{0:x2}' -f [int][char]$m.Value } })
};
function global:prompt {
$ok = $global:?;
$code = $global:LASTEXITCODE;
$e = [char]27;
$out = '';
$h = Get-History -Count 1;
$hid = [int]($h | % Id);
$new = $hid -ne $global:__ForgeTermHist;
$global:__ForgeTermHist = $hid;
if ($global:__ForgeTermRan -or $new) {
if (-not $global:__ForgeTermRan) { $out += "$e]633;E;" + (__ForgeTermEscape $h.CommandLine) + "$e\$e]133;C$e\" };
$global:__ForgeTermRan = $false;
$c = if ($ok) { 0 } elseif ($code) { $code } else { 1 };
$out += "$e]133;D;$c$e\"
};
if (-not $global:__ForgeTermReadLine -and (Test-Path Function:\PSConsoleHostReadLine)) {
$global:__ForgeTermReadLine = $function:PSConsoleHostReadLine;
function global:PSConsoleHostReadLine {
$line = & $global:__ForgeTermReadLine;
if ($line -and $line.Trim()) {
$global:__ForgeTermRan = $true;
$x = [char]27;
[Console]::Write("$x]633;E;" + (__ForgeTermEscape $line) + "$x\$x]133;C$x\")
};
$line
}
};
$l = $executionContext.SessionState.Path.CurrentLocation;
if ($l.Provider.Name -eq 'FileSystem') { $out += "$e]9;9;$($l.ProviderPath)$e\" };
$global:__ForgeTermN++;
$global:LASTEXITCODE = $code;
$p = & $global:__ForgeTermPrompt;
$out + "$e]133;A$e\$e]8;;forgeterm-prompt:$($global:__ForgeTermN)$e\" + $p + "$e]8;;$e\$e]133;B$e\"
}"#;

/// bash: exit code of the previous command, then the directory report (`{cwd}`),
/// then the prompt start. `PS1`/`PS0` are extended once per shell.
const BASH: &str = r#"__ft_s=$?; if [ -n "$__ft_ran" ]; then printf '\e]133;D;%s\e\\' "$__ft_s"; fi; __ft_ran=; {cwd}; __ft_n=$((__ft_n+1)); printf '\e]133;A\e\\\e]8;;forgeterm-prompt:%s\e\\' "$__ft_n"; if [ -z "$__ft_ps" ]; then __ft_ps=1; PS1="$PS1"'\[\e]8;;\e\\\e]133;B\e\\\]'; PS0='\e]8;;\e\\\e]133;C;ft=$((__ft_ran=1))\e\\'"$PS0"; fi; __ft_ret() { return "$1"; }; __ft_ret "$__ft_s""#;

const BASH_CWD: &str = r#"printf '\e]7;file://%s%s\e\\' "${HOSTNAME}" "${PWD}""#;

/// Git Bash: MSYS paths such as `/tmp` cannot be mapped to Windows paths by ForgeTerm,
/// so the builtin `pwd -W` prints the Windows path (no subprocess per prompt). Its
/// trailing newline ends up inside the OSC string, where it is ignored and trimmed.
const GIT_BASH_CWD: &str = r#"printf '\e]9;9;'; builtin pwd -W; printf '\e\\'"#;

/// Integration for `shell`. `inherited` looks up variables of the environment the
/// shell inherits, so existing values are extended rather than replaced.
pub fn integration(shell: &ShellSpec, inherited: impl Fn(&str) -> Option<String>) -> Integration {
    match shell.kind {
        ShellKind::PowerShell => Integration {
            args: vec!["-NoExit".into(), "-Command".into(), powershell_command()],
            env: Vec::new(),
        },
        ShellKind::Cmd => {
            let prompt = inherited("PROMPT").unwrap_or_else(|| "$P$G".into());
            Integration {
                args: Vec::new(),
                env: vec![(
                    "PROMPT".into(),
                    format!(
                        "$e]9;9;$P$e\\$e]133;A$e\\$e]8;;forgeterm-prompt:$e\\{prompt}$e]8;;$e\\$e]133;B$e\\"
                    ),
                )],
            }
        }
        ShellKind::GitBash | ShellKind::Posix if is_bash(&shell.program) => {
            let cwd = if shell.kind == ShellKind::GitBash {
                GIT_BASH_CWD
            } else {
                BASH_CWD
            };
            let ours = BASH.replace("{cwd}", cwd);
            let command = match inherited("PROMPT_COMMAND") {
                Some(existing) if !existing.trim().is_empty() => format!("{ours}; {existing}"),
                _ => ours,
            };
            Integration {
                args: Vec::new(),
                env: vec![("PROMPT_COMMAND".into(), command)],
            }
        }
        // zsh / fish: fish emits OSC 7 (and in recent versions OSC 133) itself in many
        // setups; zsh needs a hook in the user's config. WSL does not pass Windows
        // environment variables through.
        _ => Integration::default(),
    }
}

fn powershell_command() -> String {
    POWERSHELL
        .lines()
        .map(str::trim)
        .collect::<Vec<_>>()
        .join(" ")
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
        let script = &i.args[2];
        assert!(!script.contains('\n'));
        for part in [
            "]9;9;",
            "ProviderPath",
            "]133;A",
            "]133;B",
            "]133;C",
            "]133;D;$c",
            "]633;E;",
            "forgeterm-prompt:$($global:__ForgeTermN)",
            "PSConsoleHostReadLine",
        ] {
            assert!(script.contains(part), "{part}");
        }
        assert!(i.env.is_empty());
    }

    #[test]
    fn cmd_wraps_existing_prompt() {
        let cmd = ShellSpec::new("cmd", r"C:\Windows\system32\cmd.exe", vec![]);
        assert_eq!(
            integration(&cmd, none).env,
            [(
                "PROMPT".to_owned(),
                r"$e]9;9;$P$e\$e]133;A$e\$e]8;;forgeterm-prompt:$e\$P$G$e]8;;$e\$e]133;B$e\"
                    .to_owned()
            )]
        );
        let custom = integration(&cmd, |k| (k == "PROMPT").then(|| "[$T] $P$G".into()));
        assert!(
            custom.env[0]
                .1
                .contains(r"forgeterm-prompt:$e\[$T] $P$G$e]8;;")
        );
    }

    #[test]
    fn bash_keeps_existing_prompt_command() {
        let bash = ShellSpec::new("Git Bash", r"C:\Program Files\Git\bin\bash.exe", vec![]);
        let plain = integration(&bash, none);
        assert_eq!(plain.env[0].0, "PROMPT_COMMAND");
        let pc = &plain.env[0].1;
        assert!(pc.contains("pwd -W"));
        assert!(pc.starts_with("__ft_s=$?;"), "exit code read first");
        assert!(pc.contains(r"]133;D;%s"));
        assert!(pc.contains("forgeterm-prompt:%s"));
        assert!(pc.contains("PS0="));
        assert!(pc.ends_with(r#"__ft_ret "$__ft_s""#), "restores $?");
        let linux = integration(&ShellSpec::new("bash", "/bin/bash", vec![]), none);
        assert!(linux.env[0].1.contains(r"\e]7;file://"));
        let chained = integration(&bash, |k| {
            (k == "PROMPT_COMMAND").then(|| "history -a".into())
        });
        assert!(
            chained.env[0]
                .1
                .ends_with(r#"__ft_ret "$__ft_s"; history -a"#)
        );
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
