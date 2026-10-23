//! Metamorphic tests: rewrite a command without changing what it does and the verdict
//! must not change. Unlike `guard_corpus.tsv` these cases need no expected answer — the
//! original command's verdict is the oracle — so they measure parser consistency on
//! shapes nobody wrote a rule for.
//!
//! Transformations: extra whitespace, a `true;` in front or a `; true` after, a trailing
//! comment, a backslash-newline between the first two words, wrapping in `{ …; }`,
//! `bash -c '…'` / `sh -c '…'`, and the `env X=1` / `command` prefixes. Commands with
//! newlines (heredocs) only get the transformations that keep the terminator line valid.

use reflex_hooks::config::Config;
use reflex_hooks::guard::{Guard, Verdict};
use std::path::Path;

fn class(verdict: &Verdict) -> &'static str {
    match verdict {
        Verdict::Allow => "allow",
        Verdict::Block { .. } => "block",
        Verdict::Ask { .. } => "ask",
        other => panic!("not a command verdict: {other:?}"),
    }
}

/// Single-quote a script for `bash -c` / `sh -c`: `'` becomes `'\''`.
fn quote(script: &str) -> String {
    format!("'{}'", script.replace('\'', "'\\''"))
}

/// Byte offset of the first space that is outside quotes and not escaped.
fn first_unquoted_space(text: &str) -> Option<usize> {
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for (i, c) in text.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match quote {
            Some(q) if c == q => quote = None,
            Some('"') if c == '\\' => escaped = true,
            Some(_) => {}
            None if c == '\\' => escaped = true,
            None if c == '\'' || c == '"' => quote = Some(c),
            None if c == ' ' || c == '\t' => return Some(i),
            None => {}
        }
    }
    None
}

/// First word of a simple command, used to skip wrappers that cannot run shell builtins.
fn first_word(command: &str) -> &str {
    let command = command.trim_start();
    let end = first_unquoted_space(command).unwrap_or(command.len());
    command[..end].trim_start_matches(['(', '{', '!', ' '])
}

const BUILTINS: &[&str] = &[
    "source", ".", "set", "export", "unset", "cd", "eval", "exec", "for", "if", "while", "until",
    "case", "select", "alias", "local", "return", "read", "test", "[",
];

fn is_builtin(command: &str) -> bool {
    let word = first_word(command).trim_end_matches(';');
    BUILTINS.contains(&word)
}

fn rewrites(command: &str) -> Vec<(&'static str, String)> {
    let multiline = command.contains('\n');
    let mut out = vec![
        ("leading spaces", format!("   {command}")),
        ("bash -c", format!("bash -c {}", quote(command))),
        ("sh -c", format!("sh -c {}", quote(command))),
    ];
    if !multiline {
        // A trailing backslash escapes whatever comes next (this corpus has Windows
        // `cmd.exe` shapes), so appending to the line would change what it means.
        if !command.trim_end().ends_with('\\') {
            out.push(("trailing spaces", format!("{command}   ")));
            out.push(("after true", format!("{command}; true")));
            out.push(("trailing comment", format!("{command} # note")));
            out.push(("brace group", format!("{{ {command}; }}")));
        }
        out.push(("before true", format!("true; {command}")));
        if let Some(at) = first_unquoted_space(command) {
            let (head, tail) = command.split_at(at);
            out.push(("backslash-newline", format!("{head} \\\n{}", &tail[1..])));
        }
    }
    if !is_builtin(command) {
        out.push(("env prefix", format!("env REFLEX_TEST=1 {command}")));
        out.push(("command prefix", format!("command {command}")));
    }
    out
}

fn corpus() -> Vec<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/guard_corpus.tsv");
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
        .map(|line| {
            let command = line.splitn(3, '\t').nth(2).expect("three columns");
            command.replace("\\n", "\n")
        })
        .collect()
}

#[test]
fn rewrites_do_not_change_the_verdict() {
    let guard = Guard::from_config(
        &Config::default(),
        Path::new("/work/proj"),
        Some(Path::new("/home/me")),
    )
    .unwrap();
    let mut failures = Vec::new();
    let mut checked = 0usize;
    let mut by_shape: Vec<(&'static str, usize, usize)> = Vec::new();
    for command in corpus() {
        let expected = class(&guard.check_command(&command, None));
        for (shape, rewritten) in rewrites(&command) {
            checked += 1;
            let got = class(&guard.check_command(&rewritten, None));
            let slot = match by_shape.iter().position(|(name, _, _)| *name == shape) {
                Some(i) => i,
                None => {
                    by_shape.push((shape, 0, 0));
                    by_shape.len() - 1
                }
            };
            by_shape[slot].2 += 1;
            if got == expected {
                by_shape[slot].1 += 1;
            } else {
                failures.push(format!(
                    "{shape}: expected {expected}, got {got}\n    from: {command:?}\n    to:   {rewritten:?}"
                ));
            }
        }
    }
    for (shape, ok, all) in &by_shape {
        println!("{shape}: {ok}/{all}");
    }
    println!("{checked} rewritten commands checked");
    assert!(
        failures.is_empty(),
        "\n{} rewrites changed the verdict:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
