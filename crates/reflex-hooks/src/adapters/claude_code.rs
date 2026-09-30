//! Claude Code adapter. Hook reference: https://code.claude.com/docs/en/hooks
//!
//! Install adds two hooks to `.claude/settings.json` (project scope) or
//! `~/.claude/settings.json` (user scope):
//!
//! * `PreToolUse` on `Edit|Write|MultiEdit|NotebookEdit|Bash`, running
//!   `reflex hook claude-code pre-tool` (10 s timeout)
//! * `Stop`, running `reflex hook claude-code stop` (600 s timeout, since it can run tests)
//!
//! Replies are always exit code 0 with JSON on stdout (or nothing to allow):
//!
//! | Verdict        | PreToolUse                                   | Stop                                |
//! |----------------|----------------------------------------------|-------------------------------------|
//! | Allow          | empty                                        | empty                               |
//! | Block          | `permissionDecision: "deny"` + reason        | (not produced)                      |
//! | Ask            | `permissionDecision: "ask"` + reason         | (not produced)                      |
//! | RetryAgent     | (not produced)                               | `decision: "block"` + reason        |
//! | AskHuman/Notify| (not produced)                               | `systemMessage` (stop is allowed)   |
//!
//! Claude Code shows a `Stop` block reason to the model and a `systemMessage` to the
//! user. It also limits itself to a fixed number of consecutive Stop blocks; our own
//! retry budget (`tests.max_retries`) is normally reached well before that.

use super::{AgentAdapter, HookEvent, HookInput, ParseError, Probe, Rendered};
use crate::config::Scope;
use crate::guard::Verdict;
use crate::install::{FileChange, Files, InstallError, Plan, PlanCtx, HOOK_COMMAND_PREFIX};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::path::PathBuf;

pub struct ClaudeCode;

const PRE_TOOL_MATCHER: &str = "Edit|Write|MultiEdit|NotebookEdit|Bash";
const PRE_TOOL_COMMAND: &str = "reflex hook claude-code pre-tool";
const STOP_COMMAND: &str = "reflex hook claude-code stop";

