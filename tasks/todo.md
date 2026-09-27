# v0.2 — Held-Out Evaluation Corpus

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` (recommended) or
> `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `fixtures/v2_eval_blind_test.json` a genuinely held-out evaluation set, and enforce that
property with a test that runs in CI.

**Architecture:** Contamination is structural, not accidental: `docs/experiments/generate_v2_datasets.py`
samples all four splits from one shared template pool with `random.choice`, separating splits only by date
range and `task_id` offset. The fix makes contamination *impossible by construction* — partition the template
pool into disjoint per-split slices first, then consume each slice without replacement. A Rust integration
test guards against regression.

**Tech Stack:** Python 3 (fixture generator), Rust 1.88 + `serde_json` (guard test), GitHub Actions CI.

---

## Measured Starting State (2026-09-24)

Verified by direct inspection, not by trusting the README:

| Fact | Value |
| :--- | :--- |
| Template pool total (6 classes) | **34** |
| Distinct contexts across all 200 records | **34** (identical — every split draws from the same pool) |
| `blind_test` records / distinct contexts | 100 / **32** |
| `blind_test` contexts also present in another split | **30 / 32** |
| Genuinely held-out contexts in `blind_test` | **2** |

Pairwise context intersections: dev∩validation 15, dev∩calibration 14, dev∩blind_test 21,
validation∩calibration 14, validation∩blind_test 21, calibration∩blind_test 22.

---

## Success Criteria

Each is machine-checkable. The work is done when all six hold.

1. Zero context overlap between any two splits.
2. Zero repeated context within any split.
3. 200 distinct contexts across 200 records (1:1 — no record shares a context with any other).
4. Per-split class distribution unchanged from `DISTRIB` (clean .45 / transient .12 / continue .12 /
   security .15 / ambiguity .08 / hard_fail .08, clean absorbing the rounding remainder).
5. Generator is deterministic: re-running it leaves `git status` clean.
6. Criteria 1–3 are asserted by `cargo test --workspace --all-targets`, which CI already runs.

## Execution Order

Tasks are numbered for reading, but they do not dispatch in that order. Task 3 is purely additive —
it grows the template literals without touching generation logic, so it commits green on its own.
Tasks 1, 2 and 4 are atomic by design: the guard test is red until the regenerated fixtures land, so
they must be one commit or `main` goes red.

| Dispatch | Tasks | Why grouped |
| :--- | :--- | :--- |
| A | 3 | Additive content only; independently green |
| B | 1 + 2 + 4 | Test, generator fix and regenerated data must land together |
| C | 5 Steps 1-2 | Freeze-step wiring; done in `c22b1ce` |
| C′ | 5A | Selection rule + honest report; must land before the sweep result is trusted |
| C″ | 5 Steps 3-6 | Runs the fixed sweep on B's corpus and freezes its result |
| D | 6 | Documents the outcome of B, 5A and C″ |
| E | 7 | Publish: needs D committed and the full gate green |

## Required Template Pool Sizes

Derived from `int(count * ratio)` per split with `clean` absorbing the remainder (this reproduces the
existing `generate_split` behavior exactly):

| Class | DEV (30) | VALIDATION (30) | CALIBRATION (40) | EVALUATION (100) | **Pool total** |
| :--- | ---: | ---: | ---: | ---: | ---: |
| clean | 16 | 16 | 20 | 45 | **97** |
| transient | 3 | 3 | 4 | 12 | **22** |
| continue | 3 | 3 | 4 | 12 | **22** |
| security | 4 | 4 | 6 | 15 | **29** |
| ambiguity | 2 | 2 | 3 | 8 | **15** |
| hard_fail | 2 | 2 | 3 | 8 | **15** |
| **Total** | **30** | **30** | **40** | **100** | **200** |

Current pools: clean 14, transient 5, continue 4, security 6, ambiguity 3, hard_fail 2.
**166 new templates must be authored** (Task 3).

---

## File Structure

| Path | Responsibility | Action |
| :--- | :--- | :--- |
| `crates/reflex-calibration/tests/fixture_integrity.rs` | Guard test: split disjointness + intra-split uniqueness | Create |
| `crates/reflex-calibration/Cargo.toml` | Add `[dev-dependencies]` so the integration test can use serde | Modify |
| `docs/experiments/generate_v2_datasets.py` | Disjoint partitioning + expanded pools | Modify |
| `fixtures/v2_eval_{dev,validation,calibration,blind_test}.json` | Regenerated corpus | Regenerate |
| `crates/reflex-cli/src/commands/experiment.rs` | Thread the swept quality threshold into the freeze step (5); lexicographic selection + measured report (5A); held-out wording in phase banner and generated report (6) | Modify |
| `docs/experiments/live_experiment_hybrid_results.md` | Mark the 32-context description as the pre-0.2.0 corpus the historical run used | Modify |
| `fixtures/frozen_hybrid_config.json` | Thresholds refit on the clean calibration partition | Regenerate |
| `fixtures/README.md` | Replace contamination caveats with verified properties | Modify |
| `README.md:"Evaluation Evidence"` | Same | Modify |
| `CHANGELOG.md` | 0.2.0 entry | Modify |

Integration tests in `tests/` are separate crates and can only see the parent crate's public API plus
**dev-dependencies** — `[dependencies]` are not visible. This is why `Cargo.toml` must change.

Note: the repo currently uses only inline `#[cfg(test)] mod tests`. A `tests/` directory is a deliberate
deviation: this test validates repository data, not crate logic, so it does not belong inside any module.

---

## Task 1: Guard test (RED)

**Files:**
- Create: `crates/reflex-calibration/tests/fixture_integrity.rs`
- Modify: `crates/reflex-calibration/Cargo.toml`

- [ ] **Step 1: Add dev-dependencies**

Append to `crates/reflex-calibration/Cargo.toml`:

```toml
[dev-dependencies]
serde = { workspace = true }
serde_json = { workspace = true }
```

- [ ] **Step 2: Write the failing test**

Create `crates/reflex-calibration/tests/fixture_integrity.rs`:

