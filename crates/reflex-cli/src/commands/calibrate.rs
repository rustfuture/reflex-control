use reflex_calibration::{
    CalibrationCurve, CalibrationMetrics, DecisionOutcomePair, OptimizationConstraints,
    ThresholdOptimizer,
};
use reflex_core::{Outcome, ReflexAction};
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
    max_false_accept: f64,
    max_false_negative: f64,
    min_coverage: f64,
    current_threshold: f64,
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
        println!(
            "Loading ground truth outcomes from verified dataset: {}",
            target_dataset
        );
        let raw = fs::read_to_string(&target_dataset)?;
        let tasks: Vec<TaskRecord> = serde_json::from_str(&raw)?;
        for t in tasks {
            let outcome = match t.ci_outcome.to_lowercase().as_str() {
                "success" | "pass" => Outcome::Success,
                _ => Outcome::Failure,
            };
            let action = if t.risk_level.to_lowercase() == "critical" {
                ReflexAction::Escalate
            } else if t.confidence >= current_threshold {
                ReflexAction::Accept
            } else {
                ReflexAction::Verify
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
        println!("No outcome data available for calibration.");
        println!("Run 'reflex shadow run' first to record real agent decisions.");
        return Ok(());
    }

    let metrics = CalibrationMetrics::compute(&pairs, current_threshold);
    let curve = CalibrationCurve::build(&pairs, 5);

    println!("================ Reflex Calibration & Optimization Report ================");
    println!("Evaluation Dataset Size:        {}", metrics.total_samples);
    println!(
        "Accuracy:                       {:.2}%",
        metrics.accuracy * 100.0
    );
    println!(
        "Precision:                      {:.2}%",
        metrics.precision * 100.0
    );
    println!(
        "Recall:                         {:.2}%",
        metrics.recall * 100.0
    );
    println!(
        "Brier Score:                    {:.4}  (lower is better, 0 = perfect)",
        metrics.brier_score
    );
    println!("Expected Calib. Error (ECE):    {:.4}", metrics.ece);
    println!(
        "Automation Coverage:            {:.1}%",
        metrics.coverage * 100.0
    );
    println!(
        "Frontier Verifier Calls Avoided: {:.1}%",
        metrics.frontier_calls_avoided_pct
    );
    println!(
        "Projected Cost Reduction:       {:.1}%",
        metrics.cost_reduction_pct
    );
    println!(
        "False Accept Rate (FAR):        {:.2}%",
        metrics.false_accept_rate * 100.0
    );
    println!(
        "False Negative Rate (FNR):      {:.2}%",
        metrics.false_negative_rate * 100.0
    );
    println!(
        "Selective Accuracy:             {:.2}%",
        metrics.selective_accuracy * 100.0
    );

    println!("\n--- Calibration View (Confidence vs Observed Frequency) ---");
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

    println!("\n--- Threshold Optimizer Recommendation (Real Outcome Data) ---");
    let constraints = OptimizationConstraints {
        current_threshold,
        max_false_accept_rate: max_false_accept,
        max_false_negative_rate: max_false_negative,
        min_coverage,
    };

    let opt = ThresholdOptimizer::optimize(&pairs, &constraints);
    println!(
        "Current Operating Threshold:    {:.3}",
        opt.current_threshold
    );
    println!(
        "Recommended Safe Threshold:     {:.3}",
        opt.recommended_threshold
    );
    println!(
        "Expected Automation Coverage:   {:.1}%",
        opt.expected_coverage * 100.0
    );
    println!(
        "Expected Calls Avoided:         {:.1}%",
        opt.expected_frontier_calls_avoided_pct
    );
    println!(
        "Expected Cost Reduction:        {:.1}%",
        opt.expected_cost_reduction_pct
    );
    println!(
        "Expected False Accept Rate:     {:.2}%",
        opt.expected_false_accept_rate * 100.0
    );
    println!(
        "Expected False Negative Rate:   {:.2}%",
        opt.expected_false_negative_rate * 100.0
    );
    println!(
        "Optimization Status:            {}",
        if opt.is_feasible {
            "Feasible (Target Criteria Met)"
        } else {
            "Best-Effort Compromise"
        }
    );
    println!("Detail: {}", opt.explanation);
    println!("==========================================================================\n");

    Ok(())
}
