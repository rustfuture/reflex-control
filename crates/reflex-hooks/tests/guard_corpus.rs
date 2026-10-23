//! Runs `tests/fixtures/guard_corpus.tsv`: real-world shell commands and the verdict the
//! default settings must give each one. See the file header for the format.

use reflex_hooks::config::Config;
use reflex_hooks::guard::{Guard, Verdict};
use std::path::Path;

#[test]
fn guard_corpus() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/guard_corpus.tsv");
    let text = std::fs::read_to_string(&path).unwrap();
    let guard = Guard::from_config(
        &Config::default(),
        Path::new("/work/proj"),
        Some(Path::new("/home/me")),
    )
    .unwrap();

    let mut failures = Vec::new();
    let mut counts = [0usize; 3];
    for (n, line) in text.lines().enumerate() {
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let mut fields = line.splitn(3, '\t');
        let (Some(expected), Some(rule), Some(command)) =
            (fields.next(), fields.next(), fields.next())
        else {
            panic!("line {}: expected <verdict>\\t<rule>\\t<command>", n + 1);
        };
        let command = command.replace("\\n", "\n");
        let verdict = guard.check_command(&command, None);
        let (got, reason) = match &verdict {
            Verdict::Allow => ("allow", ""),
            Verdict::Ask { reason } => ("ask", reason.as_str()),
            Verdict::Block { reason } => ("block", reason.as_str()),
            other => panic!("line {}: unexpected verdict {other:?}", n + 1),
        };
        let rule_ok = rule == "-" || reason.contains(&format!("(rule `{rule}`)"));
        if got != expected || !rule_ok {
            failures.push(format!(
                "line {}: expected {expected} {rule}, got {got}: {command:?}\n    {reason}",
                n + 1
            ));
        }
        counts[["allow", "ask", "block"]
            .iter()
            .position(|v| *v == expected)
            .unwrap_or_else(|| panic!("line {}: unknown verdict {expected}", n + 1))] += 1;
    }
    println!(
        "guard corpus: {} commands ({} allow, {} ask, {} block)",
        counts.iter().sum::<usize>(),
        counts[0],
        counts[1],
        counts[2]
    );
    assert!(counts.iter().sum::<usize>() >= 150, "corpus shrank");
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}