```rust
//! Repository-data invariants for the v2 evaluation corpus.
//!
//! These guard the property that makes the evaluation meaningful: the splits must not
//! share task contexts, or the "held-out" set is measuring memorization.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde::Deserialize;

const SPLITS: [&str; 4] = ["dev", "validation", "calibration", "blind_test"];

#[derive(Deserialize)]
struct Fixture {
    tasks: Vec<Task>,
}

#[derive(Deserialize)]
struct Task {
    context: String,
    ground_truth_action: String,
}

fn load_tasks(split: &str) -> Vec<Task> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(format!("v2_eval_{split}.json"));
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    let fixture: Fixture = serde_json::from_str(&raw)
        .unwrap_or_else(|e| panic!("failed to parse {}: {e}", path.display()));
    fixture.tasks
}

fn load(split: &str) -> Vec<String> {
    load_tasks(split)
        .into_iter()
        .map(|task| task.context)
        .collect()
}

#[test]
fn splits_share_no_context() {
    let sets: BTreeMap<&str, BTreeSet<String>> = SPLITS
        .iter()
        .map(|split| (*split, load(split).into_iter().collect()))
        .collect();

    let mut violations = Vec::new();
    for (index, left) in SPLITS.iter().enumerate() {
        for right in SPLITS.iter().skip(index + 1) {
            let shared = sets[left].intersection(&sets[right]).count();
            if shared > 0 {
                violations.push(format!("{left} and {right} share {shared} contexts"));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "evaluation splits are contaminated: {violations:?}"
    );
}

#[test]
fn no_split_repeats_a_context() {
    let mut violations = Vec::new();
    for split in SPLITS {
        let contexts = load(split);
        let distinct: BTreeSet<&String> = contexts.iter().collect();
        if distinct.len() != contexts.len() {
            violations.push(format!(
                "{split}: {} records but {} distinct contexts",
                contexts.len(),
                distinct.len()
            ));
        }
    }

    assert!(
        violations.is_empty(),
        "splits contain duplicate contexts: {violations:?}"
    );
}

#[test]
fn corpus_holds_two_hundred_distinct_contexts() {
    let all: BTreeSet<String> = SPLITS.iter().flat_map(|split| load(split)).collect();
    assert_eq!(all.len(), 200, "expected 200 distinct contexts, found {}", all.len());
}

/// Disjoint partitioning must not silently reshape the class balance: a split that
/// loses its escalation cases would score well for the wrong reason.
#[test]
fn every_split_keeps_the_intended_class_mix() {
    // `ground_truth_action` is the observable proxy for the generator's task class.
    // escalate = security + ambiguity + hard_fail.
    let expected: [(&str, [(&str, usize); 4]); 4] = [
        ("dev", [("accept", 16), ("retry", 3), ("continue", 3), ("escalate", 8)]),
        ("validation", [("accept", 16), ("retry", 3), ("continue", 3), ("escalate", 8)]),
        ("calibration", [("accept", 20), ("retry", 4), ("continue", 4), ("escalate", 12)]),
        ("blind_test", [("accept", 45), ("retry", 12), ("continue", 12), ("escalate", 31)]),
    ];

    for (split, wanted) in expected {
        let mut actual: BTreeMap<String, usize> = BTreeMap::new();
        for task in load_tasks(split) {
            *actual.entry(task.ground_truth_action).or_default() += 1;
        }
        for (action, count) in wanted {
            assert_eq!(
                actual.get(action).copied().unwrap_or(0),
                count,
                "{split}: expected {count} '{action}' tasks, found {:?}",
                actual.get(action)
            );
        }
    }
}
```

- [ ] **Step 3: Run the test and confirm it fails**

Run: `cargo test -p reflex-calibration --test fixture_integrity`

Expected: `test result: FAILED. 1 passed; 3 failed`.

- `splits_share_no_context` FAILS with six violations: dev/validation 15, dev/calibration 14,
  dev/blind_test 21, validation/calibration 14, validation/blind_test 21, calibration/blind_test 22.
- `no_split_repeats_a_context` FAILS: dev 30 records / 23 distinct, validation 30 / 22,
  calibration 40 / 22, blind_test 100 / 32.
- `corpus_holds_two_hundred_distinct_contexts` FAILS: found 34.
- `every_split_keeps_the_intended_class_mix` **PASSES** — verified against the current fixtures.
  Class balance was never broken; it is guarded here so the Task 2 restructure cannot silently break it.

**Do not commit yet** — the test stays red until Task 4. The test, the generator fix, and the
regenerated data must land as one commit so `main` is never red.

---

## Task 2: Disjoint partitioning in the generator

**Files:**
- Modify: `docs/experiments/generate_v2_datasets.py`

- [ ] **Step 1: Add the pool registry**

Insert after `HARD_FAIL_TEMPLATES` (currently ends at line 73):

```python
TEMPLATE_POOLS = {
    "clean": CLEAN_TEMPLATES,
    "transient": TRANSIENT_TEMPLATES,
    "continue": CONTINUE_TEMPLATES,
    "security": SECURITY_DEFECT_TEMPLATES,
    "ambiguity": AMBIGUITY_DEFECT_TEMPLATES,
    "hard_fail": HARD_FAIL_TEMPLATES,
}
```

- [ ] **Step 2: Replace the split table with a single source of truth**

Replace the `splits = [...]` list (currently lines 257-262) with:

```python
SPLIT_PLAN = {
    "DEV":         {"count": 30,  "start": 1001, "path": "fixtures/v2_eval_dev.json",         "range": "2026-09-18 to 2026-09-21"},
    "VALIDATION":  {"count": 30,  "start": 1031, "path": "fixtures/v2_eval_validation.json",  "range": "2026-09-22 to 2026-09-25"},
    "CALIBRATION": {"count": 40,  "start": 1061, "path": "fixtures/v2_eval_calibration.json", "range": "2026-09-26 to 2026-09-29"},
    "EVALUATION":  {"count": 100, "start": 1101, "path": "fixtures/v2_eval_blind_test.json",  "range": "2026-09-30 to 2026-10-05"},
}

SPLIT_ORDER = ["DEV", "VALIDATION", "CALIBRATION", "EVALUATION"]
```

- [ ] **Step 3: Add class-count and partition helpers**

Insert directly after the `DISTRIB` definition:

```python
def class_counts(count):
    """Exact per-class record counts for a split.

    Mirrors the original behavior: floor each ratio, then let `clean` absorb the remainder.
    """
    counts = {t_type: int(count * ratio) for t_type, ratio in DISTRIB.items()}
    counts["clean"] += count - sum(counts.values())
    return counts


def partition_templates():
    """Assign every template to exactly one split.

    Disjointness is structural: each template lands in one slice, and each slice is consumed
    without replacement, so no context can appear in two splits.
    """
    need = {name: class_counts(SPLIT_PLAN[name]["count"]) for name in SPLIT_ORDER}
    assigned = {name: {} for name in SPLIT_ORDER}

    for t_type, pool in TEMPLATE_POOLS.items():
        required = sum(need[name][t_type] for name in SPLIT_ORDER)
        if len(pool) != required:
            raise SystemExit(
                f"{t_type}: need exactly {required} templates, pool has {len(pool)}"
            )
        shuffled = list(pool)
        random.shuffle(shuffled)
        cursor = 0
        for name in SPLIT_ORDER:
            take = need[name][t_type]
            assigned[name][t_type] = shuffled[cursor:cursor + take]
            cursor += take

    return assigned
```

- [ ] **Step 4: Make `generate_task` accept an explicit template**

Change the signature (line 75) to `def generate_task(task_num, split_name, task_type, template):`
and replace every `random.choice(<POOL>)` unpack with the passed-in template. The six call sites become:

```python
    if task_type == "clean":
        ctx, files, diff, unexp, sec = template
    elif task_type == "transient":
        ctx, act, retries = template
    elif task_type == "continue":
        ctx, files, diff = template
    elif task_type == "security":
        ctx, files, diff, unexp, sec = template
    elif task_type == "ambiguity":
        ctx, files, diff, unexp, sec = template
    else:  # hard_fail
        ctx, files, diff, unexp, sec = template
```

Leave every other line in each branch (the `cat`/`risk`/`det`/`gt_*`/`unsafe` assignments) untouched.
The `random.choice` calls for `cat` and `risk` stay — those are attributes, not contexts, and may repeat.

- [ ] **Step 5: Rewrite `generate_split` to consume slices without replacement**

