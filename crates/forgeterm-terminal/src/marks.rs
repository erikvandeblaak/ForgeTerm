//! Shell-integration marks: where prompts and commands begin and how they ended.
//!
//! Shells report the phases of every command with OSC 133 (FinalTerm, used by most
//! terminals) or OSC 633 (VS Code's variant, which also carries the command line):
//!
//! | Mark | Meaning |
//! |---|---|
//! | `A` | a prompt starts |
//! | `B` | the prompt ends, the user types the command |
//! | `C` | the command runs, output follows |
//! | `D;code` | the command finished (exit code optional) |
//! | `633;E;line` | the command line, escaped (`\\`, `\xAB`) |
//!
//! The grid has no place for these marks, so ForgeTerm's own shell integration also
//! wraps each prompt in an OSC 8 hyperlink with the scheme `forgeterm-prompt:<n>`.
//! alacritty stores hyperlinks in the cells of the printed prompt, so the prompt's
//! position travels with the text through scrolling, scrollback trimming and
//! reflow. ForgeTerm never opens these links; they are only found again in the grid.
//!
//! Everything here is a hint printed by whatever runs in the terminal: a program can
//! fake marks. They only steer navigation and what is offered to copy; nothing is
//! executed.

use std::collections::VecDeque;

/// URI scheme of the hyperlink that marks a prompt in the grid.
pub const PROMPT_SCHEME: &str = "forgeterm-prompt:";

/// Longest command line kept (characters).
const MAX_COMMAND_LINE: usize = 1000;
/// Commands remembered per session.
const MAX_RECORDS: usize = 1000;

/// One shell-integration event parsed from an OSC payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellMark {
    PromptStart,
    CommandStart,
    OutputStart,
    Finished(Option<i32>),
    CommandLine(String),
    /// The `forgeterm-prompt:` hyperlink; `None` when the shell cannot number its
    /// prompts (cmd).
    PromptLink(Option<u64>),
}

/// Parses one OSC payload (without `ESC ]` and terminator).
pub fn parse(payload: &[u8]) -> Option<ShellMark> {
    let s = std::str::from_utf8(payload).ok()?;
    if let Some(rest) = s.strip_prefix("8;") {
        // OSC 8 ; params ; uri
        let (_, uri) = rest.split_once(';')?;
        return prompt_link(uri).map(ShellMark::PromptLink);
    }
    let rest = s.strip_prefix("133;").or_else(|| s.strip_prefix("633;"))?;
    let (kind, args) = match rest.split_once(';') {
        Some((k, a)) => (k, Some(a)),
        None => (rest, None),
    };
    Some(match kind {
        "A" => ShellMark::PromptStart,
        "B" => ShellMark::CommandStart,
        "C" => ShellMark::OutputStart,
        "D" => ShellMark::Finished(
            args.and_then(|a| a.split(';').next())
                .and_then(|c| c.trim().parse().ok()),
        ),
        "E" if s.starts_with("633;") => {
            // E;<line>[;<nonce>]: an escaped `;` never appears raw inside the line.
            let line = args?.split(';').next()?;
            ShellMark::CommandLine(unescape(line)?)
        }
        _ => return None,
    })
}

/// `Some(id)` for a `forgeterm-prompt:` URI (`Some(None)` without a number).
pub fn prompt_link(uri: &str) -> Option<Option<u64>> {
    let id = uri.strip_prefix(PROMPT_SCHEME)?;
    if id.is_empty() {
        Some(None)
    } else {
        id.parse().ok().map(Some)
    }
}

