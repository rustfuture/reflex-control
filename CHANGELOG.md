# Changelog

All notable changes to Reflex Control are documented here.

## Unreleased

- Fixed: a backslash-newline continued line (`git \` then newline then `push --force`) was read as the first word only and a continued `rm -rf /` or `git push --force` was allowed. The shell removes the pair before parsing, and so does the guard now (outside single quotes, where it is literal). Found by the metamorphic test.

- Fixed: `git push --mirror` and `git push --prune` now hit `git.push-force` and `git.push-delete` (both rewrite or delete remote history; the short `-f`/`-d` spellings were matched, the long ones were not).

- Fixed: `rm -rf .git`, `.git/` and `sub/.git` now hit `fs.rm-rf-broad` (the repository's whole history).

- Fixed: `eval 'rm -rf /'`, `eval "git push --force"` and other literal `eval` arguments are checked as the command line they run (a substitution in them, `eval "$(…)"`, still is not).

- Fixed: `tar`, `zip`, `gzip`, `zstd`, `xz` and `bzip2` are secret readers, so `tar cf - .env`, `zip /tmp/x.zip .env` and `gzip -c .env > /tmp/x.gz` are blocked.

- Fixed: `git diff`, `git show`, `git log`, `git blame` and `git cat-file` of a secret path (`git diff .env`, `git show HEAD:.env`) are blocked; the diff prints the file.

- New tests and measurements, described in [docs/validation.md](docs/validation.md): a metamorphic test over 1 872 rewrites of the corpus (`tests/metamorphic.rs`), a 125-command bypass matrix with recorded blind spots (`tests/bypass_matrix.tsv`), an adapter parity test that parses every corpus command through all seven adapters (`tests/adapter_parity.rs`), and two scripts under `bench/`: `latency.py` (~7 ms per hook call on macOS) and `traffic.py` (what would stop in an agent's real shell history). The corpus grew to 188 commands (54 allow, 75 ask, 59 block).

- Closed ways around protected-path writes in the shell check: `ln` (both the link and its target), `truncate`, `touch`, `chmod`/`chown`/`chgrp`, `curl -o`/`--output`, `wget -O`, `git checkout`/`restore`/`rm`/`mv` on a protected path (`git restore --staged` and `git rm --cached` stay allowed), the destination of `rsync` and `scp`, and `perl -i`/`ruby -i`.

- The shell check now reads interpreter code: `python -c`, `node -e`/`--eval`, `bun`, `deno eval`, `perl -e`, `ruby -e` and `php -r`, and code fed to an interpreter through a heredoc or here-string. A string literal that names a protected path counts as a write when the code also contains a write hint (`write`, `remove`, `unlink`, `rename`, a `'w'` open mode, ...). Both `python3 -c "open('.env','w')"` and `ln -sf x .env` got through the 0.5.0 check.

- Heredoc and here-string bodies are now read according to the program that gets them: a shell parses them as shell, an interpreter scans them as code, a database client checks them as SQL, and anything else (`cat`, `tee`) treats them as data. `cat <<EOF > notes.md` with `rm -rf /` in the body is allowed; `bash <<EOF` with `echo x > .env` is blocked.

- `~/`, `$HOME/` and `${HOME}/` at the start of a path in a shell command are resolved to the home directory before matching.

- Path patterns in `protect.paths` take exceptions with a leading `!`. The defaults gain `!.env.example`, `!.env.sample` and `!.env.template`, so those files can be edited although `.env.*` matches them. An existing `.reflex.toml` keeps its `paths` as written; add the three lines to get the exceptions.

- Added a destructive-command catalog (`crates/reflex-hooks/src/rules.rs`): 22 rules with stable ids (`fs.rm-rf-broad`, `fs.find-delete`, `fs.chmod-broad`, `fs.disk`, `git.push-force`, `git.commit-no-verify`, `git.reset-hard`, `git.clean-force`, `git.discard-changes`, `git.branch-force-delete`, `git.stash-drop`, `git.push-delete`, `git.history-rewrite`, `db.sql`, `db.drop`, `db.redis-flush`, `db.orm-reset`, `container.prune`, `k8s.delete`, `iac.destroy`, `cloud.delete`, `http.delete`). It replaces the three fixed checks (`rm -rf` on a broad target, force push, `git commit --no-verify`), which are now `fs.rm-rf-broad`, `git.push-force` and `git.commit-no-verify`. It also covers Windows (`rmdir /s`, `del /s`, `Remove-Item -Recurse` on a drive root or broad target), SQL sent to a database client (in a heredoc or here-string, piped from `echo`, `printf` or `cat <<EOF`, or run inside a container with `docker|podman exec` or `kubectl|oc exec --`), and HTTP `DELETE` requests and GraphQL delete mutations sent with `curl` (the shape of the April 2026 PocketOS incident).

- Wrappers are seen through when matching rules and paths: `sudo`, `doas`, `env`, `command`, `nohup`, `time`, `exec`, `nice`, `npx`, `pnpx`, `bunx`, `uvx`, `bundle exec`, `poetry run`, `uv run`, `pipenv run`, `pdm run`, `hatch run`, `rye run`, `pnpm exec`/`dlx`, `yarn dlx` and `npm exec`.

- A command that matches a rule now asks (Claude Code, Cursor, pi) with a reason that ends in ``Confirm before it runs (rule `<id>`).``; Codex CLI, Cline, OpenCode and Kilo Code, which cannot ask, refuse it as before. The new setting `[commands] allow = ["git.reset-hard", ...]` switches single rules off; it never allows a protected write.

- Added secret-read protection: the new setting `protect.read` lists paths the agent may not read (default `.env`, `.env.*` with the three `!` exceptions, `*.pem`, `*.key`, `*.p12`, `*.pfx`, `secrets/**`, `id_rsa`, `id_ed25519`, `id_ecdsa`, `.netrc`, `.pgpass`). Blocked: Claude Code `Read` and `Grep` (when `path` is a file), OpenCode and Kilo Code `read`, and in every agent's shell the readers (`cat`, `head`, `grep`, `sed`, `jq`, `base64`, ...), `< .env`, copies that carry the file elsewhere, uploads (`curl -d @file`, `-F x=@file`, `-T file`, `wget --post-file`), `openssl -in`/`-inkey`/`-key`, `perl -ne ... .env` and interpreter code that names a secret path. `source .env`, `. .env`, `set -a; . ./.env; set +a`, `--env-file .env`, `dotenv ...`, `wc`, `ls`, `stat` and `test -f` stay allowed, so the agent can use a secret without its contents reaching the model provider; printing the loaded variables in the same command line (`source .env && echo $API_KEY`, a bare `env`, `printenv`, `export -p`) is blocked when a printed variable's name looks secret (it contains `KEY`, `TOKEN`, `SECRET`, `PASS`, `AUTH`, `URL`, `DSN` or similar), so `source .env; echo $PORT` stays allowed. The read tools of Cursor, Cline and pi are not wired (only their shell commands are checked), and `grep -r` over a directory that holds secrets is not caught.

- Behaviour change: `cat .env` and similar reads were allowed and are now blocked by default. Set `protect.read = []` to turn this off.

- Added self-protection: with `protect.own_files = true` (the default) agents may not write `.reflex.toml` (any depth), `.reflex/**`, `.claude/settings.json`, `.claude/settings.local.json`, `.cursor/hooks.json`, `.codex/hooks.json`, `.git/hooks/**`, `.clinerules/hooks/**`, the OpenCode, Kilo Code and pi plugin files, and the same user-scope files under the home directory.

- Test gate: if the test command cannot run (spawn error, exit 126 or 127, or 9009 on Windows), the agent is no longer sent back to fix it; the user gets a note to check `tests.command`. Before, a missing test runner was treated as a test failure and the agent was sent back.

- Test gate: if a fix attempt leaves exactly the same failure, the agent is stopped and the user is asked instead of using up the retry budget. Durations, thread ids in parentheses and hex addresses are ignored when comparing; counts and values are not.

- Added `reflex check`: `reflex check "<command>"`, `--write PATH`, `--read PATH` (repeatable, combinable) and `--rules`. It prints ALLOW, ASK or BLOCK with the reason, using the project's `.reflex.toml`, and runs nothing. Exit code 0 is allow, 1 is ask, 2 is block, 3 means the check itself failed (bad arguments, unreadable settings).

- The Claude Code `PreToolUse` matcher is now `Edit|Write|MultiEdit|NotebookEdit|Bash|Read|Grep`. Run `reflex install` again to update an existing install; until then `reflex doctor` reports an install with the old matcher as not installed.

- Added a corpus test (`crates/reflex-hooks/tests/guard_corpus.rs`) over 179 shell commands with the expected verdict for each (54 allow, 70 ask, 55 block), including incident shapes and a "known gaps" section (script files, variables, `eval` and base64, string concatenation in code, recursive `grep`). The golden test runners rewrite their expected output with `UPDATE_GOLDEN=1 cargo test`.

- Docs: added `docs/comparison.md` (how Reflex Control compares with prove-it-mini, kora-agentgate and Claude Code's built-in controls, and what it does not do) and a record of a second live Claude Code run, `docs/evidence/claude-code-live-2026-10-22.md`. Updated the README and `docs/agent-hooks.md`.

## 0.5.0 - 2026-10-02

- Added `reflex demo agent`: a styled walkthrough of six simulated agent steps (an edit, two protected writes, a force push, failing and then passing tests) with each decision taken by the real hook engine; no API key needed.

- Fixed shell protected-path checks to resolve relative write targets against the agent's working directory across all shell adapters, falling back to the project root when unknown.

- Added agent hooks for OpenCode, Kilo Code, Cline and pi. OpenCode, Kilo Code and pi get a small plugin or extension file (embedded in `reflex`, source in `integrations/`) that relays tool calls and finished turns to `reflex hook`; Cline gets a `PreToolUse` script. Cline has no hook that can send the agent back, so it is guarded before tool calls only; OpenCode, Kilo Code and Cline cannot ask for confirmation, so a risky command is refused (OpenCode and Kilo let the same call through when the agent repeats it). See `docs/agent-hooks.md`.

## 0.4.0 - 2026-10-02

- Added prebuilt release binaries and one-line installers. A `v*` tag now builds `reflex` for Linux (x86_64, aarch64), macOS (Intel, Apple Silicon) and Windows (x86_64) and publishes archives with SHA-256 checksums; `install.sh` (macOS/Linux) and `install.ps1` (Windows) download, verify and install the right one without Rust. See `docs/releasing.md`.
- Added agent hooks: `reflex install` (interactive setup wizard), `reflex uninstall` and `reflex doctor`, plus a `reflex hook <agent> <event>` entry point. Targets are Claude Code (`PreToolUse` and `Stop` hooks) and a generic git pre-commit hook. Hooks block writes to protected paths, ask before risky shell commands, run the project's tests when the agent stops and send failures back for a limited number of retries. Settings live in `.reflex.toml`. Hook mode uses deterministic evidence only; see `docs/agent-hooks.md`. New crate `reflex-hooks`; new dependencies `toml`, `globset`, `dirs` and `cliclack`.
- Added Cursor (`.cursor/hooks.json`) and Codex CLI (`.codex/hooks.json`) hook adapters for `reflex install`: writes to protected paths are denied, risky shell commands ask first in Cursor and are denied with a reason in Codex (which has no ask), and failing tests at the end of a turn are sent back for a limited number of retries. See `docs/agent-hooks.md`.
- `reflex run` now rejects an unrecognised `--risk` value instead of silently treating it as `low`, which would have skipped the mandatory High/Critical gate on a typo.
- README no longer says telemetry writes never block execution; they are synchronous SQLite writes.
- Added `examples/agent-runtime-gate`, mapping a runtime-style task result to Accept / Retry / Escalate offline.

## 0.3.0 - 2026-09-27

- Surfaced threshold freeze provenance in the generated evaluation report: the report now states whether thresholds were loaded from `fixtures/frozen_hybrid_config.json` (recording freeze timestamp, threshold values, and the calibration sweep's validation notes quoted verbatim) or fell back to built-in defaults (recording that they are not from a calibration sweep), and explicitly notes when `tau_accept` was overridden via `--risk-threshold`.
- `reflex experiment --phase all` now rejects `--risk-threshold` and `--quality-threshold`. Previously the sweep froze its own thresholds and the evaluation phase then applied `--risk-threshold` as an override, so the evaluated tau could differ from the frozen one while `--quality-threshold` was ignored. The flags' help text now states which phases read them, and the evaluation banners no longer claim 100 tasks regardless of the dataset.
- The calibration sweep now lists the task IDs behind the selected point's false accepts and frontier misses, with each task's actions, risk score, deterministic features and semantic signals, and prints how many distinct values each semantic signal takes. The per-task predicates are shared with the metric counts. `docs/experiments/calibration_miss_analysis.md` uses this to show that the 1/12 frontier miss seen at every mock setting (`v2-task-1075`) is a limit of the mock signals, which are derived from two deterministic flags; live Jev misses no frontier-required calibration task.
- Replaced the six hand-picked calibration settings with a 133-point grid (tau_accept 0.20–0.56 in steps of 0.02, quality threshold 0.50–0.20 in steps of 0.05), ordered so exact ties keep the lower tau and the stricter quality threshold.
- Refroze `fixtures/frozen_hybrid_config.json` from a live-Jev calibration sweep: tau_accept 0.22, quality threshold 0.40 (0 false accepts, 0/12 frontier misses, 70.0% coverage on the 40-task calibration partition; no grid point had an error). The live held-out evaluation with these thresholds is committed as `docs/experiments/live_heldout_results.md`: 0/43 false accepts, 0/31 frontier misses, 1/69 unnecessary frontier calls, 66.0% autonomous action coverage. Live Jev signals vary slightly between calls, so re-runs can differ by a task or two.

## 0.2.0 - 2026-09-27

- Partitioned the v2 evaluation corpus into disjoint context slices so the evaluation partition is genuinely held out. Previously all four splits sampled one 34-template pool, and 30 of the 32 distinct contexts in the evaluation partition also appeared in another split.
- Expanded the task template pool from 34 to 200 so each of the 200 records carries a unique context.
- Added `crates/reflex-calibration/tests/fixture_integrity.rs`, which asserts split disjointness, intra-split uniqueness, the 200-context total, and each split's class mix in CI.
- Fixed the calibration sweep: it returned its seed values (0.28 / 0.38) because no grid point met its strict zero-error rule, and it printed "0 False Accepts, 0 frontier misses" as literal text. It now selects lexicographically (false accepts, frontier misses, coverage) and reports the selected point's measured counts, warning when it is not error-free.
- Changed `reflex experiment --phase freeze`: with no thresholds it now runs the calibration sweep and freezes the selected point, recording the dataset, provider and measured counts in `validation_notes` (previously it silently wrote 0.28 / 0.38). With `--risk-threshold` and the new `--quality-threshold` it records a manual override; with only one of them it errors. Because `--provider` defaults to `jev`, a bare `--phase freeze` now calls the live API — pass `--provider mock` for an offline run.
- Refit the frozen configuration with the calibration sweep on the disjoint calibration partition, using the mock provider: tau_accept 0.25, quality threshold 0.45. The move from 0.28 / 0.38 rests on a single false accept (1 vs 0 across 40 calibration tasks), and the thresholds are not calibrated against live Jev signals.
- The generated evaluation report now names the dataset it evaluated and states that the partition is held out only for the CI-verified v2 partitions; the phase-4 banner no longer makes a held-out claim either way. `--phase all` now rejects `--dataset`, which made every phase read the same file.
- CI now regenerates the v2 fixtures and fails if the committed files differ from the generator's output.

## 0.1.1 - 2026-09-23

- Corrected calibration metric denominators: accuracy and Brier score are now computed over resolved outcomes only, and `CalibrationMetrics` reports `resolved_samples` and `unresolved_samples` alongside `total_samples`.
- Corrected cost accounting so unresolved (`Partial` / `Unknown`) outcomes are costed as escalations instead of being counted as avoided frontier calls.
- Corrected threshold promotion so a candidate threshold only promotes `Accept` and `Verify` routes and no longer overrides terminal actions.
- Added `Outcome::is_resolved()` to distinguish resolved outcomes from `Partial` and `Unknown`.
- Stopped leaking TypeSafe Jev secrets through errors: API failures no longer echo response bodies, endpoint URLs, or transport error details. `JevConfig` now redacts its `Debug` output and no longer exposes `api_key` publicly, covered by a regression test asserting no secret reaches an error string.
- Reframed the README and evaluation docs to separate implemented behavior from historical aggregate claims, and documented that the `blind_test` fixture holds only 32 distinct task contexts overlapping the other splits, so it is not an independent held-out evaluation.
- Set the workspace version to 0.1.1. The existing v0.1.0 tag is unchanged.

## 0.1.0 - 2026-09-17

- Added the frozen Candidate E / Guarded Hybrid default policy.
- Added TypeSafe Jev atomic semantic evidence integration.
- Added deterministic safety vetoes, bounded retry handling, and calibrated acceptance inside the safe envelope.
- Added telemetry, calibration statistics, synthetic evaluation fixtures, and runnable verifier-gate and shadow-mode examples.
- Corrected release metric definitions and documented the synthetic benchmark's confidence and sample-size limits.
