//! Guard logic behind the agent hooks. Pure functions only: no file, process or
//! environment access, so every rule can be unit-tested with plain values.
//!
//! Three checks feed one [`Verdict`] type:
//!
//! * [`PathPolicy`]: is this path protected by `protect.paths`?
//! * [`check_command`]: does this shell command write to a protected path, or look
//!   dangerous? This is a heuristic (see the function docs), not a sandbox.
//! * [`decide_turn_end`]: what should happen when the agent says it is done?
//!
//! # Turn-end outcomes
//!
//! Turn-end evidence is turned into a `DeterministicEvidence`, run through
//! `GuardedHybridComposer` (no semantic signals, default thresholds), and the result is
//! mapped to a hook verdict:
//!
//! | Situation                                             | Outcome                        |
//! |-------------------------------------------------------|--------------------------------|
//! | tests passed, diff <= `max_diff_lines`                | `Allow` (silent)               |
//! | tests passed, diff > `max_diff_lines`                 | `Notify` "large change"        |
//! | tests not run (no command), diff small                | `Allow`                        |
//! | tests not run, diff large                             | `Notify` "large change"        |
//! | tests failed, `retry-then-ask`, retries < max         | `RetryAgent` with output tail  |
//! | tests failed, `retry-then-ask`, retries >= max        | `AskHuman`                     |
//! | tests failed, `ask`                                   | `AskHuman`                     |
//! | tests failed, `notify`                                | `Notify`                       |
//!
//! Only the "tests passed" row depends on the composer: a pass is accepted only if the
//! composer also accepts. Failures are always handled by the mapping above, because the
//! composer without semantic signals treats every failure as non-transient and would
//! escalate at once, which is the `ask` behaviour. `retry-then-ask` needs its own budget.

use crate::config::{Config, OnFailure};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use reflex_core::{DeterministicEvidence, EvidenceVector, ReflexAction};
use reflex_policy::composer::{DecisionComposer, GuardedHybridComposer, GuardedHybridConfig};
use std::path::Path;

/// Number of trailing test-output lines sent back to the agent.
pub const OUTPUT_TAIL_LINES: usize = 40;

/// What a hook should do. Adapters translate this into their agent's protocol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Let the action or the stop through, without saying anything.
    Allow,
    /// Refuse the action. The reason is shown to the agent.
    Block { reason: String },
    /// Ask the user to confirm the action before it runs.
    Ask { reason: String },
    /// Turn end only: keep the agent working and hand it the reason.
    RetryAgent { reason: String },
    /// Turn end only: let the agent stop and tell the user why they should look.
    AskHuman { reason: String },
    /// Turn end only: let the agent stop and show the user a short message.
    Notify { message: String },
}

#[derive(Debug, thiserror::Error)]
pub enum GuardError {
    #[error("invalid protect.paths pattern `{pattern}`: {message}")]
    BadPattern { pattern: String, message: String },
}

// ─────────────────────────────────────────────────────────────────────────────
// Path policy
// ─────────────────────────────────────────────────────────────────────────────

/// Matcher over `protect.paths`.
///
/// Pattern rules:
/// * A pattern without `/` matches the file name at any depth (`.env` matches
///   `app/.env`).
/// * A pattern with `/` is relative to the project root (`secrets/**`). A leading `/`
///   is allowed and means the same; a trailing `/` means "everything below".
/// * Matching ignores case, since the common desktop file systems do.
///
/// Paths may use `/` or `\`, be relative or absolute, and may contain `.` and `..`.
/// Absolute paths inside the project root are made relative to it. Absolute paths
/// outside the root are still checked, so a name-only pattern such as `*.pem` also
/// covers `~/keys/server.pem`; root-relative patterns do not match outside the root.
#[derive(Debug, Clone)]
pub struct PathPolicy {
    root: String,
    patterns: Vec<String>,
    set: GlobSet,
}

impl PathPolicy {
    pub fn new(patterns: &[String], root: &Path) -> Result<PathPolicy, GuardError> {
        let mut builder = GlobSetBuilder::new();
        let mut kept = Vec::new();
        for raw in patterns {
            let Some(glob) = expand_pattern(raw) else {
                continue;
            };
            let compiled = GlobBuilder::new(&glob)
                .literal_separator(true)
                .case_insensitive(true)
                .build()
                .map_err(|e| GuardError::BadPattern {
                    pattern: raw.clone(),
                    message: e.kind().to_string(),
                })?;
            builder.add(compiled);
            kept.push(raw.trim().to_string());
        }
        let set = builder.build().map_err(|e| GuardError::BadPattern {
            pattern: patterns.join(", "),
            message: e.to_string(),
        })?;
        Ok(PathPolicy {
            root: normalize_separators(&root.to_string_lossy()),
            patterns: kept,
            set,
        })
    }

    /// Returns the first configured pattern that protects `path`.
    pub fn matching_pattern(&self, path: &str) -> Option<&str> {
        let rel = self.relativize(path);
        if rel.is_empty() {
            return None;
        }
        self.set
            .matches(&rel)
            .into_iter()
            .min()
            .map(|i| self.patterns[i].as_str())
    }

