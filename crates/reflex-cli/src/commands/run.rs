use chrono::Utc;
use reflex_core::{DecisionRequest, DecisionType, Observation, RiskLevel};
use reflex_jev::{JevConfig, JevProvider};
use reflex_policy::PolicyConfig;
use reflex_provider::{DecisionProvider, MockProvider};
use reflex_telemetry::{DecisionRecord, TelemetryStore};
use std::str::FromStr;
use std::sync::Arc;
use uuid::Uuid;

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

    let is_live_jev = provider_name.to_lowercase() == "jev";

    let (provider_label, provider_tag, session_prefix): (&str, &str, &str) = if is_live_jev {
        ("Jev (LIVE API)", "jev-live", "session-live-jev")
    } else {
        ("Mock/Synthetic", "mock-synthetic", "session-synthetic")
    };

    println!("================ Reflex Control Decision Execution ================");
    println!("Provider:       {provider_label}");

    let provider: Arc<dyn DecisionProvider> = if is_live_jev {
        let config = JevConfig::from_env()?;
        println!("Endpoint:       {}", config.endpoint);
        println!("Model:          {}", config.model);
        Arc::new(JevProvider::new(config))
    } else {
        Arc::new(MockProvider::new())
    };

    let session_id = format!("{}-{}", session_prefix, Uuid::new_v4());
    let effective_task_id = task_id.unwrap_or_else(|| session_id.clone());

    let obs = Observation::new(&context)
        .with_risk(risk_level)
        .with_task_id(&effective_task_id);

    let req = if let Some(opts) = options {
        DecisionRequest::new(DecisionType::Choice, obs.clone(), opts)
    } else {
        DecisionRequest::probability(obs.clone())
    };

    println!("Task ID:        {effective_task_id}");
    println!("Risk Level:     {}", obs.risk_level);
    println!("Evaluating decision...");

    let resp = provider.evaluate(&req).await?;

    let policy_config = PolicyConfig::default();
    let policy = policy_config.build_policy();
    let action = policy.decide(&resp, &obs);

    println!("\n=== Decision Result ===");
    println!("Decision ID:    {}", resp.request_id);
    println!("Provider:       {provider_label}");
    println!("Selected:       {}", resp.decision.selected);
    println!("Confidence:     {:.4}", resp.decision.confidence);
    println!("Probabilities:  {:?}", resp.decision.probabilities);
    println!("Policy Action:  {action}");
    println!("Latency:        {} ms", resp.latency_ms);
    if resp.cost_estimate > 0.0 {
        println!("Estimated Cost: ${:.6}", resp.cost_estimate);
    } else {
        println!("Estimated Cost: unknown (TypeSafe Jev token pricing not officially published)");
    }

    let rec = DecisionRecord {
        id: resp.request_id.clone(),
        timestamp: Utc::now(),
        provider: provider_tag.to_string(),
        decision_type: format!("{:?}", req.decision_type),
        context: obs.context.clone(),
        task_id: Some(effective_task_id),
        selected: resp.decision.selected.clone(),
        confidence: resp.decision.confidence,
        probabilities_json: serde_json::to_string(&resp.decision.probabilities)?,
        action,
        risk_level: obs.risk_level,
        latency_ms: resp.latency_ms,
        cost_estimate: resp.cost_estimate,
    };

    store.record_decision(&rec)?;
    println!("\nTelemetry Record Stored: {db_path} (session: {session_id})");
    println!("====================================================================");

    Ok(())
}
