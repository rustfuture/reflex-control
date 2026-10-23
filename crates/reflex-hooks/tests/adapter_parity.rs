//! Every agent adapter must hand the guard the exact command the agent is about to run.
//! A guard is only as good as the text it sees, and each adapter reads a different JSON
//! shape: this test parses the guard corpus through all seven shell-capable adapters and
//! checks that the `PreShell` command comes out byte-for-byte unchanged, so the verdict
//! cannot differ because of the adapter.

use reflex_hooks::adapters::{find, HookEvent};
use serde_json::{json, Value};
use std::path::Path;

struct Agent {
    id: &'static str,
    event: &'static str,
    input: fn(&str) -> Value,
}

fn agents() -> [Agent; 7] {
    [
        Agent {
            id: "claude-code",
            event: "pre-tool",
            input: |command| {
                json!({
                    "session_id": "sess-1", "transcript_path": "/tmp/t.jsonl",
                    "cwd": "/work/proj", "hook_event_name": "PreToolUse",
                    "tool_name": "Bash", "tool_use_id": "toolu_1",
                    "tool_input": { "command": command }
                })
            },
        },
        Agent {
            id: "codex",
            event: "pre-tool",
            input: |command| {
                json!({
                    "session_id": "sess-1", "turn_id": "turn-1",
                    "transcript_path": "/tmp/t.jsonl", "cwd": "/work/proj",
                    "hook_event_name": "PreToolUse", "permission_mode": "default",
                    "tool_name": "Bash", "tool_use_id": "call-1",
                    "tool_input": { "command": command }
                })
            },
        },
        Agent {
            id: "cursor",
            event: "shell",
            input: |command| {
                json!({
                    "conversation_id": "conv-1", "generation_id": "gen-1",
                    "hook_event_name": "beforeShellExecution", "model": "default",
                    "cursor_version": "1.7.0", "workspace_roots": ["/work/proj"],
                    "cwd": "/work/proj", "command": command
                })
            },
        },
        Agent {
            id: "opencode",
            event: "pre-tool",
            input: |command| {
                json!({
                    "cwd": "/work/proj", "session_id": "ses_1",
                    "tool": "bash", "args": { "command": command }
                })
            },
        },
        Agent {
            id: "kilo",
            event: "pre-tool",
            input: |command| {
                json!({
                    "cwd": "/work/proj", "session_id": "ses_1",
                    "tool": "bash", "args": { "command": command }
                })
            },
        },
        Agent {
            id: "cline",
            event: "pre-tool",
            input: |command| {
                json!({
                    "clineVersion": "3.40.0", "hookName": "PreToolUse",
                    "timestamp": "1768473000000", "taskId": "conv-123",
                    "workspaceRoots": ["/work/proj"], "userId": "user",
                    "preToolUse": {
                        "toolName": "execute_command",
                        "parameters": { "command": command }
                    }
                })
            },
        },
        Agent {
            id: "pi",
            event: "pre-tool",
            input: |command| {
                json!({
                    "cwd": "/work/proj", "session_id": "ses_1",
                    "tool": "bash", "args": { "command": command }
                })
            },
        },
    ]
}

fn shell_commands() -> Vec<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/guard_corpus.tsv");
    let mut commands: Vec<String> = std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
        .filter_map(|line| line.splitn(3, '\t').nth(2).map(|s| s.replace("\\n", "\n")))
        .collect();
    // Shapes that JSON escaping or a hand-written parser could mangle.
    commands.extend(
        [
            "echo \"a b\" 'c d'",
            "printf '%s\\n' one two",
            "rm -rf / # yorum",
            "echo \"$HOME\" | sed 's|/|\\\\|g'",
            "git commit -m \"$(cat <<'EOF'\ndon't panic\nEOF\n)\"",
            "python3 -c 'print(\"ş\")'",
            "echo 'back\\\\slash'",
            "true \\\n  && git status",
        ]
        .map(String::from),
    );
    commands
}

#[test]
fn every_adapter_passes_the_command_through_unchanged() {
    let commands = shell_commands();
    for agent in agents() {
        let adapter = find(agent.id).unwrap_or_else(|| panic!("no adapter {}", agent.id));
        for command in &commands {
            let stdin = (agent.input)(command).to_string();
            let parsed = adapter
                .parse(agent.event, &stdin)
                .unwrap_or_else(|e| panic!("{}: {e} for {command:?}", agent.id));
            let expected = HookEvent::PreShell {
                command: command.clone(),
            };
            assert_eq!(
                parsed.event, expected,
                "{} changed the command\n  in:  {command:?}\n  out: {:?}",
                agent.id, parsed.event
            );
        }
        println!("{}: {} commands", agent.id, commands.len());
    }
}
