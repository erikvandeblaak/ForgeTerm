//! Mission Control model: what the dashboard says about a running agent, derived
//! only from facts ForgeTerm observes itself. No guessing what an agent "does":
//!
//! - the process: starting, running, exited with a code, failed to start;
//! - a status the program reported itself (Program Status Protocol, OSC 7501);
//! - a bell that rang while the user was not looking;
//! - when the terminal last produced output.
//!
//! Without a reported status a running agent is just "Running"; how long it has
//! been quiet is shown as a fact ("no output for 2m"), never as "idle" or "waiting".

use std::time::Duration;

/// The agent's process, as the terminal sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Process {
    /// Waiting for its first layout (starts at the pane's real size).
    Starting,
    Running,
    Exited(Option<i32>),
    /// Could not be started (message from the OS).
    FailedToStart,
}

/// A status the program reported itself (OSC 7501), mirrored without GUI types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reported {
    Working,
    Blocked,
    Idle,
    Done,
    Error,
}

/// What the dashboard shows, most urgent first (see [`Status::urgency`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Status {
    /// The agent itself reported it needs the user (permission, question, login).
    NeedsYou,
    /// Reported an error, or the process failed / exited with a non-zero code.
    Failed,
    /// A bell rang while the user was elsewhere (many agents ring when they wait).
    Bell,
    /// Reported that it is working.
    Working,
    /// Reported that it finished its task.
    Done,
    /// Reported that it is idle.
    Idle,
    /// The process runs; the agent reports nothing about what it is doing.
    Running,
    Starting,
    /// The process ended cleanly.
    Exited,
}

impl Status {
    /// 0 = most urgent. Sorting cards by this puts what needs the user first.
    pub fn urgency(self) -> u8 {
        match self {
            Status::NeedsYou => 0,
            Status::Failed => 1,
            Status::Bell => 2,
            Status::Done => 3,
            Status::Working => 4,
            Status::Running => 5,
            Status::Idle => 6,
            Status::Starting => 7,
            Status::Exited => 8,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Status::NeedsYou => "Needs you",
            Status::Failed => "Failed",
            Status::Bell => "Bell",
            Status::Working => "Working",
            Status::Done => "Done",
            Status::Idle => "Idle",
            Status::Running => "Running",
            Status::Starting => "Starting",
            Status::Exited => "Exited",
        }
    }

    /// Whether the user should look at it.
    pub fn wants_attention(self) -> bool {
        matches!(
            self,
            Status::NeedsYou | Status::Failed | Status::Bell | Status::Done
        )
    }

    /// Where the label comes from, for the tooltip: so the user can judge it.
    pub fn source(self) -> &'static str {
        match self {
            Status::NeedsYou | Status::Working | Status::Done | Status::Idle => {
                "Reported by the agent itself (terminal program status, OSC 7501)."
            }
            Status::Failed => {
                "Reported by the agent, or its process failed / exited with an error code."
            }
            Status::Bell => "The terminal bell rang while you were not looking.",
            Status::Running => {
                "The process is running. The agent reports no status, so ForgeTerm does not guess what it is doing."
            }
            Status::Starting => "The terminal is starting.",
            Status::Exited => "The process ended with exit code 0.",
        }
    }
}

/// Derives the dashboard status. A reported status wins over the bell, except
/// that a dead process is always reported as such.
pub fn classify(process: &Process, reported: Option<Reported>, bell: bool) -> Status {
    match process {
        Process::Starting => return Status::Starting,
        Process::FailedToStart => return Status::Failed,
        Process::Exited(Some(0)) => return Status::Exited,
        Process::Exited(_) => return Status::Failed,
        Process::Running => {}
    }
    match reported {
        Some(Reported::Blocked) => Status::NeedsYou,
        Some(Reported::Error) => Status::Failed,
        _ if bell => Status::Bell,
        Some(Reported::Working) => Status::Working,
        Some(Reported::Done) => Status::Done,
        Some(Reported::Idle) => Status::Idle,
        None => Status::Running,
    }
}

/// Compact duration: `8s`, `4m 05s`, `2h 03m`, `3d 04h`.
pub fn format_duration(d: Duration) -> String {
    let s = d.as_secs();
    match s {
        0..60 => format!("{s}s"),
        60..3600 => format!("{}m {:02}s", s / 60, s % 60),
        3600..86400 => format!("{}h {:02}m", s / 3600, (s % 3600) / 60),
        _ => format!("{}d {:02}h", s / 86400, (s % 86400) / 3600),
    }
}

