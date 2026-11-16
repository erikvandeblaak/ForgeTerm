//! Path safety shared by every crate that touches the file system for something
//! that came from outside ForgeTerm (terminal output, a repository, a session file).
//!
//! On Windows, opening or even stat-ing `\\server\share\…` makes an SMB connection,
//! which hands the user's NTLM hash to that server. A path *string* can be checked
//! ([`is_local_path`]), but a symlink or junction inside a cloned repository can
//! point to such a share while its own path looks local. [`resolves_locally`]
//! follows links component by component — without ever touching a link's target
//! before that target has been checked — and refuses as soon as one leads off the
//! machine.

use std::fs;
use std::path::{Component, Path, PathBuf};

/// Links followed before giving up (a cycle or an absurd chain).
const MAX_LINKS: usize = 32;

/// A local absolute path: on Windows `X:\...` (a drive letter; mapped network
/// drives count as local), never `\\server\share`, `//server`, `\\?\UNC`
/// or device paths; elsewhere an absolute path.
pub fn is_local_path(path: &str) -> bool {
    if cfg!(windows) {
        let b = path.as_bytes();
        has_drive(path) && (b.len() == 2 || b[2] == b'\\' || b[2] == b'/')
    } else {
        path.starts_with('/')
    }
}

fn has_drive(p: &str) -> bool {
    let b = p.as_bytes();
    b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
}

/// Whether `path` is local and every symlink or junction on the way to it points
/// somewhere local as well. Missing components end the walk with `true` (there is
/// nothing to follow; the caller's own access then simply fails). Only
/// `symlink_metadata` and `read_link` are used, which never follow the link.
pub fn resolves_locally(path: &Path) -> bool {
    resolve(path.to_path_buf(), 0)
}

fn resolve(path: PathBuf, links: usize) -> bool {
    if links > MAX_LINKS {
        return false;
    }
    let Some(s) = path.to_str() else {
        return false;
    };
    if !is_local_path(s) {
        return false;
    }
    let components: Vec<Component<'_>> = path.components().collect();
    let mut prefix = PathBuf::new();
    for (i, comp) in components.iter().enumerate() {
        prefix.push(comp.as_os_str());
        if matches!(
            comp,
            Component::Prefix(_) | Component::RootDir | Component::CurDir
        ) {
            continue;
        }
        let meta = match fs::symlink_metadata(&prefix) {
            Ok(m) => m,
            // Does not exist (yet) or no access: nothing further to follow.
            Err(_) => return true,
        };
        if !meta.file_type().is_symlink() {
            continue;
        }
        let Ok(target) = fs::read_link(&prefix) else {
            return false;
        };
        let target = normalize_target(target);
        let base = prefix.parent().map(Path::to_path_buf).unwrap_or_default();
        let mut next = if target.is_absolute() || target.to_str().is_some_and(has_drive) {
            target
        } else if target.has_root() {
            // `\x` on Windows: root of the current drive; treat as not local.
            return false;
        } else {
            base.join(target)
        };
        for rest in &components[i + 1..] {
            next.push(rest.as_os_str());
        }
        return resolve(next, links + 1);
    }
    true
}

/// `read_link` on Windows can report `\\?\C:\x` (junctions); that is the local
/// `C:\x`. `\\?\UNC\…` stays as it is and fails [`is_local_path`].
fn normalize_target(target: PathBuf) -> PathBuf {
    if let Some(s) = target.to_str()
        && let Some(rest) = s.strip_prefix(r"\\?\")
        && has_drive(rest)
    {
        return PathBuf::from(rest);
    }
    target
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(windows)]
    fn windows_paths() {
        assert!(is_local_path(r"C:\work"));
        assert!(is_local_path("C:/work"));
        assert!(is_local_path("D:"));
        assert!(!is_local_path(r"\\server\share"));
        assert!(!is_local_path("//server/share"));
        assert!(!is_local_path(r"\\?\UNC\server\share"));
        assert!(!is_local_path(r"\\.\pipe\x"));
        assert!(!is_local_path(r"\work"));
        assert!(!is_local_path("work"));
        assert_eq!(
            normalize_target(PathBuf::from(r"\\?\C:\Users\x")),
            PathBuf::from(r"C:\Users\x")
        );
        assert_eq!(
            normalize_target(PathBuf::from(r"\\?\UNC\host\s")),
            PathBuf::from(r"\\?\UNC\host\s")
        );
    }

    #[test]
    #[cfg(unix)]
    fn unix_paths() {
        assert!(is_local_path("/home/x"));
        assert!(!is_local_path("home/x"));
    }

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("forgeterm-fs-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn plain_and_missing_paths() {
        let d = scratch("plain");
        fs::write(d.join("a.txt"), "x").unwrap();
        assert!(resolves_locally(&d.join("a.txt")));
        assert!(resolves_locally(&d.join("missing/deeper.txt")));
        assert!(!resolves_locally(Path::new("relative/path")));
        let _ = fs::remove_dir_all(&d);
    }

    /// Links to local places are fine, also relative and chained; a link that
    /// leaves the machine is refused wherever it sits in the path.
    #[test]
    fn links_are_followed_only_while_local() {
        let d = scratch("links");
        fs::create_dir_all(d.join("real")).unwrap();
        fs::write(d.join("real/f.txt"), "x").unwrap();

        /// (target, link) → create a symlink.
        type MkLink = fn(&Path, &Path) -> std::io::Result<()>;
        #[cfg(unix)]
        let (mk_dir, mk_file): (MkLink, MkLink) = (
            |t, l| std::os::unix::fs::symlink(t, l),
            |t, l| std::os::unix::fs::symlink(t, l),
        );
        #[cfg(windows)]
        let (mk_dir, mk_file): (MkLink, MkLink) = (
            |t, l| std::os::windows::fs::symlink_dir(t, l),
            |t, l| std::os::windows::fs::symlink_file(t, l),
        );

        // Creating symlinks needs Developer Mode (or admin) on Windows.
        if mk_dir(&d.join("real"), &d.join("abs-link")).is_err() {
            eprintln!("cannot create symlinks here; skipped");
            let _ = fs::remove_dir_all(&d);
            return;
        }
        mk_dir(Path::new("real"), &d.join("rel-link")).unwrap();
        mk_dir(&d.join("rel-link"), &d.join("chain")).unwrap();
        assert!(resolves_locally(&d.join("abs-link/f.txt")));
        assert!(resolves_locally(&d.join("rel-link/f.txt")));
        assert!(resolves_locally(&d.join("chain/f.txt")));

        #[cfg(windows)]
        {
            mk_file(Path::new(r"\\127.0.0.1\share\x.txt"), &d.join("unc.txt")).unwrap();
            mk_dir(Path::new(r"\\127.0.0.1\share"), &d.join("unc-dir")).unwrap();
            assert!(!resolves_locally(&d.join("unc.txt")));
            assert!(!resolves_locally(&d.join("unc-dir/deeper/x.txt")));
            // A local link to the bad link is just as bad.
            mk_file(&d.join("unc.txt"), &d.join("via.txt")).unwrap();
            assert!(!resolves_locally(&d.join("via.txt")));
        }
        #[cfg(unix)]
        {
            let _ = mk_file;
            // A cycle ends in a refusal instead of a hang.
            mk_dir(&d.join("loop-b"), &d.join("loop-a")).unwrap();
            mk_dir(&d.join("loop-a"), &d.join("loop-b")).unwrap();
            assert!(!resolves_locally(&d.join("loop-a/x")));
        }
        let _ = fs::remove_dir_all(&d);
    }
}
