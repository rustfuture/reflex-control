//! OpenCode (https://opencode.ai) and Kilo Code (https://kilo.ai), which is a fork of
//! OpenCode. Two ids, `opencode` and `kilo`, share this file: the plugin API and the
//! tools are the same, only the directories differ.
//!
//! Install writes one plugin file, `integrations/opencode/reflex.js` (a short plain-JS file
//! embedded in the binary), to the agent's plugin directory:
//!
//! | Agent    | Project scope                 | User scope                            |
//! |----------|-------------------------------|---------------------------------------|
//! | OpenCode | `.opencode/plugins/reflex.js` | `~/.config/opencode/plugins/reflex.js` |
//! | Kilo     | `.kilo/plugin/reflex.js`      | `~/.config/kilo/plugin/reflex.js`      |
//!
//! Both load every file in those directories at startup. The plugin default-exports
//! `{ id, server }`, the module shape OpenCode and Kilo document for local files. It
//! talks to Reflex through the protocol in `js_bridge.rs`:
//!
//! * `tool.execute.before` sends the tool name and arguments; a `block` reply throws,
//!   which refuses the call and shows the reason to the model. Tools: `write`, `edit`
//!   and `multiedit` (`filePath`), `apply_patch` and `patch` (`patchText`, paths taken
//!   from the patch headers) and `bash` (`command`).
//! * There is no confirmation dialog for plugins, so `ask` is answered by refusing the
//!   call once with "ask the user first"; the same call is let through the second time.
//! * The `session.idle` event is the turn end. On `retry` the plugin sends the failure
//!   to the session with `client.session.promptAsync`, so the agent keeps working;
//!   Reflex counts the retries. On `notify` it logs and shows a toast.
//!
//! Not run for subagent sessions (checked through `parentID`), for sessions whose last
//! run ended in an error, or while another check of the same session is running.

use super::js_bridge;
use super::util::{self, ManagedFile};
use super::{AgentAdapter, HookEvent, HookInput, ParseError, Probe, Rendered};
use crate::config::Scope;
use crate::guard::Verdict;
use crate::install::{Files, InstallError, Plan, PlanCtx, HOOK_COMMAND_PREFIX};
use serde_json::Value;
use std::path::PathBuf;

const PLUGIN: &str = include_str!("../../../../integrations/opencode/reflex.js");

/// The plugin as it is installed for the agent with this id.
pub fn plugin_source(agent_id: &str) -> String {
    PLUGIN.replace("__AGENT__", agent_id)
}

/// One of the two agents. `opencode::OPENCODE` and `opencode::KILO` go in the registry.
pub struct Flavor {
    id: &'static str,
    label: &'static str,
    /// Directory below the project root and below the home directory.
    project_dir: &'static [&'static str],
    user_dir: &'static [&'static str],
    detect: fn(&Probe) -> bool,
}

pub static OPENCODE: Flavor = Flavor {
    id: "opencode",
    label: "OpenCode",
    project_dir: &[".opencode", "plugins"],
    user_dir: &[".config", "opencode", "plugins"],
    detect: |probe| {
        probe.root.join(".opencode").is_dir()
            || probe.root.join("opencode.json").is_file()
            || probe.root.join("opencode.jsonc").is_file()
            || (probe.on_path)("opencode")
    },
};

pub static KILO: Flavor = Flavor {
    id: "kilo",
    label: "Kilo Code",
    project_dir: &[".kilo", "plugin"],
    user_dir: &[".config", "kilo", "plugin"],
    detect: |probe| {
        probe.root.join(".kilo").is_dir()
            || probe.root.join(".kilocode").is_dir()
            || (probe.on_path)("kilo")
    },
};

impl Flavor {
    fn plugin_path(&self, ctx: &PlanCtx) -> Result<PathBuf, InstallError> {
        let (base, dir) = match ctx.scope {
            Scope::Project => (ctx.env.root.clone(), self.project_dir),
            Scope::User => (
                ctx.env.home.clone().ok_or(InstallError::NoHome)?,
                self.user_dir,
            ),
        };
        Ok(dir
            .iter()
            .fold(base, |path, part| path.join(part))
            .join("reflex.js"))
    }
}

/// Which tools write files or run commands, and where their paths or command are.
fn classify(tool: &str, args: &Value) -> HookEvent {
    let paths = match tool {
        "write" | "edit" | "multiedit" => util::string_field(args, &["filePath"])
            .map(|p| vec![p])
            .unwrap_or_default(),
        "apply_patch" | "patch" => util::string_field(args, &["patchText"])
            .map(|patch| util::patch_paths(&patch))
            .unwrap_or_default(),
        "bash" => {
            return match util::string_field(args, &["command"]) {
                Some(command) => HookEvent::PreShell { command },
                None => HookEvent::Ignore,
            }
        }
        _ => Vec::new(),
    };
    if paths.is_empty() {
        HookEvent::Ignore
    } else {
        HookEvent::PreWrite { paths }
    }
}