/// One hook to install: event, matcher (if the event takes one), command, timeout in seconds.
pub(super) type HookSpec = (&'static str, Option<&'static str>, &'static str, u64);

const HOOK_SPECS: [HookSpec; 2] = [
    ("PreToolUse", Some(PRE_TOOL_MATCHER), PRE_TOOL_COMMAND, 10),
    ("Stop", None, STOP_COMMAND, 600),
];

/// Fields Claude Code sends that we look at. Everything else is ignored.
#[derive(Debug, Deserialize)]
struct Payload {
    cwd: Option<String>,
    session_id: Option<String>,
    tool_name: Option<String>,
    tool_input: Option<Value>,
}

fn string_field(input: &Option<Value>, key: &str) -> Option<String> {
    input
        .as_ref()?
        .get(key)?
        .as_str()
        .map(str::to_string)
        .filter(|s| !s.is_empty())
}

impl AgentAdapter for ClaudeCode {
    fn id(&self) -> &'static str {
        "claude-code"
    }

    fn label(&self) -> &'static str {
        "Claude Code"
    }

    fn hint(&self) -> &'static str {
        "blocks protected files and risky commands, checks tests when Claude stops"
    }

    fn scopes(&self) -> &'static [Scope] {
        &[Scope::Project, Scope::User]
    }

    fn detect(&self, probe: &Probe) -> bool {
        probe.root.join(".claude").is_dir() || (probe.on_path)("claude")
    }

    fn parse(&self, event: &str, stdin: &str) -> Result<HookInput, ParseError> {
        let payload: Payload = serde_json::from_str(stdin)
            .map_err(|e| ParseError(format!("cannot read hook input as JSON: {e}")))?;
        let cwd = payload.cwd.clone().map(PathBuf::from);
        let event = match event {
            "pre-tool" => {
                let tool = payload.tool_name.as_deref().unwrap_or("");
                match tool {
                    "Edit" | "Write" | "MultiEdit" => {
                        match string_field(&payload.tool_input, "file_path") {
                            Some(p) => HookEvent::PreWrite { paths: vec![p] },
                            None => HookEvent::Ignore,
                        }
                    }
                    "NotebookEdit" => match string_field(&payload.tool_input, "notebook_path") {
                        Some(p) => HookEvent::PreWrite { paths: vec![p] },
                        None => HookEvent::Ignore,
                    },
                    "Bash" => match string_field(&payload.tool_input, "command") {
                        Some(command) => HookEvent::PreShell { command },
                        None => HookEvent::Ignore,
                    },
                    _ => HookEvent::Ignore,
                }
            }
            "stop" => HookEvent::TurnEnd {
                session_id: payload.session_id.unwrap_or_default(),
            },
            other => {
                return Err(ParseError(format!(
                    "unknown claude-code hook event `{other}`"
                )))
            }
        };
        Ok(HookInput { cwd, event })
    }

    fn render(&self, event: &HookEvent, verdict: &Verdict) -> Rendered {
        let stdout = match (event, verdict) {
            (
                HookEvent::PreWrite { .. } | HookEvent::PreShell { .. },
                Verdict::Block { reason },
            ) => pre_tool_reply("deny", reason),
            (HookEvent::PreWrite { .. } | HookEvent::PreShell { .. }, Verdict::Ask { reason }) => {
                pre_tool_reply("ask", reason)
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
        Ok(vec![settings_path(ctx)?])
    }

    fn plan_install(&self, ctx: &PlanCtx, existing: &Files) -> Result<Plan, InstallError> {
        let path = settings_path(ctx)?;
        let before = existing.get(&path);
        let mut root = parse_settings(&path, before)?;
        merge_hooks(&path, &mut root, &HOOK_SPECS)?;
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
        let path = settings_path(ctx)?;
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
        let Ok(path) = settings_path(ctx) else {
            return false;
        };
        let Some(text) = existing.get(&path) else {
            return false;
        };
        let Ok(Value::Object(root)) = serde_json::from_str::<Value>(text) else {
            return false;
        };
        ["PreToolUse", "Stop"].iter().all(|event| {
            root.get("hooks")
                .and_then(|h| h.get(*event))
                .and_then(Value::as_array)
                .is_some_and(|groups| groups.iter().any(group_has_ours))
        })
    }
}

fn pre_tool_reply(decision: &str, reason: &str) -> String {
    format!(
        "{}\n",
        json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": decision,
                "permissionDecisionReason": reason,
            }
        })
    )
}

fn settings_path(ctx: &PlanCtx) -> Result<PathBuf, InstallError> {
    let base = match ctx.scope {
        Scope::Project => ctx.env.root.clone(),
        Scope::User => ctx.env.home.clone().ok_or(InstallError::NoHome)?,
    };
    Ok(base.join(".claude").join("settings.json"))
}

pub(super) fn parse_settings(
    path: &std::path::Path,
    text: Option<&String>,
) -> Result<Map<String, Value>, InstallError> {
    let Some(text) = text.filter(|t| !t.trim().is_empty()) else {
        return Ok(Map::new());
    };
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(_) => Err(InstallError::UnexpectedShape {
            path: path.to_path_buf(),
            message: "expected a JSON object at the top level".to_string(),
        }),
        Err(e) => Err(InstallError::InvalidJson {
            path: path.to_path_buf(),
            message: e.to_string(),
        }),
    }
}

pub(super) fn render_settings(root: &Map<String, Value>) -> String {
    let mut text = serde_json::to_string_pretty(root).expect("a JSON map always serializes");
    text.push('\n');
    text
}

