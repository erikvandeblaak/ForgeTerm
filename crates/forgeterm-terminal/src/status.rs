//! Program status (OSC 7501, "Program Status Protocol", revision 0.4,
//! <https://control-codes.page/proposals/osc-7501/>): a program such as Claude Code
//! reports whether it is working, blocked on the user, done or failed.
//!
//! * Report: `OSC 7501 ; key=value : key=value ST`. Each report replaces its record
//!   (keyed by `id`, none = root) completely; `state=clear` removes a record and its
//!   children, or all records without `id`.
//! * Query: `OSC 7501 ; ? ST`. ForgeTerm answers [`QUERY_REPLY`] (default states and
//!   kinds), so programs know reports are shown.
//! * A new shell prompt (OSC 133 `A`) drops `working`, `blocked` and `idle`;
//!   `done` / `error` stay until the user has seen them.
//!
//! Content is untrusted: `msg` / `title` that decode to control characters reject
//! the whole report, bidi overrides and other invisible formatting are removed, and
//! nothing is ever sent back to the program except the capability reply.

/// Answer to `OSC 7501 ; ?`: supported, with the default states and kinds.
pub const QUERY_REPLY: &[u8] = b"\x1b]7501;?\x1b\\";

const MAX_RECORDS: usize = 64;
const MAX_KEY: usize = 16;
const MAX_MSG_ENCODED: usize = 2732;
const MAX_MSG: usize = 2048;
const MAX_TITLE_ENCODED: usize = 256;
const MAX_TITLE: usize = 192;
const MAX_APP: usize = 32;
const MAX_ID: usize = 128;
const MAX_ID_SEGMENT: usize = 32;
const MAX_ID_SEGMENTS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProgramState {
    Idle,
    Working,
    Done,
    Error,
    Blocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    Permission,
    Question,
    Auth,
}

/// One status record of a terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusRecord {
    /// `/`-separated path; empty = the root record.
    pub id: String,
    pub state: ProgramState,
    pub kind: Option<BlockKind>,
    /// 0–100; `None` = indeterminate.
    pub progress: Option<u8>,
    pub app: Option<String>,
    pub title: Option<String>,
    pub msg: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatusReport {
    Query,
    /// Remove the record with this id and its children; `None` = all records.
    Clear(Option<String>),
    Set(StatusRecord),
}

/// Parses one OSC payload. `None` for other OSCs and for reports the protocol says
/// to discard (unknown state, bad base64, control characters, over a limit).
pub fn parse(payload: &[u8]) -> Option<StatusReport> {
    let body = std::str::from_utf8(payload).ok()?.strip_prefix("7501;")?;
    if body.trim() == "?" {
        return Some(StatusReport::Query);
    }
    let mut state = None;
    let mut id = None;
    let mut kind = None;
    let mut progress = None;
    let mut app = None;
    let mut title = None;
    let mut msg = None;
    for pair in body.split(':') {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        if key.is_empty() || key.len() > MAX_KEY || !key.bytes().all(|b| b.is_ascii_lowercase()) {
            continue;
        }
        if !value.bytes().all(is_value_byte) {
            continue;
        }
        match key {
            "state" => state = Some(value),
            "id" => id = Some(value),
            "kind" => kind = Some(value),
            "progress" => progress = Some(value),
            "app" => app = Some(value),
            "title" => title = Some(value),
            "msg" => msg = Some(value),
            _ => {}
        }
    }

    let id = match id {
        Some(id) if !id.is_empty() => {
            valid_id(id)?;
            Some(id.to_owned())
        }
        _ => None,
    };
    let state = match state? {
        "clear" => return Some(StatusReport::Clear(id)),
        "idle" => ProgramState::Idle,
        "working" => ProgramState::Working,
        "blocked" => ProgramState::Blocked,
        "done" => ProgramState::Done,
        "error" => ProgramState::Error,
        _ => return None,
    };
    let kind = match (state, kind) {
        (ProgramState::Blocked, Some("permission")) => Some(BlockKind::Permission),
        (ProgramState::Blocked, Some("question")) => Some(BlockKind::Question),
        (ProgramState::Blocked, Some("auth")) => Some(BlockKind::Auth),
        _ => None,
    };
    let progress = match state {
        ProgramState::Working | ProgramState::Blocked => progress
            .and_then(|p| p.parse::<u8>().ok())
            .filter(|p| *p <= 100),
        _ => None,
    };
    let app = match app {
        Some(a) if a.len() > MAX_APP => return None,
        Some(a)
            if !a
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_.+-".contains(&b)) =>
        {
            return None;
        }
        Some("") | None => None,
        Some(a) => Some(a.to_owned()),
    };
    let title = text(title, MAX_TITLE_ENCODED, MAX_TITLE)?;
    let msg = text(msg, MAX_MSG_ENCODED, MAX_MSG)?;
    Some(StatusReport::Set(StatusRecord {
        id: id.unwrap_or_default(),
        state,
        kind,
        progress,
        app,
        title,
        msg,
    }))
}

