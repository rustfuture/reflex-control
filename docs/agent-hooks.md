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
curl -fsSL https://raw.githubusercontent.com/rustfuture/reflex-control/main/install.sh | sh
reflex install
# done
```

On Windows, in PowerShell: `irm https://raw.githubusercontent.com/rustfuture/reflex-control/main/install.ps1 | iex`.
With Rust installed you can build from source instead:
`cargo install --git https://github.com/rustfuture/reflex-control reflex-cli`.
The installer puts `reflex` in `~/.local/bin` (`%LOCALAPPDATA%\reflex\bin` on Windows) and tells you if that
directory is not on your `PATH`.

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

The table is what Reflex decides; each agent decides how much of it can be carried out.
Cline cannot run tests at turn end at all, and OpenCode, Kilo Code and Cline have no way to
ask you to confirm a command (see their sections below).

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
  file. Relative write targets use the agent's working directory when provided, otherwise
  the project root. Directory changes from `cd` inside the command are not tracked.
  Treat it as a guard rail against mistakes, not a sandbox. `cat .env` is allowed.
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

### Codex CLI

Installs into `.codex/hooks.json` (this project) or `~/.codex/hooks.json` (all your
projects; `CODEX_HOME` is not consulted). Existing hooks are kept; ours are the ones whose
command starts with `reflex hook`.

- `PreToolUse` on `apply_patch` and `Bash`: Codex edits files with `apply_patch`, so the
  files named in the patch (`Add File`, `Update File`, `Delete File`, `Move to`) are
  checked, and one protected file blocks the whole patch. Shell commands get the same
  check as in Claude Code, and `apply_patch <<'EOF'` typed into the shell tool is read as a
  patch too (only when the command starts with `apply_patch`).
- `Stop`: when the tests fail, the failure output becomes the agent's next prompt, up to
  `tests.max_retries` times; otherwise you get a warning message and Codex stops.
- Codex hooks cannot ask for confirmation: it rejects `permissionDecision: "ask"`. A
  command that would ask you (force push, `rm -rf /`) is denied instead, and the agent is
  told to ask you and, if you agree, to have you run it yourself.
- Cannot see reads or other tools (MCP, web).

**Hooks must be trusted.** Codex does not run a user or project hook until you have
approved it. It asks when it starts (or open `/hooks`), and asks again whenever the hook
changes (its hash covers the event, matcher, command and timeout). Until then Reflex
Control does nothing in Codex, silently. `reflex install` prints a reminder;
`reflex doctor` only shows that the file is in place, not whether Codex trusts it. In a
project, Codex may also skip `.codex/` until the project itself is trusted (it says so
when it starts).

