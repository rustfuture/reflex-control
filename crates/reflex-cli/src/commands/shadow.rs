use chrono::Utc;
use rand::Rng;
use reflex_calibration::{
    wilson_score_interval, CalibrationCurve, CalibrationMetrics, DecisionOutcomePair,
};
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
pub struct DatasetMetadata {
    pub source: String,
    #[serde(default)]
    pub is_production_telemetry: bool,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub sample_count: usize,
    #[serde(default)]
    pub defect_count: usize,
}

#[derive(Debug, Clone, Deserialize)]
struct DatasetWrapper {
    #[serde(default)]
    pub metadata: Option<DatasetMetadata>,
    pub tasks: Vec<VerifiedAgentTask>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct VerifiedAgentTask {
    pub task_id: String,
    #[serde(default = "default_decision_type")]
    pub decision_type: String,
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

fn default_decision_type() -> String {
    "verification".to_string()
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
        if Path::new("fixtures/benchmark_dataset_5000.json").exists() {
            "fixtures/benchmark_dataset_5000.json".to_string()
        } else if Path::new("fixtures/real_agent_worker_tasks.json").exists() {
            "fixtures/real_agent_worker_tasks.json".to_string()
        } else {
            String::new()
        }
    });

    if !target_dataset.is_empty() && Path::new(&target_dataset).exists() {
        println!("Running Shadow Mode on evaluation dataset: {target_dataset}");
        let raw = fs::read_to_string(&target_dataset)?;

        let (tasks, metadata): (Vec<VerifiedAgentTask>, Option<DatasetMetadata>) =
            if let Ok(wrapper) = serde_json::from_str::<DatasetWrapper>(&raw) {
                (wrapper.tasks, wrapper.metadata)
            } else {
                let parsed: Vec<VerifiedAgentTask> = serde_json::from_str(&raw)?;
                (parsed, None)
            };

        if let Some(meta) = metadata {
            println!(
                "Provenance Classification: {} (Synthetic Benchmark Fixture, NOT Production Telemetry)",
                meta.source
            );
        } else {
            println!(
                "Provenance Classification: Curated Benchmark Fixture [Hand-curated fixture, NOT production telemetry]"
            );
        }

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
                decision_type: task.decision_type.clone(),
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

        println!("Successfully evaluated and recorded {limit} agent tasks to shadow telemetry!");
        println!("Run 'reflex shadow report' to inspect the recorded metrics.");
        return Ok(());
    }

    // Fallback simulation if no dataset file is provided
    println!("No verified dataset file found. Running Shadow Mode on {count} synthetic tasks...");
    println!("Note: Simulation mode uses purely synthetic data (NOT production telemetry).");
    let mut rng = rand::thread_rng();

