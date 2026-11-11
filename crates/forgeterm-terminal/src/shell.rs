use std::env;
use std::path::{Path, PathBuf};

/// Shell family; decides how text such as file paths must be quoted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellKind {
    /// PowerShell 7 and Windows PowerShell.
    PowerShell,
    /// Windows Command Prompt.
    Cmd,
    /// bash, zsh, sh and other POSIX shells on a POSIX system.
    Posix,
    Fish,
    /// Git for Windows' bash: POSIX quoting, `/c/...` paths.
    GitBash,
    /// WSL: POSIX quoting, `/mnt/c/...` paths.
    Wsl,
}

impl ShellKind {
    /// Infers the shell family from the program path.
    pub fn from_program(program: &str) -> Self {
        let normalized = program.replace('\\', "/").to_ascii_lowercase();
        let stem = normalized
            .rsplit('/')
            .next()
            .unwrap_or_default()
            .trim_end_matches(".exe");
        match stem {
            "pwsh" | "powershell" => Self::PowerShell,
            "cmd" => Self::Cmd,
            "fish" => Self::Fish,
            "wsl" => Self::Wsl,
            // System32\bash.exe is the WSL launcher, not Git Bash.
            "bash" if normalized.contains("/windows/system32/") => Self::Wsl,
            "bash" | "sh" if normalized.contains("/git/") => Self::GitBash,
            _ => Self::Posix,
        }
    }
}

/// A shell program that can be launched in a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellSpec {
    pub name: String,
    pub program: String,
    pub args: Vec<String>,
    pub kind: ShellKind,
}

impl ShellSpec {
    pub fn new(name: impl Into<String>, program: impl Into<String>, args: Vec<String>) -> Self {
        let program = program.into();
        Self {
            name: name.into(),
            kind: ShellKind::from_program(&program),
            program,
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
    // Empty or relative entries ("C:\\a;;C:\\b") would mean the current folder,
    // where a cloned repository could plant its own "pwsh.exe".
    env::split_paths(&env::var_os("PATH")?)
        .filter(|dir| dir.is_absolute())
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

/// `PSModulePath` for Windows PowerShell 5.1 (`powershell.exe`) without the module
/// folders of PowerShell 7, or `None` when nothing needs to change.
///
/// pwsh 7 adds its own folders (`…\PowerShell\Modules`, `…\PowerShell\7\Modules`) to
/// `PSModulePath` in its process. When ForgeTerm is started from pwsh 7 (for example
/// the VS Code terminal), Windows PowerShell inherits them, loads pwsh's PSReadLine
/// and fails ("Cannot load PSReadline module"): no history, no editing, no prompt
/// marks. Only this session's environment changes; the user's setting is untouched.
pub fn windows_powershell_module_path(program: &str, inherited: Option<&str>) -> Option<String> {
    let name = program.rsplit(['/', '\\']).next()?.to_ascii_lowercase();
    if name != "powershell.exe" && name != "powershell" {
        return None;
    }
    let inherited = inherited?;
    let kept: Vec<&str> = inherited
        .split(';')
        .filter(|entry| !is_pwsh7_module_dir(entry))
        .collect();
    (kept.len() != inherited.split(';').count()).then(|| kept.join(";"))
}

/// `…\PowerShell\Modules` or `…\PowerShell\<version>\Modules` (not
/// `…\WindowsPowerShell\…`).
fn is_pwsh7_module_dir(entry: &str) -> bool {
    let parts: Vec<String> = entry
        .trim()
        .trim_end_matches(['\\', '/'])
        .split(['\\', '/'])
        .map(str::to_ascii_lowercase)
        .collect();
    match parts.as_slice() {
        [.., ps, modules] if ps == "powershell" && modules == "modules" => true,
        [.., ps, version, modules]
            if ps == "powershell"
                && modules == "modules"
                && version.starts_with(|c: char| c.is_ascii_digit()) =>
        {
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_powershell_drops_pwsh7_module_dirs() {
        let polluted = r"C:\Users\erik\OneDrive\Documenten\PowerShell\Modules;C:\Program Files\PowerShell\Modules;c:\program files\powershell\7\Modules;C:\Program Files\WindowsPowerShell\Modules;C:\WINDOWS\system32\WindowsPowerShell\v1.0\Modules;D:\tools\psmods";
        let clean = r"C:\Program Files\WindowsPowerShell\Modules;C:\WINDOWS\system32\WindowsPowerShell\v1.0\Modules;D:\tools\psmods";
        let winps = r"C:\WINDOWS\System32\WindowsPowerShell\v1.0\powershell.exe";
        assert_eq!(
            windows_powershell_module_path(winps, Some(polluted)).as_deref(),
            Some(clean)
        );
        // Already clean, not set, or another shell: leave it alone.
        assert_eq!(windows_powershell_module_path(winps, Some(clean)), None);
        assert_eq!(windows_powershell_module_path(winps, None), None);
        assert_eq!(
            windows_powershell_module_path(
                r"C:\Program Files\PowerShell\7\pwsh.exe",
                Some(polluted)
            ),
            None
        );
        assert!(is_pwsh7_module_dir(
            r"C:\Program Files\PowerShell\7-preview\Modules\"
        ));
        assert!(!is_pwsh7_module_dir(
            r"C:\Users\x\Documents\WindowsPowerShell\Modules"
        ));
        assert!(!is_pwsh7_module_dir(r"C:\PowerShell\Scripts"));
    }

    #[test]
    fn shell_kind_from_program() {
        use ShellKind::*;
        let cases = [
            (r"C:\Program Files\PowerShell\7\pwsh.exe", PowerShell),
            (
                r"C:\WINDOWS\System32\WindowsPowerShell\v1.0\powershell.exe",
                PowerShell,
            ),
            (r"C:\WINDOWS\system32\cmd.exe", Cmd),
            (r"C:\Program Files\Git\bin\bash.exe", GitBash),
            (r"C:\Windows\System32\bash.exe", Wsl),
            (r"C:\WINDOWS\system32\wsl.exe", Wsl),
            ("/bin/bash", Posix),
            ("/usr/bin/zsh", Posix),
            ("/usr/bin/fish", Fish),
        ];
        for (program, kind) in cases {
            assert_eq!(ShellKind::from_program(program), kind, "{program}");
        }
    }
}