    pub fn is_protected(&self, path: &str) -> bool {
        self.matching_pattern(path).is_some()
    }

    /// Normalizes `path` to the form the globs are matched against.
    fn relativize(&self, path: &str) -> String {
        let collapsed = collapse_dots(&normalize_separators(path.trim()));
        let root = collapse_dots(&self.root);
        let windows = is_drive_path(&collapsed) || is_drive_path(&root);
        let stripped = strip_prefix_dir(&collapsed, &root, windows);
        let rel = match stripped {
            Some(rest) => rest.to_string(),
            None => collapsed,
        };
        // Whatever is left that is still absolute lies outside the root: drop the
        // drive or leading slash so name-only patterns (`**/x`) can still match.
        let rel = if is_drive_path(&rel) {
            rel[2..].to_string()
        } else {
            rel
        };
        rel.trim_start_matches('/').to_string()
    }
}

/// Turns one config pattern into a glob, or `None` for an empty entry.
fn expand_pattern(raw: &str) -> Option<String> {
    let mut p = normalize_separators(raw.trim());
    while let Some(rest) = p.strip_prefix("./") {
        p = rest.to_string();
    }
    if p.is_empty() {
        return None;
    }
    if p.ends_with('/') {
        p.push_str("**");
    }
    if let Some(rest) = p.strip_prefix('/') {
        return Some(rest.to_string());
    }
    if p.contains('/') {
        Some(p)
    } else {
        Some(format!("**/{p}"))
    }
}

fn normalize_separators(p: &str) -> String {
    p.replace('\\', "/")
}

fn is_drive_path(p: &str) -> bool {
    let b = p.as_bytes();
    b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
}

/// Lexically resolves `.` and `..` segments and repeated slashes.
fn collapse_dots(p: &str) -> String {
    let absolute = p.starts_with('/');
    let (drive, rest) = if is_drive_path(p) {
        p.split_at(2)
    } else {
        ("", p)
    };
    let rooted = absolute || rest.starts_with('/');
    let mut out: Vec<&str> = Vec::new();
    for seg in rest.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                if matches!(out.last(), Some(last) if *last != "..") {
                    out.pop();
                } else if !rooted {
                    out.push("..");
                }
            }
            s => out.push(s),
        }
    }
    let joined = out.join("/");
    if rooted {
        format!("{drive}/{joined}")
    } else {
        format!("{drive}{joined}")
    }
}

/// If `path` is `root` or lies below it, returns the part below the root.
fn strip_prefix_dir<'a>(path: &'a str, root: &str, ignore_case: bool) -> Option<&'a str> {
    let root = root.trim_end_matches('/');
    if root.is_empty() || !path.is_char_boundary(root.len().min(path.len())) {
        return None;
    }
    if path.len() < root.len() {
        return None;
    }
    let (head, tail) = path.split_at(root.len());
    let same = if ignore_case {
        head.eq_ignore_ascii_case(root)
    } else {
        head == root
    };
    if !same {
        return None;
    }
    if tail.is_empty() {
        return Some("");
    }
    tail.strip_prefix('/')
}

// ─────────────────────────────────────────────────────────────────────────────
// Shell command check
// ─────────────────────────────────────────────────────────────────────────────

/// Checks a shell command line before an agent runs it.
///
/// This is a heuristic over the command text, not a sandbox. It splits the line into
/// simple commands (on `;`, `&&`, `||`, `|`, `&`, newlines, parentheses), understands
/// quotes, and looks for:
///
/// * writes to a protected path: output redirection (`>`, `>>`, `&>`), `tee`, the
///   targets of `cp`, `mv`, `rm`, `install`, `sed -i`, and `dd of=`
/// * `rm` with a recursive flag on a broad target (`/`, `~`, `.`, `*`, `/usr`, ...)
/// * `git push --force` / `-f` / `+refspec`
/// * `git commit --no-verify` / `-n`, which skips the installed pre-commit hook
///
/// It also looks inside `sh -c '...'` and `bash -c '...'`. It does not expand variables,
/// globs or `$(...)`, and it cannot see writes made by other programs, so a script or
/// an interpreter one-liner can still touch a protected file. Reads (`cat .env`) are
/// not checked.
///
/// A protected-path write gives `Block`; a dangerous command gives `Ask`; the first
/// `Block` wins over any `Ask`.
pub fn check_command(command: &str, policy: &PathPolicy) -> Verdict {
    let mut ask: Option<String> = None;
    for finding in analyze(command, policy, 0) {
        match finding {
            Finding::Protected { path, pattern } => {
                return Verdict::Block {
                    reason: protected_reason(&path, &pattern, "this command writes to"),
                };
            }
            Finding::Dangerous(why) => {
                ask.get_or_insert(why);
            }
        }
    }
    match ask {
        Some(why) => Verdict::Ask {
            reason: format!("Reflex Control: {why}. Confirm before it runs."),
        },
        None => Verdict::Allow,
    }
}

