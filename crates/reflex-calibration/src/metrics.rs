use reflex_core::{Outcome, ReflexAction};
use serde::{Deserialize, Serialize};

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
    pub cost_reduction_pct: f64,
    pub total_actual_failures: usize,
    pub false_negatives: usize,
    pub false_accepts: usize,
}

impl CalibrationMetrics {
    pub fn compute(pairs: &[DecisionOutcomePair], default_threshold: f64) -> Self {
        if pairs.is_empty() {
            return Self::default();
        }

        let n = pairs.len() as f64;
        let mut brier_sum = 0.0;
        let mut tp = 0; // predicted success & actual success
        let mut fp = 0; // predicted success & actual failure
        let mut fn_cnt = 0; // predicted failure & actual success
        let mut tn = 0; // predicted failure & actual failure

        let mut accepted_count = 0;
        let mut accepted_correct = 0;
        let mut accepted_failed = 0;

        let mut escalated_count = 0;
        let mut escalated_would_succeed = 0;
        let mut total_actual_failures = 0;

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
                if actual_success {
                    accepted_correct += 1;
                } else {
                    accepted_failed += 1;
                }
            } else {
                escalated_count += 1;
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

        // Frontier verifier calls avoided = decisions that did not require frontier verification
        let frontier_calls_avoided = pairs.len().saturating_sub(escalated_count);
        let frontier_calls_avoided_pct = (frontier_calls_avoided as f64 / n) * 100.0;

        // Baseline cost: every task sent to frontier model ($0.02)
        // Reflex cost: System-1 ($0.0001) + escalated frontier ($0.02) + cheap verifier ($0.005)
        let baseline_cost = n * 0.02;
        let reflex_cost = (n * 0.0001)
            + (escalated_count as f64 * 0.02)
            + ((n - accepted_count as f64 - escalated_count as f64).max(0.0) * 0.005);
        let cost_reduction_pct = if baseline_cost > 0.0 {
            ((baseline_cost - reflex_cost) / baseline_cost) * 100.0
        } else {
            0.0
        };

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
            cost_reduction_pct,
            total_actual_failures,
            false_negatives,
            false_accepts,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calibration_metrics() {
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
        assert!(metrics.frontier_calls_avoided > 0);
    }
}
