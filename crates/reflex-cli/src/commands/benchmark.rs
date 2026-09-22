use chrono::Utc;
use reflex_calibration::eval::{
    evaluate_retry, evaluate_routing, evaluate_termination, evaluate_verification_with_polarity,
    sweep_verification_thresholds_with_polarity, VerificationPolarity,
};
use reflex_core::{
    DecisionId, DecisionRequest, DecisionType, Observation, Outcome, OutcomeSource, RiskLevel,
};
use reflex_jev::{FormulationConfig, FormulationVariant, JevConfig, JevProvider};
use reflex_policy::PolicyConfig;
use reflex_provider::{DecisionProvider, MockProvider};
use reflex_telemetry::{DecisionRecord, OutcomeRecord, TelemetryStore};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Instant;
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize)]
struct DatasetWrapper {
    pub tasks: Vec<TaskItem>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct TaskItem {
    pub task_id: String,
    #[serde(default = "default_dec_type")]
    pub decision_type: String,
    #[serde(default)]
    pub category: String,
    pub risk_level: String,
    pub context: String,
    pub ci_outcome: String,
    #[serde(default)]
    pub verifier_result: String,
    #[serde(default)]
    pub ground_truth: Option<String>,
}

fn default_dec_type() -> String {
    "verification".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrozenConfiguration {
    pub routing: String,
    pub retry: String,
    pub termination: String,
    pub verification: String,
    pub verification_threshold: f64,
}

pub async fn execute(
    provider_name: String,
    dataset_path: Option<String>,
    task_limit: usize,
    formulation: String,
    threshold_override: Option<f64>,
    db_path: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let is_live_jev = provider_name.to_lowercase() == "jev";

    if formulation.to_lowercase() == "compare" {
        return run_formulation_comparison(provider_name, dataset_path, task_limit, db_path).await;
    }

    // Determine frozen configuration if requested
    let mut routing_var = FormulationVariant::A;
    let mut retry_var = FormulationVariant::A;
    let mut term_var = FormulationVariant::A;
    let mut verif_var = FormulationVariant::A;
    let mut verif_threshold = threshold_override.unwrap_or(0.80);

    if formulation.to_lowercase() == "b" {
        routing_var = FormulationVariant::B;
        retry_var = FormulationVariant::B;
        term_var = FormulationVariant::B;
        verif_var = FormulationVariant::B;
        if threshold_override.is_none() {
            verif_threshold = 0.50; // default for defect polarity
        }
    } else if formulation.to_lowercase() == "frozen" {
        let frozen_file = "fixtures/frozen_configuration.json";
        if Path::new(frozen_file).exists() {
            let data = fs::read_to_string(frozen_file)?;
            if let Ok(fc) = serde_json::from_str::<FrozenConfiguration>(&data) {
                println!("\n>>> Loaded Frozen Winning Configuration from {frozen_file}:");
                println!("    Routing:      Formulation {}", fc.routing);
                println!("    Retry:        Formulation {}", fc.retry);
                println!("    Termination:  Formulation {}", fc.termination);
                println!("    Verification: Formulation {}", fc.verification);
                println!("    Threshold:    {:.2}", fc.verification_threshold);
                println!();

                routing_var = parse_variant(&fc.routing);
                retry_var = parse_variant(&fc.retry);
                term_var = parse_variant(&fc.termination);
                verif_var = parse_variant(&fc.verification);
                if threshold_override.is_none() {
                    verif_threshold = fc.verification_threshold;
                }
            }
        } else {
            println!(
                "Note: No frozen configuration found at {frozen_file}, defaulting to variant B."
            );
            routing_var = FormulationVariant::B;
            retry_var = FormulationVariant::B;
            term_var = FormulationVariant::B;
            verif_var = FormulationVariant::B;
            if threshold_override.is_none() {
                verif_threshold = 0.50;
            }
        }
    }

    let form_config = FormulationConfig {
        routing: routing_var,
        retry: retry_var,
        termination: term_var,
        verification: verif_var,
    };

    let (provider_label, provider_tag, session_prefix) = if is_live_jev {
        ("Jev (LIVE API)", "jev-live", "session-live-jev")
    } else {
        ("Mock/Synthetic", "mock-synthetic", "session-synthetic")
    };

    println!("================= Reflex Control Benchmark Runner =================");
    println!("Provider:       {provider_label}");
    println!(
        "Formulations:   Routing: {:?} | Retry: {:?} | Term: {:?} | Verif: {:?}",
        form_config.routing, form_config.retry, form_config.termination, form_config.verification
    );
    println!("Threshold:      {verif_threshold:.2}");

    let provider: Arc<dyn DecisionProvider> = if is_live_jev {
        let config = JevConfig::from_env()?;
        println!("Endpoint:       configured (URL redacted)");
        println!("Model:          {}", config.model);
        Arc::new(JevProvider::new_with_formulations(config, form_config))
    } else {
        Arc::new(MockProvider::new())
    };

    let session_id = format!("{}-{}", session_prefix, Uuid::new_v4());
    println!("Session ID:     {session_id}");
    println!("Telemetry Store:{db_path}");

    let target_dataset = dataset_path.unwrap_or_else(|| {
        if Path::new("fixtures/held_out_100.json").exists() {
            "fixtures/held_out_100.json".to_string()
        } else if Path::new("fixtures/formulation_selection_100.json").exists() {
            "fixtures/formulation_selection_100.json".to_string()
        } else if Path::new("fixtures/benchmark_100_live.json").exists() {
            "fixtures/benchmark_100_live.json".to_string()
        } else {
            "fixtures/benchmark_dataset_5000.json".to_string()
        }
    });

    if !Path::new(&target_dataset).exists() {
        return Err(format!("Benchmark dataset not found: {target_dataset}").into());
    }

    println!("Dataset:        {target_dataset}");
    let raw = fs::read_to_string(&target_dataset)?;
    let tasks: Vec<TaskItem> = if let Ok(wrapper) = serde_json::from_str::<DatasetWrapper>(&raw) {
        wrapper.tasks
    } else {
        serde_json::from_str(&raw)?
    };

    let total_to_eval = if task_limit > 0 && task_limit < tasks.len() {
        task_limit
    } else {
        tasks.len()
    };

    println!(
        "Executing benchmark across {} tasks (Live Network Calls: {})...",
        total_to_eval,
        if is_live_jev { "YES" } else { "NO" }
    );
    println!("───────────────────────────────────────────────────────────────────");

    let store = TelemetryStore::open(&db_path)?;
    let policy_config = PolicyConfig::default();
    let policy = policy_config.build_policy();

    let mut routing_samples = Vec::new();
    let mut retry_samples = Vec::new();
    let mut termination_samples = Vec::new();
    let mut verification_samples = Vec::new();

    let mut total_latency_ms = 0u64;
    let mut latencies = Vec::with_capacity(total_to_eval);
    let mut total_cost = 0.0;
    let bench_start = Instant::now();

    let verif_polarity = if form_config.verification == FormulationVariant::A {
        VerificationPolarity::CleanProbability
    } else {
        VerificationPolarity::DefectProbability
    };

    for (idx, t) in tasks.into_iter().take(total_to_eval).enumerate() {
        let risk = RiskLevel::from_str(&t.risk_level).unwrap_or(RiskLevel::Low);
        let outcome = match t.ci_outcome.to_lowercase().as_str() {
            "success" | "pass" => Outcome::Success,
            _ => Outcome::Failure,
        };

        let obs = Observation::new(&t.context)
            .with_task_id(&t.task_id)
            .with_risk(risk);

        let dec_type = t.decision_type.to_lowercase();
        let ground_truth = t
            .ground_truth
            .clone()
            .unwrap_or_else(|| match dec_type.as_str() {
                "routing" => {
                    if t.ci_outcome.to_lowercase() == "failure" {
                        "escalate".to_string()
                    } else if t.risk_level.to_lowercase() == "high" {
                        "verify".to_string()
                    } else {
                        "accept".to_string()
                    }
                }
                "retry" => {
                    if t.ci_outcome.to_lowercase() == "failure" {
                        if t.context.to_lowercase().contains("retry")
                            || t.context.to_lowercase().contains("429")
                        {
                            "retry".to_string()
                        } else {
                            "escalate".to_string()
                        }
                    } else {
                        "terminate".to_string()
                    }
                }
                "termination" => {
                    if t.ci_outcome.to_lowercase() == "success" {
                        "terminate".to_string()
                    } else {
                        "continue".to_string()
                    }
                }
                _ => {
                    if t.ci_outcome.to_lowercase() == "failure" {
                        "defect".to_string()
                    } else {
                        "clean".to_string()
                    }
                }
            });

        let req = match dec_type.as_str() {
            "routing" => DecisionRequest::choice(obs.clone(), vec!["accept", "verify", "escalate"]),
            "retry" => DecisionRequest::choice(obs.clone(), vec!["retry", "escalate", "terminate"]),
            "termination" => DecisionRequest::new(
                DecisionType::Probability,
                obs.clone(),
                vec!["terminate".to_string(), "continue".to_string()],
            ),
            _ => DecisionRequest::new(
                DecisionType::Probability,
                obs.clone(),
                vec!["clean".to_string(), "defect".to_string()],
            ),
        };

        let resp = provider.evaluate(&req).await?;

        total_latency_ms += resp.latency_ms;
        latencies.push(resp.latency_ms);
        total_cost += resp.cost_estimate;

        let action = policy.decide(&resp, &obs);

        // Record real prediction into telemetry
        let dec_id = DecisionId::generate();
        let dec_record = DecisionRecord {
            id: dec_id.clone(),
            timestamp: Utc::now(),
            provider: provider_tag.to_string(),
            decision_type: t.decision_type.clone(),
            context: t.context.clone(),
            task_id: Some(t.task_id.clone()),
            selected: resp.decision.selected.clone(),
            confidence: resp.decision.confidence,
            probabilities_json: serde_json::to_string(&resp.decision.probabilities)?,
            action: action.clone(),
            risk_level: risk,
            latency_ms: resp.latency_ms,
            cost_estimate: resp.cost_estimate,
        };
        store.record_decision(&dec_record)?;

        let out_record = OutcomeRecord {
            decision_id: dec_id,
            result: outcome,
            source: OutcomeSource::Ci,
            verified_at: Utc::now(),
            details: Some(t.verifier_result),
        };
        store.record_outcome(&out_record)?;

        // Collect into type-specific evaluation collections
        match dec_type.as_str() {
            "routing" => {
                routing_samples.push((ground_truth, resp.decision.selected.clone()));
            }
            "retry" => {
                retry_samples.push((ground_truth, resp.decision.selected.clone()));
            }
            "termination" => {
                let p_term = resp
                    .decision
                    .probabilities
                    .iter()
                    .find(|(k, _)| k == "terminate")
                    .map(|(_, v)| *v)
                    .or_else(|| {
                        if resp.decision.selected == "terminate" {
                            Some(resp.decision.confidence)
                        } else {
                            Some(1.0 - resp.decision.confidence)
                        }
                    });
                termination_samples.push((ground_truth, resp.decision.selected.clone(), p_term));
            }
            _ => {
                let score = if verif_polarity == VerificationPolarity::DefectProbability {
                    resp.decision
                        .probabilities
                        .iter()
                        .find(|(k, _)| k == "defect" || k == "has_defect")
                        .map(|(_, v)| *v)
                        .unwrap_or(if resp.decision.selected == "defect" {
                            resp.decision.confidence
                        } else {
                            1.0 - resp.decision.confidence
                        })
                } else {
                    resp.decision
                        .probabilities
                        .iter()
                        .find(|(k, _)| k == "clean" || k == "is_valid")
                        .map(|(_, v)| *v)
                        .unwrap_or(if resp.decision.selected == "clean" {
                            resp.decision.confidence
                        } else {
                            1.0 - resp.decision.confidence
                        })
                };

                let is_defect = ground_truth.eq_ignore_ascii_case("defect")
                    || ground_truth.eq_ignore_ascii_case("verify")
                    || t.ci_outcome.to_lowercase() == "failure";
                verification_samples.push((is_defect, score));
            }
        }

        if (idx + 1) % 5 == 0 || idx + 1 == total_to_eval {
            print!(
                "\rEvaluated: {:>4}/{} | Last latency: {:>4}ms | Avg: {:>5.1}ms",
                idx + 1,
                total_to_eval,
                resp.latency_ms,
                total_latency_ms as f64 / (idx + 1) as f64
            );
            std::io::Write::flush(&mut std::io::stdout())?;
        }
    }
    println!("\n───────────────────────────────────────────────────────────────────");

    let total_wall_time = bench_start.elapsed();

    latencies.sort_unstable();
    let p50 = if !latencies.is_empty() {
        latencies[latencies.len() / 2]
    } else {
        0
    };
    let p95 = if !latencies.is_empty() {
        latencies[(latencies.len() as f64 * 0.95) as usize]
    } else {
        0
    };
    let avg_latency = if total_to_eval > 0 {
        total_latency_ms as f64 / total_to_eval as f64
    } else {
        0.0
    };

    println!("\n==================== LIVE INFERENCE BENCHMARK REPORT ====================");
    println!("Evaluated Architecture:         {provider_label}");
    println!(
        "Evaluation Mode:                {}",
        if is_live_jev {
            "LIVE HTTP API INFERENCE"
        } else {
            "SYNTHETIC SIMULATION"
        }
    );
    println!("Evaluated Tasks:                {total_to_eval}");
    println!("API Latency (p50/p95/avg):      {p50} ms / {p95} ms / {avg_latency:.1} ms");
    println!(
        "Total Wall-Clock Time:          {:.2} s",
        total_wall_time.as_secs_f64()
    );
    if is_live_jev {
        println!(
            "Real Inference Cost:            ${total_cost:.6} (using actual TypeSafe Jev token usage: $0.042/1M tokens)"
        );
    }
    println!("=========================================================================\n");

    // 1. Routing Evaluation
    if !routing_samples.is_empty() {
        let refs: Vec<(&str, &str)> = routing_samples
            .iter()
            .map(|(a, p)| (a.as_str(), p.as_str()))
            .collect();
        let rm = evaluate_routing(&refs);

        println!("-------------------- [1] ROUTING DECISION EVALUATION --------------------");
        println!(
            "Jev Primitive:                  Choice ({:?})",
            form_config.routing
        );
        println!("Evaluated Routing Tasks:        {}", rm.total_samples);
        println!(
            "Routing Accuracy:               {:.2}%",
            rm.accuracy * 100.0
        );
        println!("Routing Macro-F1:               {:.4}", rm.macro_f1);
        println!();
        println!("--- Per-Class Performance ---");
        println!(
            "{:<12} | {:>7} | {:>9} | {:>7} | {:>7}",
            "Class", "Support", "Precision", "Recall", "F1 Score"
        );
        println!("-----------------------------------------------------------");
        for (c, m) in &rm.class_metrics {
            println!(
                "{:<12} | {:>7} | {:>8.1}% | {:>6.1}% | {:>8.4}",
                c,
                m.support,
                m.precision * 100.0,
                m.recall * 100.0,
                m.f1
            );
        }
        println!();
        println!("--- Multiclass Confusion Matrix (Routing) ---");
        print!("{}", rm.confusion_matrix.format_table());
        println!("-------------------------------------------------------------------------\n");
    }

    // 2. Retry Evaluation
    if !retry_samples.is_empty() {
        let refs: Vec<(&str, &str)> = retry_samples
            .iter()
            .map(|(a, p)| (a.as_str(), p.as_str()))
            .collect();
        let rm = evaluate_retry(&refs);

        println!("-------------------- [2] RETRY DECISION EVALUATION ----------------------");
        println!(
            "Jev Primitive:                  Choice ({:?})",
            form_config.retry
        );
        println!("Evaluated Retry Tasks:          {}", rm.total_samples);
        println!(
            "Retry Accuracy:                 {:.2}%",
            rm.accuracy * 100.0
        );
        println!("Retry Macro-F1:                 {:.4}", rm.macro_f1);
        println!();
        println!("--- Per-Class Performance ---");
        println!(
            "{:<12} | {:>7} | {:>9} | {:>7} | {:>7}",
            "Class", "Support", "Precision", "Recall", "F1 Score"
        );
        println!("-----------------------------------------------------------");
        for (c, m) in &rm.class_metrics {
            println!(
                "{:<12} | {:>7} | {:>8.1}% | {:>6.1}% | {:>8.4}",
                c,
                m.support,
                m.precision * 100.0,
                m.recall * 100.0,
                m.f1
            );
        }
        println!();
        println!("--- Multiclass Confusion Matrix (Retry) ---");
        print!("{}", rm.confusion_matrix.format_table());
        println!("-------------------------------------------------------------------------\n");
    }

    // 3. Termination Evaluation
    if !termination_samples.is_empty() {
        let refs: Vec<(&str, &str, Option<f64>)> = termination_samples
            .iter()
            .map(|(a, p, prob)| (a.as_str(), p.as_str(), *prob))
            .collect();
        let tm = evaluate_termination(&refs);

        println!("------------------ [3] TERMINATION DECISION EVALUATION ------------------");
        println!(
            "Jev Primitive:                  Noul ({:?})",
            form_config.termination
        );
        println!("Evaluated Termination Tasks:    {}", tm.total_samples);
        println!(
            "Termination Accuracy:           {:.2}%",
            tm.accuracy * 100.0
        );
        println!(
            "Balanced Accuracy:              {:.2}%",
            tm.balanced_accuracy * 100.0
        );
        println!(
            "Precision (terminate):          {:.2}%",
            tm.precision * 100.0
        );
        println!("Recall (terminate):             {:.2}%", tm.recall * 100.0);
        println!("F1 Score (terminate):           {:.4}", tm.f1);
        if let (Some(brier), Some(ece)) = (tm.brier_score, tm.ece) {
            println!("Brier Score (P(terminate)):     {brier:.4}");
            println!("Expected Calib. Error (ECE):    {ece:.4}");
        }
        println!();
        println!("--- Binary Confusion Matrix (Termination) ---");
        print!("{}", tm.confusion_matrix.format_table());
        println!("-------------------------------------------------------------------------\n");
    }

    // 4. Verification Evaluation (Safety-Critical Binary Gate)
    if !verification_samples.is_empty() {
        let vm = evaluate_verification_with_polarity(
            &verification_samples,
            verif_threshold,
            verif_polarity,
        );

        println!("----------- [4] SAFETY-CRITICAL VERIFICATION GATE EVALUATION ------------");
        println!(
            "Jev Primitive:                  Noul ({:?}, Polarity: {:?})",
            form_config.verification, verif_polarity
        );
        println!(
            "Decision Rule:                  Flag defect & verify if {} {:.2}, else autonomous pass",
            if verif_polarity == VerificationPolarity::CleanProbability { "P(clean) <" } else { "P(defect) >=" },
            verif_threshold
        );
        println!("Evaluated Verification Tasks:   {}", vm.total_samples);
        println!(
            "Defect Detection Accuracy:      {:.2}%",
            vm.accuracy * 100.0
        );
        println!(
            "Defect Precision:               {:.2}%",
            vm.precision * 100.0
        );
        println!("Defect Catch Rate (Recall):     {:.2}%", vm.recall * 100.0);
        println!("Defect F1 Score:                {:.4}", vm.f1);
        println!("Brier Score:                    {:.4}", vm.brier_score);
        println!("Expected Calib. Error (ECE):    {:.4}", vm.ece);
        println!();
        println!("--- Safety & Call Avoidance Rates ---");
        println!("  Observed False Accept Rate:   {}", vm.far_ci.format_pct());
        println!("    (Missed defects accepted / Total autonomous passes)");
        println!("  Observed False Alarm Rate:    {}", vm.fpr_ci.format_pct());
        println!("    (Unnecessary verifications / Total actual clean tasks)");
        println!(
            "  Observed Automation Coverage: {}",
            vm.coverage_ci.format_pct()
        );
        println!(
            "  Projected Frontier Avoided:   {}",
            vm.projected_frontier_calls_avoided_ci.format_pct()
        );
        println!();
        println!("--- Binary Confusion Matrix (Verification Gate) ---");
        print!("{}", vm.confusion_matrix.format_table());
        println!();

        // Statistical Proof & Rigor Check
        println!("--- Statistical Proof & Rigor (<1.0% Target at 95% Confidence) ---");
        if vm.far_ci.is_upper_bound_proven(0.01) {
            println!(
                "  [PROVEN] False Accept Rate is STATISTICALLY PROVEN < 1.00% at 95% confidence (upper bound: {:.2}%).",
                vm.far_ci.upper * 100.0
            );
        } else {
            println!(
                "  [EARLY SIGNAL ONLY] FAR {}; its 95% upper bound is {:.2}% and the resolved autonomous-pass cohort is n={}. The data do not establish FAR <1.00%.",
                vm.far_ci.format_pct(),
                vm.far_ci.upper * 100.0,
                vm.far_ci.sample_size
            );
        }
        println!();

        // Empirical Threshold Sweep
        let sweep_thresholds = [0.10, 0.20, 0.30, 0.40, 0.50, 0.60, 0.70, 0.80, 0.90, 0.95];
        let sweep = sweep_verification_thresholds_with_polarity(
            &verification_samples,
            &sweep_thresholds,
            verif_polarity,
        );
        println!("--- Empirical Verification Threshold Sweep (Data-Driven, No Assumptions) ---");
        println!(
            "{:>9} | {:>8} | {:>8} | {:>8} | {:>9} | {:>8} | {:>8} | {:>8}",
            "Threshold", "Accuracy", "FAR", "FalseAlarm", "Precision", "Recall", "F1", "Coverage"
        );
        println!(
            "----------------------------------------------------------------------------------"
        );
        for pt in &sweep {
            println!(
                "{:>9.2} | {:>7.1}% | {:>7.2}% | {:>7.2}% | {:>8.1}% | {:>7.1}% | {:>8.4} | {:>7.1}%",
                pt.threshold,
                pt.accuracy * 100.0,
                pt.far * 100.0,
                pt.fpr * 100.0,
                pt.precision * 100.0,
                pt.recall * 100.0,
                pt.f1,
                pt.coverage * 100.0
            );
        }
        println!("-------------------------------------------------------------------------\n");
    }

    println!("========================== END OF BENCHMARK REPORT ==========================\n");

    Ok(())
}

fn parse_variant(s: &str) -> FormulationVariant {
    match s.trim().to_uppercase().as_str() {
        "B" => FormulationVariant::B,
        "C" => FormulationVariant::C,
        _ => FormulationVariant::A,
    }
}

/// Runs side-by-side formulation comparison on the Selection Set
async fn run_formulation_comparison(
    provider_name: String,
    dataset_path: Option<String>,
    task_limit: usize,
    _db_path: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let is_live_jev = provider_name.to_lowercase() == "jev";

    println!("=========================================================================");
    println!("     REFLEX CONTROL: FORMULATION EXPERIMENT & SELECTION RUNNER           ");
    println!("=========================================================================");
    println!(
        "Provider:           {}",
        if is_live_jev {
            "Jev (LIVE API)"
        } else {
            "Mock/Synthetic"
        }
    );

    let (prov_a, prov_b): (Arc<dyn DecisionProvider>, Arc<dyn DecisionProvider>) = if is_live_jev {
        let config_a = JevConfig::from_env()?;
        let config_b = JevConfig::from_env()?;
        println!("Endpoint:           configured (URL redacted)");
        println!("Model:              {}", config_a.model);
        (
            Arc::new(JevProvider::new_with_formulations(
                config_a,
                FormulationConfig::all_a(),
            )),
            Arc::new(JevProvider::new_with_formulations(
                config_b,
                FormulationConfig::all_b(),
            )),
        )
    } else {
        (Arc::new(MockProvider::new()), Arc::new(MockProvider::new()))
    };

    let target_dataset = dataset_path.unwrap_or_else(|| {
        if Path::new("fixtures/formulation_selection_100.json").exists() {
            "fixtures/formulation_selection_100.json".to_string()
        } else {
            "fixtures/benchmark_100_live.json".to_string()
        }
    });

    if !Path::new(&target_dataset).exists() {
        return Err(format!("Selection dataset not found: {target_dataset}").into());
    }

    println!("Selection Dataset:  {target_dataset}");
    let raw = fs::read_to_string(&target_dataset)?;
    let tasks: Vec<TaskItem> = if let Ok(wrapper) = serde_json::from_str::<DatasetWrapper>(&raw) {
        wrapper.tasks
    } else {
        serde_json::from_str(&raw)?
    };

    let total_to_eval = if task_limit > 0 && task_limit < tasks.len() {
        task_limit
    } else {
        tasks.len()
    };

    println!(
        "Evaluating {total_to_eval} selection tasks across Formulation A and B concurrently..."
    );
    println!("─────────────────────────────────────────────────────────────────────────");

    let mut routing_a = Vec::new();
    let mut routing_b = Vec::new();
    let mut retry_a = Vec::new();
    let mut retry_b = Vec::new();
    let mut term_a = Vec::new();
    let mut term_b = Vec::new();
    let mut verif_a = Vec::new();
    let mut verif_b = Vec::new();

    let mut total_latencies = Vec::new();
    let mut total_cost = 0.0;
    let start_all = Instant::now();

    for (idx, t) in tasks.into_iter().take(total_to_eval).enumerate() {
        let risk = RiskLevel::from_str(&t.risk_level).unwrap_or(RiskLevel::Low);
        let obs = Observation::new(&t.context)
            .with_task_id(&t.task_id)
            .with_risk(risk);

        let dec_type = t.decision_type.to_lowercase();
        let ground_truth = t
            .ground_truth
            .clone()
            .unwrap_or_else(|| match dec_type.as_str() {
                "routing" => {
                    if t.ci_outcome.to_lowercase() == "failure" {
                        "escalate".to_string()
                    } else if t.risk_level.to_lowercase() == "high" {
                        "verify".to_string()
                    } else {
                        "accept".to_string()
                    }
                }
                "retry" => {
                    if t.ci_outcome.to_lowercase() == "failure" {
                        if t.context.to_lowercase().contains("retry")
                            || t.context.to_lowercase().contains("429")
                        {
                            "retry".to_string()
                        } else {
                            "escalate".to_string()
                        }
                    } else {
                        "terminate".to_string()
                    }
                }
                "termination" => {
                    if t.ci_outcome.to_lowercase() == "success" {
                        "terminate".to_string()
                    } else {
                        "continue".to_string()
                    }
                }
                _ => {
                    if t.ci_outcome.to_lowercase() == "failure" {
                        "defect".to_string()
                    } else {
                        "clean".to_string()
                    }
                }
            });

        match dec_type.as_str() {
            "routing" => {
                let req =
                    DecisionRequest::choice(obs.clone(), vec!["accept", "verify", "escalate"]);
                let (resp_a, resp_b) =
                    tokio::try_join!(prov_a.evaluate(&req), prov_b.evaluate(&req))?;
                total_latencies.push(resp_a.latency_ms);
                total_latencies.push(resp_b.latency_ms);
                total_cost += resp_a.cost_estimate + resp_b.cost_estimate;

                routing_a.push((ground_truth.clone(), resp_a.decision.selected));
                routing_b.push((ground_truth, resp_b.decision.selected));
            }
            "retry" => {
                let req =
                    DecisionRequest::choice(obs.clone(), vec!["retry", "escalate", "terminate"]);
                let (resp_a, resp_b) =
                    tokio::try_join!(prov_a.evaluate(&req), prov_b.evaluate(&req))?;
                total_latencies.push(resp_a.latency_ms);
                total_latencies.push(resp_b.latency_ms);
                total_cost += resp_a.cost_estimate + resp_b.cost_estimate;

                retry_a.push((ground_truth.clone(), resp_a.decision.selected));
                retry_b.push((ground_truth, resp_b.decision.selected));
            }
            "termination" => {
                let req = DecisionRequest::new(
                    DecisionType::Probability,
                    obs.clone(),
                    vec!["terminate".to_string(), "continue".to_string()],
                );
                let (resp_a, resp_b) =
                    tokio::try_join!(prov_a.evaluate(&req), prov_b.evaluate(&req))?;
                total_latencies.push(resp_a.latency_ms);
                total_latencies.push(resp_b.latency_ms);
                total_cost += resp_a.cost_estimate + resp_b.cost_estimate;

                let p_a = resp_a
                    .decision
                    .probabilities
                    .iter()
                    .find(|(k, _)| k == "terminate")
                    .map(|(_, v)| *v);
                let p_b = resp_b
                    .decision
                    .probabilities
                    .iter()
                    .find(|(k, _)| k == "terminate")
                    .map(|(_, v)| *v);

                term_a.push((ground_truth.clone(), resp_a.decision.selected, p_a));
                term_b.push((ground_truth, resp_b.decision.selected, p_b));
            }
            _ => {
                // Verification: call A (clean) and B (defect) independently
                let req = DecisionRequest::new(
                    DecisionType::Probability,
                    obs.clone(),
                    vec!["clean".to_string(), "defect".to_string()],
                );
                let (resp_a, resp_b) =
                    tokio::try_join!(prov_a.evaluate(&req), prov_b.evaluate(&req))?;
                total_latencies.push(resp_a.latency_ms);
                total_latencies.push(resp_b.latency_ms);
                total_cost += resp_a.cost_estimate + resp_b.cost_estimate;

                let p_clean = resp_a
                    .decision
                    .probabilities
                    .iter()
                    .find(|(k, _)| k == "clean" || k == "is_valid")
                    .map(|(_, v)| *v)
                    .unwrap_or(if resp_a.decision.selected == "clean" {
                        resp_a.decision.confidence
                    } else {
                        1.0 - resp_a.decision.confidence
                    });

                let p_defect = resp_b
                    .decision
                    .probabilities
                    .iter()
                    .find(|(k, _)| k == "defect" || k == "has_defect")
                    .map(|(_, v)| *v)
                    .unwrap_or(if resp_b.decision.selected == "defect" {
                        resp_b.decision.confidence
                    } else {
                        1.0 - resp_b.decision.confidence
                    });

                let is_defect = ground_truth.eq_ignore_ascii_case("defect")
                    || ground_truth.eq_ignore_ascii_case("verify")
                    || t.ci_outcome.to_lowercase() == "failure";

                verif_a.push((is_defect, p_clean));
                verif_b.push((is_defect, p_defect));
            }
        }

        print!(
            "\rEvaluated Task {:>3}/{} ({} Jev API calls completed)...",
            idx + 1,
            total_to_eval,
            (idx + 1) * 2
        );
        std::io::Write::flush(&mut std::io::stdout())?;
    }
    println!(
        "\nAll live calls completed in {:.2}s",
        start_all.elapsed().as_secs_f64()
    );
    println!("Total Incurred Live Jev Cost: ${total_cost:.6}");
    println!("─────────────────────────────────────────────────────────────────────────\n");

    // =========================================================================
    // 1. ROUTING FORMULATION COMPARISON
    // =========================================================================
    println!("==================== 1. ROUTING FORMULATION COMPARISON ====================");
    let refs_a: Vec<(&str, &str)> = routing_a
        .iter()
        .map(|(a, p)| (a.as_str(), p.as_str()))
        .collect();
    let refs_b: Vec<(&str, &str)> = routing_b
        .iter()
        .map(|(a, p)| (a.as_str(), p.as_str()))
        .collect();
    let rm_a = evaluate_routing(&refs_a);
    let rm_b = evaluate_routing(&refs_b);

    println!(
        "{:<25} | {:<22} | {:<22}",
        "Metric", "Formulation A (Baseline)", "Formulation B (Criteria Clarity)"
    );
    println!("-------------------------------------------------------------------------");
    println!(
        "{:<25} | {:>20.1}% | {:>20.1}%",
        "Accuracy",
        rm_a.accuracy * 100.0,
        rm_b.accuracy * 100.0
    );
    println!(
        "{:<25} | {:>21.4} | {:>21.4}",
        "Macro-F1", rm_a.macro_f1, rm_b.macro_f1
    );

    for class in ["accept", "verify", "escalate"] {
        let f1_a = rm_a.class_metrics.get(class).map(|m| m.f1).unwrap_or(0.0);
        let f1_b = rm_b.class_metrics.get(class).map(|m| m.f1).unwrap_or(0.0);
        println!("  F1 ({class:<18}) | {f1_a:>21.4} | {f1_b:>21.4}");
    }
    let routing_winner = if rm_b.macro_f1 > rm_a.macro_f1 {
        "B"
    } else if rm_b.macro_f1 < rm_a.macro_f1 {
        "A"
    } else if rm_b.accuracy >= rm_a.accuracy {
        "B"
    } else {
        "A"
    };
    println!("\n>>> Routing Formulation Winner: Formulation {routing_winner}\n");

    // =========================================================================
    // 2. RETRY FORMULATION COMPARISON
    // =========================================================================
    println!("==================== 2. RETRY FORMULATION COMPARISON ======================");
    let refs_a: Vec<(&str, &str)> = retry_a
        .iter()
        .map(|(a, p)| (a.as_str(), p.as_str()))
        .collect();
    let refs_b: Vec<(&str, &str)> = retry_b
        .iter()
        .map(|(a, p)| (a.as_str(), p.as_str()))
        .collect();
    let ret_a = evaluate_retry(&refs_a);
    let ret_b = evaluate_retry(&refs_b);

    println!(
        "{:<25} | {:<22} | {:<22}",
        "Metric", "Formulation A (Baseline)", "Formulation B (Action Distinction)"
    );
    println!("-------------------------------------------------------------------------");
    println!(
        "{:<25} | {:>20.1}% | {:>20.1}%",
        "Accuracy",
        ret_a.accuracy * 100.0,
        ret_b.accuracy * 100.0
    );
    println!(
        "{:<25} | {:>21.4} | {:>21.4}",
        "Macro-F1", ret_a.macro_f1, ret_b.macro_f1
    );

    for class in ["retry", "escalate", "terminate"] {
        let f1_a = ret_a.class_metrics.get(class).map(|m| m.f1).unwrap_or(0.0);
        let f1_b = ret_b.class_metrics.get(class).map(|m| m.f1).unwrap_or(0.0);
        println!("  F1 ({class:<18}) | {f1_a:>21.4} | {f1_b:>21.4}");
    }
    let retry_winner = if ret_b.macro_f1 > ret_a.macro_f1 {
        "B"
    } else if ret_b.macro_f1 < ret_a.macro_f1 {
        "A"
    } else if ret_b.accuracy >= ret_a.accuracy {
        "B"
    } else {
        "A"
    };
    println!("\n>>> Retry Formulation Winner: Formulation {retry_winner}\n");

    // =========================================================================
    // 3. TERMINATION FORMULATION COMPARISON
    // =========================================================================
    println!("================= 3. TERMINATION FORMULATION COMPARISON =================");
    let refs_a: Vec<(&str, &str, Option<f64>)> = term_a
        .iter()
        .map(|(a, p, prob)| (a.as_str(), p.as_str(), *prob))
        .collect();
    let refs_b: Vec<(&str, &str, Option<f64>)> = term_b
        .iter()
        .map(|(a, p, prob)| (a.as_str(), p.as_str(), *prob))
        .collect();
    let tm_a = evaluate_termination(&refs_a);
    let tm_b = evaluate_termination(&refs_b);

    println!(
        "{:<25} | {:<22} | {:<22}",
        "Metric", "Formulation A (Completion)", "Formulation B (Incomplete Inversion)"
    );
    println!("-------------------------------------------------------------------------");
    println!(
        "{:<25} | {:>20.1}% | {:>20.1}%",
        "Accuracy",
        tm_a.accuracy * 100.0,
        tm_b.accuracy * 100.0
    );
    println!(
        "{:<25} | {:>20.1}% | {:>20.1}%",
        "Balanced Accuracy",
        tm_a.balanced_accuracy * 100.0,
        tm_b.balanced_accuracy * 100.0
    );
    println!(
        "{:<25} | {:>20.1}% | {:>20.1}%",
        "Precision (terminate)",
        tm_a.precision * 100.0,
        tm_b.precision * 100.0
    );
    println!(
        "{:<25} | {:>20.1}% | {:>20.1}%",
        "Recall (terminate)",
        tm_a.recall * 100.0,
        tm_b.recall * 100.0
    );
    println!(
        "{:<25} | {:>21.4} | {:>21.4}",
        "F1 Score (terminate)", tm_a.f1, tm_b.f1
    );
    println!(
        "{:<25} | {:>21.4} | {:>21.4}",
        "Brier Score",
        tm_a.brier_score.unwrap_or(0.0),
        tm_b.brier_score.unwrap_or(0.0)
    );
    println!(
        "{:<25} | {:>21.4} | {:>21.4}",
        "ECE",
        tm_a.ece.unwrap_or(0.0),
        tm_b.ece.unwrap_or(0.0)
    );
    let term_winner = if tm_b.balanced_accuracy > tm_a.balanced_accuracy {
        "B"
    } else if tm_b.balanced_accuracy < tm_a.balanced_accuracy {
        "A"
    } else if tm_b.f1 >= tm_a.f1 {
        "B"
    } else {
        "A"
    };
    println!("\n>>> Termination Formulation Winner: Formulation {term_winner}\n");

    // =========================================================================
    // 4. VERIFICATION FORMULATION COMPARISON
    // =========================================================================
    println!("================ 4. VERIFICATION FORMULATION COMPARISON =================");
    let vm_a =
        evaluate_verification_with_polarity(&verif_a, 0.80, VerificationPolarity::CleanProbability);
    let vm_b = evaluate_verification_with_polarity(
        &verif_b,
        0.50,
        VerificationPolarity::DefectProbability,
    );

    println!(
        "{:<25} | {:<22} | {:<22}",
        "Metric", "Formulation A (P(clean))", "Formulation B (P(defect))"
    );
    println!("-------------------------------------------------------------------------");
    println!(
        "{:<25} | {:>20.1}% | {:>20.1}%",
        "Defect Accuracy",
        vm_a.accuracy * 100.0,
        vm_b.accuracy * 100.0
    );
    println!(
        "{:<25} | {:>20.1}% | {:>20.1}%",
        "Defect Precision",
        vm_a.precision * 100.0,
        vm_b.precision * 100.0
    );
    println!(
        "{:<25} | {:>20.1}% | {:>20.1}%",
        "Defect Catch Rate",
        vm_a.recall * 100.0,
        vm_b.recall * 100.0
    );
    println!(
        "{:<25} | {:>21.4} | {:>21.4}",
        "Defect F1", vm_a.f1, vm_b.f1
    );
    println!(
        "{:<25} | {:>40} | {:>40}",
        "False Accept Rate (FAR)",
        vm_a.far_ci.format_pct(),
        vm_b.far_ci.format_pct()
    );
    println!(
        "{:<25} | {:>40} | {:>40}",
        "False Alarm Rate",
        vm_a.fpr_ci.format_pct(),
        vm_b.fpr_ci.format_pct()
    );
    println!(
        "{:<25} | {:>20.1}% | {:>20.1}%",
        "Automation Coverage",
        vm_a.automation_coverage * 100.0,
        vm_b.automation_coverage * 100.0
    );
    println!(
        "{:<25} | {:>21.4} | {:>21.4}",
        "Brier Score", vm_a.brier_score, vm_b.brier_score
    );
    println!("{:<25} | {:>21.4} | {:>21.4}", "ECE", vm_a.ece, vm_b.ece);

    // Threshold sweeps
    let sweep_thresholds = [0.10, 0.20, 0.30, 0.40, 0.50, 0.60, 0.70, 0.80, 0.90, 0.95];
    let sweep_a = sweep_verification_thresholds_with_polarity(
        &verif_a,
        &sweep_thresholds,
        VerificationPolarity::CleanProbability,
    );
    let sweep_b = sweep_verification_thresholds_with_polarity(
        &verif_b,
        &sweep_thresholds,
        VerificationPolarity::DefectProbability,
    );

    println!("\n--- Threshold Sweep: Formulation A (P(clean) < Threshold => Verify) ---");
    println!(
        "{:>9} | {:>8} | {:>8} | {:>8} | {:>8} | {:>8}",
        "Threshold", "Accuracy", "FAR", "F-Alarm", "Recall", "Coverage"
    );
    println!("-----------------------------------------------------------------");
    for pt in &sweep_a {
        println!(
            "{:>9.2} | {:>7.1}% | {:>7.2}% | {:>7.2}% | {:>7.1}% | {:>7.1}%",
            pt.threshold,
            pt.accuracy * 100.0,
            pt.far * 100.0,
            pt.fpr * 100.0,
            pt.recall * 100.0,
            pt.coverage * 100.0
        );
    }

    println!("\n--- Threshold Sweep: Formulation B (P(defect) >= Threshold => Verify) ---");
    println!(
        "{:>9} | {:>8} | {:>8} | {:>8} | {:>8} | {:>8}",
        "Threshold", "Accuracy", "FAR", "F-Alarm", "Recall", "Coverage"
    );
    println!("-----------------------------------------------------------------");
    for pt in &sweep_b {
        println!(
            "{:>9.2} | {:>7.1}% | {:>7.2}% | {:>7.2}% | {:>7.1}% | {:>7.1}%",
            pt.threshold,
            pt.accuracy * 100.0,
            pt.far * 100.0,
            pt.fpr * 100.0,
            pt.recall * 100.0,
            pt.coverage * 100.0
        );
    }

    // Determine verification winner and best threshold
    // Safety-first selection: Recall must be 100% and FAR == 0% (zero missed defects).
    // Among safe candidate points, maximize automation coverage.
    let pick_safe_idx = |sweep: &[reflex_calibration::eval::ThresholdSweepPoint]| -> usize {
        let safe_indices: Vec<usize> = sweep
            .iter()
            .enumerate()
            .filter(|(_, pt)| pt.recall >= 0.999 && pt.far <= 0.001)
            .map(|(i, _)| i)
            .collect();

        if !safe_indices.is_empty() {
            safe_indices
                .into_iter()
                .max_by(|&a, &b| sweep[a].coverage.partial_cmp(&sweep[b].coverage).unwrap())
                .unwrap()
        } else {
            (0..sweep.len())
                .min_by(|&a, &b| {
                    sweep[a]
                        .far
                        .partial_cmp(&sweep[b].far)
                        .unwrap()
                        .then_with(|| sweep[b].recall.partial_cmp(&sweep[a].recall).unwrap())
                })
                .unwrap()
        }
    };

    let best_pt_a = &sweep_a[pick_safe_idx(&sweep_a)];
    let best_pt_b = &sweep_b[pick_safe_idx(&sweep_b)];

    let (verif_winner, chosen_threshold) =
        if best_pt_a.far <= best_pt_b.far && best_pt_a.recall >= best_pt_b.recall {
            ("A", best_pt_a.threshold)
        } else if best_pt_b.far < best_pt_a.far {
            ("B", best_pt_b.threshold)
        } else {
            ("A", best_pt_a.threshold)
        };
    println!("\n>>> Verification Formulation Winner: Formulation {} (Optimal Threshold: {:.2}, Recall: {:.1}%, FAR: {:.2}%, Coverage: {:.1}%)\n", verif_winner, chosen_threshold, best_pt_a.recall * 100.0, best_pt_a.far * 100.0, best_pt_a.coverage * 100.0);

    // Write frozen configuration
    let frozen = FrozenConfiguration {
        routing: routing_winner.to_string(),
        retry: retry_winner.to_string(),
        termination: term_winner.to_string(),
        verification: verif_winner.to_string(),
        verification_threshold: chosen_threshold,
    };

    let frozen_json = serde_json::to_string_pretty(&frozen)?;
    fs::write("fixtures/frozen_configuration.json", &frozen_json)?;
    println!("=========================================================================");
    println!("                  FROZEN CONFIGURATION SAVED                             ");
    println!("=========================================================================");
    println!("Saved to fixtures/frozen_configuration.json:");
    println!("{frozen_json}");
    println!("─────────────────────────────────────────────────────────────────────────");
    println!("Next Step: Run benchmark on a separately collected evaluation dataset:");
    println!(
        "  reflex benchmark --provider jev --dataset path/to/evaluation.json --formulation frozen"
    );
    println!("=========================================================================\n");

    Ok(())
}
