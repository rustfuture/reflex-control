# Evaluation Methodology & Historical Notes

This document preserves the metric definitions and historical aggregate evaluations for Reflex Control.

## Metric Contract

- **Resolved outcome** in telemetry/calibration metrics means an explicit `Success` or `Failure`. `Partial`, `Unknown`, and missing outcomes are shown separately and excluded from outcome-based risk, calibration, and optimizer metrics.
- **Telemetry FAR** is false accepts divided by resolved decisions whose recorded action is autonomous `Accept`/`Terminate`. **Threshold-evaluation FAR** uses a candidate policy cohort: `Terminate`, plus eligible `Accept`/`Verify` decisions at or above the candidate confidence threshold; its numerator and denominator exclude unresolved outcomes. Candidate E's experiment FAR instead divides tasks labeled `is_unsafe_to_accept` and predicted `Accept`/`Terminate` by all predicted `Accept`/`Terminate` tasks; it depends on those labels, not a recorded execution outcome.
- **Frontier miss rate** in the Candidate E experiment is missed frontier-required tasks divided by all tasks labeled frontier-required. It is a routing metric, not a general defect-leakage rate.
- **Unnecessary frontier-call rate** in the Candidate E experiment is frontier-routed tasks labeled non-frontier divided by all tasks labeled non-frontier.
- **Autonomous action coverage** in Candidate E is `Accept`/`Terminate`/`Retry`/`Continue` actions divided by all tasks. **Frontier calls avoided** is tasks without a frontier call divided by all tasks. The two happened to have the same reported count but are calculated independently.
- Any rate with a zero denominator is reported as unavailable; a zero count alone is not a zero risk estimate.

## Historical Candidate E Aggregates

The repository preserves historical Candidate E aggregates in [`experiments/live_experiment_hybrid_results.md`](experiments/live_experiment_hybrid_results.md). The report records 69/100 autonomous actions and 69/100 frontier calls avoided, plus 0/31 missed frontier-required tasks and 0/69 unnecessary frontier calls. These are historical aggregate claims: task-level predictions and the run manifest were not retained, so the counts cannot be independently reproduced from this checkout.

The historical run used the pre-0.2.0 version of this fixture, which held only 32 distinct task contexts, 30 of which also appeared in the development, validation, or calibration splits, so those aggregates were not measured on held-out data. Since 0.2.0 the evaluation partition shares no context with any other split (enforced in CI by `crates/reflex-calibration/tests/fixture_integrity.rs`), and [`experiments/live_heldout_results.md`](experiments/live_heldout_results.md) is the current measurement on it. The report records live Jev inference, but its outputs and exact run identity are unavailable for verification. This is curated synthetic evidence, not production validation.

The reported counts have approximate two-sided 95% Wilson intervals of 59.4%–77.2% for 69/100 autonomous actions, 0%–10.9% for 0/31 frontier misses, and 0%–5.3% for 0/69 unnecessary frontier calls. These bounds describe the reported sample counts only; zero observed events do not establish zero risk.
