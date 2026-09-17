use reflex_core::{
    DecisionRequest, DeterministicEvidence, EvidenceVector, Observation, ReflexAction, RiskLevel,
    SemanticEvidence, SIGNAL_OBJECTIVE_SATISFIED, SIGNAL_SECURITY_RISK,
};
use reflex_policy::composer::{DecisionComposer, GuardedHybridComposer};
use reflex_policy::risk_defer::RiskAbstentionPolicy;
use reflex_policy::PolicyConfig;
use reflex_provider::{DecisionProvider, MockProvider};
use reflex_telemetry::TelemetryStore;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Reflex Control: Verifier Gate Example ===");

    let _store = TelemetryStore::open_in_memory()?;
    let provider: Arc<dyn DecisionProvider> = Arc::new(
        MockProvider::new()
            .with_fixed_probability(0.96)
            .with_latency(8)
            .with_cost(0.0001),
    );

    let policy_config = PolicyConfig::default();
    let policy = policy_config.build_policy();

    // ─────────────────────────────────────────────────────────────────────────
    // Part 1: Observation Policy Gate
    // ─────────────────────────────────────────────────────────────────────────
    println!("\n--- Part 1: Policy Gate on Raw Observations ---");

    // Case 1: Low-risk routine documentation fix
    let routine_task = Observation::new("Fix typos in documentation")
        .with_task_id("task-doc-1")
        .with_risk(RiskLevel::Low);

    let req1 = DecisionRequest::probability(routine_task.clone());
    let resp1 = provider.evaluate(&req1).await?;
    let action1 = policy.decide(&resp1, &routine_task);

    println!("Task 1:     {}", routine_task.context);
    println!("Risk:       {}", routine_task.risk_level);
    println!("Confidence: {:.2}", resp1.decision.confidence);
    println!("Action:     {action1}");
    assert_eq!(action1, ReflexAction::Accept);
    println!(
        "-> Action accepted directly without calling frontier verifier! (Saved $0.02, 1800ms)"
    );

    // Case 2: High-risk database migration with high confidence
    let critical_task = Observation::new("Execute DROP COLUMN users.auth_token migration")
        .with_task_id("task-db-2")
        .with_risk(RiskLevel::Critical);

    let req2 = DecisionRequest::probability(critical_task.clone());
    let resp2 = provider.evaluate(&req2).await?;
    let action2 = policy.decide(&resp2, &critical_task);

    println!("\nTask 2:     {}", critical_task.context);
    println!("Risk:       {}", critical_task.risk_level);
    println!("Confidence: {:.2}", resp2.decision.confidence);
    println!("Action:     {action2}");
    assert_eq!(action2, ReflexAction::Escalate);
    println!("-> Safety rule enforced: High/Critical risk cannot bypass mandatory verification, despite 0.96 confidence!");

    // ─────────────────────────────────────────────────────────────────────────
    // Part 2: Guarded Hybrid Architecture with Atomic Evidence
    // ─────────────────────────────────────────────────────────────────────────
    println!("\n--- Part 2: Guarded Hybrid Architecture (Candidate E) ---");
    let hybrid_composer = GuardedHybridComposer::default();
    let risk_abstention = RiskAbstentionPolicy::default();

    // Case 3: Clean verified execution with CI passing and positive signals
    let clean_evidence = EvidenceVector::new(DeterministicEvidence {
        tests_passed: Some(true),
        ci_passed: Some(true),
        exit_code: Some(0),
        files_changed: 1,
        unexpected_files_changed: false,
        security_sensitive_files_changed: false,
        worker_completed: true,
        ..Default::default()
    })
    .with_semantic(SemanticEvidence::new(
        SIGNAL_OBJECTIVE_SATISFIED,
        0.92,
        "atomic",
        15,
    ))
    .with_semantic(SemanticEvidence::new(
        SIGNAL_SECURITY_RISK,
        0.03,
        "atomic",
        15,
    ));

    let clean_decision = hybrid_composer.compose(&clean_evidence);
    let (final_clean_action, clean_risk) =
        risk_abstention.evaluate(&clean_decision, &clean_evidence, RiskLevel::Low);

    println!(
        "Clean Task Composite Quality: {:.2}",
        clean_decision.confidence
    );
    println!("Risk Score:                   {clean_risk:.2}");
    println!("Effective Action:             {final_clean_action}");
    assert!(final_clean_action.is_autonomous_pass());
    println!("-> Clean execution passed autonomously with 0 frontier cost!");

    // Case 4: Subtle security defect that PASSED tests (e.g. leaked API credentials)
    let defect_evidence = EvidenceVector::new(DeterministicEvidence {
        tests_passed: Some(true), // Tests passed!
        ci_passed: Some(true),
        exit_code: Some(0),
        files_changed: 2,
        unexpected_files_changed: false,
        security_sensitive_files_changed: true, // Touched auth secrets!
        worker_completed: true,
        ..Default::default()
    })
    .with_semantic(SemanticEvidence::new(
        SIGNAL_OBJECTIVE_SATISFIED,
        0.95,
        "atomic",
        15,
    ))
    .with_semantic(SemanticEvidence::new(
        SIGNAL_SECURITY_RISK,
        0.85,
        "atomic",
        15,
    )); // High security risk detected by Jev!

    let defect_decision = hybrid_composer.compose(&defect_evidence);
    let (final_defect_action, defect_risk) =
        risk_abstention.evaluate(&defect_decision, &defect_evidence, RiskLevel::High);

    println!(
        "\nDefect Task Quality:          {:.2}",
        defect_decision.confidence
    );
    println!("Risk Score:                   {defect_risk:.2}");
    println!("Effective Action:             {final_defect_action}");
    assert_eq!(final_defect_action, ReflexAction::DeferToFrontier);
    println!(
        "-> Inviolable Safety Veto: Security hazard intercepted and escalated despite green tests!"
    );

    println!("\n=== All Verifier Gate examples completed successfully. ===");
    Ok(())
}
