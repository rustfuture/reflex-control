# Changelog

All notable changes to Reflex Control are documented here.

## Unreleased

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
