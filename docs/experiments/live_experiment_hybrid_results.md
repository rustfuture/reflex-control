# Guarded Hybrid v0.1 evaluation record

## Provenance

- Dataset: `fixtures/v2_eval_blind_test.json`
- Size: 100 held-out, curated synthetic tasks
- Provider: live TypeSafe Jev inference for atomic semantic signals
- Frozen configuration: `fixtures/frozen_hybrid_config.json`
- Architecture: Candidate E / Guarded Hybrid
- Production validation: none

## Observed result

| Measure | Count | Rate |
| :--- | :---: | :---: |
| Autonomous coverage | 69/100 | 69.0% |
| Frontier calls avoided | 69/100 | 69.0% |
| Defect leakage / FNR | 0/31 | 0.0% |
| False alarms / FPR | 0/69 | 0.0% |
| Unsafe-to-accept tasks | 43/100 | 43.0% |

The 31 frontier-required tasks are the denominator for defect leakage. The 69 remaining tasks are the denominator for false alarms. The 43 unsafe-to-accept tasks comprise those 31 tasks plus 12 transient failures that must be retried instead of accepted.

The historical aggregate did not retain Candidate E's count of `accept`/`terminate` decisions, so a false accept rate with the correct predicted-accept denominator cannot be reconstructed. It recorded zero false-accept events; no FAR percentage or confidence interval is claimed here.

## Statistical limits

- The approximate 95% Wilson interval for 69/100 coverage is 59.4%–77.2%.
- For 0 observed leakage events among 31 frontier-required tasks, the 95% Wilson upper bound is approximately 11.0%.
- The one-sided 95% Clopper-Pearson upper bound for the same zero-error observation is approximately 9.2%.

Zero observed errors does not prove a true zero risk rate. The controlled synthetic workload may not represent production traffic, and live provider outputs may vary across runs.
