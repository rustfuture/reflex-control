//! `reflex hook <agent> <event>`: the entry point agents call. Reads the agent's JSON
//! on stdin and answers in the agent's format. Never fails loudly; see
//! `reflex_hooks::runtime` for the fail-safe rules.

use reflex_hooks::runtime::{handle, SystemRuntime};
use std::io::Write;

pub fn execute(agent: &str, event: &str) -> ! {
    let out = handle(agent, event, &SystemRuntime);
    if !out.stdout.is_empty() {
        let mut stdout = std::io::stdout();
        let _ = stdout.write_all(out.stdout.as_bytes());
        let _ = stdout.flush();
    }
    if !out.stderr.is_empty() {
        let _ = std::io::stderr().write_all(out.stderr.as_bytes());
    }
    std::process::exit(out.exit_code);
}
