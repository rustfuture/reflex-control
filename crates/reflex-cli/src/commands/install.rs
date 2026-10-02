//! `reflex install`: the setup wizard, and its non-interactive form.
//!
//! The wizard asks a handful of questions, shows the files it would change and only
//! writes after a final confirmation. Answers already in `.reflex.toml` are used as the
//! starting point, so running it again edits the settings. Without a terminal, or with
//! `--yes`, nothing is asked: flags win, then `.reflex.toml`, then detection.

use clap::Args;
use reflex_hooks::adapters;
use reflex_hooks::config::{
    Config, OnFailure, Scope, DEFAULT_PROTECTED_PATHS, PROJECT_CONFIG_NAME,
};
use reflex_hooks::detect::{detect_agents, detect_test_command, program_on_path};
use reflex_hooks::guard::PathPolicy;
use reflex_hooks::install::{apply, files_needed, plan, read_existing, Answers, Env, Plan};
use reflex_hooks::runtime::detect_env;
use std::io::{self, IsTerminal};
use std::path::Path;

const INSTALL_HINT: &str =
    "cargo install --git https://github.com/rustfuture/reflex-control reflex-cli";

#[derive(Args, Debug)]
pub struct InstallArgs {
    /// Do not ask anything; use the flags below, then .reflex.toml, then detection
    #[arg(short, long)]
    pub yes: bool,

    /// Agents to install into, comma separated (claude-code, cursor, codex, opencode, kilo, cline, pi, git)
    #[arg(long, value_delimiter = ',')]
    pub agent: Option<Vec<String>>,

    /// Where to install: project or user
    #[arg(long)]
    pub scope: Option<Scope>,

    /// Command that runs the tests at the end of a turn ("" to run none)
    #[arg(long)]
    pub test_command: Option<String>,

    /// What to do when the tests fail: retry-then-ask, ask or notify
    #[arg(long)]
    pub on_failure: Option<OnFailure>,
}

/// Outcome the caller turns into an exit code.
pub enum Outcome {
    Done,
    Cancelled,
}

pub fn execute(args: InstallArgs) -> Result<Outcome, Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;
    let env = detect_env(&cwd);
    let existing = load_existing_config(&env);
    let detected = Detected::new(&env);

    let interactive = !args.yes && io::stdin().is_terminal() && io::stderr().is_terminal();
    let answers = if interactive {
        match wizard(&detected, existing.as_ref()) {
            Ok(a) => a,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {
                let _ = cliclack::outro_cancel("Cancelled. Nothing was written.");
                return Ok(Outcome::Cancelled);
            }
            Err(e) => return Err(e.into()),
        }
    } else {
        from_flags(&args, &detected, existing.as_ref())?
    };

    let files = read_existing(&files_needed(&answers, &env)?);
    let planned = plan(&answers, &env, &files)?;

    if interactive && planned.is_empty() {
        cliclack::log::success("Nothing to change: already set up with these settings.")?;
        for note in &planned.notes {
            cliclack::log::warning(note)?;
        }
        cliclack::outro("Done. Check status: reflex doctor · Undo: reflex uninstall")?;
        return Ok(Outcome::Done);
    }

    if interactive {
        match confirm_plan(&planned, &env) {
            Ok(true) => {}
            Ok(false) => {
                let _ = cliclack::outro_cancel("Nothing was written.");
                return Ok(Outcome::Cancelled);
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {
                let _ = cliclack::outro_cancel("Cancelled. Nothing was written.");
                return Ok(Outcome::Cancelled);
            }
            Err(e) => return Err(e.into()),
        }
        apply(&planned.changes)?;
        for note in &planned.notes {
            cliclack::log::warning(note)?;
        }
        cliclack::outro("Done. Check status: reflex doctor · Undo: reflex uninstall")?;
    } else {
        print_plain(&planned, &env);
        apply(&planned.changes)?;
        if !program_on_path("reflex") {
            eprintln!(
                "warning: `reflex` is not on your PATH, and hooks call it by name. Install it with: {INSTALL_HINT}"
            );
        }
        println!("Done. Check status: reflex doctor · Undo: reflex uninstall");
    }
    Ok(Outcome::Done)
}

