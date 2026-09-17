# Experimental archive

This directory preserves the research that led to Candidate E, the Guarded Hybrid default in v0.1. These files are historical evidence, not production validation.

## Release evaluation

The v0.1 release claim comes from `fixtures/v2_eval_blind_test.json`: 100 held-out, curated synthetic tasks evaluated with live TypeSafe Jev inference after freezing `tau_accept = 0.28` and `theta_clean = 0.38`.

Candidate E recorded:

- 69/100 autonomous coverage;
- 69/100 frontier calls avoided;
- 0/31 observed defect leakage among frontier-required tasks;
- 0/69 observed false alarms among non-frontier tasks.

The 31 frontier-required tasks are distinct from the 43 tasks marked unsafe to accept. The latter also includes 12 transient failures whose correct autonomous action is `retry`. The repository README defines every denominator and gives confidence bounds. Zero observed leakage is an early result, not proof of zero risk.

All evaluation fixtures are synthetic and curated. Timestamps in the generated fixtures describe synthetic scenario ordering rather than collection time. The inference calls used for the documented blind run were live; the workloads were not production telemetry.

## Files

- `live_experiment_hybrid_results.md` records the release evaluation and its limits.
- `live_experiment_atomic_results.md` is an earlier four-way experiment using superseded metric reporting.
- `benchmark_100_live_results.txt` and `benchmark_100_results.txt` are raw outputs from pre-atomic formulation experiments.
- `generate_v2_datasets.py` deterministically generates the synthetic evaluation splits.

The older reports remain byte-for-byte useful as historical outputs except for the archive banners. Their FAR/FNR/FPR labels should not be compared directly with the corrected v0.1 evaluator.
