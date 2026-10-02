# Reflex Control

Reflex Control decides whether an AI task can finish on its own, retry, or escalate to an expensive reasoning model.

[![CI](https://github.com/rustfuture/reflex-control/actions/workflows/ci.yml/badge.svg)](https://github.com/rustfuture/reflex-control/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

**Status:** Research prototype (v0.4.0). Tested on synthetic fixtures; not production-validated.

- Runs fast local checks (exit codes, test results, git diffs) alongside semantic risk signals.
- Blocks high-risk actions like credential leaks or schema changes with a mandatory safety veto.
- Tunes acceptance thresholds across labeled tasks with statistical confidence intervals.
- Logs runtime choices, shadow decisions, costs, and latencies to SQLite (synchronous writes on the calling thread).
- Includes a CLI to evaluate tasks offline, run calibration sweeps, and inspect telemetry.

## Quick start

Rust **1.88** or newer is required.

### 1. Build

```bash
# Clone the repository
git clone https://github.com/rustfuture/reflex-control.git
cd reflex-control

# Build the workspace
cargo build --workspace
```

### 2. Run Offline Demos & Local Mock Execution

Evaluate decisions deterministically without external credentials or network calls:

```bash
# Run the verifier gate demo
cargo run --bin reflex -- demo verifier-gate

# Evaluate an individual task decision using the local mock provider
cargo run --bin reflex -- run --provider mock \
  --context "Check whether this worker patch is safe" \
  --risk low

# Run shadow mode on the benchmark fixture
cargo run --bin reflex -- shadow run

# Run Pareto frontier cost vs risk sweep
cargo run --bin reflex -- pareto
```

The CLI can also be installed to your cargo path via `cargo install --path crates/reflex-cli`, after which commands can be called directly as `reflex <subcommand>`.

### 3. Using Live TypeSafe Jev (Optional)

To evaluate tasks against live semantic signals using [TypeSafe Jev](https://typesafe.ai) (an API that scores task quality and safety), set `JEV_API_KEY`:

```bash
export JEV_API_KEY="your_api_key_here"

cargo run --bin reflex -- run --provider jev \
  --context "Add docstrings and verify the test suite" \
  --risk low
```

---

## How it works

Reflex Control evaluates agent execution steps through a multi-layer decision pipeline:

- The engine collects deterministic signals (exit codes, test outcomes, changed files) and semantic signals (task completion, security risk, scope drift).
- A hard safety veto immediately escalates to a frontier model (a high-capability reasoning model) if the task modifies sensitive files, introduces security hazards, or exhausts retries.
- Transient errors with remaining retry budget trigger an autonomous retry rather than an expensive model call.
- Safe steps pass to a calibrated scoring gate that evaluates composite task quality against tuned thresholds.
- Steps meeting the quality threshold finish autonomously (`Accept` or `Terminate`), avoiding unnecessary calls to larger models.

---

## Workspace Architecture

The repository is organized as a Cargo workspace separating core abstractions from providers, policies, and tooling (see execution diagram in [`docs/architecture.md`](docs/architecture.md)):

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

## Runnable Examples

The repository includes standalone runnable examples under [`examples/`](examples/):

### Verifier Gate Example

Demonstrates both autonomous acceptance of safe tasks and hard safety veto rules against dangerous modifications (even when model confidence is high):

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
-> Safety rule: high and critical risk always escalate to verification, even at 0.96 confidence.

--- Part 2: Guarded Hybrid Architecture (Candidate E) ---
Clean Task Composite Quality: 0.74
Risk Score:                   0.08
Effective Action:             terminate
-> Clean execution finished autonomously without a frontier call.

Defect Task Quality:          0.85
Risk Score:                   0.90
Effective Action:             defer_to_frontier
-> Safety veto: the security risk was escalated even though tests passed.

=== All Verifier Gate examples completed successfully. ===
```

### Shadow Mode Sidecar Example

Demonstrates recording one mock prediction alongside a sample orchestrator action without mutating execution:

```bash
cargo run -p example-shadow-mode
```

### Agent Runtime Gate Example

Maps a runtime-style task result (exit status, timeout, verification result, changed files, retries left) to Accept / Retry / Escalate using the mock evidence provider, with no network access:

```bash
cargo run -p example-agent-runtime-gate
```

Sample output:
```text
=== Reflex Control: agent-runtime gate (offline, mock evidence) ===
verified fix                             -> accept (reflex action: terminate)
provider timeout, budget left            -> retry (reflex action: retry)
verification failing, budget exhausted   -> escalate (reflex action: defer_to_frontier)
green tests, secret file touched         -> escalate (reflex action: defer_to_frontier)
```

---

<a id="evaluation-benchmark--measured-results"></a>
## Evaluation Benchmark / Measured Results

**Live held-out result (0.3.0).** With thresholds frozen from a live-Jev calibration sweep (tau_accept 0.22, quality threshold 0.40, selected on the 40-task calibration partition), Candidate E was evaluated once with live Jev signals on the 100-task held-out partition: 0/43 false accepts among autonomous accepts and terminations, 0/31 missed frontier-required tasks, 1/69 unnecessary frontier calls, 66.0% autonomous action coverage, and 68.0% of frontier calls avoided. The generated report is committed as [`docs/experiments/live_heldout_results.md`](docs/experiments/live_heldout_results.md). Approximate two-sided 95% Wilson intervals are 0%–8.2% for 0/43 false accepts, 0%–11.0% for 0/31 frontier misses, 0.3%–7.8% for 1/69 unnecessary frontier calls, and 56.3%–74.5% for 66/100 autonomous actions. Live Jev signals vary slightly between calls: repeated calibration sweeps differed by one task in coverage at some grid points while selecting the same operating point, so a re-run can move these counts by a task or two. This is a single run on curated synthetic data, not production validation, and per-task predictions are not retained.

Historical Candidate E aggregates, partition history, and metric definitions are documented in [`docs/evaluation.md`](docs/evaluation.md).

---

## Datasets & Evaluation Fixtures

All fixture datasets and frozen policies reside in [`fixtures/`](fixtures/):

| Dataset File | Role | Sample Count |
| :--- | :--- | :---: |
| `fixtures/v2_eval_blind_test.json` | Curated synthetic held-out evaluation partition (legacy filename) | 100 tasks |
| `fixtures/v2_eval_calibration.json` | Systematic grid sweep split for threshold calibration ($\tau_{\text{accept}}, \theta_{\text{clean}}$) | 40 tasks |
| `fixtures/v2_eval_validation.json` | Architecture comparison split (Guarded Hybrid vs Deterministic Rules) | 30 tasks |
| `fixtures/v2_eval_dev.json` | Development & connectivity smoke testing split | 30 tasks |
| `fixtures/frozen_hybrid_config.json` | Frozen 0.3.0 policy parameters ($\tau_{\text{accept}} = 0.22, \theta_{\text{clean}} = 0.40, \text{risk}_{\text{frontier}} = 0.70$); fit with live Jev on the calibration partition | Config |

---

## Scope and Limitations

- **Synthetic fixtures:** Fixture datasets in [`fixtures/`](fixtures/) are synthetic scenarios generated via `docs/experiments/generate_v2_datasets.py` with fixed random seeds. They establish held-out split disjointness and threshold behavior under controlled conditions, not generalization to arbitrary production multi-tenant agent workloads.
- **Provider differences:** Offline evaluation via `--provider mock` uses deterministic heuristics and does not evaluate prompt language or contextual semantics. Live semantic evaluations require the TypeSafe Jev API (`JEV_API_KEY`), which introduces external network latency and minor sampling variation between runs.
- **Statistical bounds:** Zero observed false accepts or frontier misses on a 40-task or 100-task split indicate that no errors occurred in that sample, but statistical confidence bounds (e.g., Wilson intervals up to 8.2%–11.0%) reflect sample-size limits; zero observed errors do not establish zero risk.
- **Cost estimates:** Dollar savings and latency figures in demonstrations and synthetic benchmarks are calculated against assumed baseline costs ($0.02 / 1,800 ms per frontier call), not measured production bills.

---

## Tests

```bash
cargo test --workspace --all-targets
```

The test suite covers decision logic, security veto rules, threshold calibration, telemetry persistence, API integrations, and fixture split integrity.

---

## License

This project is licensed under the [MIT License](LICENSE).
