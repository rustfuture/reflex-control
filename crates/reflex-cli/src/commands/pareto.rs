use reflex_calibration::{DecisionOutcomePair, ThresholdOptimizer};
use reflex_telemetry::TelemetryStore;
use serde::Deserialize;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct TaskRecord {
    pub confidence: f64,
    pub risk_level: String,
    pub ci_outcome: String,
}

pub fn execute(
    dataset_path: Option<String>,
    db_path: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut pairs = Vec::new();

    let target_dataset = dataset_path.unwrap_or_else(|| {
        if Path::new("fixtures/real_agent_worker_tasks.json").exists() {
            "fixtures/real_agent_worker_tasks.json".to_string()
        } else {
            String::new()
        }
    });

    if !target_dataset.is_empty() && Path::new(&target_dataset).exists() {
        let raw = fs::read_to_string(&target_dataset)?;
        let tasks: Vec<TaskRecord> = serde_json::from_str(&raw)?;
        for t in tasks {
            let outcome = match t.ci_outcome.to_lowercase().as_str() {
                "success" | "pass" => reflex_core::Outcome::Success,
                _ => reflex_core::Outcome::Failure,
            };
            let action = if t.risk_level.to_lowercase() == "critical" {
                reflex_core::ReflexAction::Escalate
            } else if t.confidence >= 0.90 {
                reflex_core::ReflexAction::Accept
            } else {
                reflex_core::ReflexAction::Verify
            };
            pairs.push(DecisionOutcomePair::new(t.confidence, action, outcome));
        }
    } else {
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
        println!("Run 'reflex shadow run' first to evaluate and record real tasks.");
        return Ok(());
    }

    let pareto_points = ThresholdOptimizer::pareto_frontier(&pairs);

    println!("\n=================== Cost vs Risk Pareto Frontier ===================");
    println!("Evaluated Ground Truth Samples: {}", pairs.len());
    println!("Objective: Frontier calls avoided >= 40-50% with FAR < 1.0% and FNR < 1.0%");
    println!("────────────────────────────────────────────────────────────────────");
    println!(
        "{:<8} | {:>8} | {:>13} | {:>10} | {:>6} | {:>6} | {:<18}",
        "Cutoff", "Coverage", "Calls Avoided", "Cost Saved", "FAR", "FNR", "Pareto Status"
    );
    println!("────────────────────────────────────────────────────────────────────");

    let mut optimal_targets = Vec::new();

    for p in &pareto_points {
        let mut status_tag = String::new();
        if p.is_pareto_optimal {
            status_tag.push_str("* Optimal");
        }
        if p.is_target_met {
            status_tag.push_str(" [TARGET MET]");
            optimal_targets.push(p.clone());
        }

        println!(
            "{:<8.2} | {:>7.1}% | {:>12.1}% | {:>9.1}% | {:>5.2}% | {:>5.2}% | {:<18}",
            p.threshold,
            p.coverage_pct,
            p.calls_avoided_pct,
            p.cost_reduction_pct,
            p.false_accept_rate * 100.0,
            p.false_negative_rate * 100.0,
            status_tag
        );
    }
    println!("────────────────────────────────────────────────────────────────────");

    println!("\n--- Pareto Optimization Insights ---");
    if let Some(best) = optimal_targets.first() {
        println!(
            "Recommended Operating Point: Threshold = {:.2}",
            best.threshold
        );
        println!(
            "  - Frontier Verifier Calls Avoided: {:.1}% (Target >= 40-50% SATISFIED)",
            best.calls_avoided_pct
        );
        println!(
            "  - Autonomous Automation Coverage:  {:.1}%",
            best.coverage_pct
        );
        println!(
            "  - Expected Inference Cost Savings: {:.1}%",
            best.cost_reduction_pct
        );
        println!(
            "  - False Accept Rate (FAR):         {:.2}% (< 1.0% SATISFIED)",
            best.false_accept_rate * 100.0
        );
        println!(
            "  - False Negative Rate (FNR):       {:.2}% (< 1.0% SATISFIED)",
            best.false_negative_rate * 100.0
        );
    } else {
        println!("No single threshold met all strict targets simultaneously.");
        println!("Operators can choose along the frontier based on risk tolerance.");
    }
    println!("====================================================================\n");

    Ok(())
}
