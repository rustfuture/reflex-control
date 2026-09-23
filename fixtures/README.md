# Reflex Control: Fixtures & Evaluation Datasets

This directory contains evaluation fixtures and frozen benchmark configurations for Reflex Control.

---

## 1. V2 Curated Synthetic Evaluation Fixtures

These generated task fixtures are reproducible with `docs/experiments/generate_v2_datasets.py`. Task contexts are sampled from a small set of templates and recur across partitions; these partitions are not independent or blind test sets.

| File | Purpose | Size | Synthetic Scenario Span |
| :--- | :--- | :---: | :--- |
| `v2_eval_dev.json` | Connectivity, token counting, atomic signal parsing verification | 30 tasks | 2026-09-18 to 2026-09-21 |
| `v2_eval_validation.json` | Candidate E (Guarded Hybrid) vs Candidate C (Rules) comparison | 30 tasks | 2026-09-22 to 2026-09-25 |
| `v2_eval_calibration.json` | Systematic grid sweep of $\tau_{\text{accept}}$ and quality threshold $\theta_{\text{clean}}$ | 40 tasks | 2026-09-26 to 2026-09-29 |
| `v2_eval_blind_test.json` | Evaluation partition (legacy filename; context templates recur across other partitions) | 100 tasks | 2026-09-30 to 2026-10-05 |

The 100-task evaluation fixture contains 32 distinct contexts. It shares 21 contexts with development, 21 with validation, and 22 with calibration. A historical report describes live Jev inference, but per-task predictions and a run manifest are not checked in, so that run is not independently reproducible from this repository.

### Active Frozen Configuration
- `frozen_hybrid_config.json`: The v0.1 default configuration frozen after calibration, specifying:
  - `optimal_tau_accept`: 0.28
  - `clean_quality_accept_threshold`: 0.38
  - `max_risk_small_reasoner`: 0.58
  - `mandatory_frontier_risk`: 0.70

  The attached validation note is a historical report, not a result that can be verified from retained per-task calibration predictions.

---

## 2. Historical Research & Legacy Fixtures

| File | Description | Notes |
| :--- | :--- | :--- |
| `real_agent_worker_tasks.json` | 120 synthetic tasks modeled on agent/worker code traces | Used in early shadow-mode and Pareto sweeps. Annotated with simulated verifier & test outcomes. |
| `fresh_eval_*.json` | V1 evaluation split (predecessor to V2) | Historical reference; context templates recur across partitions, so the `fresh` filenames do not establish blind or independent evaluation. |
| `benchmark_100_controlled.json` | 100 early controlled tasks for prompt formulation experiments | Direct high-level action baseline testing. |
| `benchmark_100_live.json` | Live Jev raw responses from early formulation experiments | Historic trace data. |
| `benchmark_dataset_5000.json` | Scaled synthetic simulation dataset | Used for large-scale Pareto curve exploration. |
| `formulation_selection_100.json` | Formulation comparison dataset across choice/noul/score primitives | Pre-atomic evidence layer experiments. |
| `held_out_100.json` | Early synthetic split (legacy filename and report label) | Superseded; the filename does not establish an independent evaluation set. |
| `frozen_configuration.json` | Frozen choices for the early primitive benchmark | Used by `reflex benchmark --formulation frozen`; this is not the v0.1 policy configuration. |

> **Note on Data Provenance**:
> All fixture files in this directory are structured test fixtures and synthetic benchmarks designed for controlled evaluation. They are explicitly distinguished from unbounded production multi-tenant agent traces.
> Dates inside generated fixtures are synthetic scenario timestamps used to describe the partitions; they are not data collection dates.