/// What is in this project and on this machine.
struct Detected {
    agents: Vec<&'static str>,
    test_command: String,
}

impl Detected {
    fn new(env: &Env) -> Detected {
        Detected {
            agents: detect_agents(&env.root, &program_on_path),
            test_command: detect_test_command(&env.root),
        }
    }
}

/// The project's own `.reflex.toml`, if it has a valid one. An invalid file is reported
/// and replaced (the old one is backed up).
fn load_existing_config(env: &Env) -> Option<Config> {
    let path = env.root.join(PROJECT_CONFIG_NAME);
    if !path.is_file() {
        return None;
    }
    match Config::load_file(&path) {
        Ok(cfg) => Some(cfg),
        Err(e) => {
            eprintln!("warning: {e}. Starting from the defaults; the old file will be backed up.");
            None
        }
    }
}

fn known_agents(ids: &[String]) -> Vec<String> {
    ids.iter()
        .filter(|id| adapters::find(id).is_some())
        .cloned()
        .collect()
}

/// Config the prompts start from: the existing file, or defaults with the detected test
/// command.
fn base_config(detected: &Detected, existing: Option<&Config>) -> Config {
    match existing {
        Some(cfg) => cfg.clone(),
        None => {
            let mut cfg = Config::default();
            cfg.tests.command = detected.test_command.clone();
            cfg
        }
    }
}

