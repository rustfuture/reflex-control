# Reflex Control

[![CI](https://github.com/rustfuture/reflex-control/actions/workflows/ci.yml/badge.svg)](https://github.com/rustfuture/reflex-control/actions/workflows/ci.yml)
[![Rust 1.88+](https://img.shields.io/badge/rust-1.88%2B-orange.svg)](https://www.rust-lang.org)
[![MIT License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Architecture: Guarded Hybrid](https://img.shields.io/badge/architecture-guarded--hybrid-purple.svg)](docs/experiments/live_experiment_hybrid_results.md)
[![Status: Evaluated](https://img.shields.io/badge/evaluation-frozen--v0.1-brightgreen.svg)](fixtures/README.md)

A deterministic, calibrated **System-1 control plane and policy engine** for AI agent runtimes. It arbitrates whether an agent execution step should continue, retry, accept autonomously, request local verification, or escalate to an expensive frontier reasoning model.

Reflex Control couples zero-cost deterministic runtime checks (test suites, git diffs, execution bounds) with atomic semantic signals from [TypeSafe Jev](https://typesafe.ai). Inviolable safety rules strictly take precedence over model confidence: high-risk actions are vetoed regardless of predicted likelihood.

```
                  ┌───────────────────────────────┐
                  │    Agent Execution / Task     │
                  └──────────────┬────────────────┘
                                 │
                 ┌───────────────┴───────────────┐
                 ▼                               ▼
       [Deterministic Checks]          [Atomic Semantic Signals]
        - Exit codes & tests            - Scoped Jev evaluation
        - Changed files & blast radius  - Safety & vulnerability probes
        - Retry budgets & loop counters - Objective completion signals
                 │                               │
                 └───────────────┬───────────────┘
                                 ▼
              ┌─────────────────────────────────────┐
              │  Layer 1: Inviolable Hard Veto Gate │
              │  (Critical risk / privilege bypass) │
              └──────────────┬───────────────┬──────┘
                   Veto Path │               │ Passed Safety
                             ▼               ▼
                 ┌──────────────────┐  ┌───────────────────────────────────┐
                 │ Mandatory Frontier│  │ Layer 2: Calibrated Decision Gate │
                 │ Reasoner Escalation│ │ (Accept, Retry, Small Reasoner)   │
                 └──────────────────┘  └───────────────────────────────────┘
```

---

## Key Capabilities

- **Guarded Hybrid Decision Architecture**: Combines deterministic invariants with narrow, model-evaluated atomic propositions.
- **Hard Safety Veto**: Applies configured risk and evidence rules before confidence-based acceptance; see the policy code and examples for the implemented behavior.
- **Threshold Calibration**: Includes tools for measuring decision metrics and estimating confidence intervals on labeled outcomes.
- **Shadow Mode Telemetry**: Records predictions beside an orchestrator's action in SQLite; the included example uses a mock provider and an in-memory database.
- **Curated Evaluation Fixtures**: Includes a seeded synthetic-data generator and frozen policy configuration. These fixtures do not establish production reliability.

---

## Workspace Architecture

The repository is organized as a clean Cargo workspace separating core abstractions from providers, policies, and tooling:

| Crate | Path | Responsibility |
| :--- | :--- | :--- |
| `reflex-core` | [`crates/reflex-core`](crates/reflex-core) | Core data models, task contexts, evidence vectors, decision action enums. |
| `reflex-provider` | [`crates/reflex-provider`](crates/reflex-provider) | Provider traits and local mock/synthetic implementations for zero-dependency runs. |
| `reflex-jev` | [`crates/reflex-jev`](crates/reflex-jev) | TypeSafe Jev API integration for atomic semantic signal extraction. |
| `reflex-policy` | [`crates/reflex-policy`](crates/reflex-policy) | Guarded Hybrid policy implementation, safety veto rules, and acceptance thresholds. |
| `reflex-telemetry` | [`crates/reflex-telemetry`](crates/reflex-telemetry) | SQLite-backed decision persistence, shadow-mode evaluation logs, cost and latency tracking. |
| `reflex-calibration` | [`crates/reflex-calibration`](crates/reflex-calibration) | Grid sweep routines for policy calibration, threshold optimization, and Pareto frontier generation. |
| `reflex-cli` | [`crates/reflex-cli`](crates/reflex-cli) | Unified CLI tool (`reflex`) for execution, benchmarks, shadow mode, and demos. |

---

## Quick Start

Rust **1.88** or newer is required.

### 1. Installation & CLI Initialization

```bash
# Clone the repository
git clone https://github.com/rustfuture/reflex-control.git
cd reflex-control

# Build and install the CLI
cargo install --path crates/reflex-cli

# Initialize local state (creates reflex.db)
reflex init
```

### 2. Evaluating a Decision (Local Mock Mode)

Local mock mode runs deterministically without network calls or credentials:

```bash
reflex run --provider mock \
  --context "Check whether this worker patch is safe" \
  --risk low
```

The output includes the selected action, confidence, and telemetry record. Identifiers and measured latency vary between runs.

### 3. Using Live TypeSafe Jev

To evaluate tasks against live atomic semantic signals, set `JEV_API_KEY`:

```bash
export JEV_API_KEY="your_api_key_here"

reflex run --provider jev \
  --context "Add docstrings and verify the test suite" \
  --risk low
```

---

## Runnable Examples

The repository includes standalone runnable examples under [`examples/`](examples/):

### Verifier Gate Example

Demonstrates both autonomous acceptance of safe tasks and the inviolable safety veto against dangerous modifications (even when model confidence is high):

```bash
cargo run -p example-verifier-gate
```

Sample output:
```text
=== Reflex Control: Verifier Gate Example ===

--- Part 1: Policy Gate on Raw Observations ---
Task 1:     Fix typos in documentation
Risk:       low
Confidence: 0.96
Action:     accept
-> Policy skips a frontier verifier call (illustrative baseline: $0.02, 1,800 ms).

Task 2:     Execute DROP COLUMN users.auth_token migration
Risk:       critical
Confidence: 0.96
Action:     escalate
-> Safety rule enforced: High/Critical risk cannot bypass mandatory verification, despite 0.96 confidence!

--- Part 2: Guarded Hybrid Architecture (Candidate E) ---
Clean Task Composite Quality: 0.74
Risk Score:                   0.08
Effective Action:             terminate
-> Clean execution passed autonomously with 0 frontier cost!

Defect Task Quality:          0.85
Risk Score:                   0.90
Effective Action:             defer_to_frontier
-> Inviolable Safety Veto: Security hazard intercepted and escalated despite green tests!

=== All Verifier Gate examples completed successfully. ===
```

### Shadow Mode Sidecar Example

Demonstrates recording one mock prediction alongside a sample orchestrator action without mutating execution:

```bash
cargo run -p example-shadow-mode
```

---

## Evaluation Evidence & Historical Results

The repository preserves historical Candidate E aggregates in [`docs/experiments/live_experiment_hybrid_results.md`](docs/experiments/live_experiment_hybrid_results.md). The report records 69/100 autonomous actions and 69/100 frontier calls avoided, plus 0/31 missed frontier-required tasks and 0/69 unnecessary frontier calls. These are historical aggregate claims: task-level predictions and the run manifest were not retained, so the counts cannot be independently reproduced from this checkout.

The fixture filename says `blind_test`, but the 100-task partition contains only 32 distinct task contexts. It shares 21 contexts with the development split, 21 with validation, and 22 with calibration. It therefore is not a blind or independent held-out evaluation. The report records live Jev inference, but its outputs and exact run identity are unavailable for verification. This is curated synthetic evidence, not production validation.

The reported counts have approximate two-sided 95% Wilson intervals of 59.4%–77.2% for 69/100 autonomous actions, 0%–10.9% for 0/31 frontier misses, and 0%–5.3% for 0/69 unnecessary frontier calls. These bounds describe the reported sample counts only; zero observed events do not establish zero risk.

### Metric contract

- **Resolved outcome** in telemetry/calibration metrics means an explicit `Success` or `Failure`. `Partial`, `Unknown`, and missing outcomes are shown separately and excluded from outcome-based risk, calibration, and optimizer metrics.
- **Telemetry FAR** is false accepts divided by resolved decisions whose recorded action is autonomous `Accept`/`Terminate`. **Threshold-evaluation FAR** uses a candidate policy cohort: `Terminate`, plus eligible `Accept`/`Verify` decisions at or above the candidate confidence threshold; its numerator and denominator exclude unresolved outcomes. Candidate E's experiment FAR instead divides tasks labeled `is_unsafe_to_accept` and predicted `Accept`/`Terminate` by all predicted `Accept`/`Terminate` tasks; it depends on those labels, not a recorded execution outcome.
- **Frontier miss rate** in the Candidate E experiment is missed frontier-required tasks divided by all tasks labeled frontier-required. It is a routing metric, not a general defect-leakage rate.
- **Unnecessary frontier-call rate** in the Candidate E experiment is frontier-routed tasks labeled non-frontier divided by all tasks labeled non-frontier.
- **Autonomous action coverage** in Candidate E is `Accept`/`Terminate`/`Retry`/`Continue` actions divided by all tasks. **Frontier calls avoided** is tasks without a frontier call divided by all tasks. The two happened to have the same reported count but are calculated independently.
- Any rate with a zero denominator is reported as unavailable; a zero count alone is not a zero risk estimate.

---

## Datasets & Evaluation Fixtures

All fixture datasets and frozen policies reside in [`fixtures/`](fixtures/):

| Dataset File | Role | Sample Count |
| :--- | :--- | :---: |
| `fixtures/v2_eval_blind_test.json` | Curated synthetic evaluation partition (legacy filename; contexts recur across splits) | 100 tasks |
| `fixtures/v2_eval_calibration.json` | Systematic grid sweep split for threshold calibration ($\tau_{\text{accept}}, \theta_{\text{clean}}$) | 40 tasks |
| `fixtures/v2_eval_validation.json` | Architecture comparison split (Guarded Hybrid vs Deterministic Rules) | 30 tasks |
| `fixtures/v2_eval_dev.json` | Development & connectivity smoke testing split | 30 tasks |
| `fixtures/frozen_hybrid_config.json` | Frozen v0.1 policy parameters ($\tau_{\text{accept}} = 0.28, \theta_{\text{clean}} = 0.38, \text{risk}_{\text{frontier}} = 0.70$) | Config |

---

## Development & Testing

Run the full verification suite locally:

```bash
# Code formatting check
cargo fmt --all -- --check

# Strict workspace clippy
cargo clippy --workspace --all-targets -- -D warnings

# Run all workspace unit and integration tests
cargo test --workspace

# Run interactive demo via CLI
cargo run -p reflex-cli -- demo verifier-gate
```

---

## License

This project is licensed under the [MIT License](LICENSE).
