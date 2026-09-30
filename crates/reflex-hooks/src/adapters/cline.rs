//! Cline (https://cline.bot): the VS Code extension and the CLI, which share one hook
//! format. Hook reference: `docs/customization/hooks.mdx` and `sdk/examples/hooks/` in
//! https://github.com/cline/cline.
//!
//! Cline runs an executable file named after the event, gives it the event as JSON on
//! stdin and reads a JSON reply from stdout. Install writes one such file, `PreToolUse`,
//! that calls `reflex hook cline pre-tool`:
//!
//! | Scope   | Unix                                   | Windows                                      |
//! |---------|----------------------------------------|----------------------------------------------|
//! | Project | `.clinerules/hooks/PreToolUse`         | `.clinerules/hooks/PreToolUse.ps1`           |
//! | User    | `~/Documents/Cline/Hooks/PreToolUse`   | `~/Documents/Cline/Hooks/PreToolUse.ps1`     |
//!
//! Both the extension and the CLI read these directories (the CLI also reads
//! `.cline/hooks` and `~/.cline/hooks`, which are not used so that the hook does not
//! run twice). The script is `integrations/cline/PreToolUse` (sh) or `PreToolUse.ps1`;
//! the Windows one is untested, because the extension launches it through PowerShell.
//!
//! The tool call comes in one of two shapes: the CLI sends `tool_call.name` and
//! `tool_call.input`; the extension sends only `preToolUse.toolName` and
//! `preToolUse.parameters`, a map whose non-string values are JSON-encoded. Both are read.
//! Tools: `editor` (and the older `write_to_file`, `replace_in_file`, `delete_file`) with
//! `path`; `apply_patch`, whose input is the patch text, or `{ "input": patch }`;
//! `run_commands` (and the older `execute_command`), whose input is a string, a list of
//! strings or `{command, args}` entries, or an object holding `commands`, `command` or
//! `cmd`.
//!
//! | Verdict | Reply                                                 |
//! |---------|-------------------------------------------------------|
//! | Allow   | `{}`                                                  |
//! | Block   | `{"cancel":true,"errorMessage":"..."}`                |
//! | Ask     | the same, with a note that Cline cannot ask           |
//!
//! `cancel` stops the tool call and ends the current task run, and the reason is shown.
//! Cline has no way for a hook to ask for confirmation: the reply's `review` field is
//! read by the CLI's hook runner but not acted on, and the extension's reply format has
//! no such field. So an `Ask` also cancels.
//!
//! There is no turn-end hook: `TaskComplete` runs after the task has finished, and its
//! reply is ignored, so tests cannot send the agent back to work. Use the git pre-commit
//! hook for that.
//!
//! Hooks must be switched on: tick "Enable Hooks" in the extension's settings; the CLI
//! ignores hooks in `--yolo` mode. A file that is not ours at the same path is left
//! alone, because Cline runs one file per event.

use super::util::{self, ManagedFile};
use super::{AgentAdapter, HookEvent, HookInput, ParseError, Probe, Rendered};
use crate::config::Scope;
use crate::guard::Verdict;
use crate::install::{Files, InstallError, Plan, PlanCtx, HOOK_COMMAND_PREFIX};
use serde_json::{json, Value};
use std::path::PathBuf;

pub struct Cline;

pub const SCRIPT_UNIX: &str = include_str!("../../../../integrations/cline/PreToolUse");
pub const SCRIPT_WINDOWS: &str = include_str!("../../../../integrations/cline/PreToolUse.ps1");

const ENABLE_NOTE: &str = "Cline only runs hooks that are switched on: in VS Code, tick \"Enable \
    Hooks\" in Cline's settings (Feature Settings); the Cline CLI skips hooks with --yolo. Cline \
    has no hook that can send the agent back when tests fail, so only the checks before a tool \
    call are installed.";

const ASK_NOTE: &str = "Cline hooks cannot ask for confirmation, so the call was cancelled. \
    Run it yourself if you want it.";

/// The script for the platform Reflex is running on.
fn script() -> &'static str {
    if cfg!(windows) {
        SCRIPT_WINDOWS
    } else {
        SCRIPT_UNIX
    }
}

fn hook_path(ctx: &PlanCtx) -> Result<PathBuf, InstallError> {
    let dir = match ctx.scope {
        Scope::Project => ctx.env.root.join(".clinerules").join("hooks"),
        Scope::User => ctx
            .env
            .home
            .clone()
            .ok_or(InstallError::NoHome)?
            .join("Documents")
            .join("Cline")
            .join("Hooks"),
    };
    Ok(dir.join(if cfg!(windows) {
        "PreToolUse.ps1"
    } else {
        "PreToolUse"
    }))
}

