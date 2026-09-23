# Candidate E historical aggregate report

This page preserves the aggregate figures recorded for the Guarded Hybrid (Candidate E) experiment. The repository does not contain task-level Candidate E predictions or a run manifest, so the figures below cannot be independently reproduced from this checkout.

## Provenance and limits

- Fixture: `fixtures/v2_eval_blind_test.json` (100 curated synthetic tasks; `blind_test` is a legacy filename).
- The fixture has 32 distinct task contexts. The same contexts appear in the development (21 shared contexts), validation (21), and calibration (22) partitions. It is not a blind or context-independent held-out evaluation.
- The historical report describes the run as using live TypeSafe Jev inference. Raw per-task responses, exact model/provider identity, and a run manifest were not retained, so that claim and its outputs cannot be independently checked here.
- The fixture generator describes synthetic scenario dates. They are not collection timestamps.
- No production validation is included.

## Figures recorded by the historical report

| Measure | Recorded count | Denominator and meaning |
| :--- | :---: | :--- |
| Autonomous action coverage | 69/100 | Accept, terminate, retry, or continue actions; not necessarily verifier-free acceptance |
| Frontier calls avoided | 69/100 | Tasks for which the recorded summary says no frontier call was made; small-reasoner calls are a separate path |
| Frontier-required tasks missed | 0/31 | Missed routes among tasks whose ground-truth deferral was frontier/escalate |
| Unnecessary frontier calls | 0/69 | Frontier-routed tasks among tasks labeled non-frontier |
| Unsafe-to-accept tasks | 43/100 | Label count; 31 frontier-required tasks plus 12 transient tasks whose expected action is retry |

The two 69/100 figures happen to match in this aggregate; their definitions are different. The 43 unsafe-to-accept labels are not interchangeable with the 31 frontier-required labels. Neither label set provides independent confirmation that the prediction was correct.

The saved summary does not include the count of `Accept`/`Terminate` predictions, so the false accept rate (false accepts divided by autonomous passes) cannot be reconstructed. This page does not report a FAR point estimate or confidence interval.

## Conditional statistical bounds

If the historical counts are correct, approximate two-sided 95% Wilson intervals are 59.4%–77.2% for 69/100 autonomous actions, 0%–10.9% for 0/31 frontier misses, and 0%–5.3% for 0/69 unnecessary frontier calls. These are conditional bounds on the recorded counts, not independently verified results. Zero observed events does not establish zero risk.
