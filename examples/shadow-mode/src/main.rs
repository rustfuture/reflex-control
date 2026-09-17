use chrono::Utc;
use reflex_core::{DecisionRequest, Observation, Outcome, RiskLevel};
use reflex_policy::PolicyConfig;
use reflex_provider::{DecisionProvider, MockProvider};
use reflex_telemetry::{ShadowRecord, TelemetryStore};
use std::sync::Arc;
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Reflex Control: Shadow Mode Example ===");

    let store = TelemetryStore::open_in_memory()?;
    let provider: Arc<dyn DecisionProvider> =
        Arc::new(MockProvider::new().with_latency(5).with_cost(0.0001));
    let policy_config = PolicyConfig::default();
    let policy = policy_config.build_policy();

    // Imagine an existing orchestrator that always routes to a frontier model
    let orchestrator_action = "frontier_verify";
    let task_id = "agent-step-402";

    let obs = Observation::new("Refactor authentication session token cookie handler")
        .with_task_id(task_id)
        .with_risk(RiskLevel::Medium);

    let req = DecisionRequest::probability(obs.clone());
    let resp = provider.evaluate(&req).await?;
    let predicted_action = policy.decide(&resp, &obs);

    let record = ShadowRecord {
        id: Uuid::new_v4().to_string(),
        task_id: Some(task_id.to_string()),
        timestamp: Utc::now(),
        predicted_action: predicted_action.clone(),
        confidence: resp.decision.confidence,
        actual_action: orchestrator_action.to_string(),
        verifier_result: Some("Pass: Authentication cookie handler parsed cleanly".to_string()),
        ci_outcome: Some(Outcome::Success),
        final_outcome: Some(Outcome::Success),
        latency_ms: resp.latency_ms,
        cost_estimate: resp.cost_estimate,
    };

    store.record_shadow(&record)?;

    println!("Orchestrator Action: {}", orchestrator_action);
    println!("Reflex Shadow Pred:  {}", predicted_action);
    println!("Confidence:          {:.4}", resp.decision.confidence);
    println!("Reflex Latency:      {} ms", resp.latency_ms);
    println!("Reflex Cost:         ${:.6}", resp.cost_estimate);
    println!("Outcome:             Success");
    println!("Shadow evaluation recorded safely without altering orchestrator flow.");

    Ok(())
}
