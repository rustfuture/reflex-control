//! One adapter per coding agent.
//!
//! An adapter does three things for its agent:
//! 1. reads the JSON the agent sends to a hook and turns it into a [`HookEvent`]
//! 2. renders a [`Verdict`] in the reply format the agent expects (stdout and exit code)
//! 3. plans how to install and uninstall the hook (which files, what to put in them)
//!
//! To add an agent: create a file in this directory with a unit struct that implements
//! [`AgentAdapter`], then add it to [`registry`]. Nothing else needs to change; the
//! wizard, `reflex hook`, `reflex doctor` and uninstall all go through the registry.

use crate::config::Scope;
use crate::guard::Verdict;
use crate::install::{Files, InstallError, Plan, PlanCtx};
use std::path::{Path, PathBuf};

pub mod claude_code;
pub mod cline;
pub mod git;
mod js_bridge;
pub mod opencode;
pub mod pi;
mod util;

/// What the agent is about to do, in agent-independent terms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookEvent {
    /// A tool call that writes to these files.
    PreWrite { paths: Vec<String> },
    /// A tool call that runs this shell command.
    PreShell { command: String },
    /// The agent finished its turn and wants to stop.
    TurnEnd { session_id: String },
    /// A `git commit` is about to happen.
    PreCommit,
    /// An event the hook has nothing to say about (e.g. a read-only tool).
    Ignore,
}

/// A parsed hook call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookInput {
    /// Directory the agent was working in, if it says so.
    pub cwd: Option<PathBuf>,
    pub event: HookEvent,
}

/// How the hook process answers the agent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Rendered {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct ParseError(pub String);

/// Read-only view of the project used to detect agents.
pub struct Probe<'a> {
    pub root: &'a Path,
    /// Returns true if the named program is on `PATH`.
    pub on_path: &'a dyn Fn(&str) -> bool,
}

pub trait AgentAdapter: Sync {
    /// Stable id used in `.reflex.toml`, on the command line and in hook commands.
    fn id(&self) -> &'static str;

    /// Name shown in the wizard.
    fn label(&self) -> &'static str;

    /// One line for the wizard, e.g. what the integration covers.
    fn hint(&self) -> &'static str;

    fn scopes(&self) -> &'static [Scope];

    /// Whether the agent seems to be used in this project or on this machine.
    fn detect(&self, probe: &Probe) -> bool;

    /// Whether `reflex hook <id> <event>` should read stdin. Agents that pass JSON say
    /// yes; a git hook has no input and must not wait for one.
    fn reads_stdin(&self, _event: &str) -> bool {
        true
    }

    /// Parses the agent's input for the named hook event.
    fn parse(&self, event: &str, stdin: &str) -> Result<HookInput, ParseError>;

    /// Turns a verdict into the agent's reply.
    fn render(&self, event: &HookEvent, verdict: &Verdict) -> Rendered;

    /// Files this adapter reads or writes for the given scope.
    fn files(&self, ctx: &PlanCtx) -> Result<Vec<PathBuf>, InstallError>;

    fn plan_install(&self, ctx: &PlanCtx, existing: &Files) -> Result<Plan, InstallError>;

    fn plan_uninstall(&self, ctx: &PlanCtx, existing: &Files) -> Result<Plan, InstallError>;

    /// True if our hook is present in the files for this scope.
    fn is_installed(&self, ctx: &PlanCtx, existing: &Files) -> bool;
}

static ADAPTERS: [&dyn AgentAdapter; 6] = [
    &claude_code::ClaudeCode,
    &opencode::OPENCODE,
    &opencode::KILO,
    &cline::Cline,
    &pi::Pi,
    &git::GitHook,
];

/// All known adapters, in the order the wizard lists them.
pub fn registry() -> &'static [&'static dyn AgentAdapter] {
    &ADAPTERS
}

pub fn find(id: &str) -> Option<&'static dyn AgentAdapter> {
    registry().iter().copied().find(|a| a.id() == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_ids_are_unique_and_findable() {
        let ids: Vec<&str> = registry().iter().map(|a| a.id()).collect();
        assert_eq!(
            ids,
            ["claude-code", "opencode", "kilo", "cline", "pi", "git"]
        );
        for id in ids {
            assert_eq!(find(id).unwrap().id(), id);
        }
        assert!(find("nope").is_none());
    }
}
