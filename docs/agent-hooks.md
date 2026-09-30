# Agent hooks: plug in and forget

Reflex Control can sit inside your coding agent as a hook. Once installed, it runs
on its own at three moments: before the agent writes a file, before it runs a shell
command, and when it says it is done. You do not call it.

What it does for you:

- **Protects files.** The agent cannot write to `.env`, key files, `secrets/`, CI workflows
  and migrations (you choose the list).
- **Asks before risky commands.** `rm -rf /`, `git push --force` and similar ask you first.
- **Checks the work.** When the agent finishes, your tests run. If they fail, the agent is
  sent back to fix them, a limited number of times, and then you are asked.
- **Flags big changes.** A very large diff gets a "please review" note.

## Quick start

```sh
cargo install --git https://github.com/rustfuture/reflex-control reflex-cli
reflex install
# done
```

`reflex install` is a short wizard: it detects your tools and test command, asks a few
questions, shows the files it will change, and writes them only after you confirm.
Check the result with `reflex doctor`; undo it with `reflex uninstall`.

Hooks call `reflex` by name, so it must be on your `PATH` (`cargo install` puts it in
`~/.cargo/bin`). `install` and `doctor` warn if it is not.

## What happens, and when

Turn end means "the agent stopped and wants to hand control back".

| Situation                                                        | Outcome                                           |
|------------------------------------------------------------------|---------------------------------------------------|
| Agent writes a protected path                                    | Blocked, with the reason shown to the agent       |
| Shell command redirects into, or `cp`/`mv`/`rm`/`tee`/`sed -i` on, a protected path | Blocked                     |
| `rm -rf` on a broad target (`/`, `~`, `.`, `*`, `/usr`, ...)     | You are asked to confirm                          |
| `git push --force`, `-f` or `+ref`                               | You are asked to confirm                          |
| `git commit --no-verify` or `-n` (skips the pre-commit check)    | You are asked to confirm                          |
| Any other tool call or command                                   | Allowed, silently                                 |
| Turn end, nothing changed in the working tree                    | Allowed, tests not run                            |
| Turn end, tests pass, diff small                                 | Allowed, silently                                 |
| Turn end, tests pass, diff over `review.max_diff_lines`          | Allowed, you get a "large change, please review" note |
| Turn end, no test command configured                             | Allowed (a large diff still gets the note)        |
| Turn end, tests fail, `on_failure = "retry-then-ask"`, retries left | The agent keeps working; it is given the last 40 lines of test output |
| Turn end, tests fail, `retry-then-ask`, retries used up          | The agent stops and you are asked to look         |
| Turn end, tests fail, `on_failure = "ask"`                       | The agent stops and you are asked right away      |
| Turn end, tests fail, `on_failure = "notify"`                    | The agent stops; you get a message                |
| `git commit` with a protected file staged                        | Commit refused, files listed                      |
| `git commit` and `git.run_tests` is on and tests fail            | Commit refused, test output shown                 |
| Reflex itself breaks (bad JSON, unreadable config, no git)       | Warning on stderr, everything allowed             |

Details worth knowing:

- Tests are skipped when the working tree is identical to the last check that did not
  fail. "The working tree" is `git diff HEAD` plus the untracked files (name, size and
  modification time).
- The retry counter is per session, lives in `.reflex/state/`, resets when the tests pass
  or when you are asked, and never exceeds `tests.max_retries`.
- Test runs at turn end are stopped after 9 minutes.
- Test output is a combined stdout and stderr stream, and the tests get no stdin.

## `.reflex.toml`

Every key is optional. This is the full file with its defaults:

```toml
[agents]
enabled = ["claude-code", "git"]
scope = "project"             # "project" | "user"

[protect]
paths = [".env", ".env.*", "*.pem", "*.key", "secrets/**", ".github/workflows/**", "migrations/**"]

[tests]
command = "cargo test"        # empty string = do not run tests
on_failure = "retry-then-ask" # "retry-then-ask" | "ask" | "notify"
max_retries = 2

[review]
max_diff_lines = 800          # added + removed lines

[git]
run_tests = true              # run tests.command in the pre-commit hook
```

