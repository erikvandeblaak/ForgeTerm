//! What a project's tests are usually run with, from the files in its root. Only a
//! suggestion for a text field the user can change: ForgeTerm never runs it.

use std::path::Path;

/// The usual test command for a folder with these top-level file names.
pub fn test_command_for(names: &[&str]) -> Option<&'static str> {
    let has = |n: &str| names.iter().any(|x| x.eq_ignore_ascii_case(n));
    let ends = |suffix: &str| {
        names
            .iter()
            .any(|x| x.to_ascii_lowercase().ends_with(suffix))
    };
    if has("Cargo.toml") {
        Some("cargo test")
    } else if has("go.mod") {
        Some("go test ./...")
    } else if has("package.json") {
        Some(if has("pnpm-lock.yaml") {
            "pnpm test"
        } else if has("yarn.lock") {
            "yarn test"
        } else {
            "npm test"
        })
    } else if [
        "pyproject.toml",
        "pytest.ini",
        "setup.py",
        "setup.cfg",
        "tox.ini",
    ]
    .iter()
    .any(|n| has(n))
    {
        Some("python -m pytest")
    } else if ends(".sln") || ends(".csproj") || ends(".fsproj") {
        Some("dotnet test")
    } else if has("pom.xml") {
        Some("mvn test")
    } else if has("build.gradle") || has("build.gradle.kts") {
        Some("gradle test")
    } else if has("mix.exs") {
        Some("mix test")
    } else if has("Makefile") {
        Some("make test")
    } else {
        None
    }
}

/// [`test_command_for`] the entries directly in `dir` (names only; nothing is
/// opened, and nothing is read when `dir` leads off this computer).
pub fn test_command(dir: &Path) -> Option<&'static str> {
    if !forgeterm_fs::resolves_locally(dir) {
        return None;
    }
    let names: Vec<String> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .take(2000)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    test_command_for(&names)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggests_by_project_files() {
        assert_eq!(
            test_command_for(&["Cargo.toml", "package.json"]),
            Some("cargo test")
        );
        assert_eq!(
            test_command_for(&["package.json", "yarn.lock"]),
            Some("yarn test")
        );
        assert_eq!(
            test_command_for(&["package.json", "pnpm-lock.yaml"]),
            Some("pnpm test")
        );
        assert_eq!(test_command_for(&["package.json"]), Some("npm test"));
        assert_eq!(
            test_command_for(&["pyproject.toml"]),
            Some("python -m pytest")
        );
        assert_eq!(test_command_for(&["Shop.SLN"]), Some("dotnet test"));
        assert_eq!(test_command_for(&["go.mod"]), Some("go test ./..."));
        assert_eq!(test_command_for(&["README.md"]), None);
        assert_eq!(test_command_for(&[]), None);
    }

    #[test]
    fn reads_the_folder() {
        let dir = std::env::temp_dir().join(format!("forgeterm-testcmd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(test_command(&dir), None);
        std::fs::write(dir.join("Cargo.toml"), "").unwrap();
        assert_eq!(test_command(&dir), Some("cargo test"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
