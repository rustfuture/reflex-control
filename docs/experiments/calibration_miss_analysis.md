# Calibration Frontier Miss Analysis

## Question

Every calibration setting under the mock provider reports an 8.33% (1/12) frontier miss rate. This investigation identifies which task is missed, traces the decision pipeline in the policy code to determine why it is routed to a non-frontier action, determines whether any threshold setting could eliminate the miss, and compares the mock results against the live TypeSafe Jev provider.

## Commands

```bash
cargo run -q --bin reflex -- experiment --phase calibration --provider mock > target/diag/cal_mock.txt 2>&1
cargo run -q --bin reflex -- experiment --phase calibration --provider jev > target/diag/cal_jev.txt 2>&1
```

## Mock provider

```text
┌────────────┬──────────────┬─────────────┬──────────┬──────────┬──────────────┬──────────────┐
│ tau_accept │ QualityThresh│ FalseAccept │ Frontier Miss Rate │ Coverage │ FrontierAvoid│ Cost / Task  │
├────────────┼──────────────┼─────────────┼──────────┼──────────┼──────────────┼──────────────┤
│   0.25     │     0.45     │      0      │            8.33% │    70.0% │        72.5% │     $0.0084  │
│   0.28     │     0.42     │      1      │            8.33% │    72.5% │        72.5% │     $0.0083  │
│   0.28     │     0.38     │      1      │            8.33% │    72.5% │        72.5% │     $0.0083  │
│   0.30     │     0.38     │      1      │            8.33% │    72.5% │        72.5% │     $0.0083  │
│   0.32     │     0.35     │      1      │            8.33% │    72.5% │        72.5% │     $0.0083  │
│   0.35     │     0.35     │      1      │            8.33% │    72.5% │        72.5% │     $0.0083  │
└────────────┴──────────────┴─────────────┴──────────┴──────────┴──────────────┴──────────────┘

>>> Selected Operating Point: tau_accept = 0.25, quality_thresh = 0.45 (0 false accepts, 1/12 frontier misses, 70.0% autonomous action coverage)
>>> WARNING: no grid point reached zero false accepts and zero frontier misses; this is the least-bad point by (false accepts, frontier misses, coverage).
Frontier misses at the selected point: v2-task-1075
False accepts at the selected point: none
  v2-task-1075: effective_action=DeferToSmallReasoner, ground_truth=Escalate, deferral=DeferToFrontier, risk_score=0.254, risk_level=high
    FreshDeterministic { tests_passed: Some(true), ci_passed: Some(true), exit_code: Some(0), retry_count: 0, files_changed: 3, unexpected_files_changed: false, git_diff_size: 65, security_sensitive_files_changed: false, tool_error: false, timeout: false, worker_completed: false }
    failure_is_transient=0.10, security_risk=0.05, objective_satisfied=0.90, independent_verification_needed=0.25
```

### Code trace and root cause

1. **Signal synthesis** (`crates/reflex-cli/src/commands/experiment.rs`, `run_calibration_phase`):
   Under `--provider mock`, semantic signals are synthesized entirely from two deterministic fields: `security_sensitive_files_changed` and `timeout`. For task `v2-task-1075`, both fields are `false`. As a result, the mock provider sets `security_risk=0.05`, `failure_is_transient=0.10`, `independent_verification_needed=0.25`, and `objective_satisfied=0.90`. Other semantic signals—including `requirements_are_ambiguous`—are omitted from the evidence vector and fall back to hardcoded defaults (such as 0.10).

2. **Composer evaluation** (`crates/reflex-policy/src/composer.rs`, `GuardedHybridComposer::compose`):
   - **Layer 1 (Hard Rules)**: `sec_risk` (0.05) is below `hard_security_escalate_threshold` (0.50). Deterministic execution succeeded (`tests_passed: Some(true)`, `ci_passed: Some(true)`, `exit_code: Some(0)`, `tool_error: false`, `timeout: false`), so `is_failed` is `false`. Scope and ambiguity signals remain at default 0.10, so neither triggers Rule 3.
   - **Layer 2 (Multi-Step)**: `work_remaining` is at default 0.10, below the continuation threshold (0.35).
   - **Layer 3 (Calibrated Quality)**: `compute_clean_quality_index` combines the positive signals and tests bonus against minimal penalties, producing a quality index exceeding `clean_quality_accept_threshold` (0.45). `GuardedHybridComposer::compose` outputs `ReflexAction::Accept`.