The installer writes the file for you, with `tests.command` set to what it detected
(`Cargo.toml` -> `cargo test`, a `package.json` test script -> `npm test`,
`pyproject.toml` or `pytest.ini` -> `pytest`, `go.mod` -> `go test ./...`).

**Where it is looked for:** `.reflex.toml` in the current directory or a parent, up to
the repository root; otherwise `~/.config/reflex/reflex.toml`; otherwise the defaults.
A file that does not parse is reported by `reflex doctor`, and the hooks allow everything
until it is fixed.

**Path patterns.** A pattern without `/` matches the file name at any depth, so `.env`
also protects `app/.env`. A pattern with `/` is relative to the project root, so
`secrets/**` protects `secrets/x` but not `src/secrets/x`. Case is ignored. `.env.*` also
matches `.env.example`; remove it from the list if the agent should edit that file.
An absolute path outside the project is checked with the name-only patterns.

**Re-running the wizard.** `reflex install` reads an existing `.reflex.toml` and starts
every question from your current answers, so running it again is how you change settings.
It rewrites the file (comments you added are not kept; the original is saved once as
`.reflex.toml.reflex-bak`).

## Non-interactive use

Without a terminal, or with `--yes`, nothing is asked:

```sh
reflex install --yes --agent claude-code,git --scope project \
  --test-command "cargo test" --on-failure retry-then-ask
```

Flags win, then an existing `.reflex.toml`, then detection. `--test-command ""` turns
tests off. `reflex uninstall --yes` skips its confirmation.

## What each integration can and cannot do

### Claude Code

Installs into `.claude/settings.json` (this project) or `~/.claude/settings.json` (all
your projects). Existing settings and hooks are kept; ours are the ones whose command
starts with `reflex hook`.

- `PreToolUse` on `Edit`, `Write`, `MultiEdit`, `NotebookEdit` and `Bash`: can deny a call
  or ask you to confirm it.
- `Stop`: can send the agent back to work (with the failure output as the reason) or
  let it stop and show you a message.
- Cannot see what the agent reads, or what other tools (MCP servers, `WebFetch`, ...) do.
- The shell check reads the command text. It does not run the command, expand variables
  or follow `$(...)`, and it cannot see a script or `python -c` writing to a protected
  file. Treat it as a guard rail against mistakes, not a sandbox. `cat .env` is allowed.
- Claude Code stops sending the agent back after a fixed number of consecutive stop
  blocks of its own; the default retry budget (2) is well below it.
- A committed `.claude/settings.json` reaches teammates who do not have `reflex`
  installed. Claude Code treats a missing hook command as a non-blocking error, so it
  keeps working for them, but the protection is off. Use user scope if you do not want
  to commit it.

### Cursor

Installs into `.cursor/hooks.json` (this project) or `~/.cursor/hooks.json` (all your
projects). Existing hooks are kept; ours are the entries whose `command` starts with
`reflex hook`. Restart Cursor (or reload its hooks) after installing.

- `preToolUse` on `Write` and `Delete`: blocks a write to a protected path.
- `beforeShellExecution`: blocks a command that writes to a protected path, and asks you
  to confirm risky ones (force push, `rm -rf` on a broad target).
