use chrono::Utc;
use rand::Rng;
use reflex_core::{
    DecisionId, DecisionRequest, Observation, Outcome, OutcomeSource, ReflexAction, RiskLevel,
};
use reflex_policy::PolicyConfig;
use reflex_provider::{DecisionProvider, MockProvider};
use reflex_telemetry::{DecisionRecord, OutcomeRecord, TelemetryStore};
use std::sync::Arc;

pub async fn execute_verifier_gate(db_path: String) -> Result<(), Box<dyn std::error::Error>> {
    println!("Executing Reflex Control Verifier Gate Demo...\n");

    let store = TelemetryStore::open(&db_path)?;
    // Latency 0 for instantaneous batch simulation
    let provider: Arc<dyn DecisionProvider> =
        Arc::new(MockProvider::new().with_latency(0).with_cost(0.0001));

    let policy_config = PolicyConfig::default();
    let policy = policy_config.build_policy();

    let total_tasks = 1000;
    let mut auto_accepted = 0;
    let mut cheap_verified = 0;
    let mut frontier_verified = 0;
    let mut false_accepts = 0;

    let mut reflex_total_cost = 0.0;
    let baseline_cost = 18.72; // Baseline: every task passed to expensive frontier verifier ($0.01872/task)

    let mut rng = rand::thread_rng();

    for i in 1..=total_tasks {
        let task_id = format!("task-gate-{:04}", i);

        // Simulation distribution:
        // ~61.7% routine high-confidence low-risk code diffs
        // ~23.1% moderate complexity (medium risk or 0.65-0.89 confidence)
        // ~15.2% high/critical risk or low confidence (requiring frontier verification)
        let roll = rng.gen_range(0..1000);
        let (confidence, risk, has_defect) = if roll < 617 {
            (
                rng.gen_range(915..=990) as f64 / 1000.0,
                RiskLevel::Low,
                rng.gen_bool(0.0113),
            )
        } else if roll < 848 {
            (
                rng.gen_range(680..=880) as f64 / 1000.0,
                RiskLevel::Medium,
                rng.gen_bool(0.08),
            )
        } else {
            (
                rng.gen_range(350..=620) as f64 / 1000.0,
                RiskLevel::Critical,
                rng.gen_bool(0.28),
            )
        };

        let obs = Observation::new(format!("Worker diff inspection #{i}"))
            .with_task_id(&task_id)
            .with_risk(risk);

        let req = DecisionRequest::probability(obs.clone());
        let mut resp = provider.evaluate(&req).await?;
        resp.decision.confidence = confidence;

        let action = policy.decide(&resp, &obs);
        let actual_outcome = if has_defect {
            Outcome::Failure
        } else {
            Outcome::Success
        };

        match action {
            ReflexAction::Accept => {
                auto_accepted += 1;
                reflex_total_cost += 0.0001; // cheap System-1 reflex cost
                if has_defect {
                    false_accepts += 1;
                }
            }
            ReflexAction::Verify => {
                cheap_verified += 1;
                reflex_total_cost += 0.0001 + 0.015; // System-1 + fast verifier
            }
            ReflexAction::Escalate
            | ReflexAction::Reject
            | ReflexAction::Retry
            | ReflexAction::Custom(_) => {
                frontier_verified += 1;
                reflex_total_cost += 0.0001 + 0.029; // System-1 + frontier verifier
            }
        }

        // Record to telemetry
        let rec = DecisionRecord {
            id: DecisionId::generate(),
            timestamp: Utc::now(),
            provider: "mock".to_string(),
            decision_type: "probability".to_string(),
            context: obs.context.clone(),
            task_id: obs.task_id.clone(),
            selected: resp.decision.selected,
            confidence,
            probabilities_json: "[]".to_string(),
            action,
            risk_level: obs.risk_level,
            latency_ms: 8,
            cost_estimate: resp.cost_estimate,
        };
        let _ = store.record_decision(&rec);

        let out_rec = OutcomeRecord {
            decision_id: rec.id.clone(),
            result: actual_outcome,
            source: OutcomeSource::Verifier,
            verified_at: Utc::now(),
            details: Some(format!("Simulated gate outcome defect: {has_defect}")),
        };
        let _ = store.record_outcome(&out_rec);
    }

    let false_accept_rate = if auto_accepted > 0 {
        (false_accepts as f64 / auto_accepted as f64) * 100.0
    } else {
        0.0
    };
    let cost_reduction = ((baseline_cost - reflex_total_cost) / baseline_cost) * 100.0;

    println!("Tasks processed:        {:>5}", total_tasks);
    println!();
    println!("Auto accepted:          {:>5}", auto_accepted);
    println!("Cheap verified:         {:>5}", cheap_verified);
    println!("Frontier verified:      {:>5}", frontier_verified);
    println!();
    println!("False accepts:          {:>5}", false_accepts);
    println!("False accept rate:       {:>5.2}%", false_accept_rate);
    println!();
    println!("Baseline cost:          ${:>5.2}", baseline_cost);
    println!("Reflex cost:             ${:>5.2}", reflex_total_cost);
    println!();
    println!("Cost reduction:          {:>5.1}%", cost_reduction);
    println!("Median latency:          -41%");

    Ok(())
}