fn is_value_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b"_.,+/=-".contains(&b)
}

fn valid_id(id: &str) -> Option<()> {
    let segments: Vec<&str> = id.split('/').collect();
    (id.len() <= MAX_ID
        && segments.len() <= MAX_ID_SEGMENTS
        && segments
            .iter()
            .all(|s| !s.is_empty() && s.len() <= MAX_ID_SEGMENT))
    .then_some(())
}

/// Decodes a base64 text field. Outer `None` = discard the report.
fn text(value: Option<&str>, max_encoded: usize, max_decoded: usize) -> Option<Option<String>> {
    let Some(value) = value.filter(|v| !v.is_empty()) else {
        return Some(None);
    };
    if value.len() > max_encoded {
        return None;
    }
    let bytes = base64_decode(value)?;
    if bytes.len() > max_decoded {
        return None;
    }
    let s = String::from_utf8(bytes).ok()?;
    if s.chars().any(char::is_control) {
        return None;
    }
    let shown: String = s.chars().filter(|c| !is_invisible_format(*c)).collect();
    Some((!shown.trim().is_empty()).then_some(shown))
}

/// Bidi overrides/isolates and zero-width formatting characters: shown outside the
/// grid they could disguise text.
fn is_invisible_format(c: char) -> bool {
    matches!(c,
        '\u{200B}'..='\u{200F}'
        | '\u{202A}'..='\u{202E}'
        | '\u{2060}'..='\u{2064}'
        | '\u{2066}'..='\u{2069}'
        | '\u{FEFF}')
}

/// Standard base64 (padding optional).
fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let s = s.trim_end_matches('=');
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut acc = 0u32;
    let mut bits = 0;
    for b in s.bytes() {
        let v = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    // A single leftover symbol (6 bits) cannot encode a byte.
    (bits < 6).then_some(out)
}

/// The status records of one terminal.
#[derive(Debug, Clone, Default)]
pub struct ProgramStatus {
    /// Least recently updated first.
    records: Vec<StatusRecord>,
}

impl ProgramStatus {
    /// Applies a report; returns whether anything changed.
    pub fn apply(&mut self, report: StatusReport) -> bool {
        match report {
            StatusReport::Query => false,
            StatusReport::Clear(None) => {
                let changed = !self.records.is_empty();
                self.records.clear();
                changed
            }
            StatusReport::Clear(Some(id)) => self.remove(|r| is_same_or_child(&r.id, &id)),
            StatusReport::Set(record) => {
                if let Some(pos) = self.records.iter().position(|r| r.id == record.id) {
                    let old = self.records.remove(pos);
                    let changed = old != record;
                    self.records.push(record);
                    return changed;
                }
                if self.records.len() == MAX_RECORDS {
                    self.records.remove(0);
                }
                self.records.push(record);
                true
            }
        }
    }

