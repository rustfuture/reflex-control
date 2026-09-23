# Experimental archive

This directory preserves historical research around Candidate E, the Guarded Hybrid policy configuration. The experiments use curated synthetic fixtures and are not production validation.

## Candidate E aggregate report

[`live_experiment_hybrid_results.md`](live_experiment_hybrid_results.md) preserves the aggregate figures previously reported for `fixtures/v2_eval_blind_test.json`. That filename is retained for compatibility, but the fixture is not blind: its 100 tasks contain 32 distinct contexts, with 21, 21, and 22 contexts shared with development, validation, and calibration, respectively.

The historical report records live Jev inference, but the repository does not retain per-task predictions, raw responses, or a run manifest. The reported metrics therefore cannot be independently reproduced from this checkout. The report labels its denominators explicitly and calculates conditional confidence bounds without treating zero observed events as zero risk.

All generated timestamps are synthetic scenario dates, not collection dates. `generate_v2_datasets.py` recreates the V2 fixtures with a fixed random seed.

## Files

- `live_experiment_hybrid_results.md` records the historical aggregate and its limits.
- `live_experiment_atomic_results.md` is an earlier experiment. Its original labels and claims are preserved as historical text and should not be treated as current metric definitions.
- `benchmark_100_live_results.txt` and `benchmark_100_results.txt` are outputs from earlier formulation experiments.
- `generate_v2_datasets.py` generates the synthetic evaluation fixtures.
