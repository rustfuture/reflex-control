//! The JSON that passes between `reflex hook` and the JS plugins of OpenCode, Kilo Code
//! and pi. Those agents run plugin code inside their own process instead of calling a
//! command with the agent's JSON, so the plugin is a thin relay and Rust stays the
//! decision maker.
//!
//! The plugin runs `reflex hook <agent> <event>` and writes one JSON object to stdin:
//!
//! ```text
//! pre-tool: {"cwd": "/work/proj", "session_id": "s1", "tool": "write", "args": {...}}
//! turn-end: {"cwd": "/work/proj", "session_id": "s1"}
//! ```
//!
//! `tool` and `args` are the agent's own tool name and arguments; the adapter decides
//! which of them write files or run commands. Reflex answers with one JSON object on
//! stdout and exit code 0:
//!
//! | Verdict         | Reply                                        | The plugin                           |
//! |-----------------|----------------------------------------------|--------------------------------------|
//! | Allow           | `{"action":"allow"}`                         | does nothing                         |
//! | Block           | `{"action":"block","reason":"..."}`          | refuses the tool call                |
//! | Ask             | `{"action":"ask","reason":"..."}`            | asks the user, or refuses            |
//! | RetryAgent      | `{"action":"retry","reason":"..."}`          | sends the agent back to work         |
//! | AskHuman/Notify | `{"action":"notify","reason":"..."}`         | shows the message, lets the agent stop |
//!
//! If reflex is missing, times out, or prints anything that is not a reply of this
//! shape (a warning on stderr with empty stdout is what a malformed request produces),
//! the plugin allows.

use super::util::resolve_paths;
use super::{HookEvent, HookInput, ParseError, Rendered};
use crate::guard::Verdict;
use serde_json::{json, Value};
use std::path::PathBuf;

/// Turns the agent's tool name and arguments into the event the guard checks.
pub type Classify = fn(tool: &str, args: &Value) -> HookEvent;

pub fn parse(
    agent: &str,
    event: &str,
    stdin: &str,
    classify: Classify,
) -> Result<HookInput, ParseError> {
    let request: Value = serde_json::from_str(stdin)
        .map_err(|e| ParseError(format!("cannot read hook input as JSON: {e}")))?;
    let text = |key: &str| request.get(key).and_then(Value::as_str);
    let cwd = text("cwd").filter(|c| !c.is_empty()).map(PathBuf::from);
    let event = match event {
        "pre-tool" => resolve_paths(
            classify(
                text("tool").unwrap_or(""),
                request.get("args").unwrap_or(&Value::Null),
            ),
            text("cwd"),
        ),
        "turn-end" => HookEvent::TurnEnd {
            session_id: text("session_id").unwrap_or("").to_string(),
        },
        other => return Err(ParseError(format!("unknown {agent} hook event `{other}`"))),
    };
    Ok(HookInput { cwd, event })
}

pub fn render(event: &HookEvent, verdict: &Verdict) -> Rendered {
    let is_tool = matches!(
        event,
        HookEvent::PreWrite { .. } | HookEvent::PreShell { .. }
    );
    let reply = match verdict {
        Verdict::Block { reason } if is_tool => json!({ "action": "block", "reason": reason }),
        Verdict::Ask { reason } if is_tool => json!({ "action": "ask", "reason": reason }),
        Verdict::RetryAgent { reason } if !is_tool => {
            json!({ "action": "retry", "reason": reason })
        }
        Verdict::AskHuman { reason } if !is_tool => {
            json!({ "action": "notify", "reason": reason })
        }
        Verdict::Notify { message } if !is_tool => {
            json!({ "action": "notify", "reason": message })
        }
        _ => json!({ "action": "allow" }),
    };
    Rendered {
        stdout: format!("{reply}\n"),
        stderr: String::new(),
        exit_code: 0,
    }
}
