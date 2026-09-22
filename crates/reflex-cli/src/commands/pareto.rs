use reflex_calibration::{DecisionOutcomePair, ThresholdOptimizer};
use reflex_telemetry::TelemetryStore;
use serde::Deserialize;
use std::fs;
use std::path::Path;

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
    pub tasks: Vec<TaskRecord>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct TaskRecord {
    #[serde(default)]
    pub task_id: String,
    #[serde(default)]
    pub risk_level: Option<String>,
    pub confidence: f64,
    pub ci_outcome: String,
}

pub fn execute(
    dataset_path: Option<String>,
    db_path: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut pairs = Vec::new();
    let dataset_provenance;
    let dataset_name;

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
        dataset_name = target_dataset.clone();
        let raw = fs::read_to_string(&target_dataset)?;

        let (tasks, metadata): (Vec<TaskRecord>, Option<DatasetMetadata>) =
            if let Ok(wrapper) = serde_json::from_str::<DatasetWrapper>(&raw) {
                (wrapper.tasks, wrapper.metadata)
            } else {
                let parsed: Vec<TaskRecord> = serde_json::from_str(&raw)?;
                (parsed, None)
            };

        if let Some(meta) = metadata {
            dataset_provenance = format!(
                "{} (Synthetic Fixture, NOT Production Telemetry)",
                meta.source
            );
        } else if target_dataset.contains("real_agent_worker_tasks") {
            dataset_provenance =
                "Curated Benchmark Fixture (real_agent_worker_tasks.json) [Hand-curated fixture, NOT production telemetry]".to_string();
        } else {
            dataset_provenance =
                "Synthetic Benchmark Fixture (NOT Production Telemetry)".to_string();
        }

        for t in tasks {
            let outcome = match t.ci_outcome.to_lowercase().as_str() {
                "success" | "pass" => reflex_core::Outcome::Success,
                _ => reflex_core::Outcome::Failure,
            };
            let risk = t
                .risk_level
                .unwrap_or_else(|| "low".to_string())
                .to_lowercase();
            let action = if risk == "critical" {
                reflex_core::ReflexAction::Escalate
            } else if t.confidence >= 0.90 {
                reflex_core::ReflexAction::Accept
            } else {
                reflex_core::ReflexAction::Verify
            };
            pairs.push(DecisionOutcomePair::new(t.confidence, action, outcome));
        }
    } else {
        dataset_name = db_path.clone();
        dataset_provenance = format!("Local Telemetry SQLite DB ({db_path})");
        let store = TelemetryStore::open(&db_path)?;
        let pairs_db = store.list_paired_decisions()?;
        for (d, o) in pairs_db {
            if let Some(out) = o {
                pairs.push(DecisionOutcomePair::new(d.confidence, d.action, out.result));
            }
        }
    }

    if pairs.is_empty() {
        println!("No outcome data available for Pareto analysis.");
        println!("Run 'reflex shadow run' first to evaluate and record agent decisions.");
        return Ok(());
    }

    let pareto_points = ThresholdOptimizer::pareto_frontier(&pairs);

    println!("\n=================== Cost vs Risk Pareto Frontier (Statistical Slices) ===================");
    println!("Evaluated Dataset:        {dataset_name}");
    println!("Provenance:               {dataset_provenance}");
    println!("Evaluated Samples:        {}", pairs.len());
    let resolved_samples = pairs.iter().filter(|p| p.outcome.is_resolved()).count();
    let unresolved_samples = pairs.len() - resolved_samples;
    let defects = pairs.iter().filter(|p| p.outcome.is_failure()).count();
    println!("Resolved Outcomes:       {resolved_samples}");
    println!("Unresolved Excluded:     {unresolved_samples}");
    if resolved_samples > 0 {
        println!(
            "Observed Defects:        {} ({:.2}% of resolved outcomes)",
            defects,
            (defects as f64 / resolved_samples as f64) * 100.0
        );
    } else {
        println!("Observed Defects:        N/A (no resolved outcomes)");
    }
    println!(
        "Optimization Target:      Frontier calls avoided >= 40-50% with FAR < 1.0% and FNR < 1.0%"
    );
    println!("Confidence Level:         95% Two-Sided Wilson Score Intervals");
    println!("──────────────────────────────────────────────────────────────────────────────────────────");
    println!(
        "{:<6} | {:>8} | {:>13} | {:>10} | {:>18} | {:>18} | {:<16}",
        "Cutoff",
        "Coverage",
        "Calls Avoided",
        "Cost Saved",
        "Obs FAR [95% CI]",
        "Obs FNR [95% CI]",
        "Pareto Status"
    );
    println!("──────────────────────────────────────────────────────────────────────────────────────────");

    let mut optimal_targets = Vec::new();

    for p in &pareto_points {
        let mut status_tag = String::new();
        if p.is_pareto_optimal {
            status_tag.push_str("* Optimal");
        }
        if p.is_target_met {
            if p.is_statistically_proven {
                status_tag.push_str(" [PROVEN <1%]");
            } else {
                status_tag.push_str(" [EARLY SIGNAL]");
            }
            optimal_targets.push(p.clone());
        }

        let far_str = p.far_ci.format_pct();
        let fnr_str = p.fnr_ci.format_pct();

        println!(
            "{:<6.2} | {:>7.1}% | {:>12.1}% | {:>9.1}% | {:>18} | {:>18} | {:<16}",
            p.threshold,
            p.coverage_pct,
            p.calls_avoided_pct,
            p.cost_reduction_pct,
            far_str,
            fnr_str,
            status_tag
        );
    }
    println!("──────────────────────────────────────────────────────────────────────────────────────────");

    println!("\n--- Pareto Optimization Insights ---");
    if let Some(best) = optimal_targets.first() {
        println!(
            "Recommended Operating Point: Threshold tau* = {:.2}",
            best.threshold
        );
        println!(
            "  - Autonomous Automation Coverage:  {:.1}%",
            best.coverage_pct
        );
        println!(
            "  - Frontier Verifier Calls Avoided: {:.1}% (Target >= 40-50% SATISFIED)",
            best.calls_avoided_pct
        );
        println!(
            "  - Expected Cost Reduction:         {:.1}%",
            best.cost_reduction_pct
        );
        println!(
            "  - Observed False Accept Rate:      {}",
            best.far_ci.format_pct()
        );
        println!(
            "  - Observed False Negative Rate:    {}",
            best.fnr_ci.format_pct()
        );

        if best.is_statistically_proven {
            println!("  - Statistical Rigor:               STATISTICALLY PROVEN < 1.0% (Both FAR and FNR upper bounds < 1.0% at 95% confidence).");
        } else {
            println!(
                "  - Statistical Rigor:               EARLY SIGNAL ONLY (FAR {}, FNR {}; both upper bounds must be <1.0%).",
                best.far_ci.format_pct(),
                best.fnr_ci.format_pct()
            );
        }
    } else {
        println!("No single threshold met all strict targets simultaneously under current sample constraints.");
        println!(
            "Operators can choose along the non-dominated Pareto frontier based on risk tolerance."
        );
    }
    println!("==========================================================================================\n");

    Ok(())
}
