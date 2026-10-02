//! `reflex demo agent`: terminal demo showing Reflex Control guarding a simulated coding agent.

use console::{style, Style};
use reflex_hooks::config::Config;
use reflex_hooks::guard::{
    check_command_in_dir, check_paths, decide_turn_end, PathPolicy, TestOutcome, TurnEndInput,
    Verdict,
};
use std::io::{self, IsTerminal, Write};
use std::path::Path;
use std::time::Duration;

/// One action attempted by the simulated agent, paired with Reflex Control's verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub verb: &'static str,
    pub target: String,
    pub verdict: Verdict,
}

/// Evaluates the six scenario steps against Reflex Control's real hook engine.
pub fn run_scenario() -> Vec<Step> {
    let root = Path::new("/demo/project");
    let mut cfg = Config::default();
    cfg.tests.command = "cargo test".to_string();
    let policy =
        PathPolicy::new(&cfg.protect.paths, root).expect("default config paths are valid globs");

    vec![
        // a) agent edits src/lib.rs -> check_paths (expect Allow)
        Step {
            verb: "edit",
            target: "src/lib.rs".to_string(),
            verdict: check_paths(&["src/lib.rs"], &policy),
        },
        // b) agent writes .env -> check_paths (expect Block)
        Step {
            verb: "write",
            target: ".env".to_string(),
            verdict: check_paths(&[".env"], &policy),
        },
        // c) agent runs: echo "token=abc" >> secrets/api.txt -> check_command_in_dir (expect Block)
        Step {
            verb: "run",
            target: "echo \"token=abc\" >> secrets/api.txt".to_string(),
            verdict: check_command_in_dir(
                "echo \"token=abc\" >> secrets/api.txt",
                &policy,
                Some(root),
            ),
        },
        // d) agent runs: git push --force origin main -> check_command_in_dir (expect Ask)
        Step {
            verb: "run",
            target: "git push --force origin main".to_string(),
            verdict: check_command_in_dir("git push --force origin main", &policy, Some(root)),
        },
        // e) agent says it is done; tests fail
        //    (TestOutcome::Failed with output "test parser::empty_input ... FAILED", diff_lines 12, retries_so_far 0)
        //    -> decide_turn_end (expect RetryAgent)
        Step {
            verb: "done",
            target: "cargo test → 1 failed".to_string(),
            verdict: decide_turn_end(
                &TurnEndInput {
                    tests: TestOutcome::Failed {
                        output: "test parser::empty_input ... FAILED".to_string(),
                    },
                    diff_lines: 12,
                    retries_so_far: 0,
                },
                &cfg,
            ),
        },
        // f) agent fixes it; tests pass (TestOutcome::Passed, diff_lines 14, retries_so_far 1)
        //    -> decide_turn_end (expect Allow)
        Step {
            verb: "done",
            target: "cargo test → passed".to_string(),
            verdict: decide_turn_end(
                &TurnEndInput {
                    tests: TestOutcome::Passed,
                    diff_lines: 14,
                    retries_so_far: 1,
                },
                &cfg,
            ),
        },
    ]
}

fn format_badge(verdict: &Verdict) -> console::StyledObject<&'static str> {
    match verdict {
        Verdict::Allow => Style::new().on_green().black().bold().apply_to(" ALLOW "),
        Verdict::Block { .. } => Style::new().on_red().black().bold().apply_to(" BLOCK "),
        Verdict::Ask { .. } | Verdict::AskHuman { .. } => {
            Style::new().on_yellow().black().bold().apply_to("  ASK  ")
        }
        Verdict::RetryAgent { .. } => Style::new().on_magenta().black().bold().apply_to(" RETRY "),
        Verdict::Notify { .. } => Style::new().on_cyan().black().bold().apply_to(" NOTIFY"),
    }
}

fn get_reason(verdict: &Verdict) -> Option<&str> {
    match verdict {
        Verdict::Block { reason } => Some(reason),
        Verdict::Ask { reason } | Verdict::AskHuman { reason } => Some(reason),
        Verdict::RetryAgent { reason } => Some(reason),
        Verdict::Notify { message } => Some(message),
        Verdict::Allow => None,
    }
}

fn format_reason(reason: &str, is_retry: bool) -> String {
    let stripped = reason.strip_prefix("Reflex Control: ").unwrap_or(reason);

    let sentence = if is_retry {
        if let Some(pos1) = stripped.find(". ") {
            let after_first = &stripped[pos1 + 2..];
            if let Some(pos2) = after_first.find(". ") {
                &stripped[..pos1 + 2 + pos2 + 1]
            } else if let Some(dot_pos) = after_first.find('.') {
                &stripped[..pos1 + 2 + dot_pos + 1]
            } else {
                stripped
            }
        } else if let Some(dot_pos) = stripped.find('.') {
            &stripped[..dot_pos + 1]
        } else {
            stripped
        }
    } else if let Some(pos) = stripped.find(". ") {
        &stripped[..pos + 1]
    } else if let Some(dot_pos) = stripped.find('.') {
        &stripped[..dot_pos + 1]
    } else {
        stripped
    };

    if sentence.chars().count() > 110 {
        let max_prefix: String = sentence.chars().take(110).collect();
        if let Some(last_space) = max_prefix.rfind(' ') {
            format!("{}…", &max_prefix[..last_space])
        } else {
            format!("{max_prefix}…")
        }
    } else {
        sentence.to_string()
    }
}

