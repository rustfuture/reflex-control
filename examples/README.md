# Reflex Control Examples

This directory contains standalone examples of Reflex Control policy and telemetry APIs. They use mock/synthetic inputs and are not production integration tests.

The verifier-gate example's dollar and latency figures are illustrative baseline assumptions, not measured savings.

---

## 1. Verifier Gate (`examples/verifier-gate`)

The Verifier Gate demonstrates policy decisions from deterministic checks, atomic semantic signals, and risk thresholds.

### Running the Example

```bash
cargo run -p example-verifier-gate
```

### Key Demonstrations

1. **Policy Gate on Observations**:
   - A routine, low-risk documentation task is accepted by the configured policy.
   - High/critical risk actions (`DROP COLUMN`, privilege changes) trigger mandatory escalation to frontier reasoning regardless of high model confidence.
2. **Guarded Hybrid Architecture (Atomic Evidence)**:
   - Evaluates an `EvidenceVector` combining deterministic CI outputs (`tests_passed`, `files_changed`) with atomic semantic signals (`security_risk`, `objective_satisfied`).
   - Shows the configured risk policy escalating an example with security-sensitive changes despite passing tests. This fixture demonstrates code behavior; it is not evidence that every security defect is detected.

---

## 2. Shadow Mode Sidecar (`examples/shadow-mode`)

The Shadow Mode example records one mock prediction beside a sample orchestrator action. It uses `MockProvider` and an in-memory SQLite store; it does not connect to or evaluate a production orchestrator.

### Running the Example

```bash
cargo run -p example-shadow-mode
```

### Key Demonstrations

- Records the sample orchestrator action, Reflex's predicted action, latency, cost estimate, and an example outcome in memory.
- Illustrates the telemetry API; production shadow operation and traffic evaluation require a separate integration and validation effort.