/// VS Code's escaping: `\\` → `\`, `\xAB` → that byte. Control characters are
/// dropped from the result (the line is shown and pasted, never executed).
fn unescape(s: &str) -> Option<String> {
    let mut out = Vec::with_capacity(s.len());
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' {
            match b.get(i + 1) {
                Some(b'\\') => {
                    out.push(b'\\');
                    i += 2;
                }
                Some(b'x') => {
                    let v = s
                        .get(i + 2..i + 4)
                        .and_then(|h| u8::from_str_radix(h, 16).ok())?;
                    out.push(v);
                    i += 4;
                }
                _ => {
                    out.push(b'\\');
                    i += 1;
                }
            }
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    let text = String::from_utf8_lossy(&out);
    let clean: String = text
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_COMMAND_LINE)
        .collect();
    Some(clean)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandState {
    /// The prompt is shown; nothing ran yet.
    AtPrompt,
    Running,
    /// Finished with this exit code (`None`: the shell did not say).
    Finished(Option<i32>),
}

/// One prompt and the command typed at it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandRecord {
    /// Number from the prompt hyperlink; `None` for shells that cannot number
    /// prompts.
    pub id: Option<u64>,
    pub command: Option<String>,
    pub state: CommandState,
}

/// The commands of one session, oldest first.
#[derive(Debug, Clone, Default)]
pub struct CommandLog {
    records: VecDeque<CommandRecord>,
    /// The shell reported at least one `C` (so it marks commands, not just prompts).
    pub reports_commands: bool,
    /// A `forgeterm-prompt:` link was seen, so prompts can be found in the grid
    /// (a shell that only sends `133;A`, e.g. fish, has no links).
    pub links_prompts: bool,
}

impl CommandLog {
    pub fn apply(&mut self, mark: ShellMark) {
        match mark {
            ShellMark::PromptStart => {
                if self.records.len() == MAX_RECORDS {
                    self.records.pop_front();
                }
                self.records.push_back(CommandRecord {
                    id: None,
                    command: None,
                    state: CommandState::AtPrompt,
                });
            }
            ShellMark::PromptLink(id) => {
                self.links_prompts = true;
                if let Some(last) = self.records.back_mut()
                    && last.state == CommandState::AtPrompt
                    && last.id.is_none()
                {
                    last.id = id;
                }
            }
            ShellMark::CommandStart => {}
            ShellMark::CommandLine(line) => {
                if let Some(last) = self.records.back_mut()
                    && last.state == CommandState::AtPrompt
                {
                    last.command = Some(line);
                }
            }
            ShellMark::OutputStart => {
                self.reports_commands = true;
                if let Some(last) = self.records.back_mut()
                    && last.state == CommandState::AtPrompt
                {
                    last.state = CommandState::Running;
                }
            }
            ShellMark::Finished(code) => {
                if let Some(last) = self.records.back_mut()
                    && last.state == CommandState::Running
                {
                    last.state = CommandState::Finished(code);
                }
            }
        }
    }

    pub fn records(&self) -> impl DoubleEndedIterator<Item = &CommandRecord> {
        self.records.iter()
    }

    pub fn by_id(&self, id: u64) -> Option<&CommandRecord> {
        self.records.iter().rev().find(|r| r.id == Some(id))
    }