Replace `generate_split` (currently lines 222-237) with:

```python
def generate_split(split_name, assigned):
    plan = SPLIT_PLAN[split_name]
    counts = class_counts(plan["count"])

    types_pool = []
    for t_type, n in counts.items():
        types_pool.extend([t_type] * n)
    random.shuffle(types_pool)

    cursors = {t_type: 0 for t_type in counts}
    tasks = []
    for offset, t_type in enumerate(types_pool):
        template = assigned[split_name][t_type][cursors[t_type]]
        cursors[t_type] += 1
        tasks.append(generate_task(plan["start"] + offset, split_name, t_type, template))

    return tasks
```

- [ ] **Step 6: Rewrite the write loop**

Replace the final `for name, count, start_idx, filepath, date_range in splits:` loop
(currently lines 264-277) with:

```python
assigned = partition_templates()

for name in SPLIT_ORDER:
    plan = SPLIT_PLAN[name]
    tasks = generate_split(name, assigned)
    dataset = {
        "metadata": {
            "source": "reflex_v2_fresh_evaluation_benchmark",
            "split": name,
            "total_tasks": len(tasks),
            "temporal_range": plan["range"],
            "context_partition": "disjoint",
        },
        "tasks": tasks,
    }
    with open(plan["path"], "w") as f:
        json.dump(dataset, f, indent=2)
    print(f"Generated {plan['path']}: {len(tasks)} tasks.")
```

- [ ] **Step 7: Run the generator and confirm it refuses to run**

Run: `python3 docs/experiments/generate_v2_datasets.py`

Expected: exits non-zero with `clean: need exactly 97 templates, pool has 14`.
This is correct — the guard fires before writing bad data. Task 3 satisfies it.

---

## Task 3: Expand the template pools to 200

**Files:**
- Modify: `docs/experiments/generate_v2_datasets.py`

Author new templates until each pool hits its exact required size. Tuple shapes are fixed by the
unpack in Task 2 Step 4 — a wrong arity is a `ValueError` at generation time.

| Pool | Shape | Have | Need | Add |
| :--- | :--- | ---: | ---: | ---: |
| `CLEAN_TEMPLATES` | `(context, files_changed, git_diff_size, unexpected_files, security_files)` | 14 | 97 | **83** |
| `TRANSIENT_TEMPLATES` | `(context, action, retry_count)` | 5 | 22 | **17** |
| `CONTINUE_TEMPLATES` | `(context, files_changed, git_diff_size)` | 4 | 22 | **18** |
| `SECURITY_DEFECT_TEMPLATES` | `(context, files_changed, git_diff_size, unexpected_files, security_files)` | 6 | 29 | **23** |
| `AMBIGUITY_DEFECT_TEMPLATES` | `(context, files_changed, git_diff_size, unexpected_files, security_files)` | 3 | 15 | **12** |
| `HARD_FAIL_TEMPLATES` | `(context, files_changed, git_diff_size, unexpected_files, security_files)` | 2 | 15 | **13** |

**Authoring rules** — a template whose label does not match its text poisons the ground truth, so:

- `clean`: routine, low-blast-radius work that genuinely should be auto-accepted. `unexpected_files`
  and `security_files` are always `False`. Keep `files_changed` 1–4 and `git_diff_size` 10–90.
- `transient`: an infrastructure/network failure with no code defect. `action` is always `"retry"`;
  `retry_count` is 0 or 1. The text must name the transient cause (timeout, 429, 503, deadlock, reset).
