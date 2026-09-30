//! Agent hook integration for reflex-control.
//!
//! `reflex install` registers `reflex hook <agent> <event>` as a hook in a coding
//! agent. When the agent is about to write a file, run a command or finish a turn, it
//! calls the hook; the hook reads the agent's JSON from stdin, applies the guard logic
//! and answers in the agent's own format.
//!
//! Layout:
//! * `config`: `.reflex.toml`
//! * `guard`: pure decision logic (paths, shell commands, turn end)
//! * `adapters`: one file per agent: parse its input, render a verdict, plan its install
//! * `install`: pure install/uninstall planning plus the code that applies it
//! * `state`, `runtime`: per-session state and the IO around a hook call
//! * `detect`: which agents and which test command a project has

pub mod adapters;
pub mod config;
pub mod detect;
pub mod guard;
pub mod install;
pub mod runtime;
pub mod state;
