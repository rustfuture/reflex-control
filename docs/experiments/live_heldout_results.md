# Reflex Control: Guarded Hybrid Architecture Empirical Benchmark

**Evaluation Dataset**: `fixtures/v2_eval_blind_test.json`

**Provider mode**: `jev`

> This is a generated experimental report, not production validation. `fixtures/v2_eval_blind_test.json` shares no task context with the development, validation, or calibration partitions (verified in CI by `crates/reflex-calibration/tests/fixture_integrity.rs`), but it is curated synthetic data. Zero observed errors do not prove zero risk. FAR is false accepts divided by autonomous accepts/terminations; frontier-miss rate is missed frontier-required tasks divided by all frontier-required tasks; unnecessary frontier-call rate is frontier calls on non-frontier tasks divided by all non-frontier tasks. Autonomous action coverage includes accept/terminate/retry/continue actions; frontier calls avoided is reported separately.

**Thresholds**: Loaded from `fixtures/frozen_hybrid_config.json` (frozen 2026-09-27T18:28:16.606628+00:00): tau_accept 0.22, quality_thresh 0.40. Freeze record: Selected by the calibration sweep: fewest false accepts, then fewest frontier misses, then highest autonomous action coverage. Measured on fixtures/v2_eval_calibration.json with provider jev: 0 false accepts, 0/12 frontier misses, 70.0% autonomous action coverage. Per-task predictions are not retained, so these counts are not independently reproducible from this file alone.

### 1. Comparative Performance Matrix

| Metric | Baseline A: Direct Jev Action | Baseline B: Deterministic Only | Candidate C: Atomic Jev + Rules Baseline | Candidate E: Guarded Hybrid Architecture |
| :--- | :--- | :--- | :--- | :--- |
| **Observed False Accepts** | 0/5 | 0/45 | 0/5 | 0/43 |
| **Observed FAR** | 0.00% | 0.00% | 0.00% | 0.00% |
| **FAR 95% Wilson Upper Bound** | 43.45% | 7.87% | 43.45% | 8.20% |
| **FAR 95% exact upper (0 errors)** | 45.07% | 6.44% | 45.07% | 6.73% |
| **Frontier Miss Rate / 95% Wilson CI** | 3/31 (9.68% [95% CI: 3.35% – 24.90%]) | 23/31 (74.19% [95% CI: 56.75% – 86.30%]) | 4/31 (12.90% [95% CI: 5.13% – 28.85%]) | 0/31 (0.00% [95% CI: 0.00% – 11.03%]) |
| **Unnecessary Frontier-Call Rate / 95% Wilson CI** | 0/69 (0.00% [95% CI: 0.00% – 5.27%]) | 0/69 (0.00% [95% CI: 0.00% – 5.27%]) | 31/69 (44.93% [95% CI: 33.77% – 56.62%]) | 1/69 (1.45% [95% CI: 0.26% – 7.76%]) |
| **Autonomous Action Coverage** | 71.0% | 77.0% | 37.0% | 66.0% |
| **Frontier Calls Avoided** | 72.0% | 92.0% | 42.0% | 68.0% |
| **Action Accuracy** | 57.0% | 57.0% | 45.0% | 97.0% |
| **Deferral Accuracy** | 29.0% | 65.0% | 47.0% | 97.0% |
| **Macro-F1 Score** | 62.8% | 38.6% | 49.3% | 97.9% |
| **Latency p50** | 308 ms | 1 ms | 313 ms | 313 ms |
| **Latency p95** | 437 ms | 1 ms | 467 ms | 467 ms |
| **Average Cost / Task** | $0.00847 | $0.00315 | $0.01767 | $0.00972 |
| **Cost Reduction vs Frontier** | 71.8% | 89.5% | 41.1% | 67.6% |
| **Jev API Calls** | 100 | 0 | 100 | 100 |
