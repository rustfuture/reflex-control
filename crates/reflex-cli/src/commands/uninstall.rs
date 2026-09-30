//! `reflex uninstall`: removes the hooks that `reflex install` added.

use reflex_hooks::install::{apply, files_needed_uninstall, plan_uninstall, read_existing};
use reflex_hooks::runtime::detect_env;
use std::io::IsTerminal;

pub fn execute(yes: bool) -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;
    let env = detect_env(&cwd);
    let files = read_existing(&files_needed_uninstall(&env));
    let plan = plan_uninstall(&env, &files)?;

    if plan.is_empty() {
        println!("Nothing to uninstall: no Reflex Control hooks found.");
        return Ok(());
    }

    println!("Reflex Control will change:");
    for change in &plan.changes {
        println!("  {}", change.summary(&env));
    }
    println!("(+ created, ~ modified, - removed. .reflex.toml is kept.)");

    let interactive = !yes && std::io::stdin().is_terminal() && std::io::stderr().is_terminal();
    if interactive {
        let go = cliclack::confirm("Remove these hooks?")
            .initial_value(true)
            .interact();
        match go {
            Ok(true) => {}
            Ok(false) => {
                println!("Nothing was changed.");
                return Ok(());
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {
                println!("Cancelled. Nothing was changed.");
                return Ok(());
            }
            Err(e) => return Err(e.into()),
        }
    }

    apply(&plan.changes)?;
    println!("Done. Reflex Control hooks removed.");
    Ok(())
}