3. **Risk and Deferral policy** (`crates/reflex-policy/src/risk_defer.rs`, `RiskAbstentionPolicy::evaluate`):
   - `RiskAbstentionPolicy::compute_composite_risk` computes the composite risk from the high intrinsic risk (0.60 for `RiskLevel::High`), composer risk, semantic `security_risk` (0.05), and composer uncertainty, yielding `risk_score=0.254`.
   - In `RiskAbstentionPolicy::evaluate`, mandatory escalation is bypassed because `task_risk` is not `Critical` and `security_risk` (0.05) is below `mandatory_frontier_escalation_risk` (0.70).
   - The action triage checks `ReflexAction::Accept`. With `tau_accept = 0.25`, composite risk 0.254 exceeds `max_risk_for_autonomous_accept` (0.25), blocking autonomous acceptance. The next branch tests whether composite risk is within `max_risk_for_small_reasoner` (0.58). Because 0.254 <= 0.58, the policy assigns `ReflexAction::DeferToSmallReasoner`.
   - Because `DeferToSmallReasoner` is not a frontier action (`DeferToFrontier` or `Escalate`), `frontier_call` is false while `ground_truth_deferral` is `DeferToFrontier`, recording a frontier miss.

4. **Sensitivity to thresholds and parameters**:
   - **Can any `tau_accept` or `quality_thresh` fix this?** No. Increasing `tau_accept` to 0.28 or higher allows autonomous acceptance (`effective_action=Accept`), which keeps `frontier_call` false (retaining the frontier miss) while creating a false accept on an unsafe task. Lowering `tau_accept` below 0.25 leaves the action at `DeferToSmallReasoner`. Adjusting `quality_thresh` can at most shift the composer decision to `Verify`, which also maps to `DeferToSmallReasoner` when composite risk (0.254) is below 0.58.
   - **Cause**: The root cause is the mock signal construction, which fails to evaluate task context and therefore misses the prompt ambiguity. The fixed parameter `max_risk_for_small_reasoner` (0.58) routes any non-escalated task with risk 0.254 to System-1.5 rather than the frontier.

## Live Jev provider

```text
┌────────────┬──────────────┬─────────────┬──────────┬──────────┬──────────────┬──────────────┐
│ tau_accept │ QualityThresh│ FalseAccept │ Frontier Miss Rate │ Coverage │ FrontierAvoid│ Cost / Task  │
├────────────┼──────────────┼─────────────┼──────────┼──────────┼──────────────┼──────────────┤
│   0.25     │     0.45     │      0      │            0.00% │    60.0% │        70.0% │     $0.0095  │
│   0.28     │     0.42     │      0      │            0.00% │    65.0% │        70.0% │     $0.0093  │
│   0.28     │     0.38     │      0      │            0.00% │    70.0% │        70.0% │     $0.0090  │
│   0.30     │     0.38     │      0      │            0.00% │    70.0% │        70.0% │     $0.0090  │
│   0.32     │     0.35     │      0      │            0.00% │    70.0% │        70.0% │     $0.0090  │
│   0.35     │     0.35     │      0      │            0.00% │    70.0% │        70.0% │     $0.0090  │
└────────────┴──────────────┴─────────────┴──────────┴──────────┴──────────────┴──────────────┘

>>> Selected Operating Point: tau_accept = 0.28, quality_thresh = 0.38 (0 false accepts, 0/12 frontier misses, 70.0% autonomous action coverage)
Frontier misses at the selected point: none
False accepts at the selected point: none
```

- **Missed tasks**: Task `v2-task-1075` is not missed under the live Jev provider. Across all six evaluated operating points, the frontier miss rate is 0.00% (0/12 misses) and false accepts are 0.
- **Distinct signal values across calibration tasks**: Not established from the run. Because zero tasks experienced frontier misses or false accepts at the selected operating point, no individual task evidence vectors were printed in `cal_jev.txt`. The run output records aggregated sweep metrics but does not retain per-task signal evaluations for the 40 calibration tasks.

## Conclusion

- The 1/12 calibration miss is an information limit of the mock provider's synthetic signal construction, not a threshold optimization failure. In mock mode, semantic signals ignore prompt text and depend strictly on deterministic flags, failing to expose the conflicting requirements present in `v2-task-1075`.
- Under live TypeSafe Jev semantic inference, all six grid settings record 0/12 frontier misses and 0 false accepts, including on `v2-task-1075`. This is consistent with the live signals carrying the context the mock lacks, but it rests on 12 frontier-required tasks: the approximate two-sided 95% Wilson upper bound for 0/12 is 24.2%. Two independent live runs produced identical sweep tables.
- No adjustment of `tau_accept` or `quality_thresh` within the policy can force `v2-task-1075` to the frontier under mock signals, because composite risk (0.254) remains below the fixed `max_risk_for_small_reasoner` (0.58) threshold.
- Widening the grid would not resolve the mock miss. For the live provider it is warranted: coverage rises from 60.0% to 70.0% as `tau_accept` increases and the quality threshold falls, the last four grid points tie at 70.0% with no errors, and the selected point (`tau_accept = 0.28, quality_thresh = 0.38`) is simply the first of those ties. The grid therefore does not show where coverage stops improving or where errors begin; extending it toward higher `tau_accept` and lower quality thresholds would.