impl AgentAdapter for Flavor {
    fn id(&self) -> &'static str {
        self.id
    }

    fn label(&self) -> &'static str {
        self.label
    }

    fn hint(&self) -> &'static str {
        "blocks protected files and risky commands, checks tests when a session goes idle"
    }

    fn scopes(&self) -> &'static [Scope] {
        &[Scope::Project, Scope::User]
    }

    fn detect(&self, probe: &Probe) -> bool {
        (self.detect)(probe)
    }

    fn parse(&self, event: &str, stdin: &str) -> Result<HookInput, ParseError> {
        js_bridge::parse(self.id, event, stdin, classify)
    }

    fn render(&self, event: &HookEvent, verdict: &Verdict) -> Rendered {
        js_bridge::render(event, verdict)
    }

    fn files(&self, ctx: &PlanCtx) -> Result<Vec<PathBuf>, InstallError> {
        Ok(vec![self.plugin_path(ctx)?])
    }

    fn plan_install(&self, ctx: &PlanCtx, existing: &Files) -> Result<Plan, InstallError> {
        let file = ManagedFile {
            path: self.plugin_path(ctx)?,
            contents: plugin_source(self.id),
            executable: false,
        };
        let by_hand = format!(
            "To use Reflex Control there, move it aside and run install again, or call \
             `{HOOK_COMMAND_PREFIX} {} pre-tool` from it.",
            self.id
        );
        Ok(file.plan_install(existing, &by_hand))
    }

    fn plan_uninstall(&self, ctx: &PlanCtx, existing: &Files) -> Result<Plan, InstallError> {
        Ok(util::plan_uninstall(&self.plugin_path(ctx)?, existing))
    }

    fn is_installed(&self, ctx: &PlanCtx, existing: &Files) -> bool {
        self.plugin_path(ctx)
            .is_ok_and(|path| util::is_installed(&path, existing))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detected(adapter: &dyn AgentAdapter, dirs: &[&str], files: &[&str], binary: &str) -> bool {
        let tmp = tempfile::tempdir().unwrap();
        for dir in dirs {
            std::fs::create_dir(tmp.path().join(dir)).unwrap();
        }
        for file in files {
            std::fs::write(tmp.path().join(file), "{}").unwrap();
        }
        let on_path = |p: &str| p == binary;
        adapter.detect(&Probe {
            root: tmp.path(),
            on_path: &on_path,
        })
    }

    #[test]
    fn detection_looks_at_each_agents_own_files_and_binary() {
        assert!(!detected(&OPENCODE, &[], &[], ""));
        assert!(detected(&OPENCODE, &[".opencode"], &[], ""));
        assert!(detected(&OPENCODE, &[], &["opencode.json"], ""));
        assert!(detected(&OPENCODE, &[], &[], "opencode"));
        assert!(!detected(&OPENCODE, &[".kilo"], &[], "kilo"));

        assert!(!detected(&KILO, &[], &[], ""));
        assert!(detected(&KILO, &[".kilo"], &[], ""));
        assert!(detected(&KILO, &[".kilocode"], &[], ""));
        assert!(detected(&KILO, &[], &[], "kilo"));
        assert!(!detected(
            &KILO,
            &[".opencode"],
            &["opencode.json"],
            "opencode"
        ));
    }

    #[test]
    fn only_the_shell_and_file_writing_tools_are_checked() {
        let args = |v: Value| v;
        let write = |paths: &[&str]| HookEvent::PreWrite {
            paths: paths.iter().map(|p| p.to_string()).collect(),
        };
        let cases = [
            (
                "write",
                args(serde_json::json!({"filePath": "a"})),
                write(&["a"]),
            ),
            (
                "edit",
                args(serde_json::json!({"filePath": "a"})),
                write(&["a"]),
            ),
            (
                "multiedit",
                args(serde_json::json!({"filePath": "a"})),
                write(&["a"]),
            ),
            (
                "apply_patch",
                args(
                    serde_json::json!({"patchText": "*** Begin Patch\n*** Add File: x\n*** Delete File: y\n*** End Patch"}),
                ),
                write(&["x", "y"]),
            ),
            (
                "bash",
                args(serde_json::json!({"command": "ls"})),
                HookEvent::PreShell {
                    command: "ls".into(),
                },
            ),
            ("bash", args(serde_json::json!({})), HookEvent::Ignore),
            (
                "write",
                args(serde_json::json!({"path": "a"})),
                HookEvent::Ignore,
            ),
            (
                "apply_patch",
                args(serde_json::json!({"patchText": "nothing"})),
                HookEvent::Ignore,
            ),
            (
                "read",
                args(serde_json::json!({"filePath": "a"})),
                HookEvent::Ignore,
            ),
            ("task", Value::Null, HookEvent::Ignore),
        ];
        for (tool, args, expected) in cases {
            assert_eq!(classify(tool, &args), expected, "{tool} {args}");
        }
    }
}
