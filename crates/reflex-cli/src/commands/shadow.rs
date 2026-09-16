use chrono::Utc;
use rand::Rng;
use reflex_core::{DecisionRequest, Observation, Outcome, RiskLevel};
use reflex_policy::PolicyConfig;
use reflex_provider::{DecisionProvider, MockProvider};
use reflex_telemetry::{ShadowRecord, TelemetryStore};
use std::sync::Arc;
use uuid::Uuid;

pub async fn run_simulation(
    count: usize,
    db_path: String,
) -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "Running Shadow Mode on {} synthetic orchestrator tasks...",
        count
    );
    let store = TelemetryStore::open(&db_path)?;
    let provider: Arc<dyn DecisionProvider> =
        Arc::new(MockProvider::new().with_latency(0).with_cost(0.0001));
    let policy_config = PolicyConfig::default();
    let policy = policy_config.build_policy();

    let mut rng = rand::thread_rng();

    for i in 1..=count {
        let task_id = format!("task-shadow-{:04}", i);
        let is_routine = rng.gen_bool(0.70);
        let actual_action = if is_routine { "accept" } else { "verify" };
        let final_outcome = if is_routine {
            if rng.gen_bool(0.98) {
                Outcome::Success
            } else {
                Outcome::Failure
            }
        } else {
            if rng.gen_bool(0.90) {
                Outcome::Success
            } else {
                Outcome::Failure
            }
        };

        let risk = if rng.gen_bool(0.05) {
            RiskLevel::High
        } else {
            RiskLevel::Low
        };

        let obs = Observation::new(format!("Shadow task context for item #{i}"))
            .with_task_id(&task_id)
            .with_risk(risk);

        let req = DecisionRequest::probability(obs.clone());
        let resp = provider.evaluate(&req).await?;
        let predicted_action = policy.decide(&resp, &obs);

        let record = ShadowRecord {
            id: Uuid::new_v4().to_string(),
            task_id: Some(task_id),
            timestamp: Utc::now(),
            predicted_action,
            confidence: resp.decision.confidence,
            actual_action: actual_action.to_string(),
            final_outcome: Some(final_outcome),
            latency_ms: resp.latency_ms,
            cost_estimate: resp.cost_estimate,
        };

        store.record_shadow(&record)?;
    }

    println!(
        "Completed {} shadow mode evaluations and recorded to telemetry!",
        count
    );
    println!("Run 'reflex shadow report' to view shadow comparison metrics.");

    Ok(())
}

pub fn report(db_path: String) -> Result<(), Box<dyn std::error::Error>> {
    let store = TelemetryStore::open(&db_path)?;
    let records = store.list_shadow_records(5000)?;

    if records.is_empty() {
        println!("No shadow records found in {db_path}.");
        println!("Run 'reflex shadow run' first to generate shadow evaluations.");
        return Ok(());
    }

    let total = records.len();
    let mut agreements = 0;
    let mut reflex_accepted = 0;
    let mut orchestrator_accepted = 0;
    let mut reflex_total_cost = 0.0;
    let orchestrator_cost_per_call = 0.015; // standard frontier LLM call cost
    let mut reflex_total_latency = 0u64;
    let orchestrator_latency_avg_ms = 1850; // standard LLM latency

    for r in &records {
        let actual_is_accept = r.actual_action.to_lowercase() == "accept";
        let pred_is_accept = r.predicted_action.is_accept();

        if actual_is_accept == pred_is_accept {
            agreements += 1;
        }

        if pred_is_accept {
            reflex_accepted += 1;
        }
        if actual_is_accept {
            orchestrator_accepted += 1;
        }

        reflex_total_cost += r.cost_estimate;
        reflex_total_latency += r.latency_ms;
    }

    let agreement_rate = (agreements as f64 / total as f64) * 100.0;
    let orchestrator_total_cost = total as f64 * orchestrator_cost_per_call;
    let reflex_avg_latency = reflex_total_latency as f64 / total as f64;
    let cost_savings =
        ((orchestrator_total_cost - reflex_total_cost) / orchestrator_total_cost) * 100.0;
    let latency_reduction = ((orchestrator_latency_avg_ms as f64 - reflex_avg_latency)
        / orchestrator_latency_avg_ms as f64)
        * 100.0;

    println!("================= Reflex Shadow Mode Report =================");
    println!("Shadow Traces Evaluated:    {}", total);
    println!("Action Agreement Rate:      {:.1}%", agreement_rate);
    println!(
        "Reflex Autonomous Accepts:  {} ({:.1}%)",
        reflex_accepted,
        (reflex_accepted as f64 / total as f64) * 100.0
    );
    println!(
        "Orchestrator Accepts:       {} ({:.1}%)",
        orchestrator_accepted,
        (orchestrator_accepted as f64 / total as f64) * 100.0
    );

    println!("\n--- Performance & Economics Comparison ---");
    println!(
        "Orchestrator Est. Cost:     ${:.4} (all tasks routed to LLM)",
        orchestrator_total_cost
    );
    println!(
        "Reflex Control Cost:        ${:.4} (cheap System-1 routing)",
        reflex_total_cost
    );
    println!("Projected Cost Savings:     {:.1}%", cost_savings);
    println!(
        "Orchestrator Avg Latency:   {} ms",
        orchestrator_latency_avg_ms
    );
    println!("Reflex System-1 Latency:    {:.1} ms", reflex_avg_latency);
    println!("Decision Latency Reduction: {:.1}%", latency_reduction);
    println!("=============================================================");

    Ok(())
}
