//! Cursor adapter. Hook reference: https://cursor.com/docs/hooks
//!
//! The facts below come from secondary sources; the official page could not be read when
//! this was written (see docs/agent-hooks.md for what is and is not verified).
//!
//! Install adds three hooks to `.cursor/hooks.json` (project scope) or
//! `~/.cursor/hooks.json` (user scope), whose shape is
//! `{"version": 1, "hooks": {"<event>": [{"command": ..., "matcher": ..., "timeout": ...}]}}`:
//!
//! * `preToolUse` on `Write|Delete`, running `reflex hook cursor pre-tool` (10 s)
//! * `beforeShellExecution`, running `reflex hook cursor shell` (10 s)
//! * `stop`, running `reflex hook cursor stop` (600 s, since it can run tests)
//!
//! Shell commands go through `beforeShellExecution`, which can allow, deny or ask, so the
//! `Shell` tool is left out of the `preToolUse` matcher and ignored there: the user would
//! be asked twice otherwise.
//!
//! | Verdict        | preToolUse / beforeShellExecution                | stop                          |
//! |----------------|--------------------------------------------------|-------------------------------|
//! | Allow          | empty                                            | empty                         |
//! | Block          | `permission: "deny"` + messages, exit code 2     | (not produced)                |
//! | Ask            | `permission: "ask"` + messages (shell only)      | (not produced)                |
//! | RetryAgent     | (not produced)                                   | `followup_message`            |
//! | AskHuman/Notify| (not produced)                                   | nothing on stdout, text on stderr |
//!
//! A block writes the JSON and also exits with code 2, which Cursor treats as a deny on
//! its own, so the block holds even if the JSON is not understood. The reason goes to
//! stderr as well. Cursor lets an action through when a hook crashes or times out, which
//! matches how every adapter fails.
//!
//! `followup_message` makes Cursor submit a new user message, which is how a failed test
//! run sends the agent back to work; Cursor caps the number of these itself (`loop_limit`).
//! There is no documented way to show the user a message when the agent is allowed to
//! stop, so those verdicts are written to stderr, where Cursor logs hook output.

use super::claude_code::{is_our_command, parse_settings, render_settings, shape_err};
use super::{AgentAdapter, HookEvent, HookInput, ParseError, Probe, Rendered};
use crate::config::Scope;
use crate::guard::Verdict;
use crate::install::{FileChange, Files, InstallError, Plan, PlanCtx};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::path::PathBuf;

pub struct Cursor;

