# Reflex Control Examples

This directory contains standalone runnable examples demonstrating how to integrate Reflex Control as a System-1 control plane alongside agent loops.

---

## 1. Verifier Gate (`examples/verifier-gate`)

The Verifier Gate demonstrates gating expensive frontier verifier calls based on deterministic CI checks, atomic semantic signals, and calibrated risk thresholds.

### Running the Example

```bash
cargo run -p example-verifier-gate
```

### Key Demonstrations

1. **Policy Gate on Observations**:
   - Routine, low-risk documentation edits are autonomously accepted with zero frontier verifier cost.
   - High/critical risk actions (`DROP COLUMN`, privilege changes) trigger mandatory escalation to frontier reasoning regardless of high model confidence.
2. **Guarded Hybrid Architecture (Atomic Evidence)**:
   - Evaluates a full `EvidenceVector` combining deterministic CI outputs (`tests_passed`, `files_changed`) with atomic semantic signals (`security_risk`, `objective_satisfied`).
   - Verifies that a subtle security vulnerability (e.g. leaked credentials) that passes test suites is intercepted and escalated by Layer 1 Inviolable Hard Safety Veto rules.

---

## 2. Shadow Mode Sidecar (`examples/shadow-mode`)

The Shadow Mode example shows how to run Reflex Control alongside an existing production agent orchestrator in non-blocking observation mode.

### Running the Example

```bash
cargo run -p example-shadow-mode
```

### Key Demonstrations

- Evaluates agent tasks in the background without intercepting orchestrator flow.
- Telemetry records the orchestrator's decision, Reflex's predicted action, latency, cost estimate, and eventual execution outcome into SQLite (`reflex.db`).
- Enables teams to calculate empirical FAR, FNR, coverage, and calibration before routing live traffic through Reflex Control.