/// Verdict for a tool that writes directly to the given paths (edit/write tools).
pub fn check_paths<S: AsRef<str>>(paths: &[S], policy: &PathPolicy) -> Verdict {
    for path in paths {
        let path = path.as_ref();
        if let Some(pattern) = policy.matching_pattern(path) {
            return Verdict::Block {
                reason: protected_reason(path, pattern, "this edit writes to"),
            };
        }
    }
    Verdict::Allow
}

/// Verdict for a commit that stages the given files (git pre-commit hook).
pub fn check_staged<S: AsRef<str>>(files: &[S], policy: &PathPolicy) -> Verdict {
    let hits: Vec<String> = files
        .iter()
        .filter_map(|f| {
            let f = f.as_ref();
            policy
                .matching_pattern(f)
                .map(|pattern| format!("  {f} (matches `{pattern}`)"))
        })
        .collect();
    if hits.is_empty() {
        return Verdict::Allow;
    }
    Verdict::Block {
        reason: format!(
            "Reflex Control: commit blocked, protected files are staged:\n{}\n\
             To allow them, edit `protect.paths` in .reflex.toml. To leave them out of \
             this commit, run `git restore --staged <file>`.",
            hits.join("\n")
        ),
    }
}

fn protected_reason(path: &str, pattern: &str, what: &str) -> String {
    format!(
        "Reflex Control: {what} `{path}`, which is protected (matches `{pattern}`). \
         Do not change it. To allow it, the user can edit `protect.paths` in .reflex.toml."
    )
}

enum Finding {
    Protected { path: String, pattern: String },
    Dangerous(String),
}

/// One simple command: its words and the targets of its output redirections.
#[derive(Debug, Default)]
struct Segment {
    words: Vec<String>,
    out_targets: Vec<String>,
}

fn analyze(command: &str, policy: &PathPolicy, depth: usize) -> Vec<Finding> {
    let mut findings = Vec::new();
    for seg in split_segments(command) {
        for target in &seg.out_targets {
            check_target(target, policy, &mut findings);
        }
        analyze_segment(&seg, policy, depth, &mut findings);
    }
    findings
}

/// Like [`check_target`], but `target` may be a directory: a protected directory is
/// caught by probing a file below it.
fn check_target_or_dir(target: &str, policy: &PathPolicy, findings: &mut Vec<Finding>) {
    check_target(target, policy, findings);
    let dir = target.trim_end_matches(['/', '\\']);
    if !dir.is_empty() {
        let before = findings.len();
        check_target(&format!("{dir}/x"), policy, findings);
        // Report the path the user typed, not the probe.
        for f in &mut findings[before..] {
            if let Finding::Protected { path, .. } = f {
                *path = target.to_string();
            }
        }
    }
}

fn check_target(target: &str, policy: &PathPolicy, findings: &mut Vec<Finding>) {
    if target.is_empty() || target.starts_with("/dev/") {
        return;
    }
    if let Some(pattern) = policy.matching_pattern(target) {
        findings.push(Finding::Protected {
            path: target.to_string(),
            pattern: pattern.to_string(),
        });
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Pending {
    None,
    Out,
    In,
}

/// Accumulates words and segments while [`split_segments`] scans the input.
struct Splitter {
    segments: Vec<Segment>,
    seg: Segment,
    word: String,
    has_word: bool,
    pending: Pending,
}

impl Splitter {
    fn flush_word(&mut self) {
        if !self.has_word {
            return;
        }
        let w = std::mem::take(&mut self.word);
        match std::mem::replace(&mut self.pending, Pending::None) {
            Pending::Out => self.seg.out_targets.push(w),
            Pending::In => {}
            Pending::None => self.seg.words.push(w),
        }
        self.has_word = false;
    }

    fn end_segment(&mut self) {
        self.flush_word();
        self.pending = Pending::None;
        if !self.seg.words.is_empty() || !self.seg.out_targets.is_empty() {
            self.segments.push(std::mem::take(&mut self.seg));
        }
    }
}

/// Splits a command line into simple commands. Understands single and double quotes,
/// backslash escapes and the redirection operators; everything else is left as words.
fn split_segments(command: &str) -> Vec<Segment> {
    let chars: Vec<char> = command.chars().collect();
    let mut sp = Splitter {
        segments: Vec::new(),
        seg: Segment::default(),
        word: String::new(),
        has_word: false,
        pending: Pending::None,
    };
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];
        match c {
            '\'' => {
                sp.has_word = true;
                i += 1;
                while i < chars.len() && chars[i] != '\'' {
                    sp.word.push(chars[i]);
                    i += 1;
                }
                i += 1;
            }
            '"' => {
                sp.has_word = true;
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    if chars[i] == '\\' && i + 1 < chars.len() {
                        i += 1;
                    }
                    sp.word.push(chars[i]);
                    i += 1;
                }
                i += 1;
            }
            '\\' if i + 1 < chars.len() => {
                sp.has_word = true;
                sp.word.push(chars[i + 1]);
                i += 2;
            }
            c if c.is_whitespace() && c != '\n' => {
                sp.flush_word();
                i += 1;
            }
            '\n' | ';' | '(' | ')' | '|' => {
                sp.end_segment();
                i += 1;
            }
            '&' if chars.get(i + 1) == Some(&'>') => {
                // `&>file` / `&>>file`: redirect both streams.
                sp.flush_word();
                i += 1;
                sp.pending = Pending::Out;
            }
            '&' => {
                sp.end_segment();
                i += 1;
            }
            '>' | '<' => {
                // A bare file-descriptor number in front of the operator is part of it.
                if sp.has_word && sp.word.chars().all(|d| d.is_ascii_digit()) {
                    sp.word.clear();
                    sp.has_word = false;
                } else {
                    sp.flush_word();
                }
                let out = c == '>';
                while i < chars.len() && chars[i] == c {
                    i += 1;
                }
                if out && chars.get(i) == Some(&'|') {
                    i += 1;
                }
                if out && chars.get(i) == Some(&'&') {
                    // `2>&1` style duplication has no file target.
                    i += 1;
                    while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '-') {
                        i += 1;
                    }
                    sp.pending = Pending::None;
                } else {
                    sp.pending = if out { Pending::Out } else { Pending::In };
                }
            }
            c => {
                sp.has_word = true;
                sp.word.push(c);
                i += 1;
            }
        }
    }
    sp.end_segment();
    sp.segments
}