/// One hook to install: event, matcher (if any), command, timeout in seconds.
type Spec = (&'static str, Option<&'static str>, &'static str, u64);

const HOOK_SPECS: [Spec; 3] = [
    (
        "preToolUse",
        Some("Write|Delete"),
        "reflex hook cursor pre-tool",
        10,
    ),
    ("beforeShellExecution", None, "reflex hook cursor shell", 10),
    ("stop", None, "reflex hook cursor stop", 600),
];

/// Keys that may hold the path in a `Write` or `Delete` tool call, in the order tried.
/// The shape of `tool_input` for these tools is not documented.
const PATH_KEYS: [&str; 4] = ["file_path", "path", "filePath", "target_file"];

/// Fields Cursor sends that we look at. Everything else is ignored.
#[derive(Debug, Deserialize)]
struct Payload {
    cwd: Option<String>,
    workspace_roots: Option<Vec<String>>,
    conversation_id: Option<String>,
    tool_name: Option<String>,
    tool_input: Option<Value>,
    command: Option<String>,
}

fn tool_path(input: &Option<Value>) -> Option<String> {
    let input = input.as_ref()?;
    PATH_KEYS.iter().find_map(|key| {
        input
            .get(*key)?
            .as_str()
            .map(str::to_string)
            .filter(|s| !s.is_empty())
    })
}

impl AgentAdapter for Cursor {
    fn id(&self) -> &'static str {
        "cursor"
    }

    fn label(&self) -> &'static str {
        "Cursor"
    }

    fn hint(&self) -> &'static str {
        "blocks protected files and risky shell commands, checks tests when the agent stops"
    }

    fn scopes(&self) -> &'static [Scope] {
        &[Scope::Project, Scope::User]
    }

    fn detect(&self, probe: &Probe) -> bool {
        probe.root.join(".cursor").is_dir()
            || (probe.on_path)("cursor")
            || (probe.on_path)("cursor-agent")
    }

    fn parse(&self, event: &str, stdin: &str) -> Result<HookInput, ParseError> {
        let payload: Payload = serde_json::from_str(stdin)
            .map_err(|e| ParseError(format!("cannot read hook input as JSON: {e}")))?;
        let cwd = payload
            .cwd
            .clone()
            .filter(|c| !c.is_empty())
            .or_else(|| {
                payload
                    .workspace_roots
                    .as_ref()
                    .and_then(|roots| roots.first().cloned())
            })
            .filter(|c| !c.is_empty())
            .map(PathBuf::from);
        let event = match event {
            "pre-tool" => match payload.tool_name.as_deref().unwrap_or("") {
                "Write" | "Delete" => match tool_path(&payload.tool_input) {
                    Some(p) => HookEvent::PreWrite { paths: vec![p] },
                    None => HookEvent::Ignore,
                },
                // Shell has its own event; everything else does not write files.
                _ => HookEvent::Ignore,
            },
            "shell" => match payload.command.filter(|c| !c.trim().is_empty()) {
                Some(command) => HookEvent::PreShell { command },
                None => HookEvent::Ignore,
            },
            "stop" => HookEvent::TurnEnd {
                session_id: payload.conversation_id.unwrap_or_default(),
            },
            other => return Err(ParseError(format!("unknown cursor hook event `{other}`"))),
        };
        Ok(HookInput { cwd, event })
    }

    fn render(&self, event: &HookEvent, verdict: &Verdict) -> Rendered {
        let mut out = Rendered::default();
        match (event, verdict) {
            (
                HookEvent::PreWrite { .. } | HookEvent::PreShell { .. },
                Verdict::Block { reason },
            )
            | (HookEvent::PreWrite { .. }, Verdict::Ask { reason }) => {
                out.stdout = permission_reply("deny", reason);
                out.stderr = format!("{reason}\n");
                out.exit_code = 2;
            }
            (HookEvent::PreShell { .. }, Verdict::Ask { reason }) => {
                out.stdout = permission_reply("ask", reason);
            }
            (HookEvent::TurnEnd { .. }, Verdict::RetryAgent { reason }) => {
                out.stdout = format!("{}\n", json!({ "followup_message": reason }));
            }
            (HookEvent::TurnEnd { .. }, Verdict::AskHuman { reason }) => {
                out.stderr = format!("{reason}\n");
            }
            (HookEvent::TurnEnd { .. }, Verdict::Notify { message }) => {
                out.stderr = format!("{message}\n");
            }
            _ => {}
        }
        out
    }

    fn files(&self, ctx: &PlanCtx) -> Result<Vec<PathBuf>, InstallError> {
        Ok(vec![hooks_path(ctx)?])
    }

    fn plan_install(&self, ctx: &PlanCtx, existing: &Files) -> Result<Plan, InstallError> {
        let path = hooks_path(ctx)?;
        let before = existing.get(&path);
        let mut root = parse_settings(&path, before)?;
        merge_hooks(&path, &mut root)?;
        let after = render_settings(&root);
        let mut plan = Plan::default();
        if before.map(String::as_str) != Some(after.as_str()) {
            let mut change = FileChange::write(path, before, after);
            change.backup = true;
            plan.changes.push(change);
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
        // A file that only has the `version` key we may have added holds nothing of the
        // user's, so it goes away with our hooks.
        if root.keys().all(|k| k == "version") {
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
                .is_some_and(|entries| entries.iter().any(is_our_command))
        })
    }
}

/// Reply for the events that can allow, deny or ask. Cursor shows `user_message` to the
/// user and sends `agent_message` to the agent.
fn permission_reply(permission: &str, reason: &str) -> String {
    format!(
        "{}\n",
        json!({
            "permission": permission,
            "user_message": reason,
            "agent_message": reason,
        })
    )
}

