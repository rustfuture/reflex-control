use reflex_calibration::{
    compute_disaggregated_metrics, split_stratified, AnnotatedDecisionPair, CalibrationCurve,
    CalibrationMetrics, DecisionOutcomePair, OptimizationConstraints, ThresholdOptimizer,
};
use reflex_core::{Outcome, ReflexAction};
use reflex_telemetry::TelemetryStore;
use serde::Deserialize;
use std::fs;
use std::path::Path;

fn format_resolved_metric(value: f64, resolved_samples: usize) -> String {
    if resolved_samples == 0 {
        "N/A (n=0)".to_string()
    } else {
        format!("{value:.4}")
    }
}

fn format_resolved_percent(value: f64, resolved_samples: usize) -> String {
    if resolved_samples == 0 {
        "N/A (n=0)".to_string()
    } else {
        format!("{:.2}%", value * 100.0)
    }
}

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
    pub decision_type: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub risk_level: Option<String>,
    pub confidence: f64,
    pub ci_outcome: String,
    #[serde(default)]
    pub verifier_result: Option<String>,
}

pub fn execute(
    dataset_path: Option<String>,
    max_false_accept: f64,
    max_false_negative: f64,
    min_coverage: f64,
    current_threshold: f64,
    split_evaluation: bool,
    db_path: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut annotated_pairs = Vec::new();
    let dataset_provenance;
    let dataset_file_name;

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
        dataset_file_name = target_dataset.clone();
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
                "Curated Synthetic Benchmark Fixture (real_agent_worker_tasks.json) [Hand-curated fixture, NOT production telemetry]".to_string();
        } else {
            dataset_provenance =
                "Synthetic Evaluation Fixture (NOT Production Telemetry)".to_string();
        }

        for t in tasks {
            let outcome = match t.ci_outcome.to_lowercase().as_str() {
                "success" | "pass" => Outcome::Success,
                _ => Outcome::Failure,
            };
            let risk = t
                .risk_level
                .clone()
                .unwrap_or_else(|| "low".to_string())
                .to_lowercase();
            let action = if risk == "critical" {
                ReflexAction::Escalate
            } else if t.confidence >= current_threshold {
                ReflexAction::Accept
            } else {
                ReflexAction::Verify
            };

            let dec_type = t
                .decision_type
                .unwrap_or_else(|| "verification".to_string());
            let category = t.category.unwrap_or_else(|| "general".to_string());

            annotated_pairs.push(AnnotatedDecisionPair::new(
                t.confidence,
                action,
                outcome,
                dec_type,
                risk,
                category,
            ));
        }
    } else {
        dataset_file_name = db_path.clone();
        dataset_provenance = format!("Local Telemetry SQLite DB ({db_path})");
        let store = TelemetryStore::open(&db_path)?;
        let pairs_db = store.list_paired_decisions()?;
        for (d, o) in pairs_db {
            if let Some(out) = o {
                annotated_pairs.push(AnnotatedDecisionPair::new(
                    d.confidence,
                    d.action,
                    out.result,
                    d.decision_type,
                    d.risk_level.to_string().to_lowercase(),
                    "telemetry",
                ));
            }
        }
    }

    if annotated_pairs.is_empty() {
        println!("No outcome data available for calibration.");
        println!("Run 'reflex shadow run' first to evaluate and record agent decisions.");
        return Ok(());
    }

    let pairs: Vec<DecisionOutcomePair> = annotated_pairs
        .iter()
        .map(|a: &AnnotatedDecisionPair| a.to_pair())
        .collect();

    println!(
        "================ Reflex Statistical Calibration & Optimization Report ================"
    );
    println!("Evaluation Dataset:             {dataset_file_name}");
    println!("Provenance Classification:      {dataset_provenance}");
    println!("Total Samples in Dataset:       {}", pairs.len());
    let resolved_samples = pairs.iter().filter(|p| p.outcome.is_resolved()).count();
    let unresolved_samples = pairs.len() - resolved_samples;
    let total_defects = pairs.iter().filter(|p| p.outcome.is_failure()).count();
    println!("Resolved Outcomes:              {resolved_samples}");
    println!("Unresolved Excluded from Rates: {unresolved_samples}");
    if resolved_samples > 0 {
        println!(
            "Observed Defects (resolved):   {} ({:.2}%)",
            total_defects,
            (total_defects as f64 / resolved_samples as f64) * 100.0
        );
    } else {
        println!("Observed Defects (resolved):   N/A (no resolved outcomes)");
    }

    let constraints = OptimizationConstraints {
        current_threshold,
        max_false_accept_rate: max_false_accept,
        max_false_negative_rate: max_false_negative,
        min_coverage,
    };

    if split_evaluation && annotated_pairs.len() >= 40 {
        // Deterministic stratified 3-way partition: 50% Train, 25% Validation, 25% Test.
        let split = split_stratified(
            annotated_pairs.clone(),
            0.50,
            0.25,
            0.25,
            |p: &AnnotatedDecisionPair| p.outcome.is_failure(),
            42,
        );
        let train_defects = split
            .train
            .iter()
            .filter(|p| p.outcome.is_failure())
            .count();
        let val_defects = split.val.iter().filter(|p| p.outcome.is_failure()).count();
        let test_defects = split.test.iter().filter(|p| p.outcome.is_failure()).count();
        let train_unresolved = split
            .train
            .iter()
            .filter(|p| !p.outcome.is_resolved())
            .count();
        let val_unresolved = split
            .val
            .iter()
            .filter(|p| !p.outcome.is_resolved())
            .count();
        let test_unresolved = split
            .test
            .iter()
            .filter(|p| !p.outcome.is_resolved())
            .count();

        println!("\n--- Dataset Partitioning (Deterministic Stratified 3-Way Split) ---");
        println!(
            "  1. Calibration / Train (50%): {:>5} samples (resolved defects: {:>4}, unresolved: {:>4})",
            split.train.len(),
            train_defects,
            train_unresolved
        );
        println!(
            "  2. Validation Partition (25%): {:>5} samples (resolved defects: {:>4}, unresolved: {:>4})",
            split.val.len(),
            val_defects,
            val_unresolved
        );
        println!(
            "  3. Test Partition       (25%): {:>5} samples (resolved defects: {:>4}, unresolved: {:>4})",
            split.test.len(),
            test_defects,
            test_unresolved
        );

        // Optimize threshold ONLY on Train + Validation
        let mut train_val_pairs: Vec<DecisionOutcomePair> = Vec::new();
        train_val_pairs.extend(split.train.iter().map(|a| a.to_pair()));
        train_val_pairs.extend(split.val.iter().map(|a| a.to_pair()));

        let opt = ThresholdOptimizer::optimize(&train_val_pairs, &constraints);
        let frozen_tau = opt.recommended_threshold;

        println!("\n--- Phase 1: Threshold Optimization (Train + Validation Sets ONLY) ---");
        println!(
            "  Current Threshold:            {:.3}",
            opt.current_threshold
        );
        println!("  Selected Optimal Threshold:   {frozen_tau:.3}");
        println!(
            "  Constraint Target FAR:        <= {:.2}%",
            max_false_accept * 100.0
        );
        println!(
            "  Constraint Target FNR:        <= {:.2}%",
            max_false_negative * 100.0
        );
        println!(
            "  Constraint Target Coverage:   >= {:.1}%",
            min_coverage * 100.0
        );
        println!(
            "  Status on Train+Val:          {}",
            if opt.is_feasible {
                "Feasible"
            } else {
                "Best-Effort"
            }
        );
        println!("  Optimization Detail:          {}", opt.explanation);

        // Evaluate the selected threshold on the separate test partition.
        let test_metrics = split.evaluate_test_with_frozen_threshold(frozen_tau);

        println!(
            "\n--- Phase 2: Test Partition Performance (Selected Threshold tau* = {frozen_tau:.3}) ---"
        );
        println!(
            "  Test Partition Sample Size:   {}",
            test_metrics.total_samples
        );
        println!(
            "  Resolved Outcomes:             {}",
            test_metrics.resolved_samples
        );
        println!(
            "  Unresolved Excluded:           {}",
            test_metrics.unresolved_samples
        );
        println!(
            "  Accuracy:                     {}",
            format_resolved_percent(test_metrics.accuracy, test_metrics.resolved_samples)
        );
        println!(
            "  Brier Score:                  {}  (lower is better, 0 = perfect)",
            format_resolved_metric(test_metrics.brier_score, test_metrics.resolved_samples)
        );
        println!(
            "  Expected Calib. Error (ECE):  {}",
            format_resolved_metric(test_metrics.ece, test_metrics.resolved_samples)
        );
        println!();
        println!(
            "  Autonomous Coverage:          {}",
            test_metrics.coverage_ci.format_pct()
        );
        println!(
            "  Frontier Calls Avoided:       {}",
            test_metrics.frontier_calls_avoided_ci.format_pct()
        );
        println!(
            "  All Verifier Calls Avoided:   {:.2}% (strictly autonomous zero-verifier coverage)",
            test_metrics.all_verifier_calls_avoided_pct
        );
        println!(
            "  Projected Cost Reduction:     {}",
            test_metrics.cost_reduction_ci.format_pct()
        );
        println!();
        println!(
            "  Observed False Accept Rate:   {}",
            test_metrics.far_ci.format_pct()
        );
        println!(
            "  Observed False Negative Rate: {}",
            test_metrics.fnr_ci.format_pct()
        );

        // Statistical Proof Status (< 1% target)
        println!();
        println!("--- Statistical Proof & Rigor Assessment (<1.0% Target at 95% Confidence) ---");
        if test_metrics.far_ci.is_upper_bound_proven(max_false_accept) {
            println!("  [PROVEN] False Accept Rate (FAR) is STATISTICALLY PROVEN < {:.2}% at 95% confidence (upper bound: {:.2}%).",
                max_false_accept * 100.0, test_metrics.far_ci.upper * 100.0);
        } else {
            println!("  [EARLY SIGNAL ONLY] Observed FAR is {}; 95% upper bound is {:.2}% against the {:.2}% target. The sample does not prove the target.",
                test_metrics.far_ci.format_pct(), test_metrics.far_ci.upper * 100.0, max_false_accept * 100.0);
        }

        if test_metrics
            .fnr_ci
            .is_upper_bound_proven(max_false_negative)
        {
            println!("  [PROVEN] False Negative Rate (FNR) is STATISTICALLY PROVEN < {:.2}% at 95% confidence (upper bound: {:.2}%).",
                max_false_negative * 100.0, test_metrics.fnr_ci.upper * 100.0);
        } else {
            println!("  [EARLY SIGNAL ONLY] Observed FNR is {}; 95% upper bound is {:.2}% against the {:.2}% target. The resolved defect sample does not prove the target.",
                test_metrics.fnr_ci.format_pct(), test_metrics.fnr_ci.upper * 100.0, max_false_negative * 100.0);
        }

        // Calibration Curve on Test Set
        let test_pairs: Vec<DecisionOutcomePair> = split.test.iter().map(|a| a.to_pair()).collect();
        let curve = CalibrationCurve::build(&test_pairs, 5);
        println!("\n--- Calibration View on Test Partition (Resolved Outcomes Only) ---");
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

        // Disaggregated Metrics across Slices
        let slice_metrics = compute_disaggregated_metrics(&split.test, frozen_tau);
        println!("\n--- Disaggregated Evaluation on Test Partition (By Slice & Category) ---");
        println!(
            "{:<16} | {:<16} | {:>7} | {:>8} | {:>10} | {:>7} | {:>11} | {:>7} | {:>6} | {:>8} | {:>20} | {:>20}",
            "Slice Type",
            "Subcategory",
            "Samples",
            "Resolved",
            "Unresolved",
            "Defects",
            "FA / Pass",
            "Brier",
            "ECE",
            "Coverage",
            "FAR",
            "FNR"
        );
        println!("────────────────────────────────────────────────────────────────────────────────────────────────────────────");

        // Decision Types
        for s in slice_metrics
            .iter()
            .filter(|s| s.slice_type == "decision_type")
        {
            println!("{:<16} | {:<16} | {:>7} | {:>8} | {:>10} | {:>7} | {:>11} | {:>7} | {:>6} | {:>7} | {:>20} | {:>20}",
                "Decision Type", s.slice_name, s.sample_count, s.resolved_sample_count, s.unresolved_sample_count,
                s.defect_count, format!("{}/{}", s.false_accept_count, s.autonomous_pass_count),
                format_resolved_metric(s.brier_score, s.resolved_sample_count), format_resolved_metric(s.ece, s.resolved_sample_count),
                s.coverage_pct, s.far_ci.format_pct(), s.fnr_ci.format_pct());
        }
        println!("────────────────────────────────────────────────────────────────────────────────────────────────────────────");

        // Risk Levels
        for s in slice_metrics
            .iter()
            .filter(|s| s.slice_type == "risk_level")
        {
            println!("{:<16} | {:<16} | {:>7} | {:>8} | {:>10} | {:>7} | {:>11} | {:>7} | {:>6} | {:>7} | {:>20} | {:>20}",
                "Risk Level", s.slice_name, s.sample_count, s.resolved_sample_count, s.unresolved_sample_count,
                s.defect_count, format!("{}/{}", s.false_accept_count, s.autonomous_pass_count),
                format_resolved_metric(s.brier_score, s.resolved_sample_count), format_resolved_metric(s.ece, s.resolved_sample_count),
                s.coverage_pct, s.far_ci.format_pct(), s.fnr_ci.format_pct());
        }
        println!("────────────────────────────────────────────────────────────────────────────────────────────────────────────");

        // Task Categories
        for s in slice_metrics.iter().filter(|s| s.slice_type == "category") {
            println!("{:<16} | {:<16} | {:>7} | {:>8} | {:>10} | {:>7} | {:>11} | {:>7} | {:>6} | {:>7} | {:>20} | {:>20}",
                "Category", s.slice_name, s.sample_count, s.resolved_sample_count, s.unresolved_sample_count,
                s.defect_count, format!("{}/{}", s.false_accept_count, s.autonomous_pass_count),
                format_resolved_metric(s.brier_score, s.resolved_sample_count), format_resolved_metric(s.ece, s.resolved_sample_count),
                s.coverage_pct, s.far_ci.format_pct(), s.fnr_ci.format_pct());
        }
        println!("=================================================================================================\n");
    } else {
        // Fallback for non-split / small datasets
        let metrics = CalibrationMetrics::compute(&pairs, current_threshold);
        let curve = CalibrationCurve::build(&pairs, 5);

        println!(
            "\nEvaluation Dataset Size:        {}",
            metrics.total_samples
        );
        println!(
            "Accuracy:                       {}",
            format_resolved_percent(metrics.accuracy, metrics.resolved_samples)
        );
        println!(
            "Brier Score:                    {}",
            format_resolved_metric(metrics.brier_score, metrics.resolved_samples)
        );
        println!(
            "Expected Calib. Error (ECE):    {}",
            format_resolved_metric(metrics.ece, metrics.resolved_samples)
        );
        println!(
            "Automation Coverage:            {}",
            metrics.coverage_ci.format_pct()
        );
        println!(
            "Frontier Calls Avoided:         {}",
            metrics.frontier_calls_avoided_ci.format_pct()
        );
        println!(
            "Projected Cost Reduction:       {}",
            metrics.cost_reduction_ci.format_pct()
        );
        println!(
            "Observed False Accept Rate:     {}",
            metrics.far_ci.format_pct()
        );
        println!(
            "Observed False Negative Rate:   {}",
            metrics.fnr_ci.format_pct()
        );

        println!("\n--- Calibration View ---");
        for b in &curve.buckets {
            println!(
                "{:.2} - {:.2} {:5} {:7.1}% {:7.1}% {:.4}",
                b.min_conf,
                b.max_conf,
                b.count,
                b.mean_confidence * 100.0,
                b.observed_success_rate * 100.0,
                b.calibration_gap
            );
        }

        let opt = ThresholdOptimizer::optimize(&pairs, &constraints);
        println!(
            "\nRecommended Threshold: {:.3} (Detail: {})",
            opt.recommended_threshold, opt.explanation
        );
        println!("==========================================================================\n");
    }

    Ok(())
}
