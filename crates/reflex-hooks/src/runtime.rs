//! Everything a hook call needs from the outside world, behind the [`Runtime`] trait so
//! the flow in [`handle`] can be tested without git, files or a test suite.
//!
//! # Failing safe
//!
//! A hook must never get in the way because Reflex itself is broken. If anything in
//! [`handle`] goes wrong (unreadable stdin, malformed JSON, a config file that does not
//! parse, git missing), it prints one warning line to stderr and answers "allow" with
//! exit code 0. Deliberate blocks are the only non-zero exits, and only the git adapter
//! uses one (exit 1).

use crate::adapters::{self, HookEvent, Rendered};
use crate::config::{self, Config, ConfigSource};
use crate::guard::{
    self, check_command_in_dir, check_paths, check_staged, decide_turn_end, PathPolicy,
    TestOutcome, TurnEndInput, Verdict,
};
use crate::install::Env;
use crate::state::{self, SessionState, UntrackedFile};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, UNIX_EPOCH};

/// Test run limit at turn end. The Claude Code `Stop` hook is installed with a 600 s
/// timeout; this leaves room to answer before it fires.
pub const TURN_END_TEST_TIMEOUT: Duration = Duration::from_secs(540);

/// Test run limit in the git pre-commit hook.
pub const PRE_COMMIT_TEST_TIMEOUT: Duration = Duration::from_secs(1800);

/// Bytes of test output kept in memory (the end of the output is what matters).
const OUTPUT_CAP: usize = 1 << 20;

/// Files larger than this are not read to count lines.
const LINE_COUNT_MAX_BYTES: u64 = 2 << 20;

/// Upper bound on untracked files inspected per check.
const UNTRACKED_MAX_FILES: usize = 2000;

/// A project as the hooks see it.
#[derive(Debug, Clone)]
pub struct Project {
    pub root: PathBuf,
    pub config: Config,
    pub source: ConfigSource,
}

/// Finds the project root and config that apply to `cwd`.
pub fn resolve_project(cwd: &Path) -> Result<Project, config::ConfigError> {
    let (config, source) = Config::discover(cwd)?;
    let root = match &source {
        ConfigSource::Project(path) => path.parent().map(Path::to_path_buf),
        _ => None,
    }
    .or_else(|| config::find_repo_root(cwd))
    .unwrap_or_else(|| cwd.to_path_buf());
    Ok(Project {
        root,
        config,
        source,
    })
}

pub trait Runtime {
    fn read_stdin(&self) -> io::Result<String>;
    fn current_dir(&self) -> PathBuf;
    fn project(&self, cwd: &Path) -> Result<Project, String>;
    /// Runs the turn-end check (working tree, tests, session state) and decides.
    fn turn_end(&self, project: &Project, session_id: &str) -> Result<Verdict, String>;
    /// Paths staged for the next commit, relative to the project root.
    fn staged_files(&self, project: &Project) -> Result<Vec<String>, String>;
    /// Runs the project's test command for a commit.
    fn pre_commit_tests(&self, project: &Project) -> Result<TestOutcome, String>;
}

/// Handles one hook call end to end.
pub fn handle(agent: &str, event: &str, rt: &dyn Runtime) -> Rendered {
    match try_handle(agent, event, rt) {
        Ok(rendered) => rendered,
        Err(message) => Rendered {
            stdout: String::new(),
            stderr: format!("reflex: warning: {message}. Allowing the action.\n"),
            exit_code: 0,
        },
    }
}

fn try_handle(agent: &str, event: &str, rt: &dyn Runtime) -> Result<Rendered, String> {
    let adapter = adapters::find(agent).ok_or_else(|| format!("unknown agent `{agent}`"))?;
    let stdin = if adapter.reads_stdin(event) {
        rt.read_stdin()
            .map_err(|e| format!("cannot read hook input: {e}"))?
    } else {
        String::new()
    };
    let input = adapter.parse(event, &stdin).map_err(|e| e.to_string())?;
    let cwd = input.cwd.clone().unwrap_or_else(|| rt.current_dir());
    let project = rt.project(&cwd)?;
    let verdict = evaluate(&input.event, &project, input.cwd.as_deref(), rt)?;
    Ok(adapter.render(&input.event, &verdict))
}

