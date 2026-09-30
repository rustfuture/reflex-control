//! Codex CLI adapter. Source of the facts below: `codex-rs/hooks` in
//! https://github.com/openai/codex (engine, output parser, generated JSON schemas).
//!
//! Install adds two hooks to `.codex/hooks.json` (project scope) or `~/.codex/hooks.json`
//! (user scope). The file has the same nested shape as Claude Code's settings:
//!
//! * `PreToolUse` on `apply_patch|Bash`, running `reflex hook codex pre-tool` (10 s)
//! * `Stop`, running `reflex hook codex stop` (600 s, since it can run tests)
//!
//! Codex edits files with the `apply_patch` tool. The hook input carries the raw patch
//! text in `tool_input.command`; the files it touches are the paths on its
//! `*** Add File:`, `*** Update File:`, `*** Delete File:` and `*** Move to:` lines, relative
//! to the session's `cwd`. Shell commands arrive as tool `Bash` with `tool_input.command`.
//!
//! Replies are exit code 0 with JSON on stdout (or nothing to allow):
//!
//! | Verdict        | PreToolUse                                    | Stop                              |
//! |----------------|-----------------------------------------------|-----------------------------------|
//! | Allow          | empty                                         | empty                             |
//! | Block          | `permissionDecision: "deny"` + reason         | (not produced)                    |
//! | Ask            | `permissionDecision: "deny"` + reason         | (not produced)                    |
//! | RetryAgent     | (not produced)                                | `decision: "block"` + reason      |
//! | AskHuman/Notify| (not produced)                                | `systemMessage` (stop is allowed) |
//!
//! Codex rejects `permissionDecision: "ask"` from a hook ("unsupported"), so a command we
//! would have asked about is denied instead, with a reason that tells the agent to ask
//! the user. A `Stop` block reason becomes the agent's next prompt; `systemMessage` is
//! shown to the user as a warning.
//!
//! Codex only runs a hook once the user has trusted it. Trust is a hash of the event, the
//! matcher and the handler (command, timeout), so a changed hook has to be reviewed again.
//! Install cannot do that for the user; it says so in a note.

use super::claude_code::{
    group_has_ours, merge_hooks, parse_settings, remove_hooks, render_settings, HookSpec,
};
use super::{AgentAdapter, HookEvent, HookInput, ParseError, Probe, Rendered};
use crate::config::Scope;
use crate::guard::Verdict;
use crate::install::{FileChange, Files, InstallError, Plan, PlanCtx};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::PathBuf;

pub struct Codex;

const HOOK_SPECS: [HookSpec; 2] = [
    (
        "PreToolUse",
        Some("apply_patch|Bash"),
        "reflex hook codex pre-tool",
        10,
    ),
    ("Stop", None, "reflex hook codex stop", 600),
];

const TRUST_NOTE: &str = "Codex only runs hooks you have trusted. Start codex and review the \
     new hooks when it asks (or open /hooks); until then Reflex Control does not run in Codex.";

/// Fields Codex sends that we look at. Everything else is ignored.
#[derive(Debug, Deserialize)]
struct Payload {
    cwd: Option<String>,
    session_id: Option<String>,
    tool_name: Option<String>,
    tool_input: Option<Value>,
}

fn command_field(input: &Option<Value>) -> Option<String> {
    input
        .as_ref()?
        .get("command")?
        .as_str()
        .map(str::to_string)
        .filter(|s| !s.trim().is_empty())
}

/// Files an `apply_patch` patch adds, updates, deletes or moves to. OpenCode, Kilo Code and
/// Cline take the same patch format, so their adapters use this too.
pub(super) fn patch_paths(patch: &str) -> Vec<String> {
    const MARKERS: [&str; 4] = [
        "*** Add File: ",
        "*** Update File: ",
        "*** Delete File: ",
        "*** Move to: ",
    ];
    let mut paths: Vec<String> = Vec::new();
    for line in patch.lines() {
        let line = line.trim_end_matches('\r');
        let found = MARKERS.iter().find_map(|m| line.strip_prefix(m));
        if let Some(path) = found.map(str::trim).filter(|p| !p.is_empty()) {
            if !paths.iter().any(|p| p == path) {
                paths.push(path.to_string());
            }
        }
    }
    paths
}

