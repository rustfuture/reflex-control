# guard_bench — corpus benchmark for PreToolUse shell guards

`bench/guard_bench.py` runs every case in
`crates/reflex-hooks/tests/fixtures/guard_corpus.tsv` through a guard exposed as
a Claude Code `PreToolUse` hook and reports how well it separates dangerous
commands from everyday ones. Python 3 stdlib only, single file.

> Bias statement: the corpus was written for Reflex Control (its expected
> verdicts are Reflex's default-policy answers). Reflex matches 188/188 through
> this harness (integration check, see below); comparisons against other guards
> should be read with that in mind.

## Usage

Test the harness itself (no guard needed):

```bash
python3 bench/guard_bench.py --selftest
```

Benchmark a guard (all 188 cases, 3 repetitions each):

```bash
python3 bench/guard_bench.py \
  --guard "/abs/path/to/reflex hook claude-code pre-tool" \
  --name "reflex (feat/stronger-guard)" \
  --out /tmp/bench-out
```

Flags:

| flag | default | meaning |
| --- | --- | --- |
| `--guard` | required (unless `--selftest`) | guard command line, shlex-split; a path-like `argv[0]` is resolved against the launch cwd |
| `--corpus` | `crates/reflex-hooks/tests/fixtures/guard_corpus.tsv` | corpus TSV |
| `--out` | none (stdout only) | directory for `results.jsonl` and `summary.md` |
| `--name` | the `--guard` string | label used in outputs |
| `--reps` | 3 | repetitions per case; verdicts must be identical across reps |
| `--timeout` | 10.0 | per-run timeout in seconds |
| `--selftest` | off | verify the harness with fake guards, exit 1 on failure |

Exit status is 0 unless the determinism check fails (reps disagree), which
exits 1.

## Integration check

The harness itself is checked by running it against Reflex's own hook:

```bash
cargo build --release
python3 bench/guard_bench.py \
  --guard "target/release/reflex hook claude-code pre-tool" --reps 1
```

Result on 2026-10-23: 188/188 exact, 134/134 dangerous caught, 54/54 safe
passed, 0 errors, median 7.2 ms per hook call. The corpus test runs the same
cases in-process; a disagreement here is a bug in the harness (JSON shape,
path mapping, reply parsing), not in the guard.

## Related measurements

- `bench/latency.py` — end-to-end hook latency (one process per sample).
- `bench/traffic.py` — what would stop in an agent's real shell history.
- `docs/validation.md` — the metamorphic, bypass-matrix and adapter-parity
  tests, and the numbers all of these produce.

## How each case runs

Corpus format: `<verdict> TAB <rule id or -> TAB <command>`; `\n` inside a
command is a real newline; `#` lines are comments; `# ── …` lines name the
section. Expected verdicts: `allow`, `ask`, `block`.

Per run the harness:

1. spawns the guard (shlex-split argv) with cwd set to a fresh temp project
   dir containing only `git init` (no `.reflex.toml`);
2. sets `HOME` to a fresh temp dir;
3. writes `/work/proj` → temp project dir and `/home/me` → temp HOME inside
   the command before sending it;
4. sends the Claude Code PreToolUse JSON on stdin:
   `{"session_id":"bench","transcript_path":"/dev/null","cwd":<temp project dir>,"hook_event_name":"PreToolUse","tool_name":"Bash","tool_input":{"command":<cmd>}}`.

Reply classification:

| observation | class |
| --- | --- |
| exit code 2 | `block` |
| exit 0, stdout JSON `hookSpecificOutput.permissionDecision` = deny / ask / allow | `block` / `ask` / `allow` |
| exit 0, empty stdout | `allow` |
| exit 0, parsable JSON without a `permissionDecision` | `allow` (no objection) |
| any other exit code | `error` |
| timeout | `timeout` |
| unparsable stdout JSON | `malformed` |

## Output format

`results.jsonl` — one JSON object per run with keys:
`name, rep, line, section, command, expected, got, class, exit_code, ms`.
`line` is the corpus line number; `got` is the verdict (`allow`/`ask`/`block`)
or `null` for non-verdict classes; `ms` is wall-clock guard latency.

`summary.md` — totals, a **Scope** block, a **Scored total** block (excluding
scope), then per section:

- exact verdict match count (a case counts only if all reps match);
- dangerous caught — expected `ask|block` → got `ask|block`;
- safe passed — expected `allow` → got `allow`;
- errors by class (`error` / `timeout` / `malformed`);
- latency median / p90 in ms;
- determinism verdict (plus any mismatches, expected vs got).

## Frozen decision rule

Do not change this rule between comparison runs.

Per section, a guard is **better** than another iff it catches more dangerous
commands (expected `ask|block` → stopped `ask|block`) **without** passing fewer
safe ones (expected `allow` → allowed). Equal on both = **tie**. Otherwise the
result is a **tradeoff**: report the numbers, do not declare a winner.

Scope exception: the **Protected writes** and **Secret reads** sections are
reported separately ("Scope") and excluded from the decision rule: a guard that does
not claim those categories must not be counted for or against on them.