const WRAPPERS: &[&str] = &[
    "sudo", "doas", "env", "command", "nohup", "time", "exec", "nice", "builtin",
];

fn is_assignment(word: &str) -> bool {
    match word.split_once('=') {
        Some((name, _)) => {
            !name.is_empty()
                && !name.starts_with(|c: char| c.is_ascii_digit())
                && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        }
        None => false,
    }
}

fn base_name(word: &str) -> &str {
    word.rsplit(['/', '\\']).next().unwrap_or(word)
}

fn is_flag(word: &str) -> bool {
    word.len() > 1 && word.starts_with('-')
}

/// True for a short-option cluster such as `-rf` that contains `letter`.
fn has_short_flag(word: &str, letters: &[char]) -> bool {
    word.starts_with('-')
        && !word.starts_with("--")
        && word.len() > 1
        && word[1..].chars().all(|c| c.is_ascii_alphabetic())
        && word[1..].chars().any(|c| letters.contains(&c))
}

fn analyze_segment(seg: &Segment, policy: &PathPolicy, depth: usize, out: &mut Vec<Finding>) {
    let mut words: &[String] = &seg.words;
    // Skip `VAR=x`, `sudo`, `env -i`, ... in front of the real command.
    loop {
        match words.first() {
            Some(w) if is_assignment(w) => words = &words[1..],
            Some(w) if WRAPPERS.contains(&base_name(w)) => {
                words = &words[1..];
                while matches!(words.first(), Some(f) if is_flag(f) || is_assignment(f)) {
                    words = &words[1..];
                }
            }
            _ => break,
        }
    }
    let Some(cmd) = words.first() else {
        return;
    };
    let cmd = base_name(cmd);
    let args = &words[1..];
    let operands: Vec<&String> = args.iter().filter(|a| !is_flag(a)).collect();

    match cmd {
        "sh" | "bash" | "zsh" | "dash" | "ksh" => {
            if depth < 3 {
                if let Some(pos) = args
                    .iter()
                    .position(|a| a == "-c" || (has_short_flag(a, &['c'])))
                {
                    if let Some(script) = args.get(pos + 1) {
                        out.extend(analyze(script, policy, depth + 1));
                    }
                }
            }
        }
        "tee" => {
            for t in operands {
                check_target(t, policy, out);
            }
        }
        "cp" | "install" => {
            if let Some(dir) = target_directory(args) {
                check_target_or_dir(&dir, policy, out);
            } else if let Some(dest) = operands.last() {
                check_target_or_dir(dest, policy, out);
            }
        }
        "mv" => {
            // The sources disappear and the destination is overwritten.
            for t in &operands {
                check_target_or_dir(t, policy, out);
            }
            if let Some(dir) = target_directory(args) {
                check_target_or_dir(&dir, policy, out);
            }
        }
        "rm" | "unlink" | "shred" => {
            for t in &operands {
                check_target_or_dir(t, policy, out);
            }
            let recursive = args
                .iter()
                .any(|a| a == "--recursive" || has_short_flag(a, &['r', 'R']));
            if cmd == "rm"
                && recursive
                && (args.iter().any(|a| a == "--no-preserve-root")
                    || operands.iter().any(|t| is_broad_target(t)))
            {
                out.push(Finding::Dangerous(
                    "this command recursively deletes a broad target".to_string(),
                ));
            }
        }
        "sed" => {
            let in_place = args
                .iter()
                .any(|a| a.starts_with("--in-place") || has_short_flag(a, &['i']));
            if in_place {
                // Without -e/-f the first operand is the script, not a file.
                let script_flag = args.iter().any(|a| a == "-e" || a == "-f");
                let skip = if script_flag { 0 } else { 1 };
                for t in operands.iter().skip(skip) {
                    check_target(t, policy, out);
                }
            }
        }
        "dd" => {
            for a in args {
                if let Some(path) = a.strip_prefix("of=") {
                    check_target(path, policy, out);
                }
            }
        }
        "git" => {
            if is_forced_push(args) {
                out.push(Finding::Dangerous(
                    "this command force-pushes and can overwrite remote history".to_string(),
                ));
            }
            if skips_commit_hooks(args) {
                out.push(Finding::Dangerous(
                    "this command commits with --no-verify and skips the pre-commit checks"
                        .to_string(),
                ));
            }
        }
        _ => {}
    }
}

