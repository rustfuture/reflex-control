//! `reflex doctor`: what is installed and what the hooks will use.

use reflex_hooks::adapters;
use reflex_hooks::config::{Config, ConfigSource, Scope};
use reflex_hooks::detect::{detect_test_command, find_on_path};
use reflex_hooks::guard::PathPolicy;
use reflex_hooks::install::{files_needed_uninstall, read_existing, PlanCtx};
use reflex_hooks::runtime::detect_env;

/// Returns false if something needs attention (the exit code follows).
pub fn execute() -> Result<bool, Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;
    let env = detect_env(&cwd);
    let mut healthy = true;

    println!("Reflex Control status\n");

    match find_on_path("reflex") {
        Some(path) => println!("  reflex on PATH:  yes ({})", path.display()),
        None => {
            healthy = false;
            println!("  reflex on PATH:  no");
            println!("                   hooks call `reflex` by name; install it with");
            println!(
                "                   cargo install --git https://github.com/rustfuture/reflex-control reflex-cli"
            );
        }
    }

    let (config, source) = match Config::discover(&cwd) {
        Ok(found) => found,
        Err(e) => {
            println!("  config:          ERROR: {e}");
            println!(
                "                   hooks ignore the file and allow everything until it is fixed"
            );
            (Config::default(), ConfigSource::Defaults)
        }
    };
    if matches!(source, ConfigSource::Defaults) {
        println!("  config:          none found, using built-in defaults");
    } else {
        println!("  config:          {}", source.describe());
    }

    let detected = detect_test_command(&env.root);
    let command = config.tests.command.trim();
    if command.is_empty() {
        println!("  test command:    none (tests are not run)");
    } else {
        println!("  test command:    {command}");
    }
    if !detected.is_empty() && detected != command {
        println!("                   detected in this project: {detected}");
    }
    println!(
        "  on failure:      {} (max {} retries)",
        config.tests.on_failure, config.tests.max_retries
    );
    println!("  protected paths: {}", config.protect.paths.join(", "));
    if let Err(e) = PathPolicy::new(&config.protect.paths, &env.root) {
        healthy = false;
        println!("                   ERROR: {e}");
        println!(
            "                   hooks cannot check paths until this is fixed and allow everything"
        );
    }

    println!("\n  agents:");
    let files = read_existing(&files_needed_uninstall(&env));
    let mut any = false;
    for adapter in adapters::registry() {
        let mut found = Vec::new();
        for scope in adapter.scopes() {
            if *scope == Scope::User && env.home.is_none() {
                continue;
            }
            let ctx = PlanCtx {
                env: &env,
                scope: *scope,
            };
            if adapter.is_installed(&ctx, &files) {
                found.push(scope.as_str());
            }
        }
        if found.is_empty() {
            println!("    {:<12} not installed", adapter.id());
        } else {
            any = true;
            println!("    {:<12} installed ({})", adapter.id(), found.join(", "));
        }
    }
    if !any {
        println!("\n  Nothing is installed yet. Run: reflex install");
    }
    Ok(healthy)
}
