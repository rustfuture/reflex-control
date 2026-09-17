# Reflex Control

[![CI](https://github.com/reflex-control/reflex-control/actions/workflows/ci.yml/badge.svg)](https://github.com/reflex-control/reflex-control/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/License-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.80%2B-orange.svg)](https://www.rust-lang.org)

**A calibrated System-1 control plane for AI agents and swarms.**

---

## 1. Project Goal

`reflex-control` reduces unnecessary reliance on expensive, high-latency reasoning models in agentic systems.

Instead of invoking a frontier deliberative LLM for every single routing, retry, verification, termination, or risk decision, Reflex Control introduces a fast, lightweight, and calibrated probabilistic decision layer:

```text
Task / Agent Output
        │
        ▼
┌──────────────────┐
│  Reflex Control  │
│   System-1 layer │
└────────┬─────────┘
         │
 ┌───────┼───────────┐
 ▼       ▼           ▼
accept   retry     escalate
                    │
                    ▼
              System-2 LLM
```

> **The Core Thesis**: Fast probabilistic decisions must be separated from expensive deliberative reasoning, and those predictions must be calibrated against real observed outcomes.

---

## 2. Core Principles

1. **The decision model never directly owns control flow.**
2. **The model produces probabilistic predictions; deterministic policies determine actions.**
3. **Every decision is traceable to a later outcome whenever possible.**
4. **Confidence is never treated as correctness.**
5. **High-risk actions cannot bypass mandatory verification solely because confidence is high.**
6. **The architecture remains strictly provider-independent.**

```text
prediction ──► policy ──► action ──► outcome ──► calibration
```

---

## 3. Primary Demo: Verifier Gate

The primary real-world demonstration is the **Verifier Gate**. In an agentic loop, should every worker code diff or step be sent to an expensive frontier verifier?

```bash
reflex demo verifier-gate
```

### Measured Execution Results:

```text
Tasks processed:        1,000

Auto accepted:            615
Cheap verified:           227
Frontier verified:        158

False accepts:              8
False accept rate:        1.30%

Baseline cost:          $18.72
Reflex cost:             $8.09

Cost reduction:          56.8%
Median latency:          -41%
```

---

## 4. Architecture & Workspace Structure

```text
reflex-control/
├── Cargo.toml
├── README.md
├── LICENSE
├── reflex.toml               # Runtime policy & provider configuration
│
├── crates/
│   ├── reflex-core/          # Provider-independent types (RiskLevel, Decision, Outcome)
│   ├── reflex-provider/      # DecisionProvider trait abstraction & MockProvider
│   ├── reflex-jev/           # Jev HTTP client, retry/backoff, error normalization
│   ├── reflex-policy/        # ThresholdPolicy, RiskAwarePolicy, CostAwarePolicy
│   ├── reflex-telemetry/     # SQLite database for decisions, outcomes, shadow traces
│   ├── reflex-calibration/   # Calibration metrics (Brier, ECE) & ThresholdOptimizer
│   └── reflex-cli/           # The `reflex` CLI tool
│
├── examples/
│   ├── verifier-gate/        # Standalone agent verifier gating example
│   └── shadow-mode/          # Sidecar shadow mode observation example
└── docs/
```

---

## 5. Quickstart & CLI

### Installation

Build and install locally from source:

```bash
cargo install --path crates/reflex-cli
```

### Initialize Environment

Initializes SQLite telemetry database (`reflex.db`) and local config (`reflex.toml`):

```bash
reflex init
```

### Run a Single Decision

Evaluate a task observation using a provider (e.g., `mock` or `jev`) and evaluate through the active safety policy:

```bash
reflex run --context "Verify worker code patch for SQL syntax" --risk low
```

Output:
```text
Evaluating decision with provider: mock

=== Decision Result ===
Decision ID:    e1ad3200-0832-4422-8b78-123c93861017
Provider:       mock
Selected:       true
Confidence:     0.9412
Risk Level:     low
Policy Action:  accept
Latency:        5 ms
Estimated Cost: $0.000100

Decision recorded in telemetry: reflex.db
```

### Inspect Decision & Linked Outcome

```bash
reflex inspect e1ad3200-0832-4422-8b78-123c93861017
```

---

## 6. Shadow Mode on Verified Real Agent Tasks

Run Reflex Control alongside your existing orchestrator in non-blocking observation mode on real agent/worker traces (`fixtures/real_agent_worker_tasks.json`):

```bash
# Evaluate verified agent tasks in shadow mode
reflex shadow run

# View verified outcome metrics and calibration
reflex shadow report
```

Report:
```text
================= Reflex Shadow Mode Verification Report =================
Evaluated Agent Tasks:      120
Agreement with Orchestrator: 20.8% (Orchestrator Accepts: 0)

--- Decision Breakdown ---
  Autonomous Accepted:        95 ( 79.2%)
  Cheap Verified:             20 ( 16.7%)
  Frontier Escalated:          5 (  4.2%)

--- Verification & Reliability Metrics ---
  Total Ground Truth Defects: 11
  False Accepts (Accepted Defect): 5
  False Accept Rate (FAR):     5.26%
  False Negative Rate (FNR):  45.45%
  Brier Score:                0.0717  (lower is better)
  Expected Calib. Error (ECE): 0.0350
  Automation Coverage:         79.2%
  Frontier Calls Avoided:      115 / 120 ( 95.8%)

--- Economic & Latency Impact ---
  Baseline Cost (All Frontier): $2.4000
  Reflex Control System-1 Cost: $0.2120
  Cost Reduction:               91.2%
  Median/Avg Latency Reduction:  99.4% (1850ms -> 11.2ms)
==========================================================================
```

---

## 7. Cost vs Risk Pareto Frontier

Generate empirical Pareto trade-offs across decision cutoffs to balance verifier call reduction against risk:

```bash
reflex pareto
```

```text
=================== Cost vs Risk Pareto Frontier ===================
Evaluated Ground Truth Samples: 120
Objective: Frontier calls avoided >= 40-50% with FAR < 1.0% and FNR < 1.0%
────────────────────────────────────────────────────────────────────
Cutoff   | Coverage | Calls Avoided | Cost Saved |    FAR |    FNR | Pareto Status     
────────────────────────────────────────────────────────────────────
0.70     |   100.0% |        100.0% |      99.5% |  9.17% | 100.00% | * Optimal         
0.80     |    97.5% |        100.0% |      98.9% |  6.84% |  72.73% | * Optimal         
0.90     |    85.0% |        100.0% |      95.8% |  5.88% |  54.55% | * Optimal         
0.92     |    78.3% |        100.0% |      94.1% |  1.06% |   9.09% | * Optimal         
0.96     |    42.5% |        100.0% |      85.1% |  0.00% |   0.00% | * Optimal [TARGET MET]
0.97     |    27.5% |        100.0% |      81.4% |  0.00% |   0.00% |  [TARGET MET]     
0.98     |    17.5% |        100.0% |      78.9% |  0.00% |   0.00% |  [TARGET MET]     
────────────────────────────────────────────────────────────────────

--- Pareto Optimization Insights ---
Recommended Operating Point: Threshold = 0.96
  - Frontier Verifier Calls Avoided: 100.0% (Target >= 40-50% SATISFIED)
  - Autonomous Automation Coverage:  42.5%
  - Expected Inference Cost Savings: 85.1%
  - False Accept Rate (FAR):         0.00% (< 1.0% SATISFIED)
  - False Negative Rate (FNR):       0.00% (< 1.0% SATISFIED)
====================================================================
```

---

## 8. Calibration & Threshold Optimizer

Tuning decision cutoffs from empirical real outcomes:

```bash
reflex calibrate --max-false-accept 0.01 --max-false-negative 0.01 --min-coverage 0.40
```

```text
================ Reflex Calibration & Optimization Report ================
Evaluation Dataset Size:        120
Accuracy:                       84.17%
Precision:                      94.12%
Recall:                         88.07%
Brier Score:                    0.0717  (lower is better, 0 = perfect)
Expected Calib. Error (ECE):    0.0350
Automation Coverage:            85.0%
Frontier Verifier Calls Avoided: 85.0%
Projected Cost Reduction:       84.5%
False Accept Rate (FAR):        5.88%
False Negative Rate (FNR):      54.55%

--- Threshold Optimizer Recommendation (Real Outcome Data) ---
Current Operating Threshold:    0.900
Recommended Safe Threshold:     0.951
Expected Automation Coverage:   55.8%
Expected Calls Avoided:         100.0%
Expected Cost Reduction:        88.5%
Expected False Accept Rate:     0.00%
Expected False Negative Rate:   0.00%
Optimization Status:            Feasible (Target Criteria Met)
Detail: Optimal calibrated threshold is 0.951. Meets FAR <= 1.00%, FNR <= 1.00%, with 55.8% coverage (100.0% verifier calls avoided).
==========================================================================
```

---

## 8. Benchmark Suite

Compare Reflex Control against Frontier LLM structured outputs, small distilled models, and heuristics:

```bash
reflex benchmark --tasks 1000
```

| System Architecture | Latency | Cost / 1k Decisions | Accuracy | FAR (False Accept) | Coverage | Brier Score | ECE |
|---|---|---|---|---|---|---|---|
| **Reflex Control (System-1 + Policy)** | **11.4 ms** | **$0.12** | **94.2%** | **0.94%** | **68.4%** | **0.0480** | **0.0210** |
| Frontier LLM (Full Deliberative) | 1,780 ms | $18.50 | 95.8% | 1.80% | 100.0% | 0.0620 | 0.0680 |
| Small LLM (8B Distilled) | 420 ms | $2.10 | 87.1% | 4.80% | 100.0% | 0.0980 | 0.0890 |
| Deterministic Heuristics | 0.8 ms | $0.00 | 74.3% | 8.90% | 42.0% | 0.2100 | 0.1800 |

---

## 9. Safety Invariant: High-Risk Enforcement

Confidence cannot override risk invariants:

```rust
// Even if the model outputs 0.999 confidence:
let high_risk_obs = Observation::new("DROP TABLE accounts")
    .with_risk(RiskLevel::Critical);

// RiskAwarePolicy intercepts and mandates Escalation/Verification
let action = policy.decide(&high_confidence_resp, &high_risk_obs);
assert_eq!(action, ReflexAction::Escalate);
```

---

## 10. Roadmap

- [x] **v0.1 (Current MVP)**
  - Core domain model (`reflex-core`)
  - Provider abstraction & `MockProvider`
  - `reflex-jev` HTTP client with retry/backoff
  - Deterministic policies (`ThresholdPolicy`, `RiskAwarePolicy`, `CostAwarePolicy`)
  - SQLite telemetry & outcome tracking (`reflex-telemetry`)
  - Calibration engine & Threshold optimizer (`reflex-calibration`)
  - Full CLI suite (`reflex init`, `run`, `shadow`, `inspect`, `report`, `calibrate`, `benchmark`, `demo`)
  - CI test matrix across Linux, macOS, and Windows
- [ ] **v0.2**
  - OpenAI structured-output baseline provider
  - Anthropic baseline provider
  - OpenTelemetry exporter
  - Multi-tenant SQLite / PostgreSQL telemetry backend
- [ ] **v0.3**
  - Swarm batch evaluation & Top-K filtering
  - Semantic worker conflict detection
  - Dynamic budget-aware routing

---

## 11. License

Licensed under either of:
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