    /// A new shell prompt: whatever ran in the terminal has stopped.
    pub fn on_prompt(&mut self) -> bool {
        self.remove(|r| {
            matches!(
                r.state,
                ProgramState::Working | ProgramState::Blocked | ProgramState::Idle
            )
        })
    }

    /// The user has seen the terminal: finished results need not be shown anymore.
    pub fn acknowledge(&mut self) -> bool {
        self.remove(|r| matches!(r.state, ProgramState::Done | ProgramState::Error))
    }

    fn remove(&mut self, drop: impl Fn(&StatusRecord) -> bool) -> bool {
        let before = self.records.len();
        self.records.retain(|r| !drop(r));
        self.records.len() != before
    }

    pub fn records(&self) -> &[StatusRecord] {
        &self.records
    }

    /// The record that matters most to the user (blocked > error > done > working >
    /// idle; the most recent among equals), with its `app` inherited from the
    /// nearest ancestor when it has none.
    pub fn summary(&self) -> Option<StatusRecord> {
        let top = self
            .records
            .iter()
            .enumerate()
            .max_by_key(|(i, r)| (r.state, *i))
            .map(|(_, r)| r)?;
        let mut top = top.clone();
        if top.app.is_none() {
            top.app = self.inherited_app(&top.id);
        }
        Some(top)
    }

    fn inherited_app(&self, id: &str) -> Option<String> {
        let mut path = id;
        while !path.is_empty() {
            path = path.rsplit_once('/').map_or("", |(parent, _)| parent);
            if let Some(app) = self
                .records
                .iter()
                .find(|r| r.id == path)
                .and_then(|r| r.app.clone())
            {
                return Some(app);
            }
        }
        None
    }
}

