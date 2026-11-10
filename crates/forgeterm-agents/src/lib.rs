//! Agent adapters for command-line AI agents (Claude Code, Codex).
//!
//! Only documented, user-visible interfaces are used: the executable on `PATH`, its
//! `--version` output, and the project files the agents themselves read. Nothing is
//! written, nothing is sent anywhere, and no agent is started from here; the GUI
//! starts an agent only on an explicit user action.

mod context;
mod detect;

pub use context::{ContextItem, ContextKind, ProjectContext, project_root, scan_context};
pub use detect::{Agent, LaunchCommand, launch_command, locate, query_version};

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AgentKind {
    ClaudeCode,
    Codex,
}

impl AgentKind {
    pub const ALL: [AgentKind; 2] = [AgentKind::ClaudeCode, AgentKind::Codex];

    pub fn name(self) -> &'static str {
        match self {
            AgentKind::ClaudeCode => "Claude Code",
            AgentKind::Codex => "Codex",
        }
    }

    /// Executable name looked up on `PATH`.
    pub fn program(self) -> &'static str {
        match self {
            AgentKind::ClaudeCode => "claude",
            AgentKind::Codex => "codex",
        }
    }
}

impl fmt::Display for AgentKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}