fn default_agents(detected: &Detected, existing: Option<&Config>) -> Vec<String> {
    match existing {
        Some(cfg) => known_agents(&cfg.agents.enabled),
        None => detected.agents.iter().map(|s| s.to_string()).collect(),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Non-interactive
// ─────────────────────────────────────────────────────────────────────────────

fn from_flags(
    args: &InstallArgs,
    detected: &Detected,
    existing: Option<&Config>,
) -> Result<Answers, Box<dyn std::error::Error>> {
    let mut config = base_config(detected, existing);
    let scope = args.scope.unwrap_or(config.agents.scope);

    let agents = match &args.agent {
        Some(list) => list.iter().map(|a| a.trim().to_string()).collect(),
        None => {
            let mut list = default_agents(detected, existing);
            if scope == Scope::User {
                // The git hook is per repository; leave it out of a user-wide default.
                list.retain(|id| id != "git");
            }
            list
        }
    };
    if agents.is_empty() {
        return Err(
            "no agents detected here; pass --agent claude-code,git (or run `reflex install` in a terminal)"
                .into(),
        );
    }

    if let Some(cmd) = &args.test_command {
        config.tests.command = cmd.trim().to_string();
    }
    if let Some(mode) = args.on_failure {
        config.tests.on_failure = mode;
    }
    Ok(Answers {
        agents,
        scope,
        config,
    })
}

fn print_plain(planned: &Plan, env: &Env) {
    if planned.is_empty() {
        println!("Nothing to change: Reflex Control is already set up with these settings.");
    } else {
        println!("Reflex Control will change (+ created, ~ modified):");
        for change in &planned.changes {
            println!("  {}", change.summary(env));
        }
    }
    for note in &planned.notes {
        println!("note: {note}");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Wizard
// ─────────────────────────────────────────────────────────────────────────────

fn wizard(detected: &Detected, existing: Option<&Config>) -> io::Result<Answers> {
    cliclack::intro("Reflex Control setup")?;
    if !program_on_path("reflex") {
        cliclack::log::warning(format!(
            "`reflex` is not on your PATH, and hooks call it by name.\nInstall it with: {INSTALL_HINT}"
        ))?;
    }
    if existing.is_some() {
        cliclack::log::info(format!(
            "Found {PROJECT_CONFIG_NAME}. Your current settings are pre-selected."
        ))?;
    }
    let mut config = base_config(detected, existing);

    // 1. Agents
    let preselected = default_agents(detected, existing);
    let mut prompt = cliclack::multiselect("Which tools should Reflex Control watch?");
    for adapter in adapters::registry() {
        let mut label = adapter.label().to_string();
        if detected.agents.contains(&adapter.id()) {
            label.push_str(" (detected)");
        }
        prompt = prompt.item(adapter.id().to_string(), label, adapter.hint());
    }
    let mut agents: Vec<String> = prompt
        .initial_values(preselected)
        .required(true)
        .interact()?;

    // 2. Scope. Only asked if some chosen agent can be installed for all projects.
    let user_capable = agents
        .iter()
        .filter_map(|id| adapters::find(id))
        .any(|a| a.scopes().contains(&Scope::User));
    let scope = if user_capable {
        let initial = config.agents.scope;
        let scope = cliclack::select("Where should it be installed?")
            .item(
                Scope::Project,
                "This project",
                "hooks go into this repository",
            )
            .item(
                Scope::User,
                "All my projects",
                "hooks go into your home directory; Git pre-commit is skipped",
            )
            .initial_value(initial)
            .interact()?;
        if scope == Scope::User && agents.iter().any(|id| id == "git") {
            cliclack::log::info(
                "The Git hook is per repository, so it is skipped here. \
                 Run `reflex install` inside a project to add it.",
            )?;
            agents.retain(|id| id != "git");
        }
        scope
    } else {
        Scope::Project
    };

    // 3. Protected paths
    let mut choices: Vec<String> = DEFAULT_PROTECTED_PATHS
        .iter()
        .map(|p| p.to_string())
        .collect();
    for p in &config.protect.paths {
        if !choices.contains(p) {
            choices.push(p.clone());
        }
    }
    let checked: Vec<String> = choices
        .iter()
        .filter(|p| config.protect.paths.contains(p))
        .cloned()
        .collect();
    let mut prompt = cliclack::multiselect("Which files must the agent never change?");
    for p in &choices {
        prompt = prompt.item(p.clone(), p, "");
    }
    let mut paths: Vec<String> = prompt.initial_values(checked).required(false).interact()?;
    let extra: String =
        cliclack::input("Any other patterns to protect? (comma separated, optional)")
            .placeholder("e.g. terraform/**, *.p12")
            .required(false)
            .validate(|input: &String| {
                PathPolicy::new(&parse_patterns(input), Path::new(".")).map(|_| ())
            })
            .interact()?;
    for pattern in parse_patterns(&extra) {
        if !paths.contains(&pattern) {
            paths.push(pattern);
        }
    }
    config.protect.paths = paths;

    // 4. Test command
    let current = config.tests.command.clone();
    let mut prompt = cliclack::input(
        "Command that runs your tests when the agent finishes (Enter keeps the suggestion, \"none\" skips tests)",
    )
    .required(false);
    if !current.is_empty() {
        prompt = prompt.default_input(&current);
    } else {
        prompt = prompt.placeholder("e.g. cargo test");
    }
    let typed: String = prompt.interact()?;
    config.tests.command = normalize_test_command(&typed);

    // 5. On failure (pointless without tests)
    if !config.tests.command.is_empty() {
        let tries = config.tests.max_retries;
        config.tests.on_failure = cliclack::select("What should happen when the tests fail?")
            .item(
                OnFailure::RetryThenAsk,
                format!("Let the agent fix it, up to {tries} tries, then ask me"),
                "",
            )
            .item(OnFailure::Ask, "Stop and ask me right away", "")
            .item(OnFailure::Notify, "Only notify me", "")
            .initial_value(config.tests.on_failure)
            .interact()?;
    }

    Ok(Answers {
        agents,
        scope,
        config,
    })
}

/// Shows what would change and asks for the go-ahead. `planned` is not empty.
fn confirm_plan(planned: &Plan, env: &Env) -> io::Result<bool> {
    let lines: Vec<String> = planned.changes.iter().map(|c| c.summary(env)).collect();
    cliclack::note("Files (+ created, ~ modified)", lines.join("\n"))?;
    cliclack::confirm("Apply these changes?")
        .initial_value(true)
        .interact()
}

/// Splits `a, b ,c` into patterns, dropping empty entries.
fn parse_patterns(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for part in text.split(',') {
        let p = part.trim();
        if !p.is_empty() && !out.iter().any(|x| x == p) {
            out.push(p.to_string());
        }
    }
    out
}

/// `none`, `-` and an empty answer all mean "do not run tests".
fn normalize_test_command(typed: &str) -> String {
    let t = typed.trim();
    if t.eq_ignore_ascii_case("none") || t == "-" {
        String::new()
    } else {
        t.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detected(agents: &[&'static str], test: &str) -> Detected {
        Detected {
            agents: agents.to_vec(),
            test_command: test.to_string(),
        }
    }

    fn args() -> InstallArgs {
        InstallArgs {
            yes: true,
            agent: None,
            scope: None,
            test_command: None,
            on_failure: None,
        }
    }

    #[test]
    fn flags_default_to_detection() {
        let a = from_flags(
            &args(),
            &detected(&["claude-code", "git"], "npm test"),
            None,
        )
        .unwrap();
        assert_eq!(a.agents, ["claude-code", "git"]);
        assert_eq!(a.scope, Scope::Project);
        assert_eq!(a.config.tests.command, "npm test");
        assert_eq!(a.config.tests.on_failure, OnFailure::RetryThenAsk);
    }

    #[test]
    fn nothing_detected_means_no_test_command_and_an_error_without_agents() {
        let err = from_flags(&args(), &detected(&[], ""), None).unwrap_err();
        assert!(err.to_string().contains("--agent"));
        let mut a = args();
        a.agent = Some(vec!["git".into()]);
        let ans = from_flags(&a, &detected(&[], ""), None).unwrap();
        assert_eq!(ans.config.tests.command, "");
    }

    #[test]
    fn flags_override_the_existing_config() {
        let mut existing = Config::default();
        existing.tests.command = "make check".into();
        existing.tests.on_failure = OnFailure::Notify;
        existing.agents.enabled = vec!["git".into()];
        existing.protect.paths = vec!["only.txt".into()];

        // Nothing given: the existing settings carry over.
        let a = from_flags(
            &args(),
            &detected(&["claude-code"], "cargo test"),
            Some(&existing),
        )
        .unwrap();
        assert_eq!(a.agents, ["git"]);
        assert_eq!(a.config.tests.command, "make check");
        assert_eq!(a.config.tests.on_failure, OnFailure::Notify);
        assert_eq!(a.config.protect.paths, ["only.txt"]);

        let mut f = args();
        f.agent = Some(vec!["claude-code".into()]);
        f.test_command = Some("pytest".into());
        f.on_failure = Some(OnFailure::Ask);
        f.scope = Some(Scope::User);
        let a = from_flags(&f, &detected(&[], ""), Some(&existing)).unwrap();
        assert_eq!(a.agents, ["claude-code"]);
        assert_eq!(a.scope, Scope::User);
        assert_eq!(a.config.tests.command, "pytest");
        assert_eq!(a.config.tests.on_failure, OnFailure::Ask);
    }

    #[test]
    fn user_scope_default_leaves_git_out() {
        let mut f = args();
        f.scope = Some(Scope::User);
        let a = from_flags(&f, &detected(&["claude-code", "git"], ""), None).unwrap();
        assert_eq!(a.agents, ["claude-code"]);
    }

    #[test]
    fn empty_test_command_flag_turns_tests_off() {
        let mut f = args();
        f.test_command = Some("  ".into());
        let a = from_flags(&f, &detected(&["git"], "cargo test"), None).unwrap();
        assert_eq!(a.config.tests.command, "");
    }

    #[test]
    fn pattern_and_test_command_parsing() {
        assert_eq!(parse_patterns(" a/** , b ,, a/**"), ["a/**", "b"]);
        assert!(parse_patterns("  ").is_empty());
        assert_eq!(normalize_test_command("none"), "");
        assert_eq!(normalize_test_command("None"), "");
        assert_eq!(normalize_test_command("-"), "");
        assert_eq!(normalize_test_command(" cargo test "), "cargo test");
    }
}