/// True for `apply_patch <<'EOF' ...` and `applypatch ...` typed into the shell tool. Codex
/// runs those as a patch, but the hook only sees a `Bash` call carrying the command text.
fn is_apply_patch_command(command: &str) -> bool {
    let first = command.split_whitespace().next().unwrap_or("");
    matches!(first, "apply_patch" | "applypatch") && command.contains("*** Begin Patch")
}

fn is_absolute(path: &str) -> bool {
    path.starts_with('/') || path.starts_with('\\') || path.as_bytes().get(1) == Some(&b':')
}

/// Patch paths are relative to the session's directory; make them absolute when it is known.
pub(super) fn resolve(cwd: Option<&str>, path: String) -> String {
    match cwd.map(|c| c.trim_end_matches(['/', '\\'])) {
        Some(dir) if !dir.is_empty() && !is_absolute(&path) => format!("{dir}/{path}"),
        _ => path,
    }
}

fn write_event(cwd: Option<&str>, patch: &str) -> HookEvent {
    let paths: Vec<String> = patch_paths(patch)
        .into_iter()
        .map(|p| resolve(cwd, p))
        .collect();
    if paths.is_empty() {
        HookEvent::Ignore
    } else {
        HookEvent::PreWrite { paths }
    }
}

impl AgentAdapter for Codex {
    fn id(&self) -> &'static str {
        "codex"
    }

    fn label(&self) -> &'static str {
        "Codex CLI"
    }

    fn hint(&self) -> &'static str {
        "blocks protected files and risky commands, checks tests when Codex stops (hooks need a one-time trust in Codex)"
    }

    fn scopes(&self) -> &'static [Scope] {
        &[Scope::Project, Scope::User]
    }

    fn detect(&self, probe: &Probe) -> bool {
        probe.root.join(".codex").is_dir() || (probe.on_path)("codex")
    }

    fn parse(&self, event: &str, stdin: &str) -> Result<HookInput, ParseError> {
        let payload: Payload = serde_json::from_str(stdin)
            .map_err(|e| ParseError(format!("cannot read hook input as JSON: {e}")))?;
        let cwd = payload.cwd.clone().filter(|c| !c.is_empty());
        let event = match event {
            "pre-tool" => match payload.tool_name.as_deref().unwrap_or("") {
                "apply_patch" => match command_field(&payload.tool_input) {
                    Some(patch) => write_event(cwd.as_deref(), &patch),
                    None => HookEvent::Ignore,
                },
                "Bash" => match command_field(&payload.tool_input) {
                    Some(command) if is_apply_patch_command(&command) => {
                        match write_event(cwd.as_deref(), &command) {
                            HookEvent::Ignore => HookEvent::PreShell { command },
                            write => write,
                        }
                    }
                    Some(command) => HookEvent::PreShell { command },
                    None => HookEvent::Ignore,
                },
                _ => HookEvent::Ignore,
            },
            "stop" => HookEvent::TurnEnd {
                session_id: payload.session_id.unwrap_or_default(),
            },
            other => return Err(ParseError(format!("unknown codex hook event `{other}`"))),
        };
        Ok(HookInput {
            cwd: cwd.map(PathBuf::from),
            event,
        })
    }

    fn render(&self, event: &HookEvent, verdict: &Verdict) -> Rendered {
        let stdout = match (event, verdict) {
            (
                HookEvent::PreWrite { .. } | HookEvent::PreShell { .. },
                Verdict::Block { reason },
            ) => deny_reply(reason),
            (HookEvent::PreWrite { .. } | HookEvent::PreShell { .. }, Verdict::Ask { reason }) => {
                deny_reply(&format!(
                    "{reason} Codex hooks cannot ask for confirmation, so this was not run. \
                     Ask the user whether to go ahead; if they agree, they can run it themselves."
                ))
            }
            (HookEvent::TurnEnd { .. }, Verdict::RetryAgent { reason }) => {
                format!("{}\n", json!({ "decision": "block", "reason": reason }))
            }
            (HookEvent::TurnEnd { .. }, Verdict::AskHuman { reason }) => {
                format!("{}\n", json!({ "systemMessage": reason }))
            }
            (HookEvent::TurnEnd { .. }, Verdict::Notify { message }) => {
                format!("{}\n", json!({ "systemMessage": message }))
            }
            _ => String::new(),
        };
        Rendered {
            stdout,
            stderr: String::new(),
            exit_code: 0,
        }
    }

    fn files(&self, ctx: &PlanCtx) -> Result<Vec<PathBuf>, InstallError> {
        Ok(vec![hooks_path(ctx)?])
    }

    fn plan_install(&self, ctx: &PlanCtx, existing: &Files) -> Result<Plan, InstallError> {
        let path = hooks_path(ctx)?;
        let before = existing.get(&path);
        let mut root = parse_settings(&path, before)?;
        merge_hooks(&path, &mut root, &HOOK_SPECS)?;
        let after = render_settings(&root);
        let mut plan = Plan::default();
        if before.map(String::as_str) != Some(after.as_str()) {
            let mut change = FileChange::write(path, before, after);
            change.backup = true;
            plan.changes.push(change);
            plan.notes.push(TRUST_NOTE.to_string());
        }
        Ok(plan)
    }

    fn plan_uninstall(&self, ctx: &PlanCtx, existing: &Files) -> Result<Plan, InstallError> {
        let path = hooks_path(ctx)?;
        let mut plan = Plan::default();
        let Some(before) = existing.get(&path) else {
            return Ok(plan);
        };
        let mut root = parse_settings(&path, Some(before))?;
        if !remove_hooks(&path, &mut root)? {
            return Ok(plan);
        }
        if root.is_empty() {
            plan.changes.push(FileChange::delete(path, before));
        } else {
            plan.changes.push(FileChange::write(
                path,
                Some(before),
                render_settings(&root),
            ));
        }
        Ok(plan)
    }

    fn is_installed(&self, ctx: &PlanCtx, existing: &Files) -> bool {
        let Ok(path) = hooks_path(ctx) else {
            return false;
        };
        let Some(text) = existing.get(&path) else {
            return false;
        };
        let Ok(Value::Object(root)) = serde_json::from_str::<Value>(text) else {
            return false;
        };
        HOOK_SPECS.iter().all(|(event, ..)| {
            root.get("hooks")
                .and_then(|h| h.get(*event))
                .and_then(Value::as_array)
                .is_some_and(|groups| groups.iter().any(group_has_ours))
        })
    }
}

