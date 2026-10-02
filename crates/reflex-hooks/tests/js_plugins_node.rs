//! Runs the OpenCode/Kilo plugin and the pi extension under `node` against a fake
//! `reflex` on `PATH`, to check the parts the Rust tests cannot see: what goes to
//! `reflex` on stdin, and that a verdict, garbage, a missing binary or a session that
//! errored is turned into the right call on the agent's API. Skipped without `node`, and
//! on Windows, where the fake `reflex` is a shell script.

#![cfg(unix)]

use reflex_hooks::adapters::{opencode, pi};
use serde_json::{json, Value};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

fn node() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join("node"))
        .find(|p| p.is_file())
}

/// A directory with a fake `reflex` in it. The fake appends its arguments and stdin to
/// `log` and prints the contents of `reply`.
struct Fake {
    dir: tempfile::TempDir,
}

impl Fake {
    fn new(reply: &str) -> Fake {
        Self::with_exit(reply, 0)
    }

    fn with_exit(reply: &str, exit_code: i32) -> Fake {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let script = bin.join("reflex");
        std::fs::write(
            &script,
            format!("#!/bin/sh\n{{ echo \"ARGS $*\"; cat; echo; }} >> \"$(dirname \"$0\")/../log\"\ncat \"$(dirname \"$0\")/../reply\"\nexit {exit_code}\n"),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(dir.path().join("reply"), reply).unwrap();
        Fake { dir }
    }

    /// A `PATH` with the fake `reflex` and the system tools its script uses.
    fn path(&self) -> std::ffi::OsString {
        let dirs = [
            self.dir.path().join("bin"),
            "/usr/bin".into(),
            "/bin".into(),
        ];
        std::env::join_paths(dirs).unwrap()
    }

    /// Everything the fake was called with, one call per two lines.
    fn calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.dir.path().join("log"))
            .unwrap_or_default()
            .lines()
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect()
    }
}

/// Runs `driver` as an ES module next to the plugin file, with the given `PATH`. Returns what the driver printed.
fn run(node: &Path, plugin: &str, driver: &str, path: &std::ffi::OsStr) -> Value {
    let work = tempfile::tempdir().unwrap();
    std::fs::write(work.path().join("plugin.mjs"), plugin).unwrap();
    std::fs::write(work.path().join("driver.mjs"), driver).unwrap();
    let out = Command::new(node)
        .arg(work.path().join("driver.mjs"))
        .env("PATH", path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "driver failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!(
            "driver output {:?}: {e}",
            String::from_utf8_lossy(&out.stdout)
        )
    })
}

const OPENCODE_DRIVER: &str = r#"
import plugin from "./plugin.mjs"
const seen = []
const client = {
  session: {
    get: async ({ path }) => ({ data: path.id === "child" ? { parentID: "root" } : {} }),
    promptAsync: async (a) => seen.push(["prompt", a.path.id, a.body.parts[0].text]),
  },
  app: { log: async (a) => seen.push(["log", a.body.message]) },
  tui: { showToast: async (a) => seen.push(["toast", a.body.message]) },
}
const hooks = await plugin.server({ client, directory: "/work/proj" })
const attempt = async (input, args) => {
  try { await hooks["tool.execute.before"](input, { args }); return "ok" } catch (e) { return e.message }
}
const results = {}
results.first = await attempt({ tool: "bash", sessionID: "s1", callID: "c1" }, { command: "rm -rf /" })
results.second = await attempt({ tool: "bash", sessionID: "s1", callID: "c2" }, { command: "rm -rf /" })
results.other = await attempt({ tool: "write", sessionID: "s1", callID: "c3" }, { filePath: "a.txt" })
const idle = (id) => hooks.event({ event: { type: "session.idle", properties: { sessionID: id } } })
await idle("s1")
await idle("child")
await hooks.event({ event: { type: "session.error", properties: { sessionID: "s2" } } })
await idle("s2")
console.log(JSON.stringify({ results, seen }))
"#;

