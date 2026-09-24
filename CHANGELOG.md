# Changelog

All notable changes to Reflex Control are documented here.

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
