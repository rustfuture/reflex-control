use reflex_core::{DecisionRequest, Observation, ReflexAction, RiskLevel};
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

    // Case 1: Low-risk routine documentation fix
    let routine_task = Observation::new("Fix typos in documentation")
        .with_task_id("task-doc-1")
        .with_risk(RiskLevel::Low);

    let req1 = DecisionRequest::probability(routine_task.clone());
    let resp1 = provider.evaluate(&req1).await?;
    let action1 = policy.decide(&resp1, &routine_task);

    println!("\nTask 1: {}", routine_task.context);
    println!("Risk:   {}", routine_task.risk_level);
    println!("Confidence: {:.2}", resp1.decision.confidence);
    println!("Action: {}", action1);
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

    println!("\nTask 2: {}", critical_task.context);
    println!("Risk:   {}", critical_task.risk_level);
    println!("Confidence: {:.2}", resp2.decision.confidence);
    println!("Action: {}", action2);
    assert_eq!(action2, ReflexAction::Escalate);
    println!("-> Safety rule enforced: High/Critical risk cannot bypass mandatory verification, despite 0.96 confidence!");

    println!("\nVerifier Gate example completed successfully.");
    Ok(())
}