#[test]
fn opencode_plugin_turns_verdicts_into_agent_calls() {
    let Some(node) = node() else {
        eprintln!("node is not installed; skipping");
        return;
    };
    let plugin = opencode::plugin_source("kilo");

    // A block reply refuses every tool call with the reason; garbage lets everything through.
    for (reply, expect_blocked) in [
        (r#"{"action":"block","reason":"protected"}"#, true),
        ("nonsense", false),
        ("", false),
        (r#"{"action":"allow"}"#, false),
    ] {
        let fake = Fake::new(reply);
        let out = run(&node, &plugin, OPENCODE_DRIVER, &fake.path());
        assert_eq!(
            out["results"]["first"] == "protected",
            expect_blocked,
            "{reply}"
        );
        assert_eq!(
            out["results"]["other"] == "protected",
            expect_blocked,
            "{reply}"
        );
        assert_eq!(out["seen"], json!([]), "{reply}");
        // The plugin used the id of its agent and sent the tool call as documented.
        let calls = fake.calls();
        assert_eq!(calls[0], "ARGS hook kilo pre-tool");
        let request: Value = serde_json::from_str(&calls[1]).unwrap();
        assert_eq!(
            request,
            json!({ "cwd": "/work/proj", "session_id": "s1", "tool": "bash", "args": { "command": "rm -rf /" } })
        );
        // Two turn-end checks: the root session, and none for the subagent or the failed run.
        assert_eq!(
            calls
                .iter()
                .filter(|c| c.as_str() == "ARGS hook kilo turn-end")
                .count(),
            1,
            "{reply}: {calls:?}"
        );
    }

    // A deliberate-looking verdict is ignored when reflex exits unsuccessfully.
    let failed = Fake::with_exit(r#"{"action":"block","reason":"protected"}"#, 1);
    let out = run(&node, &plugin, OPENCODE_DRIVER, &failed.path());
    assert_eq!(out["results"]["first"], "ok");

    // No reflex on PATH: everything is allowed.
    let empty = tempfile::tempdir().unwrap();
    let out = run(&node, &plugin, OPENCODE_DRIVER, empty.path().as_os_str());
    assert_eq!(
        out["results"],
        json!({ "first": "ok", "second": "ok", "other": "ok" })
    );
    assert_eq!(out["seen"], json!([]));
}

const OPENCODE_IDLE_RACE_DRIVER: &str = r#"
import plugin from "./plugin.mjs"
const releases = []
let gets = 0
const client = {
  session: {
    get: async () => { gets++; if (gets <= 2) { await new Promise((resolve) => { releases[gets - 1] = resolve; resolveEntered[gets - 1]() }) } return { data: {} } },
    promptAsync: async () => {},
  },
  app: { log: async () => {} }, tui: { showToast: async () => {} },
}
const signal = []
const resolveEntered = []
for (let i = 0; i < 2; i++) signal[i] = new Promise((resolve) => (resolveEntered[i] = resolve))
const hooks = await plugin.server({ client, directory: "/work/proj" })
const idle = () => hooks.event({ event: { type: "session.idle", properties: { sessionID: "s1" } } })
const first = idle()
await signal[0]
await idle()
releases[0]()
await signal[1]
await idle()
releases[1]()
await first
await new Promise((resolve) => setTimeout(resolve, 30))
console.log(JSON.stringify({ gets }))
"#;

#[test]
fn opencode_plugin_rechecks_an_idle_received_during_turn_end_check() {
    let Some(node) = node() else { return };
    let fake = Fake::new(r#"{"action":"allow"}"#);
    let out = run(
        &node,
        &opencode::plugin_source("opencode"),
        OPENCODE_IDLE_RACE_DRIVER,
        &fake.path(),
    );
    assert_eq!(out["gets"], 3);
    assert_eq!(
        fake.calls()
            .iter()
            .filter(|c| c.as_str() == "ARGS hook opencode turn-end")
            .count(),
        3
    );
}

const OPENCODE_RETRY_RACE_DRIVER: &str = r#"
import plugin from "./plugin.mjs"
let release
let entered
const inGet = new Promise((resolve) => (entered = resolve))
let gets = 0
let prompts = 0
const client = {
  session: {
    get: async () => { gets++; if (gets === 1) { entered(); await new Promise((resolve) => (release = resolve)) } return { data: {} } },
    promptAsync: async () => { prompts++ },
  },
  app: { log: async () => {} }, tui: { showToast: async () => {} },
}
const hooks = await plugin.server({ client, directory: "/work/proj" })
const idle = () => hooks.event({ event: { type: "session.idle", properties: { sessionID: "s1" } } })
const first = idle()
await inGet
await idle()
release()
await first
await new Promise((resolve) => setTimeout(resolve, 30))
console.log(JSON.stringify({ gets, prompts }))
"#;

#[test]
fn opencode_retry_discards_pending_idle_recheck() {
    let Some(node) = node() else { return };
    let fake = Fake::new(r#"{"action":"retry","reason":"continue"}"#);
    let out = run(
        &node,
        &opencode::plugin_source("opencode"),
        OPENCODE_RETRY_RACE_DRIVER,
        &fake.path(),
    );
    assert_eq!(out["gets"], 1);
    assert_eq!(out["prompts"], 1);
    assert_eq!(
        fake.calls()
            .iter()
            .filter(|c| c.as_str() == "ARGS hook opencode turn-end")
            .count(),
        1
    );
}

#[test]
fn opencode_plugin_asks_once_then_lets_the_same_call_through() {
    let Some(node) = node() else {
        eprintln!("node is not installed; skipping");
        return;
    };
    let fake = Fake::new(r#"{"action":"ask","reason":"Confirm rm."}"#);
    let out = run(
        &node,
        &opencode::plugin_source("opencode"),
        OPENCODE_DRIVER,
        &fake.path(),
    );
    assert_eq!(
        out["results"]["first"],
        "Confirm rm. Ask the user first; if they agree, repeat the same call."
    );
    assert_eq!(out["results"]["second"], "ok");
    // A different call is asked about again.
    assert!(out["results"]["other"]
        .as_str()
        .unwrap()
        .starts_with("Confirm rm."));
}

#[test]
fn opencode_plugin_sends_a_retry_back_to_the_session() {
    let Some(node) = node() else {
        eprintln!("node is not installed; skipping");
        return;
    };
    let fake = Fake::new(r#"{"action":"retry","reason":"tests failed"}"#);
    let out = run(
        &node,
        &opencode::plugin_source("opencode"),
        OPENCODE_DRIVER,
        &fake.path(),
    );
    assert_eq!(out["seen"], json!([["prompt", "s1", "tests failed"]]));

    let fake = Fake::new(r#"{"action":"notify","reason":"large change"}"#);
    let out = run(
        &node,
        &opencode::plugin_source("opencode"),
        OPENCODE_DRIVER,
        &fake.path(),
    );
    assert_eq!(
        out["seen"],
        json!([["log", "large change"], ["toast", "large change"]])
    );
}

const PI_DRIVER: &str = r#"
import register from "./plugin.mjs"
const handlers = {}
register({ on: (name, fn) => (handlers[name] = fn) })
const ctx = (hasUI, confirm) => ({
  cwd: "/work/proj",
  hasUI,
  sessionManager: { getSessionId: () => "sess-1" },
  ui: { confirm: async () => confirm, notify: (m, level) => notes.push([m, level]) },
})
const notes = []
const call = (c) => handlers.tool_call({ toolName: "bash", input: { command: "rm -rf /" } }, c)
const results = {
  noUi: await call(ctx(false, true)),
  confirmed: await call(ctx(true, true)),
  declined: await call(ctx(true, false)),
  broken: await call({}),
}
const settle = (outcome) => handlers.agent_before_settle({ outcome, entries: [{ type: "custom", customType: "x" }] }, ctx(true, true))
results.settled = await settle("completed")
results.aborted = await settle("aborted")
console.log(JSON.stringify({ results, notes }))
"#;

#[test]
fn pi_extension_turns_verdicts_into_pi_results() {
    let Some(node) = node() else {
        eprintln!("node is not installed; skipping");
        return;
    };

    let fake = Fake::new(r#"{"action":"block","reason":"protected"}"#);
    let out = run(&node, pi::EXTENSION, PI_DRIVER, &fake.path());
    let blocked = json!({ "block": true, "reason": "protected" });
    assert_eq!(out["results"]["noUi"], blocked);
    assert_eq!(out["results"]["confirmed"], blocked);
    // A handler that fails must not throw: pi blocks the tool if it does.
    assert_eq!(out["results"]["broken"], Value::Null);
    let calls = fake.calls();
    assert_eq!(calls[0], "ARGS hook pi pre-tool");
    let request: Value = serde_json::from_str(&calls[1]).unwrap();
    assert_eq!(
        request,
        json!({ "cwd": "/work/proj", "session_id": "sess-1", "tool": "bash", "args": { "command": "rm -rf /" } })
    );

    // Ask: pi's own dialog decides; without a UI the call is refused.
    let fake = Fake::new(r#"{"action":"ask","reason":"Confirm."}"#);
    let out = run(&node, pi::EXTENSION, PI_DRIVER, &fake.path());
    assert_eq!(
        out["results"]["noUi"],
        json!({ "block": true, "reason": "Confirm." })
    );
    assert_eq!(out["results"]["confirmed"], Value::Null);
    assert_eq!(
        out["results"]["declined"],
        json!({ "block": true, "reason": "Confirm." })
    );

    // Retry: one more model request, with the failure appended as a message. Not after an
    // aborted run, which is not asked about at all.
    let fake = Fake::new(r#"{"action":"retry","reason":"tests failed"}"#);
    let out = run(&node, pi::EXTENSION, PI_DRIVER, &fake.path());
    assert_eq!(
        out["results"]["settled"],
        json!({
            "entries": [
                { "type": "custom", "customType": "x" },
                { "type": "custom_message", "customType": "reflex-control", "content": "tests failed", "display": true }
            ],
            "continue": true
        })
    );
    assert_eq!(out["results"]["aborted"], Value::Null);

    // Notify shows a message and lets pi settle.
    let fake = Fake::new(r#"{"action":"notify","reason":"large change"}"#);
    let out = run(&node, pi::EXTENSION, PI_DRIVER, &fake.path());
    assert_eq!(out["results"]["settled"], Value::Null);
    assert_eq!(out["notes"], json!([["large change", "warning"]]));

    // A valid block response from a failed process must fail open.
    let failed = Fake::with_exit(r#"{"action":"block","reason":"protected"}"#, 1);
    let out = run(&node, pi::EXTENSION, PI_DRIVER, &failed.path());
    assert_eq!(out["results"]["noUi"], Value::Null);

    // No reflex: everything is allowed.
    let empty = tempfile::tempdir().unwrap();
    let out = run(&node, pi::EXTENSION, PI_DRIVER, empty.path().as_os_str());
    assert_eq!(out["results"]["confirmed"], Value::Null);
    assert_eq!(out["results"]["settled"], Value::Null);
}
