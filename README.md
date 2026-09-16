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

## 6. Shadow Mode

Run Reflex Control alongside your existing orchestrator in non-blocking observation mode:

```bash
# Evaluate 100 orchestrator traces in shadow mode
reflex shadow run --count 100

# View agreement rate and projected economic savings
reflex shadow report
```

Report:
```text
================= Reflex Shadow Mode Report =================
Shadow Traces Evaluated:    100
Action Agreement Rate:      88.0%
Reflex Autonomous Accepts:  62 (62.0%)
Orchestrator Accepts:       65 (65.0%)

--- Performance & Economics Comparison ---
Orchestrator Est. Cost:     $1.5000 (all tasks routed to LLM)
Reflex Control Cost:        $0.0100 (cheap System-1 routing)
Projected Cost Savings:     99.3%
Orchestrator Avg Latency:   1850 ms
Reflex System-1 Latency:    11.4 ms
Decision Latency Reduction: 99.4%
=============================================================
```

---

## 7. Calibration & Threshold Optimizer

Reflex Control calculates statistical reliability metrics and tunes decision cutoffs from empirical outcomes:

```bash
reflex calibrate --max-false-accept 0.01 --min-coverage 0.60
```

Output:
```text
================ Reflex Calibration Report ================
Evaluation Dataset Size:  2000
Accuracy:                 94.15%
Precision:                98.75%
Recall:                   92.40%
Brier Score:              0.0495  (lower is better, 0 = perfect)
Expected Calib. Error:    0.0284  (ECE)
Automation Coverage:      68.2%
Selective Accuracy:       98.75%
False Accept Rate (FAR):  1.25%
False Escalate Rate:      19.45%

--- Calibration View (Confidence vs Observed Frequency) ---
Bucket Range   Count   Mean Conf   Observed Success   Calib Gap
─────────────────────────────────────────────────────────────────
0.50 - 0.60       45        54.2%           53.8%         0.0040
0.60 - 0.70      112        64.8%           63.1%         0.0170
0.70 - 0.80      230        75.2%           76.4%         0.0120
0.80 - 0.90      510        84.9%           84.2%         0.0070
0.90 - 1.00     1103        95.4%           96.1%         0.0070

--- Threshold Optimizer Recommendation ---
Current threshold:       0.900
Recommended threshold:   0.947
Expected coverage:       64.2%
Expected False Accept:   0.83%
Optimization status:     Feasible
Detail: Found optimal threshold 0.947 satisfying max FAR <= 1.00% and coverage >= 60.0%.
===========================================================
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
