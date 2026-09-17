use chrono::Utc;
use rand::Rng;
use reflex_calibration::{CalibrationCurve, CalibrationMetrics, DecisionOutcomePair};
use reflex_core::{DecisionId, DecisionRequest, Observation, Outcome, OutcomeSource, RiskLevel};
use reflex_policy::PolicyConfig;
use reflex_provider::{DecisionProvider, MockProvider};
use reflex_telemetry::{DecisionRecord, OutcomeRecord, ShadowRecord, TelemetryStore};
use serde::Deserialize;
use std::fs;
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct VerifiedAgentTask {
    pub task_id: String,
    #[serde(default)]
    pub category: String,
    pub context: String,
    pub risk_level: String,
    pub confidence: f64,
    pub verifier_result: String,
    pub ci_outcome: String,
    #[serde(default = "default_orchestrator_action")]
    pub actual_orchestrator_action: String,
    #[serde(default = "default_latency")]
    pub latency_ms: u64,
    #[serde(default = "default_cost")]
    pub cost_estimate: f64,
}

fn default_orchestrator_action() -> String {
    "frontier_verify".to_string()
}

fn default_latency() -> u64 {
    10
}

fn default_cost() -> f64 {
    0.0001
}

pub async fn run_shadow(
    dataset_path: Option<String>,
    count: usize,
    db_path: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let store = TelemetryStore::open(&db_path)?;
    let provider: Arc<dyn DecisionProvider> =
        Arc::new(MockProvider::new().with_latency(0).with_cost(0.0001));
    let policy_config = PolicyConfig::default();
    let policy = policy_config.build_policy();

    let target_dataset = dataset_path.unwrap_or_else(|| {
        if Path::new("fixtures/real_agent_worker_tasks.json").exists() {
            "fixtures/real_agent_worker_tasks.json".to_string()
        } else {
            String::new()
        }
    });

    if !target_dataset.is_empty() && Path::new(&target_dataset).exists() {
        println!(
            "Running Shadow Mode on verified agent task dataset: {}",
            target_dataset
        );
        let raw = fs::read_to_string(&target_dataset)?;
        let tasks: Vec<VerifiedAgentTask> = serde_json::from_str(&raw)?;

        let limit = if count > 0 && count < tasks.len() {
            count
        } else {
            tasks.len()
        };

        for task in tasks.into_iter().take(limit) {
            let risk = RiskLevel::from_str(&task.risk_level).unwrap_or(RiskLevel::Low);
            let outcome = match task.ci_outcome.to_lowercase().as_str() {
                "success" | "pass" => Outcome::Success,
                _ => Outcome::Failure,
            };

            let obs = Observation::new(&task.context)
                .with_task_id(&task.task_id)
                .with_risk(risk);

            let req = DecisionRequest::probability(obs.clone());
            let mut resp = provider.evaluate(&req).await?;
            resp.decision.confidence = task.confidence;

            let predicted_action = policy.decide(&resp, &obs);

            let record = ShadowRecord {
                id: Uuid::new_v4().to_string(),
                task_id: Some(task.task_id.clone()),
                timestamp: Utc::now(),
                predicted_action: predicted_action.clone(),
                confidence: resp.decision.confidence,
                actual_action: task.actual_orchestrator_action.clone(),
                verifier_result: Some(task.verifier_result.clone()),
                ci_outcome: Some(outcome),
                final_outcome: Some(outcome),
                latency_ms: task.latency_ms,
                cost_estimate: task.cost_estimate,
            };
            store.record_shadow(&record)?;

            // Also record to decisions & outcomes for calibration integration
            let dec_id = DecisionId::generate();
            let dec_record = DecisionRecord {
                id: dec_id.clone(),
                timestamp: Utc::now(),
                provider: "reflex-system1".to_string(),
                decision_type: "probability".to_string(),
                context: task.context,
                task_id: Some(task.task_id),
                selected: resp.decision.selected,
                confidence: task.confidence,
                probabilities_json: "[]".to_string(),
                action: predicted_action,
                risk_level: risk,
                latency_ms: task.latency_ms,
                cost_estimate: task.cost_estimate,
            };
            let _ = store.record_decision(&dec_record);

            let out_record = OutcomeRecord {
                decision_id: dec_id,
                result: outcome,
                source: OutcomeSource::Ci,
                verified_at: Utc::now(),
                details: Some(task.verifier_result),
            };
            let _ = store.record_outcome(&out_record);
        }

        println!(
            "Successfully evaluated and recorded {} real agent worker tasks to shadow telemetry!",
            limit
        );
        println!("Run 'reflex shadow report' to inspect the verified metrics.");
        return Ok(());
    }

    // Fallback simulation if no real dataset file is provided
    println!(
        "No verified dataset file found. Running Shadow Mode on {} synthetic tasks...",
        count
    );
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
        } else if rng.gen_bool(0.90) {
            Outcome::Success
        } else {
            Outcome::Failure
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
            verifier_result: Some(format!("Simulated verifier result for item #{i}")),
            ci_outcome: Some(final_outcome),
            final_outcome: Some(final_outcome),
            latency_ms: 10,
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
    let mut pairs = Vec::new();
    let mut agreements = 0;
    let mut reflex_accepted = 0;
    let mut reflex_verified = 0;
    let mut reflex_escalated = 0;
    let mut orchestrator_accepted = 0;
    let mut reflex_total_cost = 0.0;
    let orchestrator_cost_per_call = 0.02; // standard frontier LLM verifier call
    let mut reflex_total_latency = 0u64;
    let orchestrator_latency_avg_ms = 1850;

    let mut actual_defects = 0usize;
    let mut false_accepts = 0usize;
    let mut false_negatives = 0usize;

    for r in &records {
        let actual_is_accept = r.actual_action.to_lowercase() == "accept";
        let pred_is_accept = r.predicted_action.is_accept();

        if actual_is_accept == pred_is_accept {
            agreements += 1;
        }

        match r.predicted_action {
            reflex_core::ReflexAction::Accept => {
                reflex_accepted += 1;
                reflex_total_cost += 0.0001; // System-1 reflex cost
            }
            reflex_core::ReflexAction::Verify => {
                reflex_verified += 1;
                reflex_total_cost += 0.0001 + 0.005; // System-1 + fast verifier
            }
            _ => {
                reflex_escalated += 1;
                reflex_total_cost += 0.0001 + 0.02; // System-1 + frontier verifier
            }
        }

        if actual_is_accept {
            orchestrator_accepted += 1;
        }

        reflex_total_latency += r.latency_ms;

        if let Some(outcome) = r.final_outcome {
            if !outcome.is_success() {
                actual_defects += 1;
            }
            if pred_is_accept && !outcome.is_success() {
                false_accepts += 1;
                false_negatives += 1;
            }
            pairs.push(DecisionOutcomePair::new(
                r.confidence,
                r.predicted_action.clone(),
                outcome,
            ));
        }
    }

    let default_threshold = 0.90;
    let metrics = CalibrationMetrics::compute(&pairs, default_threshold);
    let curve = CalibrationCurve::build(&pairs, 5);

    let orchestrator_total_cost = total as f64 * orchestrator_cost_per_call;
    let reflex_avg_latency = reflex_total_latency as f64 / total as f64;
    let cost_savings =
        ((orchestrator_total_cost - reflex_total_cost) / orchestrator_total_cost) * 100.0;
    let latency_reduction = ((orchestrator_latency_avg_ms as f64 - reflex_avg_latency)
        / orchestrator_latency_avg_ms as f64)
        * 100.0;

    let calls_avoided = total.saturating_sub(reflex_escalated);
    let calls_avoided_pct = (calls_avoided as f64 / total as f64) * 100.0;

    let false_accept_rate = if reflex_accepted > 0 {
        (false_accepts as f64 / reflex_accepted as f64) * 100.0
    } else {
        0.0
    };

    let false_negative_rate = if actual_defects > 0 {
        (false_negatives as f64 / actual_defects as f64) * 100.0
    } else {
        0.0
    };

    println!("================= Reflex Shadow Mode Verification Report =================");
    println!("Evaluated Agent Tasks:      {}", total);
    println!(
        "Agreement with Orchestrator: {:.1}% (Orchestrator Accepts: {})",
        (agreements as f64 / total as f64) * 100.0,
        orchestrator_accepted
    );
    println!();
    println!("--- Decision Breakdown ---");
    println!(
        "  Autonomous Accepted:      {:>4} ({:5.1}%)",
        reflex_accepted,
        (reflex_accepted as f64 / total as f64) * 100.0
    );
    println!(
        "  Cheap Verified:           {:>4} ({:5.1}%)",
        reflex_verified,
        (reflex_verified as f64 / total as f64) * 100.0
    );
    println!(
        "  Frontier Escalated:       {:>4} ({:5.1}%)",
        reflex_escalated,
        (reflex_escalated as f64 / total as f64) * 100.0
    );
    println!();
    println!("--- Verification & Reliability Metrics ---");
    println!("  Total Ground Truth Defects: {}", actual_defects);
    println!("  False Accepts (Accepted Defect): {}", false_accepts);
    println!("  False Accept Rate (FAR):    {:5.2}%", false_accept_rate);
    println!("  False Negative Rate (FNR):  {:5.2}%", false_negative_rate);
    println!(
        "  Brier Score:                {:.4}  (lower is better)",
        metrics.brier_score
    );
    println!("  Expected Calib. Error (ECE): {:.4}", metrics.ece);
    println!(
        "  Automation Coverage:        {:5.1}%",
        (reflex_accepted as f64 / total as f64) * 100.0
    );
    println!(
        "  Frontier Calls Avoided:     {:>4} / {} ({:5.1}%)",
        calls_avoided, total, calls_avoided_pct
    );
    println!();
    println!("--- Economic & Latency Impact ---");
    println!(
        "  Baseline Cost (All Frontier): ${:.4}",
        orchestrator_total_cost
    );
    println!("  Reflex Control System-1 Cost: ${:.4}", reflex_total_cost);
    println!("  Cost Reduction:              {:5.1}%", cost_savings);
    println!(
        "  Median/Avg Latency Reduction: {:5.1}% ({}ms -> {:.1}ms)",
        latency_reduction, orchestrator_latency_avg_ms, reflex_avg_latency
    );

    println!("\n--- Calibration View (Confidence Buckets vs Verified Outcomes) ---");
    println!("Bucket Range   Count   Mean Conf   Observed Success   Calib Gap");
    println!("─────────────────────────────────────────────────────────────────");
    for b in &curve.buckets {
        println!(
            "{:.2} - {:.2}     {:5}     {:7.1}%        {:7.1}%         {:.4}",
            b.min_conf,
            b.max_conf,
            b.count,
            b.mean_confidence * 100.0,
            b.observed_success_rate * 100.0,
            b.calibration_gap
        );
    }
    println!("==========================================================================");

    Ok(())
}