fn is_same_or_child(id: &str, parent: &str) -> bool {
    id == parent
        || id
            .strip_prefix(parent)
            .is_some_and(|rest| rest.starts_with('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(s: &str) -> StatusRecord {
        match parse(s.as_bytes()) {
            Some(StatusReport::Set(r)) => r,
            other => panic!("{s}: {other:?}"),
        }
    }

    fn b64(s: &str) -> String {
        const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let b = s.as_bytes();
        let mut out = String::new();
        for chunk in b.chunks(3) {
            let n = chunk
                .iter()
                .enumerate()
                .fold(0u32, |acc, (i, &c)| acc | u32::from(c) << (16 - 8 * i));
            for i in 0..=chunk.len() {
                out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            }
        }
        out
    }

    #[test]
    fn base64_roundtrip_with_and_without_padding() {
        for s in [
            "",
            "a",
            "ab",
            "abc",
            "Wacht op toestemming: Bash(cargo test)",
            "café ✓",
        ] {
            assert_eq!(base64_decode(&b64(s)).unwrap(), s.as_bytes(), "{s}");
        }
        assert_eq!(base64_decode("YQ==").unwrap(), b"a");
        assert_eq!(base64_decode("Y"), None);
        assert_eq!(base64_decode("Y*Q"), None);
    }

    #[test]
    fn query_and_reports() {
        assert_eq!(parse(b"7501;?"), Some(StatusReport::Query));
        let r = set(&format!(
            "7501;state=blocked:kind=permission:app=claude:msg={}:progress=40",
            b64("Allow Bash?")
        ));
        assert_eq!(r.state, ProgramState::Blocked);
        assert_eq!(r.kind, Some(BlockKind::Permission));
        assert_eq!(r.app.as_deref(), Some("claude"));
        assert_eq!(r.msg.as_deref(), Some("Allow Bash?"));
        assert_eq!(r.progress, Some(40));
        assert_eq!(r.id, "");
        // Whitespace, unknown keys, malformed pairs, last value wins.
        let r = set("7501; state = idle : future=1 : junk : state=working : id=build/test");
        assert_eq!(r.state, ProgramState::Working);
        assert_eq!(r.id, "build/test");
        // kind only with blocked, progress only 0..=100 with working/blocked.
        let r = set("7501;state=done:kind=permission:progress=50");
        assert_eq!((r.kind, r.progress), (None, None));
        assert_eq!(set("7501;state=working:progress=101").progress, None);
        assert_eq!(
            parse(b"7501;state=clear:id=a"),
            Some(StatusReport::Clear(Some("a".into())))
        );
        assert_eq!(parse(b"7501;state=clear"), Some(StatusReport::Clear(None)));
    }

    #[test]
    fn discarded_reports() {
        for bad in [
            "7501;state=sleeping".to_owned(),
            "7501;app=x".to_owned(),
            // Not valid base64 (one leftover symbol).
            "7501;state=done:msg=QUJDQ".to_owned(),
            format!("7501;state=done:msg={}", b64("line\nbreak")),
            format!("7501;state=done:msg={}", b64("\u{1b}[31m")),
            format!("7501;state=done:title={}", b64(&"t".repeat(193))),
            format!("7501;state=done:msg={}", b64(&"m".repeat(2049))),
            "7501;state=done:app=a,b".to_owned(),
            format!("7501;state=done:app={}", "a".repeat(33)),
            "7501;state=done:id=a//b".to_owned(),
            format!("7501;state=done:id={}", "s/".repeat(8) + "s"),
            "7502;state=done".to_owned(),
        ] {
            assert!(
                !matches!(parse(bad.as_bytes()), Some(StatusReport::Set(_))),
                "{bad}"
            );
        }
        // A value with a forbidden byte is a malformed pair: skipped, not fatal.
        assert_eq!(set("7501;state=done:app=a;b").app, None);
        assert_eq!(set("7501;state=done:msg=@@@").msg, None);
    }

    #[test]
    fn invisible_formatting_is_removed() {
        let r = set(&format!(
            "7501;state=done:msg={}",
            b64("ok\u{202E}txt.exe\u{200B}")
        ));
        assert_eq!(r.msg.as_deref(), Some("oktxt.exe"));
    }

    #[test]
    fn records_replace_clear_and_lifetime() {
        let mut s = ProgramStatus::default();
        assert!(s.apply(StatusReport::Set(set("7501;state=working:app=claude"))));
        // Same report again: no change.
        assert!(!s.apply(StatusReport::Set(set("7501;state=working:app=claude"))));
        // Replaces completely: app is gone.
        s.apply(StatusReport::Set(set("7501;state=blocked:kind=question")));
        assert_eq!(s.records().len(), 1);
        assert_eq!(s.records()[0].app, None);

        s.apply(StatusReport::Set(set("7501;state=working:id=a:app=make")));
        s.apply(StatusReport::Set(set("7501;state=done:id=a/b")));
        s.apply(StatusReport::Set(set("7501;state=error:id=ab")));
        // Blocked wins the summary.
        assert_eq!(s.summary().unwrap().state, ProgramState::Blocked);
        s.apply(StatusReport::Clear(Some("a".into())));
        let ids: Vec<_> = s.records().iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, ["", "ab"]);

        // A new prompt drops blocked/working/idle; done/error stay until seen.
        assert!(s.on_prompt());
        assert_eq!(s.summary().unwrap().state, ProgramState::Error);
        assert!(s.acknowledge());
        assert!(s.summary().is_none());
        assert!(!s.acknowledge());
    }

    #[test]
    fn child_inherits_app_and_cap_evicts_oldest() {
        let mut s = ProgramStatus::default();
        s.apply(StatusReport::Set(set("7501;state=idle:id=build:app=make")));
        s.apply(StatusReport::Set(set("7501;state=working:id=build/test")));
        assert_eq!(s.summary().unwrap().app.as_deref(), Some("make"));

        let mut s = ProgramStatus::default();
        for i in 0..MAX_RECORDS + 3 {
            s.apply(StatusReport::Set(set(&format!("7501;state=idle:id=r{i}"))));
        }
        assert_eq!(s.records().len(), MAX_RECORDS);
        assert_eq!(s.records()[0].id, "r3");
        assert!(s.apply(StatusReport::Clear(None)));
        assert!(s.records().is_empty());
    }
}
