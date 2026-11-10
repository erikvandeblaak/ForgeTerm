//! Working-directory reports from the shell.
//!
//! Shells announce their current directory with OSC 7 (`ESC ] 7 ; file://host/path ST`,
//! the freedesktop / macOS convention) or OSC 9;9 (`ESC ] 9 ; 9 ; path ST`, the Windows
//! Terminal convention). The emulator ignores both, so [`CwdScanner`] watches the raw
//! PTY output for them.
//!
//! A report is only a *hint*: any program can print these sequences. ForgeTerm uses it
//! to choose where new tabs start and what the explorer shows, and only after checking
//! that the directory exists. It is never executed or interpreted otherwise.

/// Longest OSC payload that is considered; longer sequences are ignored.
const MAX_PAYLOAD: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Ground,
    Esc,
    Osc,
    OscEsc,
}

/// Incremental scanner; sequences may be split across reads.
#[derive(Debug, Clone)]
pub struct CwdScanner {
    state: State,
    payload: Vec<u8>,
    overflow: bool,
}

impl Default for CwdScanner {
    fn default() -> Self {
        Self {
            state: State::Ground,
            payload: Vec::new(),
            overflow: false,
        }
    }
}

impl CwdScanner {
    /// Feeds output bytes; returns the last directory reported in them, if any.
    pub fn feed(&mut self, bytes: &[u8]) -> Option<String> {
        let mut found = None;
        for &b in bytes {
            self.state = match (self.state, b) {
                (State::Ground, 0x1b) => State::Esc,
                (State::Ground, _) => State::Ground,
                (State::Esc, b']') => {
                    self.payload.clear();
                    self.overflow = false;
                    State::Osc
                }
                (State::Esc, 0x1b) => State::Esc,
                (State::Esc, _) => State::Ground,
                (State::Osc, 0x07) => {
                    found = self.finish().or(found);
                    State::Ground
                }
                (State::Osc, 0x1b) => State::OscEsc,
                // CAN / SUB abort a control sequence.
                (State::Osc, 0x18 | 0x1a) => State::Ground,
                (State::Osc, _) => {
                    self.push(b);
                    State::Osc
                }
                (State::OscEsc, b'\\') => {
                    found = self.finish().or(found);
                    State::Ground
                }
                // ESC followed by something else: the OSC is cut off, a new escape starts.
                (State::OscEsc, b']') => {
                    self.payload.clear();
                    self.overflow = false;
                    State::Osc
                }
                (State::OscEsc, _) => State::Ground,
            };
        }
        found
    }

    fn push(&mut self, b: u8) {
        if self.payload.len() < MAX_PAYLOAD {
            self.payload.push(b);
        } else {
            self.overflow = true;
        }
    }

    fn finish(&mut self) -> Option<String> {
        if self.overflow {
            return None;
        }
        let payload = std::str::from_utf8(&self.payload).ok()?;
        let path = if let Some(uri) = payload.strip_prefix("7;") {
            path_from_file_uri(uri)?
        } else {
            let path = payload.strip_prefix("9;9;")?.trim();
            let path = path
                .strip_prefix('"')
                .and_then(|p| p.strip_suffix('"'))
                .unwrap_or(path);
            if path.is_empty() {
                return None;
            }
            native_path(path)
        };
        // Terminal output is untrusted: a network path would make the existence
        // check below contact a remote host (SMB, leaking the Windows login hash).
        is_local_path(&path).then_some(path)
    }
}

/// `file://host/path` (percent-encoded) → native path string.
fn path_from_file_uri(uri: &str) -> Option<String> {
    let rest = uri.strip_prefix("file://")?;
    // Skip the host part; the path starts at the first '/'.
    let path = &rest[rest.find('/')?..];
    let decoded = percent_decode(path)?;
    (!decoded.is_empty()).then(|| native_path(&decoded))
}

fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Some(v) = std::str::from_utf8(&bytes[i + 1..i + 3])
                .ok()
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(v);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Converts what shells report into a path for this OS. On Windows:
/// `/C:/x` and `C:/x` → `C:\x`; MSYS-style `/c/x` → `C:\x`.
fn native_path(path: &str) -> String {
    if cfg!(windows) {
        windows_path(path)
    } else {
        path.to_owned()
    }
}