    /// The most recent command that ran (finished or still running).
    pub fn last_run(&self) -> Option<&CommandRecord> {
        self.records
            .iter()
            .rev()
            .find(|r| r.state != CommandState::AtPrompt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Option<ShellMark> {
        parse(s.as_bytes())
    }

    #[test]
    fn parses_133_and_633_marks() {
        assert_eq!(p("133;A"), Some(ShellMark::PromptStart));
        assert_eq!(p("133;A;cl=m;aid=1"), Some(ShellMark::PromptStart));
        assert_eq!(p("633;B"), Some(ShellMark::CommandStart));
        assert_eq!(p("133;C"), Some(ShellMark::OutputStart));
        assert_eq!(p("133;C;ft=1"), Some(ShellMark::OutputStart));
        assert_eq!(p("133;D;0"), Some(ShellMark::Finished(Some(0))));
        assert_eq!(
            p("633;D;-1073741510"),
            Some(ShellMark::Finished(Some(-1073741510)))
        );
        assert_eq!(p("133;D"), Some(ShellMark::Finished(None)));
        assert_eq!(p("133;D;x"), Some(ShellMark::Finished(None)));
        assert_eq!(p("133;Z"), None);
        assert_eq!(p("9;9;C:\\x"), None);
        assert_eq!(p("1337;A"), None);
    }

    #[test]
    fn command_line_is_unescaped_and_cleaned() {
        assert_eq!(
            p(r"633;E;echo a\x3bb \\x\x0a;nonce"),
            Some(ShellMark::CommandLine(r"echo a;b \x".into()))
        );
        // ESC from the line is dropped, never passed on.
        assert_eq!(
            p(r"633;E;a\x1b]0\x3bevil\x07b"),
            Some(ShellMark::CommandLine("a]0;evilb".into()))
        );
        assert_eq!(p(r"633;E;bad\xZZ"), None);
        assert_eq!(p("133;E;x"), None, "E only exists in 633");
        // UTF-8 split into escaped bytes.
        assert_eq!(
            p(r"633;E;caf\xc3\xa9"),
            Some(ShellMark::CommandLine("café".into()))
        );
    }

    #[test]
    fn prompt_links() {
        assert_eq!(
            p("8;;forgeterm-prompt:42"),
            Some(ShellMark::PromptLink(Some(42)))
        );
        assert_eq!(
            p("8;id=x;forgeterm-prompt:"),
            Some(ShellMark::PromptLink(None))
        );
        assert_eq!(p("8;;forgeterm-prompt:x"), None);
        assert_eq!(p("8;;https://example.com"), None);
        assert_eq!(p("8;;"), None);
    }

    fn log(marks: &[&str]) -> CommandLog {
        let mut l = CommandLog::default();
        for m in marks {
            l.apply(p(m).unwrap());
        }
        l
    }

    #[test]
    fn records_follow_the_command_lifecycle() {
        let l = log(&[
            "133;A",
            "8;;forgeterm-prompt:1",
            "133;B",
            r"633;E;cargo test",
            "133;C",
            "133;D;101",
            "133;A",
            "8;;forgeterm-prompt:2",
            "133;B",
        ]);
        let r: Vec<_> = l.records().cloned().collect();
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].id, Some(1));
        assert_eq!(r[0].command.as_deref(), Some("cargo test"));
        assert_eq!(r[0].state, CommandState::Finished(Some(101)));
        assert_eq!(r[1].state, CommandState::AtPrompt);
        assert_eq!(l.last_run().unwrap().id, Some(1));
        assert_eq!(l.by_id(2).unwrap().state, CommandState::AtPrompt);
        assert!(l.reports_commands);
    }

    #[test]
    fn out_of_order_marks_do_not_corrupt_records() {
        // D without C (an empty Enter in some shells), C twice, a second link.
        let l = log(&[
            "133;D;1",
            "133;A",
            "8;;forgeterm-prompt:7",
            "8;;forgeterm-prompt:8",
            "133;D;5",
            "133;C",
            "133;C",
            "133;D;0",
            "133;D;3",
        ]);
        let r: Vec<_> = l.records().cloned().collect();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].id, Some(7));
        assert_eq!(r[0].state, CommandState::Finished(Some(0)));
    }

    #[test]
    fn prompt_only_shells_and_cap() {
        let mut l = CommandLog::default();
        for _ in 0..MAX_RECORDS + 5 {
            l.apply(ShellMark::PromptStart);
            l.apply(ShellMark::PromptLink(None));
        }
        assert_eq!(l.records().count(), MAX_RECORDS);
        assert!(l.last_run().is_none());
        assert!(!l.reports_commands);
    }

    #[test]
    fn prompts_are_navigable_only_with_links() {
        // fish-style: 133;A without ForgeTerm's link cannot be found in the grid.
        let mut l = CommandLog::default();
        l.apply(ShellMark::PromptStart);
        assert!(!l.links_prompts);
        l.apply(p("8;;forgeterm-prompt:1").unwrap());
        assert!(l.links_prompts);
        // cmd: unnumbered link counts too.
        let mut cmd = CommandLog::default();
        cmd.apply(p("8;;forgeterm-prompt:").unwrap());
        assert!(cmd.links_prompts);
    }
}
