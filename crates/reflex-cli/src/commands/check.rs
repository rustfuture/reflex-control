//! `reflex check`: what the hooks would decide for a command, a write or a read, using
//! the project's own settings. Nothing is run.
//!
//! Exit code: 0 if everything is allowed, 1 if something would ask, 2 if something
//! would be blocked, 3 if the check itself failed (bad arguments, unreadable settings),
//! so the command also works in scripts and CI.

use clap::Args;
use reflex_hooks::config::{Config, ConfigSource};
use reflex_hooks::guard::{Guard, Verdict};
use reflex_hooks::rules::RULES;
use reflex_hooks::runtime::resolve_project;
use std::path::Path;

#[derive(Args, Debug)]
pub struct CheckArgs {
    /// Check a write (edit/write tool) to this path; can be repeated
    #[arg(long = "write", value_name = "PATH")]
    pub writes: Vec<String>,

    /// Check a read (read/search tool) of this path; can be repeated
    #[arg(long = "read", value_name = "PATH")]
    pub reads: Vec<String>,

    /// List the destructive-command rules and which ones this project allows
    #[arg(long)]
    pub rules: bool,

    /// Shell command to check, as an agent would run it (quote it, or put it after `--`)
    #[arg(
        trailing_var_arg = true,
        allow_hyphen_values = true,
        value_name = "COMMAND"
    )]
    pub command: Vec<String>,
}

/// Prints the report and returns the exit code.
pub fn execute(args: CheckArgs) -> Result<i32, Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;
    let project = resolve_project(&cwd)?;
    let guard = Guard::from_config(&project.config, &project.root, dirs::home_dir().as_deref())?;
    match &project.source {
        ConfigSource::Defaults => println!("settings: built-in defaults"),
        source => println!("settings: {}", source.describe()),
    }
    if args.rules {
        print!("{}", rules_table(&project.config));
        return Ok(0);
    }
    if args.command.is_empty() && args.writes.is_empty() && args.reads.is_empty() {
        return Err("nothing to check: give a command, --write PATH or --read PATH".into());
    }
    let (text, code) = report(
        &guard,
        &cwd,
        &args.command.join(" "),
        &args.writes,
        &args.reads,
    );
    print!("{text}");
    Ok(code)
}

/// The report for one `reflex check` call, and its exit code.
pub fn report(
    guard: &Guard,
    cwd: &Path,
    command: &str,
    writes: &[String],
    reads: &[String],
) -> (String, i32) {
    let mut out = String::new();
    let mut code = 0;
    // components() drops `.` segments, so `--read ./.env` checks `<cwd>/.env`, not `<cwd>/./.env`.
    let absolute = |p: &String| {
        let path: std::path::PathBuf = cwd.join(p).components().collect();
        path.to_string_lossy().into_owned()
    };
    let mut checks: Vec<(String, Verdict)> = Vec::new();
    if !command.trim().is_empty() {
        checks.push((
            format!("command: {command}"),
            guard.check_command(command, Some(cwd)),
        ));
    }
    for p in writes {
        checks.push((format!("write:   {p}"), guard.check_writes(&[absolute(p)])));
    }
    for p in reads {
        checks.push((format!("read:    {p}"), guard.check_reads(&[absolute(p)])));
    }
    for (what, verdict) in checks {
        let (label, reason, severity) = match &verdict {
            Verdict::Block { reason } => ("BLOCK", Some(reason), 2),
            Verdict::Ask { reason } => ("ASK  ", Some(reason), 1),
            _ => ("ALLOW", None, 0),
        };
        code = code.max(severity);
        out.push_str(&format!("{label}  {what}\n"));
        if let Some(reason) = reason {
            let reason = reason.strip_prefix("Reflex Control: ").unwrap_or(reason);
            out.push_str(&format!("       {reason}\n"));
        }
    }
    (out, code)
}

fn rules_table(config: &Config) -> String {
    let allowed = |id: &str| config.commands.allow.iter().any(|a| a.trim() == id);
    let width = RULES.iter().map(|(id, _)| id.len()).max().unwrap_or(0);
    let mut out = String::from("\nDestructive-command rules (asked before they run):\n\n");
    for (id, what) in RULES {
        let state = if allowed(id) { "allowed" } else { "asks   " };
        out.push_str(&format!("  {state}  {id:width$}  {what}\n"));
    }
    out.push_str("\nTo stop asking for one, add its id to `commands.allow` in .reflex.toml.\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guard() -> Guard {
        Guard::from_config(
            &Config::default(),
            Path::new("/p"),
            Some(Path::new("/home/u")),
        )
        .unwrap()
    }

    #[test]
    fn a_leading_dot_slash_is_checked_as_the_same_path() {
        let g = guard();
        let (text, code) = report(&g, Path::new("/p"), "", &[], &["./.env".to_string()]);
        assert_eq!(code, 2, "{text}");
        assert!(!text.contains("/./"), "{text}");
    }

    #[test]
    fn report_lists_each_check_and_exits_with_the_worst_verdict() {
        let g = guard();
        let cwd = Path::new("/p");
        let (text, code) = report(&g, cwd, "cargo test", &[], &[]);
        assert_eq!((text.as_str(), code), ("ALLOW  command: cargo test\n", 0));

        let (text, code) = report(&g, cwd, "git reset --hard", &[], &[]);
        assert_eq!(code, 1);
        assert!(
            text.starts_with("ASK    command: git reset --hard\n"),
            "{text}"
        );
        assert!(text.contains("(rule `git.reset-hard`)"), "{text}");
        assert!(!text.contains("Reflex Control: "), "{text}");

        let (text, code) = report(
            &g,
            cwd,
            "",
            &["src/main.rs".into(), ".env".into()],
            &["README.md".into(), "secrets/token".into()],
        );
        assert_eq!(code, 2);
        let lines: Vec<&str> = text.lines().filter(|l| !l.starts_with(' ')).collect();
        assert_eq!(
            lines,
            [
                "ALLOW  write:   src/main.rs",
                "BLOCK  write:   .env",
                "ALLOW  read:    README.md",
                "BLOCK  read:    secrets/token",
            ]
        );
    }

    #[test]
    fn rules_table_marks_allowed_rules() {
        let mut cfg = Config::default();
        cfg.commands.allow = vec!["git.reset-hard".into()];
        let table = rules_table(&cfg);
        assert!(table.contains("allowed  git.reset-hard"), "{table}");
        assert!(table.contains("asks     git.push-force"), "{table}");
        assert_eq!(table.matches("  asks   ").count() + 1, RULES.len());
    }
}