fn windows_path(path: &str) -> String {
    let p = path
        .strip_prefix('/')
        .filter(|p| has_drive(p))
        .unwrap_or(path);
    if has_drive(p) {
        return p.replace('/', "\\");
    }
    // MSYS / Git Bash: /c or /c/rest
    let b = path.as_bytes();
    if b.len() >= 2 && b[0] == b'/' && b[1].is_ascii_alphabetic() && (b.len() == 2 || b[2] == b'/')
    {
        let drive = char::from(b[1]).to_ascii_uppercase();
        let rest = path[2..].trim_start_matches('/').replace('/', "\\");
        return format!("{drive}:\\{rest}");
    }
    path.to_owned()
}

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

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(chunks: &[&[u8]]) -> Option<String> {
        let mut s = CwdScanner::default();
        let mut last = None;
        for c in chunks {
            if let Some(p) = s.feed(c) {
                last = Some(p);
            }
        }
        last
    }

    #[test]
    fn network_and_device_paths_are_refused() {
        // Would make the is_dir check contact a remote host (Windows), or are not
        // absolute paths at all.
        let mut refused = vec![
            &b"\x1b]9;9;\\\\attacker.example\\share\x07"[..],
            b"\x1b]9;9;\\\\?\\UNC\\attacker\\share\x07",
            b"\x1b]9;9;\\\\.\\pipe\\x\x07",
            b"\x1b]9;9;relative\\dir\x07",
        ];
        if cfg!(windows) {
            // On Linux "//host/share" is an ordinary local path (no network access).
            refused.push(b"\x1b]9;9;//attacker.example/share\x07");
            refused.push(b"\x1b]7;file://x//attacker.example/share\x07");
        }
        for evil in refused {
            assert_eq!(scan(&[evil]), None, "{}", String::from_utf8_lossy(evil));
        }
        if cfg!(windows) {
            assert!(is_local_path(r"C:\Users"));
            assert!(is_local_path("Z:"));
            assert!(!is_local_path(r"\\server\share"));
            assert!(!is_local_path("C:relative"));
        } else {
            assert!(is_local_path("/home/x"));
            assert!(!is_local_path("relative"));
        }
    }

    #[test]
    fn osc9_9_windows_terminal_style() {
        let got = scan(&[b"\x1b]9;9;C:\\Users\\erik\\My Docs\x1b\\PS> "]);
        let quoted = scan(&[b"\x1b]9;9;\"C:\\x y\"\x07"]);
        if cfg!(windows) {
            assert_eq!(got, Some(native(r"C:\Users\erik\My Docs")));
            assert_eq!(quoted.as_deref(), Some(native(r"C:\x y")).as_deref());
        } else {
            // A Windows path means nothing on Linux: not a local path, ignored.
            assert_eq!(got, None);
            assert_eq!(quoted, None);
            // The same report with a Linux path is accepted.
            assert_eq!(scan(&[b"\x1b]9;9;/home/x\x07"]).as_deref(), Some("/home/x"));
        }
    }

    // Test paths use the MSYS form /c/...: C:\... on Windows, absolute on Unix,
    // so they are local paths on both.
    #[test]
    fn osc7_file_uri_with_percent_encoding() {
        let got = scan(&[b"\x1b]7;file://box/c/home/erik/my%20dir\x07"]);
        assert_eq!(
            got.as_deref(),
            Some(native("/c/home/erik/my dir")).as_deref()
        );
        // Multi-byte UTF-8, percent-encoded.
        let utf8 = scan(&[b"\x1b]7;file:///c/tmp/caf%C3%A9\x1b\\"]);
        assert_eq!(utf8.as_deref(), Some(native("/c/tmp/café")).as_deref());
    }

    #[test]
    fn split_across_reads_and_surrounded_by_output() {
        let got = scan(&[b"output\x1b]7;fi", b"le://h/c/srv/da", b"ta\x1b", b"\\more"]);
        assert_eq!(got.as_deref(), Some(native("/c/srv/data")).as_deref());
    }

    #[test]
    fn last_report_wins_and_other_osc_ignored() {
        let got = scan(&[
            b"\x1b]0;title\x07\x1b]7;file:///c/a\x07\x1b]7;file:///c/b\x07\x1b]8;;http://x\x07",
        ]);
        assert_eq!(got.as_deref(), Some(native("/c/b")).as_deref());
        assert_eq!(scan(&[b"\x1b]0;just a title\x07", b"\x1b[31mred"]), None);
    }

    #[test]
    fn malformed_and_oversized_are_ignored() {
        assert_eq!(scan(&[b"\x1b]7;http://host/x\x07"]), None);
        assert_eq!(scan(&[b"\x1b]7;file://hostonly\x07"]), None);
        assert_eq!(scan(&[b"\x1b]9;9;\x07"]), None);
        let mut long = b"\x1b]7;file:///".to_vec();
        long.extend(std::iter::repeat_n(b'a', MAX_PAYLOAD + 10));
        long.push(0x07);
        assert_eq!(scan(&[&long]), None);
        // CAN aborts; the following text is not part of a path.
        assert_eq!(scan(&[b"\x1b]7;file:///a\x18/b\x07"]), None);
        // The scanner recovers afterwards.
        assert_eq!(
            scan(&[&long, b"\x1b]7;file:///c/ok\x07"]).as_deref(),
            Some(native("/c/ok")).as_deref()
        );
    }

    #[test]
    fn windows_path_conversions() {
        assert_eq!(windows_path("/C:/Users/erik"), r"C:\Users\erik");
        assert_eq!(windows_path("C:/Users/erik"), r"C:\Users\erik");
        assert_eq!(windows_path("/c/Erik/work"), r"C:\Erik\work");
        assert_eq!(windows_path("/d"), r"D:\");
        assert_eq!(windows_path(r"C:\already"), r"C:\already");
        assert_eq!(windows_path("/usr/bin"), "/usr/bin");
    }

    fn native(p: &str) -> String {
        native_path(p)
    }
}