- `continue`: genuine mid-progress work. The text must state progress explicitly ("3 of 6 done,
  continuing"). `files_changed` 2–5, `git_diff_size` 80–200.
- `security`: a real vulnerability that **passes tests** — that is the whole point of the class.
  `unexpected_files` and `security_files` are always `True`. Keep `files_changed` 1–3.
- `ambiguity`: scope violation or contradictory instruction — the worker did something not asked for,
  or the request conflicts with itself. Large blast radius: `files_changed` 8–30, `git_diff_size` 300–1000.
- `hard_fail`: a deterministic, unambiguous failure (failing assertions, compile error).
  `unexpected_files` and `security_files` are `False`.

Every context string must be unique across **all six pools** — the guard test checks the global set.
Spread text across the six `CATEGORIES` so no category is starved.

- [ ] **Step 1: Author the 83 new `CLEAN_TEMPLATES`**

Follow the shape of the existing entries, for example:

```python
    ("Add structured logging fields to the order-cancellation handler. Tests pass.", 1, 26, False, False),
    ("Replace hand-rolled retry loop in the email dispatcher with the shared backoff helper.", 2, 48, False, False),
    ("Add an index hint comment above the slow reporting query. No schema change.", 1, 9, False, False),
```

- [ ] **Step 2: Author the 17 new `TRANSIENT_TEMPLATES`**

```python
    ("Publish build artifact to the internal registry. Upload aborted with HTTP 502 from the proxy.", "retry", 1),
    ("Resolve package index for the CI container image. DNS lookup failed for the mirror host.", "retry", 0),
```

- [ ] **Step 3: Author the 18 new `CONTINUE_TEMPLATES`**

```python
    ("Backfill denormalized order totals. Partitions 1-3 of 9 verified; continuing to partition 4.", 3, 120),
    ("Port integration tests to the new fixture harness. 12 of 40 files ported; work continuing.", 5, 165),
```

- [ ] **Step 4: Author the 23 new `SECURITY_DEFECT_TEMPLATES`**

```python
    ("Cache decoded session tokens in a process-global map with no expiry or tenant scoping.", 2, 40, True, True),
    ("Replace constant-time signature comparison with a plain string equality check.", 1, 12, True, True),
```

- [ ] **Step 5: Author the 12 new `AMBIGUITY_DEFECT_TEMPLATES`**

```python
    ("Task asked to rename one config key, but the worker rewrote the deployment manifests, the secrets loader, and the CI pipeline.", 17, 640, True, True),
    ("Prompt requested a read-only cost report, but the worker modified billing rate tables in place.", 9, 380, True, True),
```

- [ ] **Step 6: Author the 13 new `HARD_FAIL_TEMPLATES`**

```python
    ("Run contract tests for the shipping adapter. 7 assertions failed on unexpected null carrier id.", 3, 95, False, False),
    ("Build the CLI release binary. Linker failed with undefined symbol in the telemetry crate.", 2, 55, False, False),
```

- [ ] **Step 7: Verify every pool is exactly the required size**

Run:

```bash
python3 - <<'PY'
import re
src = open('docs/experiments/generate_v2_datasets.py').read()
want = {'CLEAN_TEMPLATES':97,'TRANSIENT_TEMPLATES':22,'CONTINUE_TEMPLATES':22,
        'SECURITY_DEFECT_TEMPLATES':29,'AMBIGUITY_DEFECT_TEMPLATES':15,'HARD_FAIL_TEMPLATES':15}
ok = True
for m in re.finditer(r'^([A-Z_]+_TEMPLATES)\s*=\s*\[(.*?)^\]', src, re.S|re.M):
    name, n = m.group(1), m.group(2).count('("')
    flag = 'OK ' if n == want.get(name) else 'BAD'
    ok &= n == want.get(name)
    print(f'{flag} {name}: {n} (want {want.get(name)})')
raise SystemExit(0 if ok else 1)
PY
```

Expected: six `OK` lines, exit 0.

---

## Task 4: Regenerate and turn the guard green

**Files:**
- Regenerate: `fixtures/v2_eval_{dev,validation,calibration,blind_test}.json`

- [ ] **Step 1: Regenerate the corpus**

Run: `python3 docs/experiments/generate_v2_datasets.py`

Expected: four `Generated fixtures/...: N tasks.` lines, exit 0.

- [ ] **Step 2: Run the guard test**

Run: `cargo test -p reflex-calibration --test fixture_integrity`

Expected: `test result: ok. 4 passed; 0 failed`.

- [ ] **Step 3: Confirm the overlap is actually zero, independently of the test**

Run:

```bash
python3 -c "
import json, itertools
s={n:set(t['context'] for t in json.load(open(f'fixtures/v2_eval_{n}.json'))['tasks'])
   for n in ['dev','validation','calibration','blind_test']}
for a,b in itertools.combinations(s,2): print(f'{a} ∩ {b} = {len(s[a]&s[b])}')
print('union =', len(set().union(*s.values())))
"
```

Expected: every intersection `0`, `union = 200`.

- [ ] **Step 4: Confirm determinism**

Run: `python3 docs/experiments/generate_v2_datasets.py && git status --short fixtures/`

Expected: the four fixture files appear as modified relative to `HEAD` (they changed in Step 1),
but running the generator twice in a row produces no further change. Verify with:

```bash
md5 fixtures/v2_eval_blind_test.json
python3 docs/experiments/generate_v2_datasets.py >/dev/null
md5 fixtures/v2_eval_blind_test.json
```

Expected: identical hashes.

- [ ] **Step 5: Run the full suite and lints**

Run:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
```

Expected: all three clean. Test count rises from 53 to 57.

- [ ] **Step 6: Commit**

```bash
git add crates/reflex-calibration/Cargo.toml \
        crates/reflex-calibration/tests/fixture_integrity.rs \
        docs/experiments/generate_v2_datasets.py \
        fixtures/v2_eval_dev.json fixtures/v2_eval_validation.json \
        fixtures/v2_eval_calibration.json fixtures/v2_eval_blind_test.json
git commit -m "feat(fixtures): make the evaluation splits genuinely held out

The four v2 splits all sampled from one shared template pool, so 30 of the
32 distinct contexts in blind_test also appeared in dev, validation, or
calibration. The split was measuring memorization, not generalization.

Partition the pool into disjoint per-split slices and consume each slice
without replacement, which makes overlap structurally impossible, and
expand the pool from 34 to 200 templates so each record has a unique
context. A guard test asserts disjointness, intra-split uniqueness, and
the 200-context total.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

## Task 5A: Make the calibration sweep select a real operating point

**Decision (2026-09-27): option 2 + option 1** from the Review's "Decision needed" section. The selection
rule becomes lexicographic — fewest false accepts, then fewest frontier misses, then highest autonomous
coverage — and the report prints the measured values of the selected point, with an explicit warning
when it is not error-free. Applies lesson **L5**: no seeded search variables, no outcome text baked into
a format string.

**Why counts, not rates:** every grid point is evaluated on the same 40-task partition, so
`frontier_missed_count` orders candidates exactly as `frontier_miss_rate` does, without `Option<f64>`
(rate is `None` when there are no frontier-required tasks) or float comparison in the ordering.

**Why `min_by`:** `Iterator::min_by` returns the *first* of several equal minima, so exact ties keep the
earlier grid point deterministically, with no extra tie-break code. Coverage is `f64`, so it is compared
with `total_cmp` (a total order; no `partial_cmp().unwrap()`).

**Done:** `c0b5508` (Steps 1-7), plus two review follow-ups:
- `dcc62ec` — `freeze_configuration` takes its `validation_notes`; `--phase freeze` with no thresholds runs the
  sweep and records the selected point's measured counts (`sweep_provenance`), with both thresholds records a
  manual override, with one errors. Removes the last seeded `unwrap_or(0.28)/unwrap_or(0.38)` in the freeze path.
- `a0dd1c0` — the provenance note names the dataset path and provider actually used, not "the calibration
  partition" (wrong under `--dataset`).

Gate at `a0dd1c0`: fmt + clippy clean, **65/65** tests. Spec review ✅, code-quality review approved.

**Files:**
- Modify: `crates/reflex-cli/src/commands/experiment.rs` (new items above `run_calibration_phase`;
  selection block at lines ~572-652; `"all"` arm at ~228-229; `validation_notes` in
  `freeze_configuration`; tests in `mod tests` at ~1519)

- [x] **Step 1: Write the failing tests**

Append inside `mod tests` in `experiment.rs`, after `candidate_rates_with_empty_denominators_are_unavailable`:

```rust
    fn point(
        tau: f64,
        false_accepts: usize,
        frontier_misses: usize,
        coverage_pct: f64,
    ) -> SweepPoint {
        SweepPoint {
            tau,
            quality: 0.40,
            false_accepts,
            frontier_misses,
            frontier_required: 12,
            coverage_pct,
        }
    }

    #[test]
    fn selection_prefers_fewer_false_accepts_over_coverage() {
        let risky = point(0.30, 1, 0, 90.0);
        let safe = point(0.25, 0, 1, 50.0);
        assert_eq!(select_operating_point(&[risky, safe]), Some(safe));
    }

    #[test]
    fn selection_breaks_false_accept_ties_on_frontier_misses() {
        let more_misses = point(0.30, 0, 2, 90.0);
        let fewer_misses = point(0.25, 0, 1, 60.0);
        assert_eq!(
            select_operating_point(&[more_misses, fewer_misses]),
            Some(fewer_misses)
        );
    }

    #[test]
    fn selection_breaks_error_ties_on_coverage() {
        let lower = point(0.25, 0, 1, 60.0);
        let higher = point(0.28, 0, 1, 70.0);
        assert_eq!(select_operating_point(&[lower, higher]), Some(higher));
    }

    #[test]
    fn selection_keeps_the_earlier_grid_point_on_an_exact_tie() {
        let first = point(0.25, 0, 1, 70.0);
        let second = point(0.28, 0, 1, 70.0);
        assert_eq!(select_operating_point(&[first, second]), Some(first));
    }

    #[test]
    fn selection_on_an_empty_grid_is_none() {
        assert_eq!(select_operating_point(&[]), None);
    }

    #[test]
    fn report_prints_measured_errors_and_warns_when_not_error_free() {
        let report = describe_operating_point(&point(0.25, 2, 1, 70.0));
        assert!(report.contains("2 false accepts"), "{report}");
        assert!(report.contains("1/12 frontier misses"), "{report}");
        assert!(report.contains("WARNING"), "{report}");
    }

    #[test]
    fn report_does_not_warn_for_an_error_free_point() {
        let report = describe_operating_point(&point(0.25, 0, 0, 70.0));
        assert!(report.contains("0 false accepts"), "{report}");
        assert!(report.contains("0/12 frontier misses"), "{report}");
        assert!(!report.contains("WARNING"), "{report}");
    }
```

- [x] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p reflex-cli --bin reflex -- selection report_ > /tmp/rc-5a-red.txt 2>&1; grep -E 'error\[E0|cannot find' /tmp/rc-5a-red.txt`

Expected: compile failure — `cannot find struct, variant or union type SweepPoint` and
`cannot find function select_operating_point` / `describe_operating_point`. (Per **L1**: full output
goes to the file; filter only when reading.)

- [x] **Step 3: Add the type, the selection function and the report function**

Insert directly above `async fn run_calibration_phase(`:

```rust
/// One evaluated point of the calibration grid.
#[derive(Debug, Clone, Copy, PartialEq)]
struct SweepPoint {
    tau: f64,
    quality: f64,
    false_accepts: usize,
    frontier_misses: usize,
    frontier_required: usize,
    coverage_pct: f64,
}

/// Picks the operating point lexicographically: fewest false accepts, then fewest
/// frontier misses, then highest autonomous coverage. Exact ties keep the earlier grid
/// point. `None` only for an empty grid — there is no default to fall back to here.
fn select_operating_point(points: &[SweepPoint]) -> Option<SweepPoint> {
    points.iter().copied().min_by(|a, b| {
        (a.false_accepts, a.frontier_misses)
            .cmp(&(b.false_accepts, b.frontier_misses))
            .then_with(|| b.coverage_pct.total_cmp(&a.coverage_pct))
    })
}

/// Reports the selected point from its measured values only.
fn describe_operating_point(p: &SweepPoint) -> String {
    let mut report = format!(
        ">>> Selected Operating Point: tau_accept = {:.2}, quality_thresh = {:.2} ({} false accepts, {}/{} frontier misses, {:.1}% autonomous action coverage)",
        p.tau, p.quality, p.false_accepts, p.frontier_misses, p.frontier_required, p.coverage_pct
    );
    if p.false_accepts > 0 || p.frontier_misses > 0 {
        report.push_str(
            "\n>>> WARNING: no grid point reached zero false accepts and zero frontier misses; this is the least-bad point by (false accepts, frontier misses, coverage).",
        );
    }
    report
}
```

- [x] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p reflex-cli --bin reflex -- selection report_ > /tmp/rc-5a-green.txt 2>&1; grep 'test result' /tmp/rc-5a-green.txt`

Expected: `test result: ok. 7 passed; 0 failed`.

- [x] **Step 5: Route `run_calibration_phase` through the new functions**

Change its signature:

```rust
async fn run_calibration_phase(
    args: &ExperimentArgs,
) -> Result<SweepPoint, Box<dyn std::error::Error>> {
```

Delete the three seeded variables:

```rust
    let mut best_tau = 0.28;
    let mut best_quality = 0.38;
    let mut best_coverage = 0.0;
```

and put this in their place:

```rust
    let mut points = Vec::with_capacity(candidate_settings.len());
```

Replace the strict-match block at the end of the loop body:

```rust
        // Strict requirement: zero observed false accepts and frontier misses.
        if m.false_accept_count == 0
            && m.frontier_miss_rate == Some(0.0)
            && m.autonomous_coverage_pct >= best_coverage
        {
            best_tau = tau;
            best_quality = q_thresh;
            best_coverage = m.autonomous_coverage_pct;
        }
```

with:

```rust
        points.push(SweepPoint {
            tau,
            quality: q_thresh,
            false_accepts: m.false_accept_count,
            frontier_misses: m.frontier_missed_count,
            frontier_required: m.frontier_required_tasks,
            coverage_pct: m.autonomous_coverage_pct,
        });
```

Replace everything after the table's closing `println!("└────…┘");` up to the end of the function:

```rust
    println!(
        "\n>>> Best Observed Operating Point: tau_accept = {best_tau:.2}, quality_thresh = {best_quality:.2} (0 False Accepts, 0 frontier misses, {best_coverage:.1}% autonomous action coverage)"
    );

    Ok((best_tau, best_quality))
}
```

with:

```rust
    let selected = select_operating_point(&points).ok_or("calibration grid is empty")?;
    println!("\n{}", describe_operating_point(&selected));

    Ok(selected)
}
```

Update the `"all"` arm in `run_experiment`:

```rust
            let selected = run_calibration_phase(&args).await?;
            freeze_configuration(selected.tau, selected.quality)?;
```

In `freeze_configuration`, replace the `validation_notes` string so the file records how it was chosen:

```rust
        validation_notes: "Parameters selected by the calibration sweep: fewest false accepts, then fewest frontier misses, then highest autonomous action coverage. This file does not retain per-task predictions, so observed calibration counts are not independently reproducible from the configuration alone.".to_string(),
```

- [x] **Step 6: Run the full gate**

Run:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets > /tmp/rc-5a-full.txt 2>&1; grep 'test result' /tmp/rc-5a-full.txt
```

Expected: fmt and clippy silent; test totals sum to **64** (57 before + 7 new), 0 failed.

- [x] **Step 7: Commit**

```bash
git add crates/reflex-cli/src/commands/experiment.rs
git commit -m "fix(experiment): select the calibration point instead of returning seeds

run_calibration_phase seeded best_tau/best_quality with 0.28/0.38 and only
overwrote them on a zero-false-accept, zero-miss match that never occurred,
so it always returned its seeds, and the report hardcoded '0 False Accepts,
0 frontier misses' as literal text.

Select lexicographically (false accepts, frontier misses, coverage) with
min_by, and report the selected point's measured counts, warning when it is
not error-free.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

## Task 5: Recalibrate the frozen configuration

`fixtures/frozen_hybrid_config.json` (`optimal_tau_accept` 0.28, `clean_quality_accept_threshold` 0.38,
`max_risk_small_reasoner` 0.58, `mandatory_frontier_risk` 0.70) was fit on the contaminated corpus.
After Task 4 those numbers refer to data that no longer exists. Leaving them is the same class of
dishonesty v0.1.1 set out to fix.

**Files:**
- Modify: `fixtures/frozen_hybrid_config.json`

**Blocking defect found while writing this plan:** `run_freeze_step`
(`crates/reflex-cli/src/commands/experiment.rs:673-677`) hardcodes the quality threshold:

```rust
async fn run_freeze_step(args: &ExperimentArgs) -> Result<(), Box<dyn std::error::Error>> {
    let tau = args.risk_threshold.unwrap_or(0.28);
    freeze_configuration(tau, 0.38)?;   // <- 0.38 is literal; the sweep's best_quality is discarded
    Ok(())
}
```

The calibration phase computes `(best_tau, best_quality)` and returns both, but the freeze step ignores
`best_quality` and always writes `0.38`. Recalibration is therefore impossible without fixing the wiring
first — Step 1 does that.

**Files:**
- Modify: `crates/reflex-cli/src/commands/experiment.rs`
- Modify: `fixtures/frozen_hybrid_config.json`

- [x] **Step 1: Let the freeze step accept a swept quality threshold** — `c22b1ce`

Add to `ExperimentArgs` (after the `risk_threshold` field at line 42-44):

```rust
    /// Override clean-quality acceptance threshold theta_clean
    #[arg(long)]
    pub quality_threshold: Option<f64>,
```

Then replace `run_freeze_step`:

```rust
async fn run_freeze_step(args: &ExperimentArgs) -> Result<(), Box<dyn std::error::Error>> {
    let tau = args.risk_threshold.unwrap_or(0.28);
    let quality = args.quality_threshold.unwrap_or(0.38);
    freeze_configuration(tau, quality)?;
    Ok(())
}
```

- [x] **Step 2: Verify it builds and lints** — `c22b1ce`

Run:

```bash
cargo clippy --workspace --all-targets -- -D warnings
cargo run -q --bin reflex -- experiment --help
```

Expected: clippy clean; help output now lists `--quality-threshold`.

- [ ] **Step 3: Sweep and freeze in one command**

**Requires Task 5A (done).** Since `dcc62ec`, `--phase freeze` without thresholds runs the calibration sweep
itself and freezes the selected point together with its measured provenance — no copying numbers by hand.

Run: `cargo run -q --bin reflex -- experiment --phase freeze --provider mock > /tmp/rc-freeze.txt 2>&1; grep -E 'Selected|WARNING|FROZEN' /tmp/rc-freeze.txt`

`--provider mock` is required: the default provider is `jev` (live API, needs a key). Expected — reproduced
three times (dry run, 5A smoke test, `a0dd1c0` smoke test):

```
>>> Selected Operating Point: tau_accept = 0.25, quality_thresh = 0.45 (0 false accepts, 1/12 frontier misses, 70.0% autonomous action coverage)
>>> WARNING: no grid point reached zero false accepts and zero frontier misses; ...
>>> Configuration FROZEN to fixtures/frozen_hybrid_config.json
```

If the selected point differs from 0.25 / 0.45, stop: save the full table into the Review section and
report instead of committing.

- [ ] **Step 4: Inspect the frozen file**

Run: `git diff fixtures/frozen_hybrid_config.json`

Expected: `optimal_tau_accept` 0.25, `clean_quality_accept_threshold` 0.45, `max_risk_small_reasoner` and
`mandatory_frontier_risk` unchanged (0.58 / 0.7), a new `timestamp`, and `validation_notes` reading
`… Measured on fixtures/v2_eval_calibration.json with provider mock: 0 false accepts, 1/12 frontier misses, 70.0% …`.

- [ ] **Step 5: Record old and new thresholds side by side**

Write both sets, plus the selected point's false accepts, frontier misses and coverage, into the
Review section of this file. Old values for reference:
`optimal_tau_accept` 0.28, `clean_quality_accept_threshold` 0.38, `max_risk_small_reasoner` 0.58,
`mandatory_frontier_risk` 0.70. If a threshold moves materially, that movement is itself the finding —
it quantifies how much the contamination was distorting the fit.

- [ ] **Step 6: Commit**

```bash
git add fixtures/frozen_hybrid_config.json tasks/todo.md
git commit -m "fix(fixtures): refit the frozen configuration on the clean calibration split

The previous 0.28/0.38 were the sweep's seed values, never a fit, and on the
held-out calibration split they produce 1 false accept. The fixed sweep
selects 0.25/0.45: 0 false accepts, 1/12 frontier misses, 70.0% coverage
(mock provider), recorded in the file's validation_notes.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

## Task 6: Update the documentation claims

Every statement that the corpus is contaminated becomes false once Task 4 lands, and every mention of
0.28 / 0.38 as the fitted values becomes false once Task 5 lands. Historical claims stay, but move to the
past tense and name the corpus they were measured on — they were real measurements of the *old* data.

`<T>` / `<Q>` below are **0.25** / **0.45** (Task 5); use the values actually in `fixtures/frozen_hybrid_config.json`.

**Files:**
- Modify: `fixtures/README.md`, `README.md`, `CHANGELOG.md`, `Cargo.toml`, `Cargo.lock`
- Modify: `crates/reflex-cli/src/commands/experiment.rs` (lines ~684, ~689, ~1378)
- Modify: `docs/experiments/live_experiment_hybrid_results.md` (line 8)

- [ ] **Step 1: `fixtures/README.md`**

Line 9 — replace the sentence beginning "Task contexts are sampled from a small set of templates" with:

```markdown
Each partition draws from its own disjoint slice of the template pool, so no task context appears in more than one partition.
```

Line 16 — replace the `v2_eval_blind_test.json` row with:

```markdown
| `v2_eval_blind_test.json` | Held-out evaluation partition (legacy filename; no context shared with other partitions) | 100 tasks | 2026-09-30 to 2026-10-05 |
```

Line 18 — replace the whole paragraph beginning "The 100-task evaluation fixture contains 32 distinct contexts" with
(the second half keeps the still-true manifest limitation):

```markdown
The four partitions hold disjoint task contexts: 200 records over 200 distinct contexts, with no context repeated inside a partition and none shared between partitions. This is enforced by `crates/reflex-calibration/tests/fixture_integrity.rs`, which runs in CI, so the property cannot regress silently. The corpus remains curated synthetic data: it establishes that the evaluation partition is held out, not that the policy performs as measured in production. A historical report describes live Jev inference on the pre-0.2.0 corpus, but per-task predictions and a run manifest are not checked in, so that run is not independently reproducible from this repository.
```

"Active Frozen Configuration" — change "The v0.1 default configuration frozen after calibration" to
"The 0.2.0 configuration refit on the held-out calibration partition", and the two values to `<T>` / `<Q>`.

Line 36 (`fresh_eval_*` row) stays unchanged — those legacy fixtures are still contaminated.

- [ ] **Step 2: `README.md`**

Line 168 — replace the paragraph beginning "The fixture filename says `blind_test`" with:

```markdown
The historical run used the pre-0.2.0 version of this fixture, which held only 32 distinct task contexts, 30 of which also appeared in the development, validation, or calibration splits, so those aggregates were not measured on held-out data. Since 0.2.0 the evaluation partition shares no context with any other split (enforced in CI by `crates/reflex-calibration/tests/fixture_integrity.rs`), but the historical figures have not been re-measured on it. The report records live Jev inference, but its outputs and exact run identity are unavailable for verification. This is curated synthetic evidence, not production validation.
```

Line 166 (the 69/100 paragraph) stays unchanged — it is already framed as historical.

Line 189 — the `v2_eval_blind_test.json` row's purpose cell becomes
`Curated synthetic held-out evaluation partition (legacy filename)`.

Line 193 — the `frozen_hybrid_config.json` row becomes
`Frozen 0.2.0 policy parameters ($\tau_{\text{accept}} = <T>, \theta_{\text{clean}} = <Q>, \text{risk}_{\text{frontier}} = 0.70$)`.

- [ ] **Step 3: Runtime text in `experiment.rs`**

Line ~684:

```rust
// 4. EVALUATION PHASE (LEGACY `blind` NAME)
```

Line ~689 — neutral on purpose: `--version v1` routes this phase to the still-contaminated
`fresh_eval_*` fixtures, so the banner must not claim held-out:

```rust
    println!(" PHASE 4: EVALUATION (100 CURATED SYNTHETIC TASKS)");
```

Line ~1378 — the report hardcodes the v2 filename two lines above, so a v2 claim is accurate here.
Replace the sentence
`The fixture reuses task contexts across its partitions, so this is not a blind or independent held-out evaluation.`
with
`The evaluation partition shares no task context with the development, validation, or calibration partitions, but it is curated synthetic data.`

- [ ] **Step 4: `docs/experiments/live_experiment_hybrid_results.md` line 8**

Replace the bullet beginning "The fixture has 32 distinct task contexts" with:

```markdown
- At the time of this run the fixture (pre-0.2.0) had 32 distinct task contexts, shared with the development (21), validation (21), and calibration (22) partitions, so this run was not a blind or context-independent evaluation. The fixture has since been regenerated as a held-out partition; these figures were not re-measured on it.
```

- [ ] **Step 5: Verify no stale claim survives**

Run:

```bash
git grep -nE 'recur across|reuses task contexts|NOT BLIND|32 distinct' -- ':!tasks/*' ':!CHANGELOG.md'
git grep -nE '0\.28|0\.38' -- '*.md' ':!tasks/*' ':!CHANGELOG.md'
```

Expected: first command prints only `fixtures/README.md:36` (the legacy `fresh_eval_*` row) and the two
past-tense mentions from Steps 2 and 4. Second prints nothing, unless `<T>`/`<Q>` happen to equal them.

- [ ] **Step 6: Add the 0.2.0 CHANGELOG entry**

Insert above `## 0.1.1 - 2026-09-23`, dated with `date +%F` on the day of the commit:

```markdown
## 0.2.0 - <YYYY-MM-DD>

- Partitioned the v2 evaluation corpus into disjoint context slices so the evaluation partition is genuinely held out. Previously all four splits sampled one 34-template pool, and 30 of the 32 distinct contexts in the evaluation partition also appeared in another split.
- Expanded the task template pool from 34 to 200 so each of the 200 records carries a unique context.
- Added `crates/reflex-calibration/tests/fixture_integrity.rs`, which asserts split disjointness, intra-split uniqueness, and the 200-context total in CI.
- Fixed the calibration sweep: it returned its seed values (0.28 / 0.38) because no grid point met its strict zero-error rule, and it printed "0 False Accepts, 0 frontier misses" as literal text. It now selects lexicographically (false accepts, frontier misses, coverage) and reports the selected point's measured counts, warning when it is not error-free.
- Changed `reflex experiment --phase freeze`: with no thresholds it now runs the calibration sweep and freezes the selected point, recording the dataset, provider and measured counts in `validation_notes` (previously it silently wrote 0.28 / 0.38). With `--risk-threshold` and the new `--quality-threshold` it records a manual override; with only one of them it errors. Because `--provider` defaults to `jev`, a bare `--phase freeze` now calls the live API — pass `--provider mock` for an offline run.
- Refit the frozen configuration on the held-out calibration partition: tau_accept <T>, quality threshold <Q>.
```

- [ ] **Step 7: Bump the version (lesson L4)**

Set `version = "0.2.0"` in root `Cargo.toml` (line 16), then `cargo check --workspace` so `Cargo.lock`
updates. Confirm the three sources agree: `Cargo.toml` says 0.2.0, the top `## ` of `CHANGELOG.md` says
0.2.0, and the tag will be created as `v0.2.0` in Task 7.

- [ ] **Step 8: Full gate, then commit**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets > /tmp/rc-6-full.txt 2>&1; grep 'test result' /tmp/rc-6-full.txt
```

Expected: fmt and clippy silent; 65 tests, 0 failed.

```bash
git add README.md fixtures/README.md CHANGELOG.md Cargo.toml Cargo.lock \
  crates/reflex-cli/src/commands/experiment.rs docs/experiments/live_experiment_hybrid_results.md
git commit -m "docs: state the verified held-out property and prepare 0.2.0

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

## Task 7: Publish

Pushing is outward-facing: confirm with the user before Step 2.

- [ ] **Step 1: Push the pending `main` commit first**

Local `main` is 1 commit ahead of `origin/main` (`3823a02`, the 0.1.1 changelog). Push it first so the
PR diff contains only this branch's work — and so a squash merge (lesson **L2**) cannot orphan it.

```bash
git push origin main
```

- [ ] **Step 2: Push the branch and open the PR**

```bash
git push -u origin feat/held-out-evaluation-corpus
gh pr create --base main --title "v0.2.0: held-out evaluation corpus and honest calibration sweep" \
  --body "$(sed -n '/^## 0.2.0/,/^## 0.1.1/p' CHANGELOG.md | sed '1d;$d'; printf '\n\n🤖 Generated with [Claude Code](https://claude.com/claude-code)\n')"
```

- [ ] **Step 3: After CI is green and the PR is merged — tag and release**

```bash
git switch main && git pull origin main
git tag v0.2.0 && git push origin v0.2.0
gh release create v0.2.0 --title "v0.2.0" --notes "$(sed -n '/^## 0.2.0/,/^## 0.1.1/p' CHANGELOG.md | sed '$d')"
```

- [ ] **Step 4: Clean up**

Per **L2**, compare content before deleting: `git diff --quiet main feat/held-out-evaluation-corpus && echo safe`,
record `git rev-parse feat/held-out-evaluation-corpus`, then `git branch -d` locally and
`git push origin --delete feat/held-out-evaluation-corpus`.

---

## Out of Scope

- **The unretained run manifest.** The historical Candidate E aggregates cannot be reproduced because
  per-task predictions were never checked in. That is a real gap, but it is about the *experiment run*,
  not the *corpus*. Fixing contamination first is the right order — recovering a manifest computed over a
  contaminated corpus would not produce a trustworthy number.
- **The legacy `fresh_eval_*` and `held_out_100.json` fixtures.** Same contamination, but they are
  documented as historical and nothing depends on them for current claims.
- **Renaming `v2_eval_blind_test.json`.** The name becomes accurate once Task 4 lands; renaming would
  churn `experiment.rs` for no behavioral gain.
- **Library defaults** `RiskDeferralConfig::default()` (`reflex-policy/src/risk_defer.rs:19`, 0.28) and
  `GuardedHybridConfig::default()` (`reflex-policy/src/composer.rs:503`, 0.38), and the missing-file
  fallback at `experiment.rs:~708`. Defaults are not fits; the frozen file is the fitted artifact.
  Changing library defaults is an API-visible behavior change and deserves its own decision.
- **Widening the calibration grid (option 3).** Only six clustered points are swept. Worth doing, but
  it changes the experiment, not the correctness of the selection — a separate change.
- **The 8.33% frontier miss (1 of 12) present at every grid point.** It is a finding about the policy
  itself, not the thresholds; no threshold in the grid removes it. Investigate the missed task separately.
- **The "69% autonomous coverage" claim in the `rustfuture/rustfuture` profile README.** It quotes the
  historical run on the contaminated corpus. Different repository — reword it after 0.2.0 ships.

---

## Review

**Paused 2026-09-24.** Branch `feat/held-out-evaluation-corpus`, working tree clean, 57/57 tests green,
`cargo fmt` and `cargo clippy -- -D warnings` both clean. Not pushed.

- [x] Task 1 — Guard test (RED) — `c1149b5`
- [x] Task 2 — Disjoint partitioning — `c1149b5`
- [x] Task 3 — Pool expansion to 200 — `3ef3140`
- [x] Task 4 — Regenerate, guard green — `c1149b5`
- [x] Task 5A — Lexicographic selection + measured report — `c0b5508`, `dcc62ec`, `a0dd1c0`
- [~] Task 5 — Recalibrate frozen config — Steps 1-2 done (`c22b1ce`); Steps 3-6 next
- [ ] Task 6 — Documentation and 0.2.0
- [ ] Task 7 — Publish (push `main`, push branch, PR, tag `v0.2.0`, release)

### Outcome against the success criteria

| # | Criterion | Result |
| :--- | :--- | :--- |
| 1 | Zero cross-split context overlap | **Met** — all six pairwise intersections are 0 |
| 2 | Zero intra-split repeats | **Met** — 30/30, 30/30, 40/40, 100/100 records to distinct contexts |
| 3 | 200 distinct contexts over 200 records | **Met** — union is exactly 200 |
| 4 | Class distribution preserved | **Met** — guarded by `every_split_keeps_the_intended_class_mix` |
| 5 | Generator deterministic | **Met** — re-running leaves fixture hashes identical |
| 6 | Criteria 1-3 asserted in CI | **Met** — 4 tests in `fixture_integrity.rs`; suite went 53 → 57 |

Red state before the fix matched the plan's prediction exactly: 1 passed, 3 failed, with intersections
15/14/21/14/21/22 and a 34-context union.

### BLOCKER: the calibration sweep never produces a result

`run_calibration_phase` (`crates/reflex-cli/src/commands/experiment.rs:572-652`) seeds its search
variables with the values it is supposed to discover:

```rust
let mut best_tau = 0.28;        // == the current frozen value
let mut best_quality = 0.38;    // == the current frozen value
let mut best_coverage = 0.0;
```

They are overwritten only when a candidate has `false_accept_count == 0` **and**
`frontier_miss_rate == Some(0.0)`. Measured on both corpora, no candidate ever satisfies this:

| Corpus | Distinct contexts | False accepts | Frontier miss rate | Printed "best point" |
| :--- | ---: | ---: | ---: | :--- |
| Old (contaminated) | 22 / 40 | 3 | 25.00% | `0.28 / 0.38 — 0 FA, 0 miss` |
| New (clean) | 40 / 40 | 1 | 8.33% | `0.28 / 0.38 — 0 FA, 0 miss` |

So the frozen thresholds were never a fit — they are the seeds, surviving a search that always fails.
Worse, line 649 hardcodes `(0 False Accepts, 0 frontier misses, ...)` into the format string, so the
success claim is a constant rather than a measurement and cannot be falsified from the output.

The defect predates this branch: it reproduces identically on the pre-change fixtures.

**Why this blocks Task 5:** freezing 0.28/0.38 now would re-freeze the seeds while the tool prints an
unearned claim — the exact failure mode this work set out to remove.

### Decision needed before resuming

What should "best operating point" mean when no candidate achieves zero false accepts and zero
frontier misses? Three options considered:

1. **Honest reporting only.** Print the real metrics; say explicitly when no candidate qualifies.
   Leave 0.28/0.38 but document them as defaults, not a fit. Smallest change.
2. **Relax the selection rule (recommended).** Lexicographic: minimise false accepts, then miss rate,
   then maximise coverage. On the clean corpus this selects **0.25 / 0.45** (0 false accepts, 8.33%
   miss, 70.0% coverage) — the frozen config would become a genuine sweep result for the first time.
   Combine with option 1's reporting fix.
3. **Widen the grid.** Only six clustered points are swept; a qualifying point may exist outside it.

Note under every option: 8.33% frontier miss (1 of 12 escalation-required tasks) appears at every grid
point on the clean corpus. That is a substantive finding about the policy, not an artifact.

### Decision (2026-09-27)

**Option 2 + option 1**, as recommended above — implemented by Task 5A. The frozen configuration becomes
a real sweep result, and the report can no longer claim zero errors it did not measure.

**Dry run (2026-09-27, throwaway worktree, discarded):** Task 5A's code applied verbatim from this plan —
every "replace this" snippet matched the source exactly; clippy clean; **64/64 tests** pass; the 7 new
tests pass. `rustfmt` rewrapped the `point` helper signature — the plan now carries the formatted version.
Mock sweep on the clean calibration partition:

| tau / Q | False accepts | Frontier miss | Coverage |
| :--- | ---: | ---: | ---: |
| **0.25 / 0.45** (selected) | **0** | 8.33% | 70.0% |
| 0.28 / 0.42 | 1 | 8.33% | 72.5% |
| 0.28 / 0.38 (current frozen) | 1 | 8.33% | 72.5% |
| 0.30 / 0.38 | 1 | 8.33% | 72.5% |
| 0.32 / 0.35 | 1 | 8.33% | 72.5% |
| 0.35 / 0.35 | 1 | 8.33% | 72.5% |

The currently frozen 0.28 / 0.38 produces a false accept on held-out data. Moving to 0.25 / 0.45 trades
2.5 points of coverage for eliminating it.

### Task 5 result: refit frozen configuration

| Parameter | Old (seed, never fit) | New (sweep on held-out calibration split, mock provider) |
| :--- | ---: | ---: |
| `optimal_tau_accept` | 0.28 | 0.25 |
| `clean_quality_accept_threshold` | 0.38 | 0.45 |
| `max_risk_small_reasoner` | 0.58 | 0.58 (not swept) |
| `mandatory_frontier_risk` | 0.70 | 0.70 (not swept) |

Measured on `fixtures/v2_eval_calibration.json` (40 tasks):

| tau / Q | False accepts | Frontier miss | Coverage |
| :--- | ---: | ---: | ---: |
| **0.25 / 0.45** (selected) | **0** | **8.33%** | **70.0%** |
| 0.28 / 0.42 | 1 | 8.33% | 72.5% |
| 0.28 / 0.38 (old frozen) | 1 | 8.33% | 72.5% |
| 0.30 / 0.38 | 1 | 8.33% | 72.5% |
| 0.32 / 0.35 | 1 | 8.33% | 72.5% |
| 0.35 / 0.35 | 1 | 8.33% | 72.5% |

The old frozen point produces 1 false accept on held-out data. The refit removes it at a cost of 2.5 points of
autonomous coverage (72.5% → 70.0%). Both thresholds moved, and that movement is itself the finding: 0.28 / 0.38
were the search's seed values surviving a sweep that never matched, not a fit.

### Findings from Task 5A review (not changed; recorded for follow-up)

- **Selected point is on the grid boundary** (lowest tau, highest Q). The grid likely does not bracket the
  optimum — strengthens the case for option 3 (widen the grid).
- **No coverage floor.** Pure lexicographic order would pick an escalate-everything point (0 FA, 0 miss, ~0%
  coverage) over 1 FA at 70%. Not reachable on the current grid; a minimum-coverage constraint is a design
  decision for later.
- **`--phase all` with threshold args** (pre-existing): `all` freezes the swept tau, but the evaluation phase
  then applies `--risk-threshold` as an override (`args.risk_threshold.unwrap_or(frozen_tau)`) and ignores
  `--quality-threshold`, so the frozen and evaluated tau can differ.

### Resume checklist

1. ~~Task 5A~~ — done (`c0b5508`, `dcc62ec`, `a0dd1c0`; 65/65 tests).
2. Task 5 Steps 3-6: `--phase freeze --provider mock`, check 0.25 / 0.45, record old vs new here, commit.
3. Task 6: docs, runtime text, CHANGELOG 0.2.0, version bump.
4. Task 7: push `main` (still 1 ahead of `origin/main`), push branch, PR, then tag and release.
