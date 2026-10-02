//! pi, the coding agent from https://github.com/badlogic/pi-mono (docs:
//! `packages/coding-agent/docs/extensions.md`).
//!
//! Install writes one extension file, `integrations/pi/reflex.ts` (a short plain-JS
//! file with a `.ts` name, which pi loads through jiti without compiling), to
//! `.pi/extensions/reflex.ts` (project scope) or `~/.pi/agent/extensions/reflex.ts`
//! (user scope). pi only loads project extensions once you have trusted the project.
//! The extension talks to Reflex through the protocol in `js_bridge.rs`:
//!
//! * `tool_call` sends the tool name and input; `block` returns `{ block: true, reason }`.
//!   Tools: `write` and `edit` (`path`), `bash` and `powershell` (`command`). `ask` opens
//!   pi's own confirm dialog (refused without a UI).
//! * `agent_before_settle` is the turn end, and only for runs that completed. On `retry`
//!   the handler appends the failure as a `custom_message` entry and returns
//!   `continue: true`, which makes pi send one more model request; Reflex counts the
//!   retries, so the loop ends. On `notify` it shows a notification.
//!
//! pi blocks a tool whose `tool_call` handler throws, so the extension catches every
//! error and lets the call through.

use super::js_bridge;
use super::util::{self, ManagedFile};
use super::{AgentAdapter, HookEvent, HookInput, ParseError, Probe, Rendered};
use crate::config::Scope;
use crate::guard::Verdict;
use crate::install::{Files, InstallError, Plan, PlanCtx, HOOK_COMMAND_PREFIX};
use serde_json::Value;
use std::path::PathBuf;

pub const EXTENSION: &str = include_str!("../../../../integrations/pi/reflex.ts");

pub struct Pi;

/// Which tools write files or run commands, and where their path or command is.
fn classify(tool: &str, args: &Value) -> HookEvent {
    match tool {
        "write" | "edit" => match util::string_field(args, &["path"]) {
            Some(path) => HookEvent::PreWrite { paths: vec![path] },
            None => HookEvent::Ignore,
        },
        "bash" | "powershell" => match util::string_field(args, &["command"]) {
            Some(command) => HookEvent::PreShell { command },
            None => HookEvent::Ignore,
        },
        _ => HookEvent::Ignore,
    }
}

fn extension_path(ctx: &PlanCtx) -> Result<PathBuf, InstallError> {
    let dir = match ctx.scope {
        Scope::Project => ctx.env.root.join(".pi"),
        Scope::User => ctx
            .env
            .home
            .clone()
            .ok_or(InstallError::NoHome)?
            .join(".pi")
            .join("agent"),
    };
    Ok(dir.join("extensions").join("reflex.ts"))
}

impl AgentAdapter for Pi {
    fn id(&self) -> &'static str {
        "pi"
    }

    fn label(&self) -> &'static str {
        "pi"
    }

    fn hint(&self) -> &'static str {
        "blocks protected files and risky commands, checks tests when the agent finishes"
    }

    fn scopes(&self) -> &'static [Scope] {
        &[Scope::Project, Scope::User]
    }

    fn detect(&self, probe: &Probe) -> bool {
        probe.root.join(".pi").is_dir() || (probe.on_path)("pi")
    }

    fn parse(&self, event: &str, stdin: &str) -> Result<HookInput, ParseError> {
        js_bridge::parse("pi", event, stdin, classify)
    }

    fn render(&self, event: &HookEvent, verdict: &Verdict) -> Rendered {
        js_bridge::render(event, verdict)
    }

    fn files(&self, ctx: &PlanCtx) -> Result<Vec<PathBuf>, InstallError> {
        Ok(vec![extension_path(ctx)?])
    }

    fn plan_install(&self, ctx: &PlanCtx, existing: &Files) -> Result<Plan, InstallError> {
        let file = ManagedFile {
            path: extension_path(ctx)?,
            contents: EXTENSION.to_string(),
            executable: false,
        };
        let by_hand = format!(
            "To use Reflex Control there, move it aside and run install again, or call \
             `{HOOK_COMMAND_PREFIX} pi pre-tool` from it."
        );
        Ok(file.plan_install(existing, &by_hand))
    }

    fn plan_uninstall(&self, ctx: &PlanCtx, existing: &Files) -> Result<Plan, InstallError> {
        Ok(util::plan_uninstall(&extension_path(ctx)?, existing))
    }

    fn is_installed(&self, ctx: &PlanCtx, existing: &Files) -> bool {
        extension_path(ctx).is_ok_and(|path| util::is_installed(&path, existing))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn detection_looks_at_the_project_dir_and_the_binary() {
        let tmp = tempfile::tempdir().unwrap();
        let probe = |on_path: &dyn Fn(&str) -> bool, root: &std::path::Path| {
            Pi.detect(&Probe { root, on_path })
        };
        assert!(!probe(&|_| false, tmp.path()));
        assert!(probe(&|p| p == "pi", tmp.path()));
        std::fs::create_dir(tmp.path().join(".pi")).unwrap();
        assert!(probe(&|_| false, tmp.path()));
    }

    #[test]
    fn only_the_shell_and_file_writing_tools_are_checked() {
        let write = |p: &str| HookEvent::PreWrite {
            paths: vec![p.to_string()],
        };
        let shell = |c: &str| HookEvent::PreShell {
            command: c.to_string(),
        };
        assert_eq!(classify("write", &json!({"path": "a"})), write("a"));
        assert_eq!(
            classify("edit", &json!({"path": "a", "edits": []})),
            write("a")
        );
        assert_eq!(classify("bash", &json!({"command": "ls"})), shell("ls"));
        assert_eq!(
            classify("powershell", &json!({"command": "ls"})),
            shell("ls")
        );
        assert_eq!(classify("write", &json!({})), HookEvent::Ignore);
        assert_eq!(classify("read", &json!({"path": "a"})), HookEvent::Ignore);
    }
}
