//! Generic git `pre-commit` hook.
//!
//! Install writes a small shell script to the repository's hooks directory (which
//! follows `core.hooksPath`). The script calls `reflex hook git pre-commit`, which checks
//! the staged files against `protect.paths` and, if `git.run_tests` is on and a test
//! command is set, runs the tests. Either failing stops the commit (exit 1).
//!
//! If a pre-commit hook that is not ours already exists, install keeps it: it is renamed
//! to `pre-commit.local` (same mode) and our script runs it first, passing the same
//! arguments and stopping if it fails. This is safe because git only looks for the
//! name `pre-commit`, and the old script keeps its own directory. Install does not touch
//! anything in two cases, and prints the line to add by hand instead:
//! * a `pre-commit.local` already exists (renaming would overwrite it), or
//! * the hooks directory is inside the working tree (`core.hooksPath` pointing at a
//!   tracked folder such as `.husky` or `.githooks`), because editing it changes files
//!   the whole team shares.
//!
//! The script only turns exit code 1 from `reflex` into a blocked commit. Any other
//! failure (a crash, a missing binary) lets the commit through, so a broken install
//! never blocks work.

use super::{AgentAdapter, HookEvent, HookInput, ParseError, Probe, Rendered};
use crate::config::Scope;
use crate::guard::Verdict;
use crate::install::{FileChange, Files, InstallError, Plan, PlanCtx, HOOK_COMMAND_PREFIX};
use std::path::PathBuf;

pub struct GitHook;

/// Present in every script we write; used to tell our hook from someone else's.
pub const MARKER: &str = "reflex-control managed hook";

const SCRIPT: &str = r#"#!/bin/sh
# reflex-control managed hook. Installed by `reflex install`, removed by `reflex uninstall`.
hook_dir=$(dirname "$0")
if [ -x "$hook_dir/pre-commit.local" ]; then
  "$hook_dir/pre-commit.local" "$@" || exit $?
fi
if command -v reflex >/dev/null 2>&1; then
  reflex hook git pre-commit
  # Only exit code 1 means "block"; anything else is a problem with reflex itself.
  [ $? -eq 1 ] && exit 1
else
  echo "reflex-control: 'reflex' is not on PATH, skipping its checks." >&2
fi
exit 0
"#;

pub fn script() -> &'static str {
    SCRIPT
}

fn is_ours(text: &str) -> bool {
    text.contains(MARKER)
}

fn manual_line() -> String {
    format!("{HOOK_COMMAND_PREFIX} git pre-commit")
}

impl AgentAdapter for GitHook {
    fn id(&self) -> &'static str {
        "git"
    }

    fn label(&self) -> &'static str {
        "Git pre-commit hook"
    }

    fn hint(&self) -> &'static str {
        "checks staged files and runs tests before each commit"
    }

    fn scopes(&self) -> &'static [Scope] {
        &[Scope::Project]
    }

    fn detect(&self, probe: &Probe) -> bool {
        probe.root.join(".git").exists()
    }

    fn reads_stdin(&self, _event: &str) -> bool {
        false
    }

    fn parse(&self, event: &str, _stdin: &str) -> Result<HookInput, ParseError> {
        match event {
            "pre-commit" => Ok(HookInput {
                cwd: None,
                event: HookEvent::PreCommit,
            }),
            other => Err(ParseError(format!("unknown git hook event `{other}`"))),
        }
    }

    fn render(&self, _event: &HookEvent, verdict: &Verdict) -> Rendered {
        match verdict {
            Verdict::Allow => Rendered::default(),
            Verdict::Block { reason }
            | Verdict::Ask { reason }
            | Verdict::RetryAgent { reason }
            | Verdict::AskHuman { reason } => Rendered {
                stdout: String::new(),
                stderr: format!("{reason}\n"),
                exit_code: 1,
            },
            Verdict::Notify { message } => Rendered {
                stdout: String::new(),
                stderr: format!("{message}\n"),
                exit_code: 0,
            },
        }
    }

    fn files(&self, ctx: &PlanCtx) -> Result<Vec<PathBuf>, InstallError> {
        if ctx.scope != Scope::Project {
            return Ok(Vec::new());
        }
        Ok(match &ctx.env.git_hooks_dir {
            Some(dir) => vec![dir.join("pre-commit"), dir.join("pre-commit.local")],
            None => Vec::new(),
        })
    }

    fn plan_install(&self, ctx: &PlanCtx, existing: &Files) -> Result<Plan, InstallError> {
        if ctx.scope != Scope::Project {
            return Err(InstallError::UnsupportedScope {
                agent: self.id().to_string(),
                scope: ctx.scope,
            });
        }
        let dir = ctx
            .env
            .git_hooks_dir
            .clone()
            .ok_or_else(|| InstallError::NotGitRepo(ctx.env.root.clone()))?;
        let hook = dir.join("pre-commit");
        let local = dir.join("pre-commit.local");
        let mut plan = Plan::default();

        if ctx.env.hooks_dir_in_repo {
            plan.notes.push(format!(
                "The git hooks directory {} is part of the repository, so it was not changed. \
                 Add this line to its pre-commit hook: {}",
                dir.display(),
                manual_line()
            ));
            return Ok(plan);
        }

        let mut ours = FileChange::write(hook.clone(), existing.get(&hook), SCRIPT.to_string());
        ours.executable = true;

        match existing.get(&hook) {
            None => plan.changes.push(ours),
            Some(text) if is_ours(text) => {
                if text != SCRIPT {
                    plan.changes.push(ours);
                }
            }
            Some(foreign) => {
                if existing.contains_key(&local) {
                    plan.notes.push(format!(
                        "A pre-commit hook and a pre-commit.local already exist in {}, so the \
                         hook was not changed. Add this line to the pre-commit hook: {}",
                        dir.display(),
                        manual_line()
                    ));
                } else {
                    let mut saved = FileChange::write(local, None, foreign.clone());
                    saved.perm_from = Some(hook.clone());
                    plan.changes.push(saved);
                    plan.changes.push(ours);
                }
            }
        }
        Ok(plan)
    }

    fn plan_uninstall(&self, ctx: &PlanCtx, existing: &Files) -> Result<Plan, InstallError> {
        let mut plan = Plan::default();
        let Some(dir) = &ctx.env.git_hooks_dir else {
            return Ok(plan);
        };
        if ctx.scope != Scope::Project || ctx.env.hooks_dir_in_repo {
            return Ok(plan);
        }
        let hook = dir.join("pre-commit");
        let local = dir.join("pre-commit.local");
        let Some(text) = existing.get(&hook).filter(|t| is_ours(t)) else {
            return Ok(plan);
        };
        match existing.get(&local) {
            Some(saved) => {
                let mut restore = FileChange::write(hook, Some(text), saved.clone());
                restore.perm_from = Some(local.clone());
                plan.changes.push(restore);
                plan.changes.push(FileChange::delete(local, saved));
            }
            None => plan.changes.push(FileChange::delete(hook, text)),
        }
        Ok(plan)
    }

    fn is_installed(&self, ctx: &PlanCtx, existing: &Files) -> bool {
        ctx.env
            .git_hooks_dir
            .as_ref()
            .and_then(|d| existing.get(&d.join("pre-commit")))
            .is_some_and(|t| is_ours(t))
    }
}