- `stop`: when the tests fail, the agent gets the failure output as a follow-up message and
  keeps working, up to `tests.max_retries` times. When it is out of tries, or for a large
  change, the message goes to stderr (Cursor's hooks log) and the agent stops; Cursor has
  no documented way to show a message at that point.
- A block also exits with code 2, which Cursor treats as a deny by itself, so it holds
  even if the JSON reply is not understood. Cursor lets an action through if a hook
  crashes or times out.
- The shell tool is left out of the `preToolUse` matcher on purpose: `beforeShellExecution`
  is the one that can ask, and using both would ask you twice.
- Cannot see reads, edits by tools other than `Write` and `Delete`, or what MCP tools
  do. The shell check has the limits listed under Claude Code.

**Not verified.** The official hooks page (https://cursor.com/docs/hooks) could not be read
while this was written; everything above comes from secondary descriptions of it, and
nothing has been run against a real Cursor. In particular:

- whether an edit to an existing file arrives as a `Write` call (if Cursor uses another
  tool name for in-place edits, those are not checked);
- the shape of `tool_input` for `Write` and `Delete`; the adapter looks for the path under
  `file_path`, `path`, `filePath` and `target_file`, and allows the call if none is there;
- whether `matcher` on `preToolUse` is honoured (the adapter ignores other tools either way);
- that `timeout` is in seconds, and that `stop` accepts `followup_message` from a hook
  installed like this.

If Cursor rejects the file, `reflex uninstall` removes our entries again.

### Git pre-commit

Installs `.git/hooks/pre-commit` (or the directory `core.hooksPath` points to). It
checks the staged files against `protect.paths` and, if `git.run_tests` is on and a test
command is set, runs the tests. Either failing refuses the commit (exit code 1).

- It runs for every commit made in that repository, whichever tool made it, but only
  in this clone: hooks are not shared through git. It is project scope only.
- `git commit --no-verify` skips it, as with any hook. When an agent with a Reflex hook tries
  that, you are asked to confirm first.
- If `reflex` crashes or is not on `PATH`, the commit goes through; only a deliberate
  refusal (exit code 1) blocks.
- If a pre-commit hook of yours already exists, it is renamed to `pre-commit.local` and
  our script runs it first with the same arguments; if it fails, the commit stops there.
  `reflex uninstall` puts it back. Nothing is changed, and the one line to add by hand
  (`reflex hook git pre-commit`) is printed instead, if `pre-commit.local` already exists,
  or if the hooks directory is inside the repository (for example `.husky` or
  `.githooks`), because that would edit files your whole team shares.

## If the hook breaks

A hook must never lock you out of your own agent. If Reflex cannot read its input, cannot
parse `.reflex.toml`, or cannot run git, it prints one warning line to stderr and allows
the action (exit code 0). Run `reflex doctor` to see what is wrong. The only non-zero exit
is the git hook's deliberate refusal.

## Files the installer touches

| File                                     | Change                                                          |
|------------------------------------------|-----------------------------------------------------------------|
| `.reflex.toml` (or `~/.config/reflex/reflex.toml`) | created or rewritten                                  |
| `.claude/settings.json` (or `~/.claude/`) | our hook entries merged in, everything else kept                |
| `.cursor/hooks.json` (or `~/.cursor/`)   | our hook entries merged in, everything else kept                |
| `.git/hooks/pre-commit`                  | created, or an existing hook chained as above                   |
| `.gitignore`                             | `.reflex/` and `*.reflex-bak` appended (project scope)          |

Files that are rewritten are copied to `<file>.reflex-bak` first, once. Installing twice
changes nothing the second time. `reflex uninstall` removes only our hook entries and
files, and deletes a settings file that ends up empty; it leaves `.reflex.toml` and
`.gitignore` alone.

## About the decision engine

Hook mode uses the same `GuardedHybridComposer` as the rest of Reflex Control, fed with
deterministic evidence only (test result, diff size, retry count) and no semantic
signals. A pass is accepted only if the composer accepts it; failures are handled by the
retry and ask rules above, because without semantic signals the composer treats every
failure as non-transient.

**What this does not show.** Hook mode uses deterministic checks only by default. The
held-out results in the README (0/31 missed escalations) were measured with Jev semantic
signals and do not describe hook mode. Nothing here has been measured on real agent
sessions, and the checks above cannot judge whether a change is correct beyond "the
tests pass".

## Adding another agent

An agent is one file in `crates/reflex-hooks/src/adapters/` implementing `AgentAdapter`
(parse the agent's hook input, render a verdict in its reply format, plan its install
and uninstall) plus one line in the registry in `adapters/mod.rs`. The wizard, `reflex
hook`, `reflex doctor` and uninstall pick it up from the registry.
