# How Reflex Control compares

Written 2026-10-22, updated 2026-10-23. This compares Reflex Control with two other ways to
keep an AI coding agent from doing damage: the test-gate hooks prove-it-mini and
kora-agentgate, and the controls built into Claude Code. The facts about the other tools are taken from their public
descriptions, listed under [Sources](#sources). A cell that says "Not checked" means those
sources do not say; it is not a claim that the feature is missing.

Reflex Control is a second layer, not a replacement for a sandbox. Anthropic's hooks
documentation recommends the permission system for hard allow and deny rules. Use deny
rules and the sandbox for those. Reflex Control adds the parts they do not: a test gate when
the agent says it is done, confirmation for destructive commands, and a check that the agent
does not edit the guard itself.

## The table

| | Reflex Control | prove-it-mini, kora-agentgate | Claude Code built-in |
| :--- | :--- | :--- | :--- |
| Protected writes | Yes. Path patterns with `!` exceptions, checked for edit tools and for shell commands (redirects, `cp`, `mv`, `ln`, `curl -o`, `git checkout <file>`, ...) | No: no file or command protection | Permission deny rules; the OS sandbox covers Bash and child processes |
| Secret reads | Yes. `protect.read`: read tools in Claude Code (`Read`, `Grep` on a file), OpenCode and Kilo Code; shell commands in every agent | No: no file protection | Deny rules such as `Read(.env)`; sandbox `denyRead` |
| Destructive-command coverage | 22 rules, all on by default, each with an id; ask or refuse | No: no command protection | Auto mode with a classifier; permission rules |
| Interpreter one-liners and heredocs | Yes, by heuristic: `python -c`, `node -e`, `perl -e`, ... and heredocs, read by the program that gets them | No | The sandbox covers Bash and child processes, so it does not depend on reading the command |
| Test gate at stop | Yes: runs your test command, sends failures back, limited retries, then asks | **Yes: this is their purpose.** Claude Code Stop hooks that block "done" while tests fail | No built-in gate; a Stop hook can add one |
| Stuck-agent detection | Yes: the same failure twice asks you; a test command that cannot run does not send the agent back | Not checked | No test gate |
| Self-protection of its own config | Yes: `protect.own_files` | Not checked | Not checked |
| Explain or dry-run CLI | `reflex check` (ALLOW, ASK or BLOCK with the reason; runs nothing) and `reflex check --rules` | Not checked | Not checked |
| Agents supported | 7 agents and a git pre-commit hook: Claude Code, Cursor, Codex CLI, OpenCode, Kilo Code, Cline, pi | Claude Code | Claude Code |
| What it is | Rust CLI and hooks, MIT licence | Claude Code Stop hooks | Part of Claude Code |

## What each one is

### prove-it-mini and kora-agentgate

Claude Code Stop hooks. When the agent says it is done, they block it while the tests fail.
They have no file protection and no command protection.

Where Reflex Control overlaps: the test gate. Reflex Control's gate runs your test command,
sends the last 40 lines of a failure back to the agent, limits the retries, stops when a fix
leaves the same failure, and does not send the agent back when the test command cannot run.
These details were not compared with the other two. If a test gate is all you need, they do
only that.

### Claude Code built-in controls

- **Permission deny rules**, for example `Read(.env)`.
- **The OS sandbox.** It covers Bash and its child processes and has `denyRead`.
- **Auto mode**, which uses a classifier.

These are enforced by Claude Code itself or the operating system, and Anthropic's hooks
documentation recommends the permission system for hard allow and deny. A hook is a script that
reads the tool call and decides; Reflex Control is one. It does not cover what a sandbox
covers, because it reads command text and does not control the process.

## Which to use

- For rules that must hold whatever the agent does: Claude Code permission deny rules and the
  sandbox.
- For a test gate only: prove-it-mini or kora-agentgate, or Reflex Control's.
- For protected files, secret reads that stay out of the model's context, confirmation for
  destructive commands, a test gate and a guard that agents cannot edit, in one hook: Reflex
  Control. It is a research prototype (see below).

These are not exclusive. Reflex Control's installer keeps the hooks that are already in a
settings file, so it can be installed next to another hook. That combination was not tested.

## What Reflex Control does not do

- **It is not a sandbox.** It reads the text of a command. It does not run it, expand variables
  or follow `$(...)`, and nothing stops a process at the operating-system level. Claude Code's
  sandbox does.
- **It does not see what the text hides.** Known gaps, recorded in the corpus test
  ([`guard_corpus.tsv`](../crates/reflex-hooks/tests/fixtures/guard_corpus.tsv)): the contents
  of script files (`python3 scripts/cleanup.py`, `./reset-db.sh`, `make clean-all`); paths and
  commands built at run time (`F=.env; echo x > $F`, `eval "$(... | base64 -d)"`); string
  concatenation in interpreter code; recursive `grep -r` over a directory that holds secrets.
  Loading a secret and printing it in the same command line (`source .env && echo $API_KEY`)
  is blocked, but a separate, later `echo $API_KEY` in a shell that kept the variables is not
  seen.
- **Its command catalog is small.** 22 rules, no opt-in packs. You can switch a
  rule off with `[commands] allow`; you cannot add rules in `.reflex.toml`. A destructive command
  that is not in the catalog passes.
- **It supports seven agents,** plus the git pre-commit hook; other agents get no protection.
- **It cannot ask in every agent.** Codex CLI, Cline, OpenCode and Kilo Code have no way for a
  hook to ask for confirmation, so a command that would ask is refused instead. OpenCode and Kilo
  Code let an identical repeat through.
- **Secret-read protection is not complete across agents.** The read tools of Cursor, Cline and
  pi are not wired, so only their shell commands are checked. Codex CLI is checked through its
  shell commands only. Claude Code `Grep` is checked only when its `path` is a file.
- **It does not watch tools it is not wired to.** MCP servers, web tools and, in Claude Code,
  `Glob`.
- **It does not judge whether a change is right.** It runs the test command you set and reacts
  to the result. A suite that exits with 126, 127 or 9009 for its own reasons is read as "could
  not run".
- **It depends on being installed.** A `.claude/settings.json` that is committed reaches
  teammates who do not have `reflex`; Claude Code keeps working for them, with the protection
  off.
- **It is a research prototype, tested on synthetic fixtures.** Only Claude Code has
  been run in real sessions: two short headless runs with one model, on macOS
  ([2026-10-02](evidence/claude-code-live-2026-10-02.md),
  [2026-10-22](evidence/claude-code-live-2026-10-22.md)). The other agents were checked against
  their source code or secondary descriptions. The corpus of 179 commands is a regression test,
  not a measure of how often the guard is right. See [agent-hooks.md](agent-hooks.md#limits)
  for the limits in detail.

## Sources

Read on 2026-10-22.

- prove-it-mini: https://github.com/OlegTestov/prove-it-mini
- kora-agentgate: https://github.com/Krako-Labs/kora-agentgate
- Claude Code hooks documentation: https://code.claude.com/docs/en/hooks

For Reflex Control the facts come from this repository: `crates/reflex-hooks/src/guard.rs`,
`rules.rs` and `config.rs`, the corpus test, and [agent-hooks.md](agent-hooks.md). Star counts
and features of the other projects change; check the sources before you rely on a row.