fn deny_reply(reason: &str) -> String {
    format!(
        "{}\n",
        json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": "deny",
                "permissionDecisionReason": reason,
            }
        })
    )
}

fn hooks_path(ctx: &PlanCtx) -> Result<PathBuf, InstallError> {
    let base = match ctx.scope {
        Scope::Project => ctx.env.root.clone(),
        Scope::User => ctx.env.home.clone().ok_or(InstallError::NoHome)?,
    };
    Ok(base.join(".codex").join("hooks.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patch_paths_cover_every_header_once() {
        let patch = "*** Begin Patch\n*** Add File: a.txt\n+x\n*** Update File: src/b.rs\n*** Move to: src/c.rs\n@@\n-a\n+b\n*** Delete File: d.txt\n*** Update File: a.txt\n*** End Patch\n";
        assert_eq!(
            patch_paths(patch),
            ["a.txt", "src/b.rs", "src/c.rs", "d.txt"]
        );
        // Added lines that look like headers are prefixed with `+` and are not headers.
        assert_eq!(
            patch_paths("*** Begin Patch\n*** Add File: a\n+*** Add File: .env\n"),
            ["a"]
        );
        assert!(patch_paths("nothing here").is_empty());
    }

    #[test]
    fn relative_paths_are_resolved_against_cwd() {
        assert_eq!(resolve(Some("/w/p/"), ".env".into()), "/w/p/.env");
        assert_eq!(resolve(Some("/w/p"), "/etc/x".into()), "/etc/x");
        assert_eq!(resolve(Some("C:\\w"), "D:\\x".into()), "D:\\x");
        assert_eq!(resolve(None, ".env".into()), ".env");
        assert_eq!(resolve(Some(""), ".env".into()), ".env");
    }

    #[test]
    fn apply_patch_typed_into_the_shell_is_recognised() {
        assert!(is_apply_patch_command(
            "apply_patch <<'EOF'\n*** Begin Patch\n*** Add File: a\n+x\n*** End Patch\nEOF"
        ));
        assert!(!is_apply_patch_command("echo apply_patch"));
        assert!(!is_apply_patch_command("apply_patch --help"));
    }
}
