# Reflex Control

[![CI](https://github.com/rustfuture/reflex-control/actions/workflows/ci.yml/badge.svg)](https://github.com/rustfuture/reflex-control/actions/workflows/ci.yml)
[![Rust 1.88+](https://img.shields.io/badge/rust-1.88%2B-orange.svg)](https://www.rust-lang.org)
[![MIT License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

A Rust policy engine for AI agent runtimes. It decides whether an agent should continue, retry, accept a result, request verification, or escalate to a stronger model.

Reflex Control combines deterministic checks with signals from [TypeSafe Jev](https://typesafe.ai). Jev evaluates narrow statements about the current task—such as whether a failure is transient or a change carries security risk—and returns structured evidence. Safety rules always take precedence over model confidence.

## How it works

The default **Guarded Hybrid** policy combines:

- deterministic evidence such as test status, retry count, and changed files;
- atomic task signals from TypeSafe Jev;
- calibrated thresholds for acceptance and model routing.

Every decision and its eventual outcome can be stored in SQLite for inspection, calibration, and shadow-mode evaluation. The frozen v0.1 policy is available in [`fixtures/frozen_hybrid_config.json`](fixtures/frozen_hybrid_config.json).

## Quick start

Rust 1.88 or newer is required.

```bash
cargo install --path crates/reflex-cli
reflex init
reflex run --provider mock \
  --context "Check whether this worker patch is safe" \
  --risk low
```

Mock mode runs locally without credentials. To use TypeSafe Jev, set `JEV_API_KEY` and select the `jev` provider:

```bash
export JEV_API_KEY="your_key"
reflex run --provider jev \
  --context "Add docstrings and verify the test suite" \
  --risk low
```

## Evaluation

The included held-out benchmark contains 100 curated synthetic tasks. With the frozen v0.1 configuration, Reflex Control handled 69 tasks without a frontier call and routed all 31 frontier-required tasks correctly.

| Result | v0.1 |
| --- | ---: |
| Autonomous coverage | 69% |
| Frontier-required tasks routed correctly | 31/31 |
| Observed defect leakage | 0/31 |
| Observed false alarms | 0/69 |

This benchmark is reproducible, but it is synthetic and should not be read as a production reliability claim. The datasets and raw reports are in [`fixtures/`](fixtures/) and [`docs/experiments/`](docs/experiments/).

## Examples

```bash
cargo run -p example-verifier-gate
cargo run -p example-shadow-mode
```

The first example shows autonomous acceptance and a hard security veto. The second records decisions beside an existing agent loop without changing its behavior. See [`examples/README.md`](examples/README.md) for details.

## Development

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

The workspace contains separate crates for the core model, providers, policy, telemetry, calibration, and CLI. CI runs on Linux, macOS, and Windows with Rust 1.88 and stable.

## Roadmap

The next release will focus on real-workload evaluation, shadow-mode calibration, additional evidence providers, and OpenTelemetry export.

## License

[MIT](LICENSE)