/// Value of `-t DIR` / `--target-directory=DIR` for cp, mv and install.
fn target_directory(args: &[String]) -> Option<String> {
    let mut iter = args.iter();
    while let Some(a) = iter.next() {
        if let Some(v) = a.strip_prefix("--target-directory=") {
            return Some(v.to_string());
        }
        if a == "-t" || a == "--target-directory" {
            return iter.next().cloned();
        }
    }
    None
}

fn is_broad_target(target: &str) -> bool {
    let t = normalize_separators(target);
    if matches!(
        t.as_str(),
        "/" | "/*"
            | "~"
            | "~/"
            | "~/*"
            | "$HOME"
            | "$HOME/"
            | "$HOME/*"
            | "${HOME}"
            | "."
            | "./"
            | ".."
            | "../"
            | "*"
            | "./*"
            | "../*"
            | ".*"
    ) {
        return true;
    }
    // A top-level directory such as /usr or /etc/.
    let trimmed = t.trim_end_matches('/');
    t.starts_with('/') && !trimmed.is_empty() && !trimmed[1..].contains('/')
}

/// Splits `git` arguments into the subcommand and its arguments, skipping git's own
/// options (`-C dir`, `-c key=val`, ...).
fn git_subcommand(args: &[String]) -> Option<(&str, &[String])> {
    let mut i = 0;
    while let Some(a) = args.get(i) {
        if a == "-C" || a == "-c" || a == "--git-dir" || a == "--work-tree" {
            i += 2;
        } else if is_flag(a) {
            i += 1;
        } else {
            return Some((a.as_str(), &args[i + 1..]));
        }
    }
    None
}

fn is_forced_push(args: &[String]) -> bool {
    let Some(("push", rest)) = git_subcommand(args) else {
        return false;
    };
    rest.iter().any(|a| {
        a == "--force"
            || (a.starts_with('-') && !a.starts_with("--") && has_short_flag(a, &['f']))
            || (a.starts_with('+') && a.len() > 1)
    })
}