/// Parses `value` if it is a string holding a JSON object or array, which is how the
/// extension passes non-string parameters.
fn dejson(value: &Value) -> Value {
    if let Value::String(text) = value {
        if let Ok(parsed @ (Value::Object(_) | Value::Array(_))) = serde_json::from_str(text) {
            return parsed;
        }
    }
    value.clone()
}

/// A word for a shell command line, quoted if it needs to be.
fn shell_word(word: &str) -> String {
    let plain = !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./=:@%+,".contains(c));
    if plain {
        word.to_string()
    } else {
        format!("'{}'", word.replace('\'', "'\\''"))
    }
}

/// One command as a string: a plain string, `{command}` or `{cmd}` (a shell command line),
/// or `{command, args}`, which Cline runs directly without a shell.
fn command_entry(entry: &Value) -> Option<String> {
    match entry {
        Value::String(s) => Some(s.clone()),
        Value::Object(map) => {
            let command = map.get("command").and_then(Value::as_str);
            match (command, map.get("args").map(dejson)) {
                (Some(program), Some(Value::Array(args))) => {
                    let mut line = shell_word(program);
                    for arg in args.iter().filter_map(Value::as_str) {
                        line.push(' ');
                        line.push_str(&shell_word(arg));
                    }
                    Some(line)
                }
                (Some(line), _) => Some(line.to_string()),
                (None, _) => map.get("cmd").and_then(Value::as_str).map(str::to_string),
            }
        }
        _ => None,
    }
}

/// Every command a `run_commands` input holds.
fn commands(input: &Value) -> Vec<String> {
    let input = dejson(input);
    let list = match &input {
        Value::Array(list) => list.clone(),
        Value::Object(map) if map.contains_key("commands") => match dejson(&map["commands"]) {
            Value::Array(list) => list,
            single => vec![single],
        },
        single => vec![single.clone()],
    };
    list.iter().filter_map(command_entry).collect()
}

/// The patch text of an `apply_patch` input.
fn patch_text(input: &Value) -> Option<String> {
    match dejson(input) {
        Value::String(text) => Some(text),
        other => util::string_field(&other, &["input", "patchText"]),
    }
}

fn classify(tool: &str, input: &Value) -> HookEvent {
    match tool {
        "editor" | "write_to_file" | "replace_in_file" | "delete_file" => {
            match util::string_field(&dejson(input), &["path", "file_path", "filePath"]) {
                Some(path) => HookEvent::PreWrite { paths: vec![path] },
                None => HookEvent::Ignore,
            }
        }
        "apply_patch" => {
            let paths = patch_text(input)
                .map(|patch| util::patch_paths(&patch))
                .unwrap_or_default();
            if paths.is_empty() {
                HookEvent::Ignore
            } else {
                HookEvent::PreWrite { paths }
            }
        }
        "run_commands" | "execute_command" => {
            let commands = commands(input);
            if commands.is_empty() {
                HookEvent::Ignore
            } else {
                HookEvent::PreShell {
                    command: commands.join("\n"),
                }
            }
        }
        _ => HookEvent::Ignore,
    }
}