fn evaluate(
    event: &HookEvent,
    project: &Project,
    cwd: Option<&Path>,
    rt: &dyn Runtime,
) -> Result<Verdict, String> {
    let policy =
        |p: &Project| PathPolicy::new(&p.config.protect.paths, &p.root).map_err(|e| e.to_string());
    match event {
        HookEvent::Ignore => Ok(Verdict::Allow),
        HookEvent::PreWrite { paths } => Ok(check_paths(paths, &policy(project)?)),
        HookEvent::PreShell { command } => {
            Ok(check_command_in_dir(command, &policy(project)?, cwd))
        }
        HookEvent::TurnEnd { session_id } => rt.turn_end(project, session_id),
        HookEvent::PreCommit => {
            let staged = rt.staged_files(project)?;
            let verdict = check_staged(&staged, &policy(project)?);
            if verdict != Verdict::Allow {
                return Ok(verdict);
            }
            let cfg = &project.config;
            if !cfg.git.run_tests || cfg.tests.command.trim().is_empty() {
                return Ok(Verdict::Allow);
            }
            match rt.pre_commit_tests(project)? {
                TestOutcome::Failed { output } => Ok(Verdict::Block {
                    reason: format!(
                        "Reflex Control: commit blocked, the tests failed (`{}`). Last {} lines \
                         of output:\n\n{}\n\nFix the failures, or skip this check once with \
                         `git commit --no-verify`.",
                        cfg.tests.command.trim(),
                        guard::OUTPUT_TAIL_LINES,
                        guard::tail_lines(&output, guard::OUTPUT_TAIL_LINES)
                    ),
                }),
                _ => Ok(Verdict::Allow),
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// The real thing
// ─────────────────────────────────────────────────────────────────────────────

pub struct SystemRuntime;

impl Runtime for SystemRuntime {
    fn read_stdin(&self) -> io::Result<String> {
        let mut buf = Vec::new();
        io::stdin().read_to_end(&mut buf)?;
        Ok(String::from_utf8_lossy(&buf).into_owned())
    }

    fn current_dir(&self) -> PathBuf {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    }

    fn project(&self, cwd: &Path) -> Result<Project, String> {
        resolve_project(cwd).map_err(|e| e.to_string())
    }

    fn turn_end(&self, project: &Project, session_id: &str) -> Result<Verdict, String> {
        let root = &project.root;
        let mut st = state::load(root, session_id);
        let snapshot = Snapshot::take(root);

        let (fingerprint, diff_lines) = match &snapshot {
            Some(s) => {
                if s.is_clean() {
                    // Nothing changed in the working tree, so there is nothing to check.
                    if st.retries != 0 {
                        st.retries = 0;
                        let _ = state::save(root, session_id, &st);
                    }
                    return Ok(Verdict::Allow);
                }
                let fp = state::fingerprint(&s.diff, &s.untracked);
                if st.last_ok && st.fingerprint.as_deref() == Some(fp.as_str()) {
                    // Same tree as the last check, which did not fail.
                    return Ok(Verdict::Allow);
                }
                (Some(fp), s.diff_lines())
            }
            None => (None, 0),
        };

        let command = project.config.tests.command.trim();
        let tests = if command.is_empty() {
            TestOutcome::NotRun
        } else {
            run_tests(command, root, TURN_END_TEST_TIMEOUT)
        };

        let verdict = decide_turn_end(
            &TurnEndInput {
                tests: tests.clone(),
                diff_lines,
                retries_so_far: st.retries,
            },
            &project.config,
        );

        st = SessionState {
            fingerprint,
            last_ok: !matches!(tests, TestOutcome::Failed { .. }),
            retries: match verdict {
                Verdict::RetryAgent { .. } => st.retries + 1,
                _ => 0,
            },
        };
        // Losing the state only costs a repeated test run, so a failed write is ignored.
        let _ = state::save(root, session_id, &st);
        Ok(verdict)
    }

    fn staged_files(&self, project: &Project) -> Result<Vec<String>, String> {
        let out = git(&project.root, &["diff", "--cached", "--name-only", "-z"])
            .ok_or("cannot list staged files with git")?;
        Ok(out
            .split('\0')
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect())
    }

    fn pre_commit_tests(&self, project: &Project) -> Result<TestOutcome, String> {
        let command = project.config.tests.command.trim();
        eprintln!("reflex: running `{command}` before the commit...");
        Ok(run_tests(command, &project.root, PRE_COMMIT_TEST_TIMEOUT))
    }
}

/// What `git` says about the working tree.
struct Snapshot {
    diff: String,
    untracked: Vec<UntrackedFile>,
    untracked_lines: usize,
}

impl Snapshot {
    /// `None` outside a git repository or if git cannot be run.
    fn take(root: &Path) -> Option<Snapshot> {
        let diff = match git(root, &["diff", "HEAD", "--no-color", "--no-ext-diff"]) {
            Some(d) => d,
            // No commits yet: there is no HEAD, so combine staged and unstaged changes.
            None => {
                let staged = git(root, &["diff", "--cached", "--no-color", "--no-ext-diff"])?;
                let unstaged = git(root, &["diff", "--no-color", "--no-ext-diff"])?;
                staged + &unstaged
            }
        };
        let names = git(root, &["ls-files", "--others", "--exclude-standard", "-z"])?;
        let mut untracked = Vec::new();
        let mut untracked_lines = 0;
        for name in names
            .split('\0')
            .filter(|s| !s.is_empty())
            .take(UNTRACKED_MAX_FILES)
        {
            let path = root.join(name);
            let Ok(meta) = std::fs::metadata(&path) else {
                continue;
            };
            let mtime_nanos = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_nanos());
            if meta.is_file() && meta.len() <= LINE_COUNT_MAX_BYTES {
                untracked_lines += count_text_lines(&path);
            }
            untracked.push(UntrackedFile {
                path: name.to_string(),
                len: meta.len(),
                mtime_nanos,
            });
        }
        Some(Snapshot {
            diff,
            untracked,
            untracked_lines,
        })
    }

    fn is_clean(&self) -> bool {
        self.diff.trim().is_empty() && self.untracked.is_empty()
    }

    fn diff_lines(&self) -> usize {
        changed_lines(&self.diff) + self.untracked_lines
    }
}

/// Added plus removed lines in a unified diff.
pub fn changed_lines(diff: &str) -> usize {
    diff.lines()
        .filter(|l| {
            (l.starts_with('+') && !l.starts_with("+++"))
                || (l.starts_with('-') && !l.starts_with("---"))
        })
        .count()
}

/// Number of lines in a text file; 0 for binary or unreadable files.
fn count_text_lines(path: &Path) -> usize {
    match std::fs::read(path) {
        Ok(bytes) if !bytes[..bytes.len().min(8192)].contains(&0) => {
            let newlines = bytes.iter().filter(|b| **b == b'\n').count();
            newlines + usize::from(bytes.last().is_some_and(|b| *b != b'\n'))
        }
        _ => 0,
    }
}

/// Runs a git command in `root`. `None` if git is missing or the command fails.
fn git(root: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Describes the machine for the installer.
pub fn detect_env(cwd: &Path) -> Env {
    let root = git(cwd, &["rev-parse", "--show-toplevel"])
        .map(|s| PathBuf::from(s.trim()))
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| cwd.to_path_buf());
    let git_hooks_dir = git(&root, &["rev-parse", "--git-path", "hooks"])
        .map(|s| PathBuf::from(s.trim()))
        .map(|p| if p.is_absolute() { p } else { root.join(p) });
    let hooks_dir_in_repo = git_hooks_dir
        .as_ref()
        .is_some_and(|d| d.starts_with(&root) && !d.starts_with(root.join(".git")));
    Env {
        root,
        home: dirs::home_dir(),
        git_hooks_dir,
        hooks_dir_in_repo,
    }
}

/// Runs `command` through the platform shell in `cwd` and classifies the result.
/// Stdin is closed, so the command cannot consume the hook's own input.
pub fn run_tests(command: &str, cwd: &Path, timeout: Duration) -> TestOutcome {
    let mut cmd = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.args(["/C", command]);
        c
    } else {
        let mut c = Command::new("sh");
        c.args(["-c", command]);
        c
    };
    cmd.current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("REFLEX_HOOK", "1");

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return TestOutcome::Failed {
                output: format!("could not start `{command}`: {e}"),
            }
        }
    };

    let buffer = Arc::new(Mutex::new(Vec::<u8>::new()));
    let mut readers = Vec::new();
    if let Some(out) = child.stdout.take() {
        readers.push(spawn_reader(out, Arc::clone(&buffer)));
    }
    if let Some(err) = child.stderr.take() {
        readers.push(spawn_reader(err, Arc::clone(&buffer)));
    }

    let started = Instant::now();
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if started.elapsed() >= timeout => {
                timed_out = true;
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(_) => break None,
        }
    };

    // Let the readers drain the pipes, but do not hang on a background process that
    // inherited them and keeps them open.
    let grace = Instant::now();
    while !timed_out
        && readers.iter().any(|r| !r.is_finished())
        && grace.elapsed() < Duration::from_secs(2)
    {
        std::thread::sleep(Duration::from_millis(10));
    }

    let mut output =
        String::from_utf8_lossy(&buffer.lock().unwrap_or_else(|e| e.into_inner())).into_owned();
    if timed_out {
        output.push_str(&format!(
            "\n[reflex] `{command}` timed out after {} seconds and was stopped\n",
            timeout.as_secs()
        ));
    }
    match status {
        Some(s) if s.success() => TestOutcome::Passed,
        _ => TestOutcome::Failed { output },
    }
}