/// Renders the demo output to terminal.
pub fn render_demo(steps: &[Step]) -> Result<(), Box<dyn std::error::Error>> {
    let is_fast = std::env::var("REFLEX_DEMO_FAST")
        .map(|v| v != "0" && !v.is_empty())
        .unwrap_or(false)
        || !io::stdout().is_terminal();

    let box_width = 46;
    let horizontal = "─".repeat(box_width);
    println!("  {}", style(format!("╭{horizontal}╮")).dim());
    println!(
        "  {}  {}  ·  {}  {}",
        style("│").dim(),
        style("Reflex Control").bold(),
        style("guarding a coding agent").dim(),
        style("│").dim()
    );
    println!("  {}", style(format!("╰{horizontal}╯")).dim());
    println!();

    for step in steps {
        let badge = format_badge(&step.verdict);
        let verb_col = format!("{:<6}", step.verb);
        println!("  {badge}  {} {}", style(verb_col).dim(), step.target);

        if matches!(
            step.verdict,
            Verdict::Block { .. } | Verdict::Ask { .. } | Verdict::RetryAgent { .. }
        ) {
            if let Some(reason) = get_reason(&step.verdict) {
                let is_retry = matches!(step.verdict, Verdict::RetryAgent { .. });
                let formatted = format_reason(reason, is_retry);
                println!("           ↳ {}", style(formatted).dim());
            }
        }

        io::stdout().flush()?;
        if !is_fast {
            std::thread::sleep(Duration::from_millis(450));
        }
    }

    let total = steps.len();
    let blocked = steps
        .iter()
        .filter(|s| matches!(s.verdict, Verdict::Block { .. }))
        .count();
    let needs_approval = steps
        .iter()
        .filter(|s| matches!(s.verdict, Verdict::Ask { .. } | Verdict::AskHuman { .. }))
        .count();
    let sent_back = steps
        .iter()
        .filter(|s| matches!(s.verdict, Verdict::RetryAgent { .. }))
        .count();

    println!();
    println!(
        "  {total} agent steps   {} blocked   {} needs your approval   {} sent back   {}",
        style(blocked).red().bold(),
        style(needs_approval).yellow().bold(),
        style(sent_back).magenta().bold(),
        style("tests green").green().bold()
    );
    println!();
    let disclaimer = concat!(
        "  Decisions come from Reflex Control's real hook engine; ",
        "the agent and its tests are simulated."
    );
    println!("{}", style(disclaimer).dim());
    io::stdout().flush()?;

    Ok(())
}

/// Entry point for `reflex demo agent`.
pub fn execute() -> Result<(), Box<dyn std::error::Error>> {
    let steps = run_scenario();
    render_demo(&steps)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use reflex_hooks::guard::Verdict;

    #[test]
    fn test_scenario_verdicts() {
        let steps = run_scenario();
        assert_eq!(steps.len(), 6);

        let kinds: Vec<&'static str> = steps
            .iter()
            .map(|s| match s.verdict {
                Verdict::Allow => "Allow",
                Verdict::Block { .. } => "Block",
                Verdict::Ask { .. } => "Ask",
                Verdict::RetryAgent { .. } => "RetryAgent",
                Verdict::AskHuman { .. } => "AskHuman",
                Verdict::Notify { .. } => "Notify",
            })
            .collect();

        assert_eq!(
            kinds,
            vec!["Allow", "Block", "Block", "Ask", "RetryAgent", "Allow"]
        );

        assert!(matches!(steps[0].verdict, Verdict::Allow));
        assert!(matches!(steps[1].verdict, Verdict::Block { .. }));
        assert!(matches!(steps[2].verdict, Verdict::Block { .. }));
        assert!(matches!(steps[3].verdict, Verdict::Ask { .. }));
        assert!(matches!(steps[4].verdict, Verdict::RetryAgent { .. }));
        assert!(matches!(steps[5].verdict, Verdict::Allow));
    }

    #[test]
    fn test_format_reason() {
        let r_block = "Reflex Control: this edit writes to `.env`, which is protected \
                       (matches `.env`). Do not change it. To allow it, the user can edit.";
        assert_eq!(
            format_reason(r_block, false),
            "this edit writes to `.env`, which is protected (matches `.env`)."
        );

        let r_retry = "Reflex Control: the tests failed (`cargo test`). This is fix \
                       attempt 1 of 2. Fix the failures, then finish again.";
        assert_eq!(
            format_reason(r_retry, true),
            "the tests failed (`cargo test`). This is fix attempt 1 of 2."
        );

        let long = "word ".repeat(30);
        let formatted = format_reason(&long, false);
        assert!(formatted.chars().count() <= 111);
        assert!(formatted.ends_with('…'));
        assert!(!formatted.ends_with(" …"));
    }

    #[test]
    fn test_render_fast() {
        std::env::set_var("REFLEX_DEMO_FAST", "1");
        let steps = run_scenario();
        render_demo(&steps).expect("render should succeed");
    }
}
