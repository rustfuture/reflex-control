use crate::stats::{bootstrap_metric_ci, wilson_score_interval, ConfidenceInterval};
use reflex_core::{Outcome, ReflexAction};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionOutcomePair {
    pub confidence: f64,
    pub action: ReflexAction,
    pub outcome: Outcome,
}

impl DecisionOutcomePair {
    pub fn new(confidence: f64, action: ReflexAction, outcome: Outcome) -> Self {
        Self {
            confidence: confidence.clamp(0.0, 1.0),
            action,
            outcome,
        }
    }

    pub fn is_success(&self) -> bool {
        self.outcome.is_success()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnotatedDecisionPair {
    pub confidence: f64,
    pub action: ReflexAction,
    pub outcome: Outcome,
    pub decision_type: String,
    pub risk_level: String,
    pub category: String,
}

impl AnnotatedDecisionPair {
    pub fn new(
        confidence: f64,
        action: ReflexAction,
        outcome: Outcome,
        decision_type: impl Into<String>,
        risk_level: impl Into<String>,
        category: impl Into<String>,
    ) -> Self {
        Self {
            confidence: confidence.clamp(0.0, 1.0),
            action,
            outcome,
            decision_type: decision_type.into(),
            risk_level: risk_level.into(),
            category: category.into(),
        }
    }

    pub fn to_pair(&self) -> DecisionOutcomePair {
        DecisionOutcomePair::new(self.confidence, self.action.clone(), self.outcome)
    }

    pub fn is_success(&self) -> bool {
        self.outcome.is_success()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CalibrationMetrics {
    pub total_samples: usize,
    pub accuracy: f64,
    pub precision: f64,
    pub recall: f64,
    pub f1: f64,
    pub brier_score: f64,
    pub ece: f64,
    pub coverage: f64,
    pub selective_accuracy: f64,
    pub false_accept_rate: f64,
    pub false_escalate_rate: f64,
    pub false_negative_rate: f64,
    pub frontier_calls_avoided: usize,
    pub frontier_calls_avoided_pct: f64,
    pub all_verifier_calls_avoided_pct: f64,
    pub cost_reduction_pct: f64,
    pub total_actual_failures: usize,
    pub false_negatives: usize,
    pub false_accepts: usize,

    // 95% Confidence Intervals
    pub far_ci: ConfidenceInterval,
    pub fnr_ci: ConfidenceInterval,
    pub coverage_ci: ConfidenceInterval,
    pub frontier_calls_avoided_ci: ConfidenceInterval,
    pub cost_reduction_ci: ConfidenceInterval,
}

impl CalibrationMetrics {
    pub fn compute(pairs: &[DecisionOutcomePair], default_threshold: f64) -> Self {
        if pairs.is_empty() {
            return Self::default();
        }

        let n = pairs.len() as f64;
        let mut brier_sum = 0.0;
        let mut tp = 0;
        let mut fp = 0;
        let mut fn_cnt = 0;
        let mut tn = 0;

        let mut accepted_count = 0;
        let mut accepted_correct = 0;
        let mut accepted_failed = 0;

        let mut escalated_count = 0;
        let mut escalated_would_succeed = 0;
        let mut cheap_verified_count = 0;
        let mut total_actual_failures = 0;

        let mut sample_costs = Vec::with_capacity(pairs.len());

        for pair in pairs {
            let p = pair.confidence;
            let y = if pair.is_success() { 1.0 } else { 0.0 };
            brier_sum += (p - y).powi(2);

            let actual_success = pair.is_success();
            if !actual_success {
                total_actual_failures += 1;
            }

            let pred_success = p >= default_threshold;
            if pred_success && actual_success {
                tp += 1;
            } else if pred_success && !actual_success {
                fp += 1;
            } else if !pred_success && actual_success {
                fn_cnt += 1;
            } else {
                tn += 1;
            }

            let is_accepted = pair.action.is_accept() || p >= default_threshold;
            if is_accepted {
                accepted_count += 1;
                sample_costs.push(0.0001);
                if actual_success {
                    accepted_correct += 1;
                } else {
                    accepted_failed += 1;
                }
            } else if pair.action.is_verify() || p >= 0.65 {
                cheap_verified_count += 1;
                sample_costs.push(0.0001 + 0.005);
            } else {
                escalated_count += 1;
                sample_costs.push(0.0001 + 0.02);
                if actual_success {
                    escalated_would_succeed += 1;
                }
            }
        }

        let accuracy = (tp + tn) as f64 / n;
        let precision = if tp + fp > 0 {
            tp as f64 / (tp + fp) as f64
        } else {
            0.0
        };
        let recall = if tp + fn_cnt > 0 {
            tp as f64 / (tp + fn_cnt) as f64
        } else {
            0.0
        };
        let f1 = if precision + recall > 0.0 {
            2.0 * (precision * recall) / (precision + recall)
        } else {
            0.0
        };

        let brier_score = brier_sum / n;

        // Compute ECE with 10 standard equal-width bins
        let num_bins = 10;
        let mut bin_counts = vec![0usize; num_bins];
        let mut bin_conf_sum = vec![0.0f64; num_bins];
        let mut bin_correct_sum = vec![0.0f64; num_bins];

        for pair in pairs {
            let bin_idx = ((pair.confidence * num_bins as f64).floor() as usize).min(num_bins - 1);
            bin_counts[bin_idx] += 1;
            bin_conf_sum[bin_idx] += pair.confidence;
            if pair.is_success() {
                bin_correct_sum[bin_idx] += 1.0;
            }
        }

        let mut ece = 0.0;
        for i in 0..num_bins {
            let count = bin_counts[i];
            if count > 0 {
                let bin_conf = bin_conf_sum[i] / count as f64;
                let bin_acc = bin_correct_sum[i] / count as f64;
                let bin_weight = count as f64 / n;
                ece += bin_weight * (bin_acc - bin_conf).abs();
            }
        }

        let coverage = accepted_count as f64 / n;
        let selective_accuracy = if accepted_count > 0 {
            accepted_correct as f64 / accepted_count as f64
        } else {
            0.0
        };

        let false_accepts = accepted_failed;
        let false_negatives = accepted_failed;

        let false_accept_rate = if accepted_count > 0 {
            false_accepts as f64 / accepted_count as f64
        } else {
            0.0
        };

        let false_negative_rate = if total_actual_failures > 0 {
            false_negatives as f64 / total_actual_failures as f64
        } else {
            0.0
        };

        let false_escalate_rate = if escalated_count > 0 {
            escalated_would_succeed as f64 / escalated_count as f64
        } else {
            0.0
        };

        // Mathematically verified Calls Avoided calculations:
        // 1. All verifier calls avoided = strictly autonomous accepts (0 verifier calls of any kind)
        let all_verifier_calls_avoided_pct = (accepted_count as f64 / n) * 100.0;

        // 2. Frontier calls avoided = tasks that did NOT call the expensive frontier model (accepted + cheap verified)
        let frontier_calls_avoided = accepted_count + cheap_verified_count;
        let frontier_calls_avoided_pct = (frontier_calls_avoided as f64 / n) * 100.0;

        // Economic calculation:
        // Baseline: all tasks to frontier model at $0.02
        let baseline_cost = n * 0.02;
        let reflex_total_cost: f64 = sample_costs.iter().sum();
        let cost_reduction_pct = if baseline_cost > 0.0 {
            ((baseline_cost - reflex_total_cost) / baseline_cost) * 100.0
        } else {
            0.0
        };

        // Statistical 95% Confidence Intervals
        let far_ci = wilson_score_interval(false_accepts, accepted_count, 0.95);
        let fnr_ci = wilson_score_interval(false_negatives, total_actual_failures, 0.95);
        let coverage_ci = wilson_score_interval(accepted_count, pairs.len(), 0.95);
        let frontier_calls_avoided_ci =
            wilson_score_interval(frontier_calls_avoided, pairs.len(), 0.95);

        // Bootstrap CI for cost reduction (savings per task relative to baseline 0.02)
        let savings_per_task: Vec<f64> = sample_costs
            .iter()
            .map(|&c| ((0.02 - c) / 0.02).clamp(0.0, 1.0))
            .collect();
        let cost_reduction_ci = bootstrap_metric_ci(&savings_per_task, 1000, 0.95, 42);

        Self {
            total_samples: pairs.len(),
            accuracy,
            precision,
            recall,
            f1,
            brier_score,
            ece,
            coverage,
            selective_accuracy,
            false_accept_rate,
            false_escalate_rate,
            false_negative_rate,
            frontier_calls_avoided,
            frontier_calls_avoided_pct,
            all_verifier_calls_avoided_pct,
            cost_reduction_pct,
            total_actual_failures,
            false_negatives,
            false_accepts,
            far_ci,
            fnr_ci,
            coverage_ci,
            frontier_calls_avoided_ci,
            cost_reduction_ci,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SliceMetric {
    pub slice_type: String,
    pub slice_name: String,
    pub sample_count: usize,
    pub defect_count: usize,
    pub brier_score: f64,
    pub ece: f64,
    pub accuracy: f64,
    pub coverage_pct: f64,
    pub false_accept_rate: f64,
    pub far_ci: ConfidenceInterval,
    pub false_negative_rate: f64,
    pub fnr_ci: ConfidenceInterval,
}

impl SliceMetric {
    pub fn compute_slice(
        slice_type: impl Into<String>,
        slice_name: impl Into<String>,
        annotated: &[AnnotatedDecisionPair],
        threshold: f64,
    ) -> Self {
        let pairs: Vec<DecisionOutcomePair> = annotated.iter().map(|a| a.to_pair()).collect();
        let m = CalibrationMetrics::compute(&pairs, threshold);

        Self {
            slice_type: slice_type.into(),
            slice_name: slice_name.into(),
            sample_count: m.total_samples,
            defect_count: m.total_actual_failures,
            brier_score: m.brier_score,
            ece: m.ece,
            accuracy: m.accuracy,
            coverage_pct: m.coverage * 100.0,
            false_accept_rate: m.false_accept_rate,
            far_ci: m.far_ci,
            false_negative_rate: m.false_negative_rate,
            fnr_ci: m.fnr_ci,
        }
    }
}

/// Computes disaggregated calibration and error metrics across Decision Types, Risk Classes, and Categories.
pub fn compute_disaggregated_metrics(
    records: &[AnnotatedDecisionPair],
    threshold: f64,
) -> Vec<SliceMetric> {
    let mut by_decision_type: HashMap<String, Vec<AnnotatedDecisionPair>> = HashMap::new();
    let mut by_risk_level: HashMap<String, Vec<AnnotatedDecisionPair>> = HashMap::new();
    let mut by_category: HashMap<String, Vec<AnnotatedDecisionPair>> = HashMap::new();

    for r in records {
        by_decision_type
            .entry(r.decision_type.clone())
            .or_default()
            .push(r.clone());
        by_risk_level
            .entry(r.risk_level.clone())
            .or_default()
            .push(r.clone());
        by_category
            .entry(r.category.clone())
            .or_default()
            .push(r.clone());
    }

    let mut slices = Vec::new();

    // Sort keys for deterministic output
    let mut dt_keys: Vec<_> = by_decision_type.keys().cloned().collect();
    dt_keys.sort();
    for k in dt_keys {
        slices.push(SliceMetric::compute_slice(
            "decision_type",
            &k,
            &by_decision_type[&k],
            threshold,
        ));
    }

    let mut risk_keys: Vec<_> = by_risk_level.keys().cloned().collect();
    risk_keys.sort();
    for k in risk_keys {
        slices.push(SliceMetric::compute_slice(
            "risk_level",
            &k,
            &by_risk_level[&k],
            threshold,
        ));
    }

    let mut cat_keys: Vec<_> = by_category.keys().cloned().collect();
    cat_keys.sort();
    for k in cat_keys {
        slices.push(SliceMetric::compute_slice(
            "category",
            &k,
            &by_category[&k],
            threshold,
        ));
    }

    slices
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calibration_metrics_with_ci() {
        let pairs = vec![
            DecisionOutcomePair::new(0.95, ReflexAction::Accept, Outcome::Success),
            DecisionOutcomePair::new(0.90, ReflexAction::Accept, Outcome::Success),
            DecisionOutcomePair::new(0.85, ReflexAction::Accept, Outcome::Failure), // false accept
            DecisionOutcomePair::new(0.50, ReflexAction::Verify, Outcome::Failure),
            DecisionOutcomePair::new(0.40, ReflexAction::Escalate, Outcome::Success), // false escalate
        ];

        let metrics = CalibrationMetrics::compute(&pairs, 0.80);
        assert_eq!(metrics.total_samples, 5);
        assert!(metrics.brier_score > 0.0);
        assert!(metrics.false_accept_rate > 0.0);
        assert!(metrics.false_negative_rate > 0.0);
        assert_eq!(metrics.total_actual_failures, 2);
        assert_eq!(metrics.false_negatives, 1);
        assert!(metrics.coverage > 0.5);

        // Verify CI intervals are computed and bounded
        assert!(metrics.far_ci.lower <= metrics.far_ci.point_estimate);
        assert!(metrics.far_ci.point_estimate <= metrics.far_ci.upper);
        assert!(metrics.coverage_ci.sample_size == 5);
    }

    #[test]
    fn test_disaggregated_slices() {
        let records = vec![
            AnnotatedDecisionPair::new(
                0.95,
                ReflexAction::Accept,
                Outcome::Success,
                "probability",
                "low",
                "docs",
            ),
            AnnotatedDecisionPair::new(
                0.92,
                ReflexAction::Accept,
                Outcome::Success,
                "probability",
                "low",
                "docs",
            ),
            AnnotatedDecisionPair::new(
                0.70,
                ReflexAction::Verify,
                Outcome::Failure,
                "choice",
                "high",
                "security",
            ),
        ];

        let slices = compute_disaggregated_metrics(&records, 0.90);
        assert!(!slices.is_empty());
        assert!(slices.iter().any(|s| s.slice_name == "probability"));
        assert!(slices.iter().any(|s| s.slice_name == "security"));
    }
}