/// Output recency as a fact: "output now", "no output for 2m 10s".
pub fn output_recency(since_last_output: Option<Duration>) -> String {
    match since_last_output {
        None => "no output yet".to_owned(),
        Some(d) if d < Duration::from_secs(2) => "output now".to_owned(),
        Some(d) => format!("no output for {}", format_duration(d)),
    }
}

/// 0.0–1.0: how "live" the output is, fading out over `fade`. Drives the activity
/// indicator; 1.0 right after output, 0.0 after `fade` or with no output.
pub fn liveness(since_last_output: Option<Duration>, fade: Duration) -> f32 {
    match since_last_output {
        Some(d) if d < fade => 1.0 - d.as_secs_f32() / fade.as_secs_f32(),
        _ => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_state_wins() {
        assert_eq!(
            classify(&Process::Exited(Some(0)), Some(Reported::Working), true),
            Status::Exited
        );
        assert_eq!(
            classify(&Process::Exited(Some(2)), None, false),
            Status::Failed
        );
        assert_eq!(
            classify(&Process::Exited(None), None, false),
            Status::Failed
        );
        assert_eq!(
            classify(&Process::FailedToStart, None, false),
            Status::Failed
        );
        assert_eq!(classify(&Process::Starting, None, true), Status::Starting);
    }

    #[test]
    fn reported_status_maps_without_guessing() {
        let r = |s| classify(&Process::Running, Some(s), false);
        assert_eq!(r(Reported::Blocked), Status::NeedsYou);
        assert_eq!(r(Reported::Error), Status::Failed);
        assert_eq!(r(Reported::Working), Status::Working);
        assert_eq!(r(Reported::Done), Status::Done);
        assert_eq!(r(Reported::Idle), Status::Idle);
        // Nothing reported: only "running", never idle/waiting.
        assert_eq!(classify(&Process::Running, None, false), Status::Running);
    }

    #[test]
    fn bell_is_below_needs_you_and_errors_but_above_the_rest() {
        let b = |s| classify(&Process::Running, s, true);
        assert_eq!(b(Some(Reported::Blocked)), Status::NeedsYou);
        assert_eq!(b(Some(Reported::Error)), Status::Failed);
        assert_eq!(b(Some(Reported::Working)), Status::Bell);
        assert_eq!(b(None), Status::Bell);
    }

    #[test]
    fn urgency_orders_attention_first() {
        let mut all = [
            Status::Exited,
            Status::Running,
            Status::Bell,
            Status::NeedsYou,
            Status::Working,
            Status::Failed,
            Status::Done,
            Status::Idle,
            Status::Starting,
        ];
        all.sort_by_key(|s| s.urgency());
        assert_eq!(all[0], Status::NeedsYou);
        assert_eq!(all[1], Status::Failed);
        assert!(all[..4].iter().all(|s| s.wants_attention()));
        assert!(all[4..].iter().all(|s| !s.wants_attention()));
    }

    #[test]
    fn durations_are_compact() {
        let d = Duration::from_secs;
        assert_eq!(format_duration(d(0)), "0s");
        assert_eq!(format_duration(d(59)), "59s");
        assert_eq!(format_duration(d(65)), "1m 05s");
        assert_eq!(format_duration(d(3600 + 180)), "1h 03m");
        assert_eq!(format_duration(d(86400 * 3 + 3600 * 4)), "3d 04h");
    }

    #[test]
    fn recency_and_liveness() {
        assert_eq!(output_recency(None), "no output yet");
        assert_eq!(
            output_recency(Some(Duration::from_millis(500))),
            "output now"
        );
        assert_eq!(
            output_recency(Some(Duration::from_secs(130))),
            "no output for 2m 10s"
        );
        let fade = Duration::from_secs(10);
        assert_eq!(liveness(None, fade), 0.0);
        assert_eq!(liveness(Some(Duration::ZERO), fade), 1.0);
        assert!((liveness(Some(Duration::from_secs(5)), fade) - 0.5).abs() < 1e-6);
        assert_eq!(liveness(Some(Duration::from_secs(30)), fade), 0.0);
    }
}