pub(super) fn is_our_command(hook: &Value) -> bool {
    hook.get("command")
        .and_then(Value::as_str)
        .is_some_and(|c| c.trim_start().starts_with(HOOK_COMMAND_PREFIX))
}

pub(super) fn group_has_ours(group: &Value) -> bool {
    group
        .get("hooks")
        .and_then(Value::as_array)
        .is_some_and(|hooks| hooks.iter().any(is_our_command))
}

pub(super) fn shape_err(path: &std::path::Path, what: &str) -> InstallError {
    InstallError::UnexpectedShape {
        path: path.to_path_buf(),
        message: format!("{what} has an unexpected type; fix it and run install again"),
    }
}

/// Adds or updates our hooks, keeping everything else. The `hooks` object of Codex's
/// `hooks.json` has the same shape, so its adapter shares this.
pub(super) fn merge_hooks(
    path: &std::path::Path,
    root: &mut Map<String, Value>,
    specs: &[HookSpec],
) -> Result<(), InstallError> {
    let hooks = root
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| shape_err(path, "\"hooks\""))?;

    for &(event, matcher, command, timeout) in specs {
        let groups = hooks
            .entry(event)
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .ok_or_else(|| shape_err(path, &format!("\"hooks.{event}\"")))?;

        let mut placed = false;
        // Groups that only held duplicates of our hook and are now empty.
        let mut emptied = Vec::new();
        for (gi, group) in groups.iter_mut().enumerate() {
            let matcher_target = group.as_object().is_some();
            let Some(list) = group.get_mut("hooks").and_then(Value::as_array_mut) else {
                continue;
            };
            let alone = list.iter().all(is_our_command);
            let mut removed = false;
            let mut updated = false;
            // Update the first of our entries in place; drop any further duplicates.
            let mut i = 0;
            while i < list.len() {
                if !is_our_command(&list[i]) {
                    i += 1;
                } else if placed {
                    list.remove(i);
                    removed = true;
                } else {
                    let obj = list[i].as_object_mut().expect("checked by is_our_command");
                    obj.insert("type".into(), json!("command"));
                    obj.insert("command".into(), json!(command));
                    obj.insert("timeout".into(), json!(timeout));
                    placed = true;
                    updated = alone && matcher_target;
                    i += 1;
                }
            }
            if removed && list.is_empty() {
                emptied.push(gi);
            }
            if updated {
                if let (Some(m), Some(g)) = (matcher, group.as_object_mut()) {
                    g.insert("matcher".into(), json!(m));
                }
            }
        }
        for gi in emptied.into_iter().rev() {
            groups.remove(gi);
        }
        if !placed {
            let mut group = Map::new();
            if let Some(m) = matcher {
                group.insert("matcher".into(), json!(m));
            }
            group.insert(
                "hooks".into(),
                json!([{ "type": "command", "command": command, "timeout": timeout }]),
            );
            groups.push(Value::Object(group));
        }
    }
    Ok(())
}

/// Removes our hooks and anything left empty by that. Returns whether anything changed.
pub(super) fn remove_hooks(
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
        let Some(groups) = hooks.get_mut(&event).and_then(Value::as_array_mut) else {
            continue;
        };
        let mut event_changed = false;
        for group in groups.iter_mut() {
            if let Some(list) = group.get_mut("hooks").and_then(Value::as_array_mut) {
                let n = list.len();
                list.retain(|h| !is_our_command(h));
                event_changed |= list.len() != n;
            }
        }
        if !event_changed {
            continue;
        }
        changed = true;
        // Drop groups we emptied. A group the user left empty on purpose is not ours
        // to remove, but one that held only our hook is.
        groups.retain(|g| {
            g.get("hooks")
                .and_then(Value::as_array)
                .is_none_or(|l| !l.is_empty())
        });
        if groups.is_empty() {
            hooks.shift_remove(&event);
        }
    }
    if hooks.is_empty() && changed {
        root.shift_remove("hooks");
    }
    Ok(changed)
}
