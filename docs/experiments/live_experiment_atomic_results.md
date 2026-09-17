# Reflex Control: Empirical 4-Way Comparative Evaluation

> Historical research artifact. This predates the corrected release metric definitions and is preserved for reproducibility only. Its aggregate FAR/FNR/FPR labels must not be used as v0.1 release evidence; see the repository README for current definitions.

**Evaluation Dataset**: 100 Fresh Held-Out Tasks (`fixtures/fresh_eval_blind_test.json`)

### 1. Comparative Performance Matrix

| Metric | Baseline A: Direct Jev Action | Baseline B: Deterministic Only | Candidate C: Atomic Jev + Rules | Candidate D: Atomic Jev + Learned |
| :--- | :--- | :--- | :--- | :--- |
| **Observed False Accepts** | 0/74 | 0/74 | 0/74 | 0/74 |
| **Observed FAR** | 0.00% | 0.00% | 0.00% | 0.00% |
| **FAR 95% Wilson Upper Bound** | 4.93% | 4.93% | 4.93% | 4.93% |
| **FAR 95% Clopper-Pearson Upper** | 3.97% | 3.97% | 3.97% | 3.97% |
| **False Negative Rate (FNR)** | 10.8% | 100.0% | 0.0% | 59.5% |
| **False Positive Rate (FPR)** | 0.0% | 0.0% | 57.7% | 65.4% |
| **Autonomous Coverage** | 48.0% | 83.0% | 21.0% | 39.0% |
| **Frontier Calls Avoided** | 67.0% | 100.0% | 26.0% | 68.0% |
| **Action Accuracy** | 95.0% | 23.0% | 53.0% | 62.0% |
| **Deferral Accuracy** | 46.0% | 40.0% | 53.0% | 54.0% |
| **Macro-F1 Score** | 95.8% | 26.4% | 38.8% | 72.2% |
| **Latency p50** | 769 ms | 1 ms | 764 ms | 764 ms |
| **Latency p95** | 1360 ms | 1 ms | 1320 ms | 1320 ms |
| **Average Cost / Task** | $0.01087 | $0.00085 | $0.02247 | $0.01107 |
| **Cost Reduction vs Frontier** | 63.8% | 97.2% | 25.1% | 63.1% |
| **Jev API Calls** | 100 | 0 | 100 | 100 |
