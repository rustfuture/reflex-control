//! The bypass matrix: the same dangerous act written another way must still stop, and
//! harmless look-alikes must still pass. `tests/fixtures/bypass_matrix.tsv` holds the
//! cases; `caught` cells are the contract, `gap` cells record blind spots the heuristic
//! cannot see (a fix flips one and this test says to move it), `safe` cells guard against
//! overblocking. The per-class numbers are the measurement; the test fails only on a
//! contract break, a closed gap or a false positive.

use reflex_hooks::config::Config;
use reflex_hooks::guard::{Guard, Verdict};
use std::collections::BTreeMap;
use std::path::Path;

fn class(verdict: &Verdict) -> &'static str {
    match verdict {
        Verdict::Allow => "allow",
        Verdict::Ask { .. } => "ask",
        Verdict::Block { .. } => "block",
        other => panic!("not a command verdict: {other:?}"),
    }
}

#[test]
fn dangerous_commands_are_caught_however_they_are_written() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bypass_matrix.tsv");
    let text = std::fs::read_to_string(&path).unwrap();
    let guard = Guard::from_config(
        &Config::default(),
        Path::new("/work/proj"),
        Some(Path::new("/home/me")),
    )
    .unwrap();

    #[derive(Default)]
    struct Stats {
        caught: usize,
        caught_all: usize,
        gaps: usize,
        safe: usize,
        safe_all: usize,
    }
    let mut stats: BTreeMap<&str, Stats> = BTreeMap::new();
    let mut misses = Vec::new();
    let mut closed_gaps = Vec::new();
    let mut false_positives = Vec::new();
    let mut cases = 0usize;
    for (n, line) in text.lines().enumerate() {
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let mut fields = line.splitn(3, '\t');
        let (Some(section), Some(expect), Some(command)) =
            (fields.next(), fields.next(), fields.next())
        else {
            panic!(
                "line {}: expected <class>\\t<caught|gap|safe>\\t<command>",
                n + 1
            );
        };
        let command = command.replace("\\n", "\n");
        let got = class(&guard.check_command(&command, None));
        let entry = stats.entry(section).or_default();
        cases += 1;
        match expect {
            "caught" => {
                entry.caught_all += 1;
                if got == "allow" {
                    misses.push(format!("{section}: {command:?} was allowed"));
                } else {
                    entry.caught += 1;
                }
            }
            "gap" => {
                entry.gaps += 1;
                if got != "allow" {
                    closed_gaps.push(format!(
                        "{section}: {command:?} now gives {got}; move it to `caught`"
                    ));
                }
            }
            "safe" => {
                entry.safe_all += 1;
                if got != "allow" {
                    false_positives.push(format!("{section}: {command:?} gives {got}"));
                } else {
                    entry.safe += 1;
                }
            }
            other => panic!("line {}: unknown expectation {other:?}", n + 1),
        }
    }

    println!("{:<12} {:>10} {:>5} {:>4}", "class", "caught", "gap", "fp");
    let mut total = (0, 0, 0, 0);
    for (section, s) in &stats {
        println!(
            "{section:<12} {:>7}/{:<2} {:>5} {:>4}",
            s.caught,
            s.caught_all,
            s.gaps,
            s.safe_all - s.safe
        );
        total.0 += s.caught;
        total.1 += s.caught_all;
        total.2 += s.gaps;
        total.3 += s.safe_all - s.safe;
    }
    println!(
        "{cases} cases: {}/{} caught, {} known gaps, {} false positives",
        total.0, total.1, total.2, total.3
    );

    assert!(misses.is_empty(), "bypasses:\n{}", misses.join("\n"));
    assert!(
        closed_gaps.is_empty(),
        "the heuristic got stronger:\n{}",
        closed_gaps.join("\n")
    );
    assert!(
        false_positives.is_empty(),
        "false positives:\n{}",
        false_positives.join("\n")
    );
}