**Verified against the Codex source** (`codex-rs/hooks` and `codex-rs/core` in
https://github.com/openai/codex, main as of 2026-09-30): the `hooks.json` shape
(`hooks` -> event -> `[{matcher, hooks: [{type, command, timeout}]}]`, `timeout` in
seconds, no other top-level key but `description`); the `PreToolUse` and `Stop` input
fields; `apply_patch` and `Bash` as tool names, with the patch text in
`tool_input.command`; deny by JSON or by exit code 2 with a reason on stderr; `ask` and
`allow` rejected from `PreToolUse`; `decision: "block"` and `systemMessage` on `Stop`;
that untrusted hooks do not run; and an integration test there showing that a
`PreToolUse` deny stops `apply_patch` before it writes.

**Not verified.** Codex issue 27833 reports that an `apply_patch` deny was not enforced on
0.133.0. The issue itself could not be read from here, so its status is unknown; the
enforcement is in the current source and has a test, but which released version has it was
not checked. Test with your version: ask Codex to edit `.env` and see the edit refused.
Nothing was run against a real Codex either.

### OpenCode

Installs one plugin file: `.opencode/plugins/reflex.js` (this project) or
`~/.config/opencode/plugins/reflex.js` (all your projects; `XDG_CONFIG_HOME` is not
consulted). OpenCode loads every file in those directories at startup, so restart it after
installing. Nothing else is changed, and `reflex uninstall` deletes only that file.

The plugin is a short JS file that lives inside the `reflex` binary (source:
`integrations/opencode/reflex.js`; no npm dependencies). It runs in OpenCode's own process
and only relays: it sends each tool call to `reflex hook opencode pre-tool` and each idle
session to `reflex hook opencode turn-end`, and applies the JSON verdict that comes back.
The decisions are made by `reflex`. If `reflex` is missing, takes longer than 10 s (10 min
at turn end, where it runs the tests) or answers with anything unexpected, the plugin
allows. It starts `reflex` as a child process without waiting on it, so OpenCode stays
responsive while the tests run.

- `write`, `edit`, `multiedit` and `apply_patch` (the files named in the patch): a write to
  a protected path is refused by throwing, and the reason goes to the model.
- `bash`: a command that writes to a protected path is refused the same way.
- **Risky commands cannot be confirmed.** OpenCode has no confirmation prompt for plugins.
  A command that would ask you (force push, `rm -rf /`) is refused once, with a message
  telling the agent to ask you first; if the agent then repeats exactly the same call, it
  goes through. That is weaker than Claude Code's prompt: the agent could repeat the call
  without asking you.
- Turn end is the `session.idle` event. When the tests fail, the failure output is sent
  to the session as a new message (`client.session.promptAsync`) and the agent keeps
  working, up to `tests.max_retries` times. When it is out of tries, or for a large change,
  the message goes to OpenCode's log and, in the terminal UI, a toast.
- Not checked at turn end: subagent sessions (the parent is checked instead) and a session
  whose last run ended in an error.
- Cannot see reads, or what MCP and custom tools do. The shell check has the limits listed
  under Claude Code.

**Verified against the OpenCode source** (`sst/opencode`, `dev` as of 2026-09-30): the
plugin directories; that files there load as modules whose default export is `{ id,
server }`; the `tool.execute.before` hook and that a throw refuses the call; the tool ids
and arguments (`write` and `edit` with `filePath`, `apply_patch` with `patchText` in the
`*** Update File:` format, `bash` with `command`); the `session.idle`, `session.error` and
`session.status` events; and `client.session.get`, `client.session.promptAsync`,
`client.app.log` and `client.tui.showToast` in the SDK. The `permission.ask` hook is
declared there but nothing calls it, which is why there is no confirmation.

**Not verified.** The plugin has been run only against a fake `reflex` and a stand-in for
OpenCode's plugin API (under Node and Bun), never inside a real OpenCode. Assumed:
- versions from before the `{ id, server }` module shape (about March 2026) may not load
  the file;
- `multiedit` and `patch` are tool names from older versions and are not in the current
  source; they are checked in case a version still uses them;
- that a `session.idle` after you press Esc is skipped because OpenCode also reports an
  error for that stop; if it does not, the tests run after a stop too;
- that sending a message from inside the idle event starts a new run cleanly.

### Kilo Code

Kilo Code is a fork of OpenCode and uses the same plugin, installed for Kilo:
`.kilo/plugin/reflex.js` (this project) or `~/.config/kilo/plugin/reflex.js` (all your
projects). Kilo does not read `.opencode/`. Its VS Code extension runs the Kilo CLI, and
Kilo's documentation says plugins work in both. Restart Kilo after installing. Everything
under OpenCode applies, with `reflex hook kilo ...` as the command.

**Verified** in the Kilo source (`Kilo-Org/kilocode`, as of 2026-09-30) and its plugin
documentation: the plugin directories (`plugin/` or `plugins/` inside `.kilo/`, the older
`.kilocode/` and `~/.config/kilo/`), the `{ id, server }` module shape that its
documentation asks for local files, the same hooks, tool ids and SDK calls as OpenCode.
**Not verified:** nothing was run in Kilo, and the JetBrains extension was not checked.

### Cline

Installs one hook script, `PreToolUse`: `.clinerules/hooks/PreToolUse` (this project) or
`~/Documents/Cline/Hooks/PreToolUse` (all your projects). On Windows the file is
`PreToolUse.ps1`, which Cline runs through PowerShell. It is a few lines that run
`reflex hook cline pre-tool`. The VS Code extension and the Cline CLI both read these
locations (the CLI also reads `.cline/hooks`, which is left alone so that the hook does not
run twice).

**Turn on hooks.** Cline runs hooks only when they are enabled: in VS Code, tick "Enable
Hooks" in Cline's settings (Feature Settings). The CLI runs them unless started with
`--yolo` (as Cline's hook README says). `reflex install` prints this reminder; `reflex doctor` only shows that the file
is in place.

- `editor` (and the older `write_to_file`, `replace_in_file`, `delete_file`) and
  `apply_patch` (the files named in the patch): a write to a protected path is cancelled.
- `run_commands` (and the older `execute_command`): a command that writes to a protected
  path is cancelled. Every command in the call is checked.
- **Cancelling ends the task run.** Cline stops the tool call and the current run, and shows
  the reason; you have to send Cline another message to go on.
- **Risky commands cannot be confirmed.** Cline's reply format has a `review` field that is
  supposed to ask you, but the CLI reads it and does nothing with it, and the extension's
  format has no such field. A command that would ask you (force push, `rm -rf /`) is
  therefore cancelled, with a message saying so; run it yourself if you want it.
- **No tests at the end of a task.** Cline's `TaskComplete` hook runs after the task has
  finished and its reply is ignored, so a hook cannot send the agent back to fix failing
  tests. `reflex install` therefore installs no turn-end hook for Cline. Add the Git
  pre-commit hook: it runs the tests before any commit, whichever tool makes it.
- Cannot see reads, other tools or MCP. The `apply_patch` call of the VS Code extension
  carries its patch only in the tool's input; if Cline passes that as a bare string the
  extension's hook payload has no parameters, and the patch is not checked. The shell
  check has the limits listed under Claude Code.
- Cline runs one file per event and directory. If a `PreToolUse` of yours is already
  there, it is not changed; call `reflex hook cline pre-tool` from it instead (it reads
  Cline's JSON on stdin and prints the reply).
- The extension stops a hook after 30 s, the CLI after 120 s; the check itself takes a
  moment.

**Verified against the Cline source** (`cline/cline`, `main` as of 2026-09-30): the hook
directories and file names for the CLI, the extension and both operating systems; that a
hook file is run with the event as JSON on stdin; both payload shapes (`tool_call.name` and
`tool_call.input` for the CLI, `preToolUse.toolName` and `preToolUse.parameters`, with
non-string values JSON-encoded, for the extension); the tool names and their input shapes;
`cancel` and `errorMessage` stopping the run; `review` being parsed but ignored; and
`TaskComplete` / `agent_end` being observe-only.

**Not verified.** Nothing was run in Cline. The Windows script was not run at all, not even
under PowerShell. Reading the hook's stdin as text under Windows PowerShell 5.1 uses the
console code page, so non-ASCII paths may be misread there.

### pi

Installs one extension file: `.pi/extensions/reflex.ts` (this project; pi loads project
extensions only after you have trusted the project) or `~/.pi/agent/extensions/reflex.ts`
(all your projects; `PI_CODING_AGENT_DIR` is not consulted). Restart pi or run `/reload`.
The file is a short plain-JS extension inside the `reflex` binary (source:
`integrations/pi/reflex.ts`, no dependencies) that relays to `reflex hook pi pre-tool` and
`reflex hook pi turn-end`, with the same fail-open rules and child process as the OpenCode
plugin. Every handler catches its own errors, because pi blocks a tool whose `tool_call`
handler throws.

- `write` and `edit`: a write to a protected path is blocked, with the reason.
- `bash` and `powershell`: a command that writes to a protected path is blocked; a risky
  one opens pi's own confirmation dialog (and is blocked when pi has no UI, as in print
  mode).
- Turn end is `agent_before_settle`, for runs that completed. When the tests fail, the
  failure output is added as a message and pi makes one more model request, up to
  `tests.max_retries` times in total. When it is out of tries, or for a large change, you get
  a notification.
- Cannot see reads or other tools. The shell check has the limits listed under Claude Code.

**Verified against the pi source** (`badlogic/pi-mono`, `main` as of 2026-09-30): the
extension directories and that a default-exported factory is loaded from a `.ts` file; the
`tool_call` event, `{ block, reason }` and the failure-blocks rule; `agent_before_settle`,
its `outcome`, and that a handler's `entries` replace the list (so the extension returns the
existing entries plus its own) and `continue: true` requests one more request; the
`custom_message` entry shape; the tool names and arguments (`path`, `command`); and
`ctx.cwd`, `ctx.hasUI`, `ctx.ui.confirm` and `ctx.ui.notify`.

**Not verified.** The extension has only run against a fake `reflex` and a stand-in for pi's
API under Node, not inside pi. Assumed: that a `custom_message` entry reaches the model as
a user message on the continued request (the source says custom messages are sent to the
model, but this was not run), and that jiti loads the file as written.

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
| `.codex/hooks.json` (or `~/.codex/`)     | our hook entries merged in, everything else kept                |
| `.opencode/plugins/reflex.js` (or `~/.config/opencode/plugins/`) | one plugin file, created or updated     |
| `.kilo/plugin/reflex.js` (or `~/.config/kilo/plugin/`) | one plugin file, created or updated               |
| `.clinerules/hooks/PreToolUse` (or `~/Documents/Cline/Hooks/`; `.ps1` on Windows) | one hook script, created or updated |
| `.pi/extensions/reflex.ts` (or `~/.pi/agent/extensions/`) | one extension file, created or updated        |
| `.git/hooks/pre-commit`                  | created, or an existing hook chained as above                   |
| `.gitignore`                             | `.reflex/` and `*.reflex-bak` appended (project scope)          |

Files that are rewritten are copied to `<file>.reflex-bak` first, once. Installing twice
changes nothing the second time. `reflex uninstall` removes only our hook entries and
files, and deletes a settings file that ends up empty; it leaves `.reflex.toml` and
`.gitignore` alone.

The plugin, extension and script files of OpenCode, Kilo Code, Cline and pi are wholly
ours and carry a `reflex-control managed hook` line in their header. Install updates such a
file to the current version; a file of yours at the same path is never overwritten (the
install says what to add to it by hand); uninstall deletes only files with that line.

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