impl AgentAdapter for Cline {
    fn id(&self) -> &'static str {
        "cline"
    }

    fn label(&self) -> &'static str {
        "Cline"
    }

    fn hint(&self) -> &'static str {
        "blocks protected files and risky commands; cannot run tests at the end of a task"
    }

    fn scopes(&self) -> &'static [Scope] {
        &[Scope::Project, Scope::User]
    }

    fn detect(&self, probe: &Probe) -> bool {
        probe.root.join(".clinerules").exists()
            || probe.root.join(".cline").is_dir()
            || (probe.on_path)("cline")
    }

    fn parse(&self, event: &str, stdin: &str) -> Result<HookInput, ParseError> {
        if event != "pre-tool" {
            return Err(ParseError(format!("unknown cline hook event `{event}`")));
        }
        let payload: Value = serde_json::from_str(stdin)
            .map_err(|e| ParseError(format!("cannot read hook input as JSON: {e}")))?;
        let cwd = payload
            .pointer("/workspaceRoots/0")
            .and_then(Value::as_str)
            .filter(|root| !root.is_empty())
            .map(PathBuf::from);
        // The CLI sends the rich `tool_call`; the extension only `preToolUse`.
        let call = match payload.pointer("/tool_call/name").and_then(Value::as_str) {
            Some(name) => Some((name, payload.pointer("/tool_call/input"))),
            None => payload
                .pointer("/preToolUse/toolName")
                .and_then(Value::as_str)
                .map(|name| (name, payload.pointer("/preToolUse/parameters"))),
        };
        let event = match call {
            Some((tool, input)) => classify(tool, input.unwrap_or(&Value::Null)),
            None => HookEvent::Ignore,
        };
        Ok(HookInput { cwd, event })
    }

    fn render(&self, event: &HookEvent, verdict: &Verdict) -> Rendered {
        let is_tool = matches!(
            event,
            HookEvent::PreWrite { .. } | HookEvent::PreShell { .. }
        );
        let reply = match verdict {
            Verdict::Block { reason } if is_tool => {
                json!({ "cancel": true, "errorMessage": reason })
            }
            Verdict::Ask { reason } if is_tool => {
                json!({ "cancel": true, "errorMessage": format!("{reason} {ASK_NOTE}") })
            }
            _ => json!({}),
        };
        Rendered {
            stdout: format!("{reply}\n"),
            stderr: String::new(),
            exit_code: 0,
        }
    }

    fn files(&self, ctx: &PlanCtx) -> Result<Vec<PathBuf>, InstallError> {
        Ok(vec![hook_path(ctx)?])
    }

    fn plan_install(&self, ctx: &PlanCtx, existing: &Files) -> Result<Plan, InstallError> {
        let file = ManagedFile {
            path: hook_path(ctx)?,
            contents: script().to_string(),
            executable: true,
        };
        let by_hand = format!(
            "Cline runs one PreToolUse file, so add this to it instead: `{HOOK_COMMAND_PREFIX} \
             cline pre-tool` (it reads Cline's JSON on stdin and prints the reply)."
        );
        let mut plan = file.plan_install(existing, &by_hand);
        if !plan.changes.is_empty() {
            plan.notes.push(ENABLE_NOTE.to_string());
        }
        Ok(plan)
    }

    fn plan_uninstall(&self, ctx: &PlanCtx, existing: &Files) -> Result<Plan, InstallError> {
        Ok(util::plan_uninstall(&hook_path(ctx)?, existing))
    }

    fn is_installed(&self, ctx: &PlanCtx, existing: &Files) -> bool {
        hook_path(ctx).is_ok_and(|path| util::is_installed(&path, existing))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn detection_looks_at_cline_files_and_the_binary() {
        let detect = |root: &std::path::Path, binary: &str| {
            let on_path = |p: &str| p == binary;
            Cline.detect(&Probe {
                root,
                on_path: &on_path,
            })
        };
        let tmp = tempfile::tempdir().unwrap();
        assert!(!detect(tmp.path(), ""));
        assert!(detect(tmp.path(), "cline"));
        // `.clinerules` is a directory in new projects and a single file in old ones.
        std::fs::write(tmp.path().join(".clinerules"), "rules").unwrap();
        assert!(detect(tmp.path(), ""));
        let other = tempfile::tempdir().unwrap();
        std::fs::create_dir(other.path().join(".cline")).unwrap();
        assert!(detect(other.path(), ""));
    }

    #[test]
    fn run_commands_input_shapes() {
        let cases = [
            (json!("ls"), vec!["ls"]),
            (json!(["ls", "pwd"]), vec!["ls", "pwd"]),
            (json!({"commands": ["ls", "pwd"]}), vec!["ls", "pwd"]),
            (json!({"commands": "ls"}), vec!["ls"]),
            (json!({"command": "ls -l"}), vec!["ls -l"]),
            (json!({"cmd": "ls -l"}), vec!["ls -l"]),
            // The extension JSON-encodes what is not a string.
            (json!({"commands": "[\"ls\",\"pwd\"]"}), vec!["ls", "pwd"]),
            (
                json!({"commands": [{"command": "rm", "args": ["-rf", "my dir"]}]}),
                vec!["rm -rf 'my dir'"],
            ),
            (
                json!({"command": "git", "args": ["push", "--force"]}),
                vec!["git push --force"],
            ),
            (json!({"commands": [1, null, {"args": []}]}), vec![]),
            (json!(42), vec![]),
        ];
        for (input, expected) in cases {
            assert_eq!(commands(&input), expected, "{input}");
        }
    }

    #[test]
    fn scripts_call_reflex_and_allow_without_it() {
        for text in [SCRIPT_UNIX, SCRIPT_WINDOWS] {
            assert!(text.contains(super::util::MARKER));
            assert!(text.contains("reflex hook cline pre-tool"));
            assert!(
                text.contains("'{}'"),
                "must answer {{}} when reflex is missing"
            );
        }
        assert!(SCRIPT_UNIX.starts_with("#!/bin/sh\n"));
    }
}
