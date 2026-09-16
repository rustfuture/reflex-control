use chrono::Utc;
use reflex_core::{DecisionRequest, DecisionType, Observation, RiskLevel};
use reflex_jev::{JevConfig, JevProvider};
use reflex_policy::PolicyConfig;
use reflex_provider::{DecisionProvider, MockProvider};
use reflex_telemetry::{DecisionRecord, TelemetryStore};
use std::str::FromStr;
use std::sync::Arc;

pub async fn execute(
    provider_name: String,
    context: String,
    task_id: Option<String>,
    risk: String,
    options: Option<Vec<String>>,
    db_path: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let risk_level = RiskLevel::from_str(&risk).unwrap_or(RiskLevel::Low);
    let store = TelemetryStore::open(&db_path)?;

    let mut obs = Observation::new(&context).with_risk(risk_level);
    if let Some(ref tid) = task_id {
        obs = obs.with_task_id(tid);
    }

    let provider: Arc<dyn DecisionProvider> = if provider_name.to_lowercase() == "jev" {
        let config = JevConfig::default();
        Arc::new(JevProvider::new(config))
    } else {
        Arc::new(MockProvider::new())
    };

    let req = if let Some(opts) = options {
        DecisionRequest::new(DecisionType::Choice, obs.clone(), opts)
    } else {
        DecisionRequest::probability(obs.clone())
    };

    println!("Evaluating decision with provider: {}", provider.name());
    let resp = provider.evaluate(&req).await?;

    let policy_config = PolicyConfig::default();
    let policy = policy_config.build_policy();
    let action = policy.decide(&resp, &obs);

    println!("\n=== Decision Result ===");
    println!("Decision ID:    {}", resp.request_id);
    println!("Provider:       {}", resp.provider);
    println!("Selected:       {}", resp.decision.selected);
    println!("Confidence:     {:.4}", resp.decision.confidence);
    println!("Risk Level:     {}", obs.risk_level);
    println!("Policy Action:  {}", action);
    println!("Latency:        {} ms", resp.latency_ms);
    println!("Estimated Cost: ${:.6}", resp.cost_estimate);

    let rec = DecisionRecord {
        id: resp.request_id.clone(),
        timestamp: Utc::now(),
        provider: resp.provider.clone(),
        decision_type: format!("{:?}", req.decision_type),
        context: obs.context.clone(),
        task_id: obs.task_id.clone(),
        selected: resp.decision.selected.clone(),
        confidence: resp.decision.confidence,
        probabilities_json: serde_json::to_string(&resp.decision.probabilities)?,
        action,
        risk_level: obs.risk_level,
        latency_ms: resp.latency_ms,
        cost_estimate: resp.cost_estimate,
    };

    store.record_decision(&rec)?;
    println!("\nDecision recorded in telemetry: {}", db_path);

    Ok(())
}