fn hooks_path(ctx: &PlanCtx) -> Result<PathBuf, InstallError> {
    let base = match ctx.scope {
        Scope::Project => ctx.env.root.clone(),
        Scope::User => ctx.env.home.clone().ok_or(InstallError::NoHome)?,
    };
    Ok(base.join(".cursor").join("hooks.json"))
}

/// Adds or updates our hooks and the `version` key, keeping everything else. Unlike
/// Claude Code, Cursor's event lists hold the hook entries directly, with no groups.
fn merge_hooks(path: &std::path::Path, root: &mut Map<String, Value>) -> Result<(), InstallError> {
    if !root.contains_key("version") {
        root.insert("version".into(), json!(1));
    }
    let hooks = root
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| shape_err(path, "\"hooks\""))?;

    for (event, matcher, command, timeout) in HOOK_SPECS {
        let entries = hooks
            .entry(event)
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .ok_or_else(|| shape_err(path, &format!("\"hooks.{event}\"")))?;

        // Update the first of our entries in place and drop any further duplicates.
        let mut placed = false;
        entries.retain_mut(|entry| {
            if !is_our_command(entry) {
                return true;
            }
            if placed {
                return false;
            }
            placed = true;
            let obj = entry.as_object_mut().expect("checked by is_our_command");
            obj.insert("command".into(), json!(command));
            match matcher {
                Some(m) => obj.insert("matcher".into(), json!(m)),
                None => obj.shift_remove("matcher"),
            };
            obj.insert("timeout".into(), json!(timeout));
            true
        });
        if !placed {
            let mut entry = Map::new();
            entry.insert("command".into(), json!(command));
            if let Some(m) = matcher {
                entry.insert("matcher".into(), json!(m));
            }
            entry.insert("timeout".into(), json!(timeout));
            entries.push(Value::Object(entry));
        }
    }
    Ok(())
}

/// Removes our hooks and the event lists that leaves empty. Returns whether anything changed.
fn remove_hooks(
    path: &std::path::Path,
    root: &mut Map<String, Value>,
) -> Result<bool, InstallError> {
    let Some(hooks) = root.get_mut("hooks") else {
        return Ok(false);
    };
    let hooks = hooks
        .as_object_mut()
        .ok_or_else(|| shape_err(path, "\"hooks\""))?;
    let mut changed = false;
    let events: Vec<String> = hooks.keys().cloned().collect();
    for event in events {
        let Some(entries) = hooks.get_mut(&event).and_then(Value::as_array_mut) else {
            continue;
        };
        let n = entries.len();
        entries.retain(|e| !is_our_command(e));
        if entries.len() != n {
            changed = true;
            if entries.is_empty() {
                hooks.shift_remove(&event);
            }
        }
    }
    if hooks.is_empty() && changed {
        root.shift_remove("hooks");
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_is_found_under_any_of_the_known_keys() {
        for key in PATH_KEYS {
            let input = Some(json!({ key: "/w/a.txt", "contents": "x" }));
            assert_eq!(tool_path(&input).as_deref(), Some("/w/a.txt"), "{key}");
        }
        assert_eq!(tool_path(&Some(json!({ "file_path": "" }))), None);
        assert_eq!(tool_path(&Some(json!("just a string"))), None);
        assert_eq!(tool_path(&None), None);
    }

    #[test]
    fn cwd_falls_back_to_the_first_workspace_root() {
        let a = Cursor
            .parse(
                "stop",
                r#"{"conversation_id":"c1","workspace_roots":["/w/p","/w/q"]}"#,
            )
            .unwrap();
        assert_eq!(a.cwd, Some(PathBuf::from("/w/p")));
        assert_eq!(
            a.event,
            HookEvent::TurnEnd {
                session_id: "c1".into()
            }
        );
        let b = Cursor.parse("stop", r#"{"cwd":"/w/x"}"#).unwrap();
        assert_eq!(b.cwd, Some(PathBuf::from("/w/x")));
        assert_eq!(Cursor.parse("stop", "{}").unwrap().cwd, None);
    }
}
