# Workspace Architecture & Execution Flow

This document details the workspace layout and decision flow in Reflex Control.

## Execution Flow Diagram

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
              │  Layer 1: Hard Safety Veto Gate     │
              │  (Critical risk / privilege bypass) │
              └──────────────┬───────────────┬──────┘
                   Veto Path │               │ Passed Safety
                             ▼               ▼
                 ┌──────────────────┐  ┌───────────────────────────────────┐
                 │ Mandatory Frontier│  │ Layer 2: Calibrated Decision Gate │
                 │ Reasoner Escalation│ │ (Accept, Retry, Small Reasoner)   │
                 └──────────────────┘  └───────────────────────────────────┘
```