fn spawn_reader<R: Read + Send + 'static>(
    mut source: R,
    buffer: Arc<Mutex<Vec<u8>>>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut chunk = [0u8; 8192];
        while let Ok(n) = source.read(&mut chunk) {
            if n == 0 {
                break;
            }
            let mut buf = buffer.lock().unwrap_or_else(|e| e.into_inner());
            buf.extend_from_slice(&chunk[..n]);
            if buf.len() > OUTPUT_CAP {
                let excess = buf.len() - OUTPUT_CAP;
                buf.drain(..excess);
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// In-memory runtime for testing the flow in `handle`.
    struct Fake {
        stdin: io::Result<String>,
        project: Result<Project, String>,
        turn_end: Verdict,
        staged: Vec<String>,
        tests: TestOutcome,
    }

    impl Fake {
        fn new(stdin: &str) -> Fake {
            Fake {
                stdin: Ok(stdin.to_string()),
                project: Ok(Project {
                    root: PathBuf::from("/work/proj"),
                    config: Config::default(),
                    source: ConfigSource::Defaults,
                }),
                turn_end: Verdict::Allow,
                staged: Vec::new(),
                tests: TestOutcome::Passed,
            }
        }
    }

    impl Runtime for Fake {
        fn read_stdin(&self) -> io::Result<String> {
            match &self.stdin {
                Ok(s) => Ok(s.clone()),
                Err(e) => Err(io::Error::new(e.kind(), e.to_string())),
            }
        }
        fn current_dir(&self) -> PathBuf {
            PathBuf::from("/work/proj")
        }
        fn project(&self, _cwd: &Path) -> Result<Project, String> {
            self.project.clone()
        }
        fn turn_end(&self, _p: &Project, _s: &str) -> Result<Verdict, String> {
            Ok(self.turn_end.clone())
        }
        fn staged_files(&self, _p: &Project) -> Result<Vec<String>, String> {
            Ok(self.staged.clone())
        }
        fn pre_commit_tests(&self, _p: &Project) -> Result<TestOutcome, String> {
            Ok(self.tests.clone())
        }
    }

    #[test]
    fn shell_adapters_resolve_writes_from_a_project_subdirectory() {
        use serde_json::json;

        for agent in [
            "claude-code",
            "codex",
            "cursor",
            "opencode",
            "kilo",
            "cline",
            "pi",
        ] {
            for cwd in [Some("/p/app"), None] {
                for (command, blocked_in_app, blocked_at_root) in [
                    ("echo x > ../secrets/token.txt", true, false),
                    ("echo x > secrets/token.txt", false, true),
                    ("echo x > /p/secrets/abs.txt", true, true),
                    ("echo x > ../../secrets/token.txt", false, false),
                ] {
                    let mut payload = match agent {
                        "claude-code" | "codex" => json!({
                            "tool_name": "Bash", "tool_input": { "command": command }
                        }),
                        "cursor" => json!({ "command": command }),
                        "cline" => json!({
                            "tool_call": { "name": "run_commands", "input": { "commands": [command] } }
                        }),
                        _ => json!({ "tool": "bash", "args": { "command": command } }),
                    };
                    if let Some(cwd) = cwd {
                        if agent == "cline" {
                            payload["workspaceRoots"] = json!([cwd]);
                        } else {
                            payload["cwd"] = json!(cwd);
                        }
                    }
                    let mut rt = Fake::new(&payload.to_string());
                    rt.project.as_mut().unwrap().root = PathBuf::from("/p");
                    let event = if agent == "cursor" {
                        "shell"
                    } else {
                        "pre-tool"
                    };
                    let out = handle(agent, event, &rt);
                    let blocked = if cwd.is_some() {
                        blocked_in_app
                    } else {
                        blocked_at_root
                    };
                    let context = format!("{agent}, cwd={cwd:?}, command={command:?}");
                    if agent == "cursor" && blocked {
                        assert!(
                            out.stderr.contains("secrets/**"),
                            "{context}: {}",
                            out.stderr
                        );
                        assert_eq!(out.exit_code, 2, "{context}");
                    } else {
                        assert!(out.stderr.is_empty(), "{context}: {}", out.stderr);
                        assert_eq!(out.exit_code, 0, "{context}");
                    }
                    if !blocked {
                        match agent {
                            "claude-code" | "codex" | "cursor" => {
                                assert!(out.stdout.is_empty(), "{context}: {}", out.stdout);
                            }
                            "cline" => assert_eq!(out.stdout, "{}\n", "{context}"),
                            _ => {
                                let reply: serde_json::Value =
                                    serde_json::from_str(&out.stdout).unwrap();
                                assert_eq!(reply["action"], "allow", "{context}");
                            }
                        }
                    } else {
                        let reply: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
                        match agent {
                            "claude-code" | "codex" => assert_eq!(
                                reply["hookSpecificOutput"]["permissionDecision"], "deny",
                                "{context}"
                            ),
                            "cursor" => assert_eq!(reply["permission"], "deny", "{context}"),
                            "cline" => assert_eq!(reply["cancel"], true, "{context}"),
                            _ => assert_eq!(reply["action"], "block", "{context}"),
                        }
                        assert!(
                            out.stdout.contains("secrets/**"),
                            "{context}: {}",
                            out.stdout
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn malformed_input_allows_and_warns() {
        let out = handle("claude-code", "pre-tool", &Fake::new("{ not json"));
        assert_eq!(out.exit_code, 0);
        assert_eq!(out.stdout, "");
        assert!(out.stderr.starts_with("reflex: warning:"), "{}", out.stderr);
        assert!(out.stderr.contains("Allowing"));
    }

    #[test]
    fn config_error_allows_and_warns() {
        let mut rt = Fake::new(r#"{"tool_name":"Write","tool_input":{"file_path":".env"}}"#);
        rt.project = Err("invalid .reflex.toml: bad value".into());
        let out = handle("claude-code", "pre-tool", &rt);
        assert_eq!(out.exit_code, 0);
        assert_eq!(out.stdout, "");
        assert!(out.stderr.contains("invalid .reflex.toml"));
    }

    #[test]
    fn unknown_agent_and_event_allow_and_warn() {
        let rt = Fake::new("{}");
        for (agent, event) in [
            ("nope", "pre-tool"),
            ("claude-code", "nope"),
            ("git", "nope"),
        ] {
            let out = handle(agent, event, &rt);
            assert_eq!(out.exit_code, 0, "{agent} {event}");
            assert!(out.stderr.contains("warning"), "{agent} {event}");
        }
    }

    #[test]
    fn unreadable_stdin_allows_and_warns() {
        let mut rt = Fake::new("");
        rt.stdin = Err(io::Error::other("closed"));
        let out = handle("claude-code", "stop", &rt);
        assert_eq!(out.exit_code, 0);
        assert!(out.stderr.contains("cannot read hook input"));
    }

    #[test]
    fn git_pre_commit_blocks_on_protected_files_then_on_failing_tests() {
        let mut rt = Fake::new("");
        rt.staged = vec!["src/a.rs".into(), "config/.env".into()];
        let out = handle("git", "pre-commit", &rt);
        assert_eq!(out.exit_code, 1);
        assert!(out.stderr.contains("config/.env"));
        assert!(out.stderr.contains(".reflex.toml"));

        rt.staged = vec!["src/a.rs".into()];
        assert_eq!(handle("git", "pre-commit", &rt).exit_code, 0);

        rt.tests = TestOutcome::Failed {
            output: "boom".into(),
        };
        let out = handle("git", "pre-commit", &rt);
        assert_eq!(out.exit_code, 1);
        assert!(out.stderr.contains("boom"));
        assert!(out.stderr.contains("--no-verify"));

        // git.run_tests = false or an empty command skips the tests.
        let mut cfg = Config::default();
        cfg.git.run_tests = false;
        rt.project = Ok(Project {
            root: PathBuf::from("/work/proj"),
            config: cfg,
            source: ConfigSource::Defaults,
        });
        assert_eq!(handle("git", "pre-commit", &rt).exit_code, 0);
    }

    #[test]
    fn changed_lines_counts_added_and_removed_only() {
        let diff = "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1,2 +1,2 @@\n-old\n+new\n same\n";
        assert_eq!(changed_lines(diff), 2);
        assert_eq!(changed_lines(""), 0);
    }

    #[cfg(unix)]
    #[test]
    fn run_tests_reports_pass_fail_and_output() {
        let tmp = tempfile::tempdir().unwrap();
        let t = Duration::from_secs(30);
        assert_eq!(run_tests("exit 0", tmp.path(), t), TestOutcome::Passed);
        match run_tests("echo out; echo err 1>&2; exit 3", tmp.path(), t) {
            TestOutcome::Failed { output } => {
                assert!(output.contains("out"));
                assert!(output.contains("err"));
            }
            other => panic!("expected failure, got {other:?}"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn run_tests_stops_a_command_that_runs_too_long() {
        let tmp = tempfile::tempdir().unwrap();
        let started = Instant::now();
        let outcome = run_tests("sleep 30", tmp.path(), Duration::from_millis(300));
        assert!(started.elapsed() < Duration::from_secs(10));
        match outcome {
            TestOutcome::Failed { output } => assert!(output.contains("timed out")),
            other => panic!("expected failure, got {other:?}"),
        }
    }

    /// Tests that drive a real git repository and `sh`.
    #[cfg(unix)]
    mod real_git {
        use super::*;
        use crate::config::OnFailure;

        fn sh(dir: &Path, script: &str) {
            let ok = Command::new("sh")
                .args(["-c", script])
                .current_dir(dir)
                .status()
                .unwrap()
                .success();
            assert!(ok, "script failed: {script}");
        }

        fn repo_with_commit() -> tempfile::TempDir {
            let tmp = tempfile::tempdir().unwrap();
            sh(
                tmp.path(),
                "git init -q . && git config user.email t@example.com && git config user.name t \
                 && echo one > a.txt && git add a.txt && git commit -q -m init",
            );
            tmp
        }

        fn project(root: &Path, test_command: &str, on_failure: OnFailure) -> Project {
            let mut config = Config::default();
            config.tests.command = test_command.to_string();
            config.tests.on_failure = on_failure;
            Project {
                root: root.to_path_buf(),
                config,
                source: ConfigSource::Defaults,
            }
        }

        #[test]
        fn turn_end_flow_on_a_real_repository() {
            let repo = repo_with_commit();
            let root = repo.path();
            // Keep the log outside the repository: an untracked file that changes on every
            // run would (correctly) change the fingerprint too.
            let outside = tempfile::tempdir().unwrap();
            let counter = outside.path().join("runs.log");
            // The "test suite" records each run and fails while `fail` exists.
            let cmd = format!("echo run >> {}; test ! -e fail", counter.display());
            let p = project(root, &cmd, OnFailure::RetryThenAsk);
            let rt = SystemRuntime;
            let runs = || std::fs::read_to_string(&counter).map_or(0, |t| t.lines().count());

            // Clean tree: nothing to check, tests not run.
            assert_eq!(rt.turn_end(&p, "s").unwrap(), Verdict::Allow);
            assert_eq!(runs(), 0);

            // A change with failing tests: retry, budget 2.
            sh(root, "echo two > a.txt; touch fail");
            assert!(matches!(
                rt.turn_end(&p, "s").unwrap(),
                Verdict::RetryAgent { .. }
            ));
            assert_eq!(state::load(root, "s").retries, 1);
            // A failed check is never skipped, even if the tree is the same.
            assert!(matches!(
                rt.turn_end(&p, "s").unwrap(),
                Verdict::RetryAgent { .. }
            ));
            assert_eq!(state::load(root, "s").retries, 2);
            // Budget used up: hand back to the user, and the counter starts over.
            assert!(matches!(
                rt.turn_end(&p, "s").unwrap(),
                Verdict::AskHuman { .. }
            ));
            assert_eq!(state::load(root, "s").retries, 0);
            assert_eq!(runs(), 3);

            // Fixed: tests pass, counter stays at zero.
            sh(root, "rm fail; echo three > a.txt");
            assert_eq!(rt.turn_end(&p, "s").unwrap(), Verdict::Allow);
            assert_eq!(runs(), 4);
            assert!(state::load(root, "s").last_ok);

            // Same tree again: skipped.
            assert_eq!(rt.turn_end(&p, "s").unwrap(), Verdict::Allow);
            assert_eq!(runs(), 4);

            // An edit to an untracked file is a change, even though the file list is the same.
            sh(root, "echo new > b.txt");
            assert_eq!(rt.turn_end(&p, "s").unwrap(), Verdict::Allow);
            assert_eq!(runs(), 5);
            std::thread::sleep(Duration::from_millis(20));
            sh(root, "echo newer, longer > b.txt");
            assert_eq!(rt.turn_end(&p, "s").unwrap(), Verdict::Allow);
            assert_eq!(runs(), 6);

            // Other sessions have their own counters.
            assert_eq!(state::load(root, "other").retries, 0);
            // The state directory does not show up in git status.
            let status = git(root, &["status", "--porcelain"]).unwrap();
            assert!(!status.contains(".reflex"), "{status}");
        }

        #[test]
        fn turn_end_with_no_test_command_and_a_large_diff_notifies() {
            let repo = repo_with_commit();
            let root = repo.path();
            let mut p = project(root, "", OnFailure::RetryThenAsk);
            p.config.review.max_diff_lines = 5;
            sh(root, "seq 1 50 > big.txt");
            let v = SystemRuntime.turn_end(&p, "s").unwrap();
            assert!(matches!(v, Verdict::Notify { .. }), "{v:?}");
        }

        #[test]
        fn turn_end_works_in_a_repository_without_commits() {
            let tmp = tempfile::tempdir().unwrap();
            sh(
                tmp.path(),
                "git init -q . && echo x > a.txt && git add a.txt",
            );
            let p = project(tmp.path(), "true", OnFailure::Ask);
            assert_eq!(SystemRuntime.turn_end(&p, "s").unwrap(), Verdict::Allow);
            assert!(state::load(tmp.path(), "s").last_ok);
        }

        #[test]
        fn detect_env_finds_root_and_hooks_dir() {
            let repo = repo_with_commit();
            let sub = repo.path().join("src");
            std::fs::create_dir(&sub).unwrap();
            let env = detect_env(&sub);
            let root = std::fs::canonicalize(repo.path()).unwrap();
            assert_eq!(std::fs::canonicalize(&env.root).unwrap(), root);
            let hooks = std::fs::canonicalize(env.git_hooks_dir.unwrap()).unwrap();
            assert_eq!(hooks, root.join(".git/hooks"));
            assert!(!env.hooks_dir_in_repo);

            sh(repo.path(), "git config core.hooksPath .githooks");
            let env = detect_env(repo.path());
            assert!(env.hooks_dir_in_repo);
        }
    }
}