/// `git commit --no-verify` (or `-n`) bypasses the pre-commit hook this crate installs.
fn skips_commit_hooks(args: &[String]) -> bool {
    let Some(("commit", rest)) = git_subcommand(args) else {
        return false;
    };
    rest.iter().any(|a| {
        a == "--no-verify"
            || (a.starts_with('-') && !a.starts_with("--") && has_short_flag(a, &['n']))
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Turn-end decision
// ─────────────────────────────────────────────────────────────────────────────

/// Result of running `tests.command` at the end of an agent turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestOutcome {
    Passed,
    Failed {
        output: String,
    },
    /// No test command is configured, or the tests were not run.
    NotRun,
}

/// Facts about the finished turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnEndInput {
    pub tests: TestOutcome,
    /// Added plus removed lines in the working tree.
    pub diff_lines: usize,
    /// How many times this session has already been sent back to fix the tests.
    pub retries_so_far: u32,
}

pub fn decide_turn_end(input: &TurnEndInput, cfg: &Config) -> Verdict {
    let max_retries = cfg.tests.max_retries;
    let command = cfg.tests.command.trim();

    if let TestOutcome::Failed { output } = &input.tests {
        return match cfg.tests.on_failure {
            OnFailure::Notify => Verdict::Notify {
                message: format!("Reflex Control: tests failed (`{command}`). Not retrying."),
            },
            OnFailure::Ask => Verdict::AskHuman {
                reason: format!(
                    "Reflex Control: tests failed (`{command}`), so the agent stopped for your review."
                ),
            },
            OnFailure::RetryThenAsk if input.retries_so_far < max_retries => Verdict::RetryAgent {
                reason: format!(
                    "Reflex Control: the tests failed (`{command}`). This is fix attempt {} of \
                     {max_retries}. Fix the failures, then finish again. Last {OUTPUT_TAIL_LINES} \
                     lines of output:\n\n{}",
                    input.retries_so_far + 1,
                    tail_lines(output, OUTPUT_TAIL_LINES)
                ),
            },
            OnFailure::RetryThenAsk => Verdict::AskHuman {
                reason: format!(
                    "Reflex Control: tests are still failing (`{command}`) after {max_retries} \
                     automatic fix attempts, so please take a look."
                ),
            },
        };
    }

    let tests_passed = match input.tests {
        TestOutcome::Passed => Some(true),
        _ => None,
    };
    let composer = GuardedHybridComposer::new(GuardedHybridConfig {
        max_retries: max_retries as usize,
        ..GuardedHybridConfig::default()
    });
    let decision = composer.compose(&EvidenceVector::new(DeterministicEvidence {
        tests_passed,
        retry_count: input.retries_so_far as usize,
        git_diff_size: input.diff_lines,
        worker_completed: true,
        ..DeterministicEvidence::default()
    }));

    if tests_passed == Some(true)
        && !matches!(
            decision.action,
            ReflexAction::Accept | ReflexAction::Terminate
        )
    {
        // Green tests that the engine still will not accept: fail closed.
        return Verdict::Notify {
            message: "Reflex Control: tests passed, but the change could not be confirmed. \
                      Please review."
                .to_string(),
        };
    }

    if input.diff_lines > cfg.review.max_diff_lines {
        return Verdict::Notify {
            message: format!(
                "Reflex Control: large change ({} lines changed, limit {}), please review.",
                input.diff_lines, cfg.review.max_diff_lines
            ),
        };
    }
    Verdict::Allow
}

/// Last `n` lines of `text`, without trailing blank lines.
pub fn tail_lines(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.trim_end().lines().collect();
    let start = lines.len().saturating_sub(n);
    lines[start..].join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::OnFailure;
    use std::path::PathBuf;

    fn policy_with(patterns: &[&str], root: &str) -> PathPolicy {
        let p: Vec<String> = patterns.iter().map(|s| s.to_string()).collect();
        PathPolicy::new(&p, &PathBuf::from(root)).unwrap()
    }

    fn default_policy() -> PathPolicy {
        PathPolicy::new(&Config::default().protect.paths, Path::new("/work/proj")).unwrap()
    }

    #[test]
    fn path_matching_table() {
        let default = default_policy();
        let custom = policy_with(&["config/prod.yaml", "vendor/", "*.sqlite"], "/work/proj");
        let win = policy_with(&[".env", "secrets/**"], "C:\\Users\\me\\proj");

        // (policy, path, protected?)
        let cases: Vec<(&PathPolicy, &str, bool)> = vec![
            // defaults
            (&default, ".env", true),
            (&default, ".env.local", true),
            (&default, ".env.production", true),
            (&default, "cert.pem", true),
            (&default, "server.key", true),
            (&default, "secrets/db.json", true),
            (&default, "secrets/nested/deep/x.txt", true),
            (&default, ".github/workflows/ci.yml", true),
            (&default, "migrations/001_init.sql", true),
            // nested: name patterns match at any depth
            (&default, "app/.env", true),
            (&default, "a/b/c/.env.test", true),
            (&default, "deploy/tls/site.pem", true),
            // root-relative patterns do not match at depth
            (&default, "src/secrets/x.rs", false),
            (&default, "docs/migrations/notes.md", false),
            (&default, "sub/.github/workflows/ci.yml", false),
            // not protected
            (&default, "src/main.rs", false),
            (&default, "README.md", false),
            (&default, "environment.rs", false),
            (&default, "src/env.rs", false),
            (&default, "secrets", false),
            // case is ignored
            (&default, "SECRETS/Key.txt", true),
            (&default, "Server.PEM", true),
            // custom patterns
            (&custom, "config/prod.yaml", true),
            (&custom, "config/dev.yaml", false),
            (&custom, "vendor/lib/x.c", true),
            (&custom, "data/app.sqlite", true),
            (&custom, ".env", false),
            // Windows separators
            (&default, "secrets\\db.json", true),
            (&default, "app\\.env", true),
            (&default, ".github\\workflows\\ci.yml", true),
            (&default, "src\\main.rs", false),
            // absolute paths under the root
            (&default, "/work/proj/.env", true),
            (&default, "/work/proj/secrets/a.txt", true),
            (&default, "/work/proj/src/main.rs", false),
            (&default, "/work/proj/src/secrets/a.txt", false),
            // absolute path outside the root: only name patterns apply
            (&default, "/home/me/keys/server.pem", true),
            (&default, "/home/me/secrets/a.txt", false),
            // a sibling directory sharing the root as a string prefix is outside it
            (&default, "/work/proj-other/secrets/a.txt", false),
            // `.` and `..` are resolved
            (&default, "./.env", true),
            (&default, "src/../.env", true),
            (&default, "src/./main.rs", false),
            (&default, "/work/proj/app/../secrets/x", true),
            // Windows absolute paths
            (&win, "C:\\Users\\me\\proj\\.env", true),
            (&win, "c:\\users\\me\\proj\\secrets\\k.txt", true),
            (&win, "C:/Users/me/proj/app/.env", true),
            (&win, "C:\\Users\\me\\proj\\src\\main.rs", false),
            (&win, "C:\\Users\\me\\proj\\src\\secrets\\k.txt", false),
        ];
        for (policy, path, expected) in cases {
            assert_eq!(
                policy.is_protected(path),
                expected,
                "path {path:?} expected protected={expected}"
            );
        }
    }

    #[test]
    fn matching_pattern_reports_the_config_entry() {
        let p = default_policy();
        assert_eq!(p.matching_pattern("app/.env.local"), Some(".env.*"));
        assert_eq!(p.matching_pattern("secrets/x"), Some("secrets/**"));
        assert_eq!(p.matching_pattern("src/lib.rs"), None);
    }

    #[test]
    fn bad_pattern_is_an_error_and_empty_entries_are_skipped() {
        let bad = PathPolicy::new(&["[".to_string()], Path::new("/x"));
        assert!(matches!(bad, Err(GuardError::BadPattern { .. })));
        let p = PathPolicy::new(&["".to_string(), "  ".to_string()], Path::new("/x")).unwrap();
        assert!(!p.is_protected(".env"));
    }

    fn verdict_kind(v: &Verdict) -> &'static str {
        match v {
            Verdict::Allow => "allow",
            Verdict::Block { .. } => "block",
            Verdict::Ask { .. } => "ask",
            Verdict::RetryAgent { .. } => "retry",
            Verdict::AskHuman { .. } => "ask-human",
            Verdict::Notify { .. } => "notify",
        }
    }

    #[test]
    fn shell_heuristics_table() {
        let policy = default_policy();
        let cases = [
            // writes to protected paths are blocked
            ("echo SECRET=1 > .env", "block"),
            ("echo SECRET=1 >> .env", "block"),
            ("echo x>.env", "block"),
            ("cat foo 2> .env.local", "block"),
            ("echo hi &> secrets/out.txt", "block"),
            ("echo hi | tee secrets/out.txt", "block"),
            ("echo hi | tee -a app/.env", "block"),
            ("cp backup.txt .env", "block"),
            ("cp -t secrets/ a.txt", "block"),
            ("mv .env .env.bak", "block"),
            ("mv new.pem old.pem", "block"),
            ("rm .env", "block"),
            ("rm -f secrets/db.json", "block"),
            ("rm -rf secrets", "block"),
            ("cp a.txt secrets", "block"),
            ("sed -i 's/a/b/' .env", "block"),
            ("sed --in-place -e 's/a/b/' migrations/001.sql", "block"),
            ("dd if=/dev/zero of=server.key", "block"),
            ("cd app && echo x > .env", "block"),
            ("FOO=1 sudo tee .env", "block"),
            ("bash -c 'echo x > .env'", "block"),
            ("sh -c \"cp a.txt secrets/a.txt\"", "block"),
            ("echo x > \".env\"", "block"),
            ("/bin/rm .env", "block"),
            // reads and harmless commands pass this layer
            ("cat .env", "allow"),
            ("grep KEY .env", "allow"),
            ("cp .env /tmp/copy", "allow"),
            ("cargo test", "allow"),
            ("ls -la", "allow"),
            ("echo hi > out.txt", "allow"),
            ("echo hi > /dev/null", "allow"),
            ("cargo test 2>&1 | tail -n 5", "allow"),
            ("sed -i 's/a/b/' src/main.rs", "allow"),
            ("sed 's/a/b/' .env", "allow"),
            ("echo \"a > .env\"", "allow"),
            ("git commit -m 'update .env docs'", "allow"),
            ("rm -rf target", "allow"),
            ("git push origin main", "allow"),
            ("git push --force-with-lease origin feature", "allow"),
            ("git commit -am 'fix'", "allow"),
            ("git push --no-verify origin main", "allow"),
            // dangerous commands ask
            ("rm -rf /", "ask"),
            ("rm -rf /*", "ask"),
            ("rm -rf ~", "ask"),
            ("rm -fr .", "ask"),
            ("rm -r *", "ask"),
            ("rm --recursive --force /usr", "ask"),
            ("sudo rm -rf /", "ask"),
            ("git push --force", "ask"),
            ("git push -f origin main", "ask"),
            ("git push origin +main", "ask"),
            ("git -C repo push --force origin main", "ask"),
            ("bash -c 'rm -rf /'", "ask"),
            ("git commit --no-verify -m 'wip'", "ask"),
            ("git commit -nm 'wip'", "ask"),
            ("git -C repo commit -n", "ask"),
            // block wins over ask
            ("rm -rf / && echo x > .env", "block"),
        ];
        for (cmd, expected) in cases {
            let got = verdict_kind(&check_command(cmd, &policy));
            assert_eq!(got, expected, "command {cmd:?}");
        }
    }

    #[test]
    fn block_reason_names_the_file_and_how_to_allow_it() {
        let v = check_command("echo x > app/.env", &default_policy());
        let Verdict::Block { reason } = v else {
            panic!("expected block");
        };
        assert!(reason.contains("app/.env"));
        assert!(reason.contains("`.env`"));
        assert!(reason.contains("protect.paths"));
    }

    #[test]
    fn staged_check_lists_every_protected_file() {
        let p = default_policy();
        assert_eq!(check_staged(&["src/a.rs", "README.md"], &p), Verdict::Allow);
        let Verdict::Block { reason } = check_staged(&["src/a.rs", ".env", "k/x.pem"], &p) else {
            panic!("expected block");
        };
        assert!(reason.contains("  .env (matches `.env`)"));
        assert!(reason.contains("  k/x.pem (matches `*.pem`)"));
        assert!(!reason.contains("src/a.rs"));
        assert!(reason.contains(".reflex.toml"));
    }

    #[test]
    fn check_paths_blocks_first_protected() {
        let p = default_policy();
        assert_eq!(check_paths(&["src/a.rs", "b.rs"], &p), Verdict::Allow);
        assert!(matches!(
            check_paths(&["src/a.rs", "/work/proj/.env"], &p),
            Verdict::Block { .. }
        ));
        assert_eq!(check_paths::<&str>(&[], &p), Verdict::Allow);
    }

    fn cfg(on_failure: OnFailure, max_retries: u32, max_diff: usize) -> Config {
        let mut c = Config::default();
        c.tests.on_failure = on_failure;
        c.tests.max_retries = max_retries;
        c.review.max_diff_lines = max_diff;
        c
    }

    fn input(tests: TestOutcome, diff: usize, retries: u32) -> TurnEndInput {
        TurnEndInput {
            tests,
            diff_lines: diff,
            retries_so_far: retries,
        }
    }

    fn failed() -> TestOutcome {
        TestOutcome::Failed {
            output: "test foo ... FAILED".to_string(),
        }
    }

    #[test]
    fn turn_end_mapping_table() {
        use OnFailure::*;
        let cases: Vec<(TestOutcome, usize, u32, OnFailure, &str)> = vec![
            (TestOutcome::Passed, 10, 0, RetryThenAsk, "allow"),
            (TestOutcome::Passed, 800, 0, RetryThenAsk, "allow"),
            (TestOutcome::Passed, 801, 0, RetryThenAsk, "notify"),
            (TestOutcome::Passed, 10, 2, Ask, "allow"),
            (TestOutcome::NotRun, 10, 0, RetryThenAsk, "allow"),
            (TestOutcome::NotRun, 5000, 0, RetryThenAsk, "notify"),
            // retry-then-ask with max_retries = 2
            (failed(), 10, 0, RetryThenAsk, "retry"),
            (failed(), 10, 1, RetryThenAsk, "retry"),
            (failed(), 10, 2, RetryThenAsk, "ask-human"),
            (failed(), 10, 7, RetryThenAsk, "ask-human"),
            // failure wins over a large diff
            (failed(), 5000, 0, RetryThenAsk, "retry"),
            // ask: immediately, whatever the retry count
            (failed(), 10, 0, Ask, "ask-human"),
            (failed(), 10, 1, Ask, "ask-human"),
            // notify: never retries, never asks
            (failed(), 10, 0, Notify, "notify"),
            (failed(), 10, 5, Notify, "notify"),
        ];
        for (tests, diff, retries, mode, expected) in cases {
            let c = cfg(mode, 2, 800);
            let got = verdict_kind(&decide_turn_end(&input(tests.clone(), diff, retries), &c));
            assert_eq!(
                got, expected,
                "tests={tests:?} diff={diff} retries={retries} mode={mode}"
            );
        }
    }

    #[test]
    fn max_retries_zero_asks_at_once() {
        let c = cfg(OnFailure::RetryThenAsk, 0, 800);
        assert!(matches!(
            decide_turn_end(&input(failed(), 1, 0), &c),
            Verdict::AskHuman { .. }
        ));
    }

    #[test]
    fn retry_reason_carries_the_last_40_lines() {
        let output: String = (1..=100).map(|n| format!("line {n}\n")).collect();
        let v = decide_turn_end(
            &input(TestOutcome::Failed { output }, 1, 0),
            &cfg(OnFailure::RetryThenAsk, 2, 800),
        );
        let Verdict::RetryAgent { reason } = v else {
            panic!("expected retry");
        };
        assert!(reason.contains("attempt 1 of 2"));
        assert!(reason.contains("line 100"));
        assert!(reason.contains("line 61"));
        assert!(!reason.contains("line 60\n"));
        assert!(!reason.contains("line 1\n"));
    }

    #[test]
    fn ask_human_reason_is_one_sentence_and_names_the_command() {
        let c = cfg(OnFailure::RetryThenAsk, 2, 800);
        let Verdict::AskHuman { reason } = decide_turn_end(&input(failed(), 1, 2), &c) else {
            panic!("expected ask-human");
        };
        assert!(reason.contains("cargo test"));
        assert!(!reason.contains('\n'));
        assert!(reason.ends_with('.'));
        assert_eq!(reason.matches('.').count(), 1);
    }

    #[test]
    fn large_change_message_says_review() {
        let Verdict::Notify { message } = decide_turn_end(
            &input(TestOutcome::Passed, 900, 0),
            &cfg(OnFailure::Ask, 2, 800),
        ) else {
            panic!("expected notify");
        };
        assert!(message.contains("large change"));
        assert!(message.contains("review"));
    }

    #[test]
    fn tail_lines_keeps_the_end() {
        assert_eq!(tail_lines("a\nb\nc\n\n", 2), "b\nc");
        assert_eq!(tail_lines("a", 5), "a");
        assert_eq!(tail_lines("", 5), "");
    }
}
