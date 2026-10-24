# Validation: how well the guard decides (2026-10-23)

These are measurements, not claims. Each one names its oracle, its method and what it
cannot see. All of them run from the repository with no network access and no
dependencies beyond `cargo` and Python 3's standard library.

| What | Where | Command |
| --- | --- | --- |
| Corpus (what the defaults must decide) | `crates/reflex-hooks/tests/fixtures/guard_corpus.tsv` | `cargo test -p reflex-hooks --test guard_corpus -- --nocapture` |
| Metamorphic rewrites (parser consistency) | `crates/reflex-hooks/tests/metamorphic.rs` | `cargo test -p reflex-hooks --test metamorphic -- --nocapture` |
| Bypass matrix (same act, another spelling) | `crates/reflex-hooks/tests/fixtures/bypass_matrix.tsv` | `cargo test -p reflex-hooks --test bypass_matrix -- --nocapture` |
| Adapter parity (each agent's JSON → the same command) | `crates/reflex-hooks/tests/adapter_parity.rs` | `cargo test -p reflex-hooks --test adapter_parity -- --nocapture` |
| Hook latency | `bench/latency.py` | `python3 bench/latency.py --reflex target/release/reflex` |
| Real agent traffic (local transcripts) | `bench/traffic.py` | `python3 bench/traffic.py --reflex target/release/reflex --out /tmp/traffic` |

The corpus is a regression test written for Reflex: its expected verdicts are Reflex's
own answers, so "corpus 100%" says the decisions did not change, not that they are right.
The other measurements do not take expected answers from the same place as the code:
metamorphic rewrites use the original command's verdict as the oracle, the bypass matrix
lists the command in the shape it is actually written, and the traffic measurement takes
commands that really ran.

## 1. Metamorphic rewrites

A rewrite that does not change what a command does must not change the verdict. Each of
the 179 corpus commands is rewritten in eleven ways and the verdict is compared with the
original's (1 872 comparisons):

| rewrite | same verdict |
| --- | --- |
| leading spaces | 179/179 |
| `bash -c '…'` | 179/179 |
| `sh -c '…'` | 179/179 |
| trailing spaces | 165/165 |
| `; true` after | 165/165 |
| ` # comment` after | 165/165 |
| `{ …; }` | 165/165 |
| `true;` in front | 167/167 |
| backslash-newline after the first word | 166/166 |
| `env REFLEX_TEST=1` in front | 171/171 |
| `command` in front | 171/171 |

Finding: the first run agreed on 91/166 backslash-newline rewrites — `git \` + newline +
`push --force` was allowed. A shell deletes backslash-newline before it parses the line;
the guard did not. Fixed (`guard::strip_line_continuations`), and continuous lines are now
in the corpus (`rm \` + newline + `-rf /`).

Not covered: a trailing backslash escapes the next character, so appending to such a
command changes what it does and it gets no rewrite.

## 2. Bypass matrix

125 commands: 99 dangerous spellings that must stop, 18 harmless look-alikes that must
stay allowed, and 8 recorded blind spots (they must still pass; when one starts stopping,
the test fails and says to move the line, so a fix cannot be undone by accident).

| class | caught | known gaps |
| --- | ---: | ---: |
| path (`\rm`, `/bin/rm`, `command`, `env`, `timeout`, `nice`, `nohup`, `sudo -u`) | 8/8 | 0 |
| options (`-r -f`, `--recursive --force`, `-fr`, `-Rf`, `--`) | 5/5 | 0 |
| separators (`&&`, `;`, `\|\|`, newline, subshell) | 5/5 | 0 |
| shell (`bash -c`, `sh -c`, `bash -lc`, `zsh -c`, nested, `eval '…'`) | 7/7 | 1 (`eval "$(…)"`) |
| git (force, `-f`, `-fu`, `--mirror`, `--prune`, `+ref`, `:ref`, `-C`, `--git-dir`, reset, clean, discard, reflog, update-ref, filter-branch) | 17/17 | 0 |
| sql (`-c`, heredoc, pipe, `exec psql -c`, case) | 8/8 | 0 |
| secret reads (`head`, `tail`, `sed`, `awk`, `xxd`, `tar`, `zip`, `gzip`, `git diff`, `git show`, uploads) | 20/20 | 2 (`grep -r PASSWORD .`, `git diff .`) |
| protected writes (append, `tee`, `install`, `dd`, `sed -i`, `perl -pi`, `truncate`, `chmod`, `mv`, `shred`) | 14/14 | 0 |
| cloud/k8s/docker/terraform, HTTP DELETE | 6/6 + 3/3 | 0 |
| find (`-delete`, `-exec rm`) | 3/3 | 2 (`\| xargs rm`) |
| variables (`$HOME`) | 1/1 | 3 (`F=/; rm -rf "$F"`, `X=rm; $X`, `$(pwd)`) |
| Windows (`cmd.exe`, PowerShell) | 2/2 | 0 |
| false positives (safe look-alikes) | — | 0 of 18 |

Fixed because of this matrix, each now in the corpus:

- `git push --mirror` / `--prune` (history on the remote, was allowed),
- `rm -rf .git` (the repository's history),
- `eval 'rm -rf /'` (the script was treated as data),
- `tar`/`zip`/`gzip` as readers of a secret,
- `git diff/show/log/blame/cat-file <secret>` (the patch prints the file).

What it cannot see: values built at run time (variables, `$()`), the contents of a script
file, and `xargs` arguments. These are listed as gaps, not hidden.

## 3. Adapter parity

Each of the seven agent adapters (Claude Code, Codex, Cursor, OpenCode, Kilo Code, Cline,
pi) has its own hook JSON. The test parses every corpus command plus eight shapes that
JSON escaping or a hand-written parser could mangle (quotes, backslashes, Unicode,
heredoc in `$(…)`, backslash-newline) through every adapter and checks that the guard
receives the command byte-for-byte: 7 × 196 = 1 372 parses, 0 differences.
What it does not cover: whether the agent calls the hook at the right moment on its side.

## 4. Latency

`reflex 0.5.0` release build, macOS, one fresh `reflex hook claude-code pre-tool` process
per sample, stdin = Claude Code's PreToolUse JSON, 100 samples per command after 5
warm-up runs, throwaway `git init` project, throwaway HOME, default settings:

| command | verdict | first | p50 | p90 | p99 | max |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| `git status` | allow | 7.1 ms | 7.0 ms | 7.8 ms | 8.4 ms | 8.4 ms |
| `git reset --hard HEAD` | ask | 7.1 ms | 6.8 ms | 7.2 ms | 8.0 ms | 8.0 ms |
| `cat .env` | block | 6.7 ms | 6.8 ms | 7.4 ms | 8.6 ms | 8.6 ms |

About 7 ms per tool call, well inside the 10 s the agent allows a hook. This is the cost
on a project with default settings; a project with a large `.reflex.toml` or a slow disk
should measure its own with `bench/latency.py`.

## 5. Real agent traffic (local only)

`bench/traffic.py` reads the Bash commands from local agent transcripts (Claude Code
`~/.claude/projects`, Codex `~/.codex/sessions`), runs each distinct command through
`reflex check --` in a throwaway project with default settings and counts the decisions.
Commands stay on this machine; only counts are reported.

| source | transcripts | shell calls | distinct | allow | ask | block | error |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Claude Code | 123 | 4 004 | 3 824 | 3 747 | 34 | 43 | 0 |
| Codex | 521 | 26 746 | 12 584 | 12 434 | 6 | 144 | 0 |

16 404 distinct commands, 226 would stop (1.4 %; 40 ask, 187 block, 0 errors), and all
226 are in `bench/traffic.md` of the measuring run (not committed: they contain local
paths). What actually stopped:

| reason | commands |
| --- | ---: |
| reading `.env`, `.env.*`, `secrets/**`, `*.pem`, `id_rsa` — in a shell command or in a `python3 <<PY` / `node <<NODE` heredoc | 152 |
| `node -e "require('dotenv')…"` — the module name is `dotenv`, a secret-file name | 3 |
| writing `.env`, `.github/workflows/**`, `secrets/**` | 14 |
| adding a rule to Reflex's own `.reflex.toml` | 1 |
| an own-settings file (`~/.gemini/.../settings.json.bak-*`) in a compound command | 1 |
| scripts that write into `.env`/`.env.live` inside their own heredoc text | 16 |
| `git.branch-force-delete` | 20 |
| `git.reset-hard` | 7 |
| `git.clean-force`, `git.discard-changes`, `fs.rm-rf-broad` | 2 each |
| `git.push-force`, `git.push-delete`, `git.history-rewrite` | 1 each |

The single new false-positive shape is the third row: `protect.read` matches file names,
so `dotenv` matches the `.env` pattern. It is the only one of 16 404 commands and is not
fixed here because the fix changes match semantics; it is recorded instead.

Reading the block list is the useful part: the secret and protected-write decisions are
what stops a human-driven agent from reading its own `.env` while debugging. That is the
policy working, not an alarm, but it is friction without a switch: `commands.allow` turns
off rules, and reads are blocked by `protect.read`, which has no per-pattern allow yet. A
user who wants `sed -n 1,40p .env` to work has to edit `protect.read` itself. Making that
easy (an allow list, or `reflex check` telling the user which config line to change) is
the next product question, not a measurement.

Limits: these are one person's transcripts, mostly long autonomous sessions. They say
what would have stopped, not whether the user would have agreed; a `block` on a command
the user wrote themselves is a false alarm in the eye of that user.

## What is still not measured

- Whether the guard is *right* on commands it allows: no independent oracle exists for
  that without running the command and watching what it does (a sandbox experiment).
- The other six agents have not been run live; their hook wiring is checked against the
  agents' documented JSON and golden files, not against a live session.
- Windows and macOS `cmd.exe`/PowerShell shapes are covered by 2 cases only.