    for i in 1..=count {
        let task_id = format!("task-shadow-{i:04}");
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

    println!("Completed {count} shadow mode evaluations and recorded to telemetry!");
    println!("Run 'reflex shadow report' to view shadow comparison metrics.");

    Ok(())
}

pub fn report(db_path: String) -> Result<(), Box<dyn std::error::Error>> {
    let store = TelemetryStore::open(&db_path)?;
    let records = store.list_shadow_records(10000)?;

    if records.is_empty() {
        println!("No shadow records found in {db_path}.");
        println!("Run 'reflex shadow run' first to generate shadow evaluations.");
        return Ok(());
    }

    let total = records.len();
    let mut pairs = Vec::new();
    let mut agreements = 0;
    let mut reflex_accepted = 0;
    let mut reflex_verified_clean = 0;
    let mut reflex_verified_defect = 0;
    let mut reflex_escalated = 0;
    let mut orchestrator_accepted = 0;
    let mut reflex_total_cost = 0.0;
    let orchestrator_cost_per_call = 0.02; // standard frontier LLM verifier call
    let mut reflex_total_latency = 0u64;
    let orchestrator_latency_avg_ms = 1850;

    let mut resolved_outcomes = 0usize;
    let mut partial_outcomes = 0usize;
    let mut unknown_outcomes = 0usize;
    let mut missing_outcomes = 0usize;
    let mut reflex_accepted_resolved = 0usize;
    let mut reflex_verified_unresolved = 0usize;
    let mut frontier_status_known = 0usize;
    let mut actual_defects = 0usize;
    let mut false_accepts = 0usize;

    for r in &records {
        let actual_is_accept = r.actual_action.to_lowercase() == "accept";
        let pred_is_accept = r.predicted_action.is_accept();

        if actual_is_accept == pred_is_accept {
            agreements += 1;
        }

        match r.final_outcome {
            Some(outcome @ (reflex_core::Outcome::Success | reflex_core::Outcome::Failure)) => {
                resolved_outcomes += 1;
                if outcome.is_failure() {
                    actual_defects += 1;
                    if r.predicted_action.is_autonomous_pass() {
                        false_accepts += 1;
                    }
                }
            }
            Some(reflex_core::Outcome::Partial) => partial_outcomes += 1,
            Some(reflex_core::Outcome::Unknown) => unknown_outcomes += 1,
            None => missing_outcomes += 1,
        }

        match r.predicted_action {
            reflex_core::ReflexAction::Accept | reflex_core::ReflexAction::Terminate => {
                reflex_accepted += 1;
                reflex_total_cost += 0.0001; // System-1 reflex cost
                frontier_status_known += 1;
                if r.final_outcome.is_some_and(|o| o.is_resolved()) {
                    reflex_accepted_resolved += 1;
                }
            }
            reflex_core::ReflexAction::Verify => match r.final_outcome {
                Some(reflex_core::Outcome::Success) => {
                    reflex_verified_clean += 1;
                    frontier_status_known += 1;
                    reflex_total_cost += 0.0001 + 0.005;
                }
                Some(reflex_core::Outcome::Failure) => {
                    reflex_verified_defect += 1;
                    frontier_status_known += 1;
                    reflex_total_cost += 0.0001 + 0.005 + 0.02;
                }
                Some(reflex_core::Outcome::Partial | reflex_core::Outcome::Unknown) | None => {
                    reflex_verified_unresolved += 1;
                }
            },
            _ => {
                reflex_escalated += 1;
                frontier_status_known += 1;
                reflex_total_cost += 0.0001 + 0.02; // System-1 + frontier verifier
            }
        }

        if actual_is_accept {
            orchestrator_accepted += 1;
        }

        reflex_total_latency += r.latency_ms;

        if let Some(outcome) = r.final_outcome {
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

    let known_cost_tasks = total.saturating_sub(reflex_verified_unresolved);
    let orchestrator_total_cost = known_cost_tasks as f64 * orchestrator_cost_per_call;
    let reflex_avg_latency = reflex_total_latency as f64 / total as f64;
    let cost_savings =
        ((orchestrator_total_cost - reflex_total_cost) / orchestrator_total_cost) * 100.0;
    let latency_reduction = ((orchestrator_latency_avg_ms as f64 - reflex_avg_latency)
        / orchestrator_latency_avg_ms as f64)
        * 100.0;

    // Mathematically verified frontier calls avoided:
    // Avoided frontier calls = tasks autonomously accepted + tasks cheap verified clean without escalation.
    // Tasks that are directly escalated OR cheap verified with defect escalate to the frontier model.
    let frontier_calls_avoided = reflex_accepted + reflex_verified_clean;
    let frontier_avoided_ci =
        wilson_score_interval(frontier_calls_avoided, frontier_status_known, 0.95);
    let coverage_ci = wilson_score_interval(reflex_accepted, total, 0.95);
    let far_ci = wilson_score_interval(false_accepts, reflex_accepted_resolved, 0.95);
    let fnr_ci = wilson_score_interval(false_accepts, actual_defects, 0.95);

    println!("================= Reflex Shadow Mode Verification Report =================");
    println!("Evaluated Shadow Tasks:     {total}");
    println!("Resolved Outcomes:          {resolved_outcomes}");
    println!(
        "Partial / Unknown / Missing: {partial_outcomes} / {unknown_outcomes} / {missing_outcomes}"
    );
    println!("Telemetry Store:            {db_path}");
    println!("Data Nature:                Shadow Pipeline Telemetry (Fixture / Agent Runs)");
    println!(
        "Agreement with Orchestrator: {:.1}% (Orchestrator Accepts: {})",
        (agreements as f64 / total as f64) * 100.0,
        orchestrator_accepted
    );
    println!();
    println!("--- Decision Breakdown ---");
    println!(
        "  Autonomous Passes (all):  {:>5} ({:5.1}%)",
        reflex_accepted,
        (reflex_accepted as f64 / total as f64) * 100.0
    );
    println!(
        "  Cheap Verified (Clean):   {:>5} ({:5.1}%)",
        reflex_verified_clean,
        (reflex_verified_clean as f64 / total as f64) * 100.0
    );
    println!(
        "  Cheap Verified (Defect):  {:>5} ({:5.1}%) -> Escalated to Frontier",
        reflex_verified_defect,
        (reflex_verified_defect as f64 / total as f64) * 100.0
    );
    println!(
        "  Direct Frontier Escalated:{:>5} ({:5.1}%)",
        reflex_escalated,
        (reflex_escalated as f64 / total as f64) * 100.0
    );
    println!();
    println!("--- Verification & Reliability Metrics (95% Confidence Intervals) ---");
    println!("  Total Ground Truth Defects: {actual_defects}");
    println!("  Unresolved Verified Actions: {reflex_verified_unresolved}");
    println!("  FAR Numerator / Denominator: {false_accepts} / {reflex_accepted_resolved}");
    println!("  Observed False Accept Rate: {}", far_ci.format_pct());
    println!("  Observed False Negative Rate: {}", fnr_ci.format_pct());
    if metrics.resolved_samples > 0 {
        println!(
            "  Brier Score:                {:.4}  (lower is better)",
            metrics.brier_score
        );
        println!("  Expected Calib. Error (ECE): {:.4}", metrics.ece);
    } else {
        println!("  Brier Score:                N/A (n=0)");
        println!("  Expected Calib. Error (ECE): N/A (n=0)");
    }
    println!("  Automation Coverage:        {}", coverage_ci.format_pct());
    println!(
        "  Frontier Calls Avoided:     {}",
        frontier_avoided_ci.format_pct()
    );
    println!(
        "  All Verifier Calls Avoided: {:.2}% (Autonomous zero-verifier coverage)",
        (reflex_accepted as f64 / total as f64) * 100.0
    );
    println!();
    println!("--- Economic & Latency Impact ---");
    println!("  Baseline Cost (Resolved Cohort): ${orchestrator_total_cost:.4}");
    println!("  Reflex Control System-1 Cost: ${reflex_total_cost:.4}");
    if known_cost_tasks > 0 {
        println!("  Cost Reduction:              {cost_savings:5.1}% (resolved cost cohort n={known_cost_tasks})");
    } else {
        println!("  Cost Reduction:              N/A (no resolved cost cohort)");
    }
    println!(
        "  Median/Avg Latency Reduction: {latency_reduction:5.1}% ({orchestrator_latency_avg_ms}ms -> {reflex_avg_latency:.1}ms)"
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
