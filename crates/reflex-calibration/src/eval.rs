use crate::stats::{wilson_score_interval, ConfidenceInterval};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Performance metric for a single class in a multiclass or binary classification task
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassMetric {
    pub class_name: String,
    pub true_positives: usize,
    pub false_positives: usize,
    pub false_negatives: usize,
    pub support: usize, // Total actual occurrences of this class
    pub precision: f64,
    pub recall: f64,
    pub f1: f64,
}

/// Generic Multiclass Confusion Matrix
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MulticlassConfusionMatrix {
    pub classes: Vec<String>,
    // counts[actual_idx][predicted_idx]
    pub counts: Vec<Vec<usize>>,
    pub total: usize,
}

impl MulticlassConfusionMatrix {
    pub fn new(classes: Vec<String>) -> Self {
        let n = classes.len();
        Self {
            classes,
            counts: vec![vec![0; n]; n],
            total: 0,
        }
    }

    pub fn record(&mut self, actual: &str, predicted: &str) {
        let act_idx = self
            .classes
            .iter()
            .position(|c| c.eq_ignore_ascii_case(actual));
        let pred_idx = self
            .classes
            .iter()
            .position(|c| c.eq_ignore_ascii_case(predicted));

        if let (Some(a), Some(p)) = (act_idx, pred_idx) {
            self.counts[a][p] += 1;
            self.total += 1;
        }
    }

    pub fn accuracy(&self) -> f64 {
        if self.total == 0 {
            return 0.0;
        }
        let diagonal: usize = (0..self.classes.len()).map(|i| self.counts[i][i]).sum();
        diagonal as f64 / self.total as f64
    }

    pub fn compute_class_metrics(&self) -> BTreeMap<String, ClassMetric> {
        let mut map = BTreeMap::new();
        for (i, class) in self.classes.iter().enumerate() {
            let tp = self.counts[i][i];
            let fp: usize = (0..self.classes.len())
                .filter(|&r| r != i)
                .map(|r| self.counts[r][i])
                .sum();
            let fn_cnt: usize = (0..self.classes.len())
                .filter(|&c| c != i)
                .map(|c| self.counts[i][c])
                .sum();
            let support: usize = self.counts[i].iter().sum();

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

            map.insert(
                class.clone(),
                ClassMetric {
                    class_name: class.clone(),
                    true_positives: tp,
                    false_positives: fp,
                    false_negatives: fn_cnt,
                    support,
                    precision,
                    recall,
                    f1,
                },
            );
        }
        map
    }

    pub fn macro_f1(&self) -> f64 {
        let metrics = self.compute_class_metrics();
        let active: Vec<&ClassMetric> = metrics.values().filter(|m| m.support > 0).collect();
        if active.is_empty() {
            return 0.0;
        }
        let sum_f1: f64 = active.iter().map(|m| m.f1).sum();
        sum_f1 / active.len() as f64
    }

    pub fn format_table(&self) -> String {
        let mut out = String::new();
        // Header
        out.push_str("                        Actual ->\n");
        let mut header = format!("{:<22}", "Predicted");
        for c in &self.classes {
            header.push_str(&format!("{c:>14}"));
        }
        header.push_str(&format!("{:>14}\n", "Pred Total"));
        out.push_str(&header);
        out.push_str(&format!(
            "{:-<width$}\n",
            "",
            width = 22 + (self.classes.len() + 1) * 14
        ));

        // For each predicted class (row)
        for (p_idx, p_class) in self.classes.iter().enumerate() {
            let mut row = format!("{p_class:<22}");
            let mut p_total = 0;
            for a_idx in 0..self.classes.len() {
                let cnt = self.counts[a_idx][p_idx];
                p_total += cnt;
                row.push_str(&format!("{cnt:>14}"));
            }
            row.push_str(&format!("{p_total:>14}\n"));
            out.push_str(&row);
        }
        out.push_str(&format!(
            "{:-<width$}\n",
            "",
            width = 22 + (self.classes.len() + 1) * 14
        ));

        // Actual totals
        let mut actual_row = format!("{:<22}", "Actual Total");
        for a_idx in 0..self.classes.len() {
            let total_act: usize = self.counts[a_idx].iter().sum();
            actual_row.push_str(&format!("{total_act:>14}"));
        }
        actual_row.push_str(&format!("{:>14}\n", self.total));
        out.push_str(&actual_row);

        out
    }
}

/// Binary Confusion Matrix with strictly defined safety terminology
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinaryConfusionMatrix {
    pub positive_label: String,
    pub negative_label: String,
    pub true_positives: usize,  // Predicted Positive, Actual Positive
    pub false_positives: usize, // Predicted Positive, Actual Negative (False Alarm)
    pub false_negatives: usize, // Predicted Negative, Actual Positive (Missed Positive / False Accept)
    pub true_negatives: usize,  // Predicted Negative, Actual Negative
    pub total: usize,
}

impl BinaryConfusionMatrix {
    pub fn new(positive_label: impl Into<String>, negative_label: impl Into<String>) -> Self {
        Self {
            positive_label: positive_label.into(),
            negative_label: negative_label.into(),
            true_positives: 0,
            false_positives: 0,
            false_negatives: 0,
            true_negatives: 0,
            total: 0,
        }
    }

    pub fn record(&mut self, is_actual_positive: bool, is_predicted_positive: bool) {
        self.total += 1;
        match (is_predicted_positive, is_actual_positive) {
            (true, true) => self.true_positives += 1,
            (true, false) => self.false_positives += 1,
            (false, true) => self.false_negatives += 1,
            (false, false) => self.true_negatives += 1,
        }
    }

    pub fn accuracy(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            (self.true_positives + self.true_negatives) as f64 / self.total as f64
        }
    }

    pub fn precision(&self) -> f64 {
        if self.true_positives + self.false_positives == 0 {
            0.0
        } else {
            self.true_positives as f64 / (self.true_positives + self.false_positives) as f64
        }
    }

    pub fn recall(&self) -> f64 {
        if self.true_positives + self.false_negatives == 0 {
            0.0
        } else {
            self.true_positives as f64 / (self.true_positives + self.false_negatives) as f64
        }
    }

    pub fn specificity(&self) -> f64 {
        if self.true_negatives + self.false_positives == 0 {
            0.0
        } else {
            self.true_negatives as f64 / (self.true_negatives + self.false_positives) as f64
        }
    }

    pub fn balanced_accuracy(&self) -> f64 {
        (self.recall() + self.specificity()) / 2.0
    }

    pub fn f1(&self) -> f64 {
        let p = self.precision();
        let r = self.recall();
        if p + r > 0.0 {
            2.0 * (p * r) / (p + r)
        } else {
            0.0
        }
    }

    /// False Negative Rate: FN / (TP + FN)
    /// Fraction of actual positive instances (e.g. defects) that were missed.
    pub fn false_negative_rate(&self) -> f64 {
        let actual_positives = self.true_positives + self.false_negatives;
        if actual_positives == 0 {
            0.0
        } else {
            self.false_negatives as f64 / actual_positives as f64
        }
    }

    /// False Positive Rate / False Alarm Rate: FP / (TN + FP)
    /// Fraction of actual negative instances (e.g. clean tasks) that were incorrectly flagged.
    pub fn false_positive_rate(&self) -> f64 {
        let actual_negatives = self.true_negatives + self.false_positives;
        if actual_negatives == 0 {
            0.0
        } else {
            self.false_positives as f64 / actual_negatives as f64
        }
    }

    /// False Accept Rate: FN / (TN + FN)
    /// Fraction of predicted negative instances (e.g. autonomous passes) that are actually defective.
    pub fn false_accept_rate(&self) -> f64 {
        let predicted_negatives = self.true_negatives + self.false_negatives;
        if predicted_negatives == 0 {
            0.0
        } else {
            self.false_negatives as f64 / predicted_negatives as f64
        }
    }

    pub fn format_table(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "                        [Actual {}]    [Actual {}]\n",
            self.positive_label, self.negative_label
        ));
        out.push_str(&format!(
            "  [Predicted {:<9}]   {:>14}    {:>15}\n",
            self.positive_label, self.true_positives, self.false_positives
        ));
        out.push_str(&format!(
            "  [Predicted {:<9}]   {:>14}    {:>15}\n",
            self.negative_label, self.false_negatives, self.true_negatives
        ));
        out
    }
}

/// Routing evaluation metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingMetrics {
    pub total_samples: usize,
    pub accuracy: f64,
    pub macro_f1: f64,
    pub class_metrics: BTreeMap<String, ClassMetric>,
    pub confusion_matrix: MulticlassConfusionMatrix,
}

/// Retry evaluation metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryMetrics {
    pub total_samples: usize,
    pub accuracy: f64,
    pub macro_f1: f64,
    pub class_metrics: BTreeMap<String, ClassMetric>,
    pub confusion_matrix: MulticlassConfusionMatrix,
}

/// Termination evaluation metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminationMetrics {
    pub total_samples: usize,
    pub accuracy: f64,
    pub balanced_accuracy: f64,
    pub precision: f64, // for "terminate"
    pub recall: f64,    // for "terminate"
    pub f1: f64,        // for "terminate"
    pub confusion_matrix: BinaryConfusionMatrix,
    pub brier_score: Option<f64>,
    pub ece: Option<f64>,
}

/// Polarity for verification probability interpretation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerificationPolarity {
    /// High score means Clean / Safe to Accept. Defect flagged when score < threshold.
    CleanProbability,
    /// High score means Defect / Requires Verification. Defect flagged when score >= threshold.
    DefectProbability,
}

/// Safety-critical Verification Gate evaluation metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationMetrics {
    pub total_samples: usize,
    pub threshold: f64,
    pub confusion_matrix: BinaryConfusionMatrix,

    // Safety-critical Rate Definitions:
    /// False Negative Rate: FN / (TP + FN) = fraction of actual defects missed
    pub false_negative_rate: f64,
    pub fnr_ci: ConfidenceInterval,

    /// False Positive Rate / False Alarm Rate: FP / (TN + FP) = fraction of clean tasks unnecessarily sent to verifier
    pub false_positive_rate: f64,
    pub fpr_ci: ConfidenceInterval,

    /// False Accept Rate: FN / (TN + FN) = fraction of autonomous passes that are defective
    pub false_accept_rate: f64,
    pub far_ci: ConfidenceInterval,

    pub precision: f64, // Defect detection precision: TP / (TP + FP)
    pub recall: f64,    // Defect catch rate (Recall): TP / (TP + FN) = 1 - FNR
    pub f1: f64,        // Defect detection F1
    pub accuracy: f64,
    pub balanced_accuracy: f64,
    pub brier_score: f64,
    pub ece: f64,
    pub automation_coverage: f64, // fraction of tasks passed autonomously: (TN + FN) / total
    pub coverage_ci: ConfidenceInterval,
    pub projected_frontier_calls_avoided_pct: f64,
    pub projected_frontier_calls_avoided_ci: ConfidenceInterval,
}

/// Single point in an empirical threshold sweep
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThresholdSweepPoint {
    pub threshold: f64,
    pub accuracy: f64,
    pub far: f64,
    pub fnr: f64,
    pub fpr: f64,
    pub precision: f64,
    pub recall: f64,
    pub f1: f64,
    pub coverage: f64,
}

// ─────────────────────────────────────────────────────────────────────────────
// Evaluation Functions
// ─────────────────────────────────────────────────────────────────────────────

pub fn evaluate_routing(samples: &[(&str, &str)]) -> RoutingMetrics {
    let classes = vec![
        "accept".to_string(),
        "verify".to_string(),
        "escalate".to_string(),
    ];
    let mut matrix = MulticlassConfusionMatrix::new(classes);
    for &(actual, predicted) in samples {
        matrix.record(actual, predicted);
    }

    let accuracy = matrix.accuracy();
    let macro_f1 = matrix.macro_f1();
    let class_metrics = matrix.compute_class_metrics();

    RoutingMetrics {
        total_samples: samples.len(),
        accuracy,
        macro_f1,
        class_metrics,
        confusion_matrix: matrix,
    }
}

pub fn evaluate_retry(samples: &[(&str, &str)]) -> RetryMetrics {
    let classes = vec![
        "retry".to_string(),
        "escalate".to_string(),
        "terminate".to_string(),
    ];
    let mut matrix = MulticlassConfusionMatrix::new(classes);
    for &(actual, predicted) in samples {
        matrix.record(actual, predicted);
    }

    let accuracy = matrix.accuracy();
    let macro_f1 = matrix.macro_f1();
    let class_metrics = matrix.compute_class_metrics();

    RetryMetrics {
        total_samples: samples.len(),
        accuracy,
        macro_f1,
        class_metrics,
        confusion_matrix: matrix,
    }
}

pub fn evaluate_termination(samples: &[(&str, &str, Option<f64>)]) -> TerminationMetrics {
    let mut matrix = BinaryConfusionMatrix::new("terminate", "continue");
    let mut prob_pairs = Vec::new();

    for &(actual, predicted, p_term_opt) in samples {
        let is_act_term = actual.eq_ignore_ascii_case("terminate");
        let is_pred_term = predicted.eq_ignore_ascii_case("terminate");
        matrix.record(is_act_term, is_pred_term);

        if let Some(p) = p_term_opt {
            prob_pairs.push((p.clamp(0.0, 1.0), is_act_term));
        }
    }

    let (brier_score, ece) = if !prob_pairs.is_empty() {
        let n = prob_pairs.len() as f64;
        let brier_sum: f64 = prob_pairs
            .iter()
            .map(|&(p, is_term)| {
                let y = if is_term { 1.0 } else { 0.0 };
                (p - y).powi(2)
            })
            .sum();
        let brier = brier_sum / n;

        // ECE with 5 bins
        let num_bins = 5;
        let mut bin_counts = vec![0usize; num_bins];
        let mut bin_conf_sum = vec![0.0f64; num_bins];
        let mut bin_correct_sum = vec![0.0f64; num_bins];

        for &(p, is_term) in &prob_pairs {
            let idx = ((p * num_bins as f64).floor() as usize).min(num_bins - 1);
            bin_counts[idx] += 1;
            bin_conf_sum[idx] += p;
            if is_term {
                bin_correct_sum[idx] += 1.0;
            }
        }

        let mut ece_val = 0.0;
        for i in 0..num_bins {
            let count = bin_counts[i];
            if count > 0 {
                let bin_conf = bin_conf_sum[i] / count as f64;
                let bin_acc = bin_correct_sum[i] / count as f64;
                let bin_weight = count as f64 / n;
                ece_val += bin_weight * (bin_acc - bin_conf).abs();
            }
        }

        (Some(brier), Some(ece_val))
    } else {
        (None, None)
    };

    TerminationMetrics {
        total_samples: samples.len(),
        accuracy: matrix.accuracy(),
        balanced_accuracy: matrix.balanced_accuracy(),
        precision: matrix.precision(),
        recall: matrix.recall(),
        f1: matrix.f1(),
        confusion_matrix: matrix,
        brier_score,
        ece,
    }
}

pub fn evaluate_verification(
    samples: &[(bool, f64)], // (actual_is_defect, p_clean)
    threshold: f64,
) -> VerificationMetrics {
    evaluate_verification_with_polarity(samples, threshold, VerificationPolarity::CleanProbability)
}

pub fn evaluate_verification_with_polarity(
    samples: &[(bool, f64)], // (actual_is_defect, score)
    threshold: f64,
    polarity: VerificationPolarity,
) -> VerificationMetrics {
    let mut matrix = BinaryConfusionMatrix::new("defect (verify)", "clean (skip)");
    let n = samples.len();

    let mut brier_sum = 0.0;
    let num_bins = 5;
    let mut bin_counts = vec![0usize; num_bins];
    let mut bin_conf_sum = vec![0.0f64; num_bins];
    let mut bin_correct_sum = vec![0.0f64; num_bins];

    for &(actual_defect, score) in samples {
        let score_clamped = score.clamp(0.0, 1.0);

        let pred_defect = match polarity {
            VerificationPolarity::CleanProbability => score_clamped < threshold,
            VerificationPolarity::DefectProbability => score_clamped >= threshold,
        };

        matrix.record(actual_defect, pred_defect);

        // Calibration evaluation based on semantic polarity
        let (p_eval, y_eval) = match polarity {
            VerificationPolarity::CleanProbability => {
                let actual_clean = !actual_defect;
                let y = if actual_clean { 1.0 } else { 0.0 };
                (score_clamped, y)
            }
            VerificationPolarity::DefectProbability => {
                let y = if actual_defect { 1.0 } else { 0.0 };
                (score_clamped, y)
            }
        };

        brier_sum += (p_eval - y_eval).powi(2);

        let bin_idx = ((p_eval * num_bins as f64).floor() as usize).min(num_bins - 1);
        bin_counts[bin_idx] += 1;
        bin_conf_sum[bin_idx] += p_eval;
        if y_eval > 0.5 {
            bin_correct_sum[bin_idx] += 1.0;
        }
    }

    let brier_score = if n > 0 { brier_sum / n as f64 } else { 0.0 };

    let mut ece = 0.0;
    if n > 0 {
        for i in 0..num_bins {
            let count = bin_counts[i];
            if count > 0 {
                let bin_conf = bin_conf_sum[i] / count as f64;
                let bin_acc = bin_correct_sum[i] / count as f64;
                let bin_weight = count as f64 / n as f64;
                ece += bin_weight * (bin_acc - bin_conf).abs();
            }
        }
    }

    // Safety metrics terminology strictly enforced:
    // 1. False Negatives (FN) = actual defect incorrectly allowed to pass / skipped
    let total_actual_defects = matrix.true_positives + matrix.false_negatives;
    let false_negatives = matrix.false_negatives;
    let false_negative_rate = if total_actual_defects > 0 {
        false_negatives as f64 / total_actual_defects as f64
    } else {
        0.0
    };
    let fnr_ci = wilson_score_interval(false_negatives, total_actual_defects, 0.95);

    // 2. False Positives (FP) / False Alarms = clean task unnecessarily sent to verifier
    let total_actual_clean = matrix.true_negatives + matrix.false_positives;
    let false_positives = matrix.false_positives;
    let false_positive_rate = if total_actual_clean > 0 {
        false_positives as f64 / total_actual_clean as f64
    } else {
        0.0
    };
    let fpr_ci = wilson_score_interval(false_positives, total_actual_clean, 0.95);

    // 3. False Accept Rate (FAR) = fraction of autonomous passes that contain defects
    let autonomous_passes = matrix.true_negatives + matrix.false_negatives;
    let false_accept_rate = if autonomous_passes > 0 {
        false_negatives as f64 / autonomous_passes as f64
    } else {
        0.0
    };
    let far_ci = wilson_score_interval(false_negatives, autonomous_passes, 0.95);

    let coverage = if n > 0 {
        autonomous_passes as f64 / n as f64
    } else {
        0.0
    };
    let coverage_ci = wilson_score_interval(autonomous_passes, n, 0.95);
    let calls_avoided_ci = wilson_score_interval(autonomous_passes, n, 0.95);

    VerificationMetrics {
        total_samples: n,
        threshold,
        accuracy: matrix.accuracy(),
        balanced_accuracy: matrix.balanced_accuracy(),
        precision: matrix.precision(),
        recall: matrix.recall(),
        f1: matrix.f1(),
        confusion_matrix: matrix,
        false_negative_rate,
        fnr_ci,
        false_positive_rate,
        fpr_ci,
        false_accept_rate,
        far_ci,
        brier_score,
        ece,
        automation_coverage: coverage,
        coverage_ci,
        projected_frontier_calls_avoided_pct: coverage * 100.0,
        projected_frontier_calls_avoided_ci: calls_avoided_ci,
    }
}

pub fn sweep_verification_thresholds(
    samples: &[(bool, f64)],
    thresholds: &[f64],
) -> Vec<ThresholdSweepPoint> {
    sweep_verification_thresholds_with_polarity(
        samples,
        thresholds,
        VerificationPolarity::CleanProbability,
    )
}

pub fn sweep_verification_thresholds_with_polarity(
    samples: &[(bool, f64)],
    thresholds: &[f64],
    polarity: VerificationPolarity,
) -> Vec<ThresholdSweepPoint> {
    thresholds
        .iter()
        .map(|&th| {
            let m = evaluate_verification_with_polarity(samples, th, polarity);
            ThresholdSweepPoint {
                threshold: th,
                accuracy: m.accuracy,
                far: m.false_accept_rate,
                fnr: m.false_negative_rate,
                fpr: m.false_positive_rate,
                precision: m.precision,
                recall: m.recall,
                f1: m.f1,
                coverage: m.automation_coverage,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_multiclass_confusion_matrix() {
        let classes = vec![
            "accept".to_string(),
            "verify".to_string(),
            "escalate".to_string(),
        ];
        let mut matrix = MulticlassConfusionMatrix::new(classes);

        matrix.record("accept", "accept");
        matrix.record("accept", "accept");
        matrix.record("accept", "verify");
        matrix.record("verify", "verify");
        matrix.record("escalate", "escalate");

        assert_eq!(matrix.total, 5);
        assert!((matrix.accuracy() - 0.80).abs() < 1e-4);

        let metrics = matrix.compute_class_metrics();
        assert_eq!(metrics["accept"].true_positives, 2);
        assert_eq!(metrics["accept"].false_negatives, 1);
        assert_eq!(metrics["verify"].false_positives, 1);
        assert!(matrix.macro_f1() > 0.0);
    }

    #[test]
    fn test_verification_confusion_matrix_definitions_and_formulas() {
        // Construct a ground truth test case:
        // Total samples: 10
        // Actual defects: 4
        // Actual clean: 6
        //
        // Classifier output:
        // Flagged for verification (predicted defect): 5
        //   - 3 are real defects -> TP = 3
        //   - 2 are clean -> FP = 2 (False Alarm / False Positive)
        // Autonomous passes (predicted clean): 5
        //   - 1 is a defect -> FN = 1 (Missed Defect / False Negative)
        //   - 4 are clean -> TN = 4

        let mut m = BinaryConfusionMatrix::new("defect (verify)", "clean (skip)");

        // 3 TP
        m.record(true, true);
        m.record(true, true);
        m.record(true, true);

        // 2 FP: clean task unnecessarily sent to verifier (False Alarm)
        m.record(false, true);
        m.record(false, true);

        // 1 FN: actual defect incorrectly allowed to pass (Missed Defect)
        m.record(true, false);

        // 4 TN: clean task correctly allowed to pass
        m.record(false, false);
        m.record(false, false);
        m.record(false, false);
        m.record(false, false);

        assert_eq!(m.total, 10);
        assert_eq!(m.true_positives, 3, "TP must be 3");
        assert_eq!(m.false_positives, 2, "FP (False Alarm) must be 2");
        assert_eq!(m.false_negatives, 1, "FN (Missed Defect) must be 1");
        assert_eq!(m.true_negatives, 4, "TN must be 4");

        // Accuracy = (TP + TN) / Total = (3 + 4) / 10 = 0.70
        assert!((m.accuracy() - 0.70).abs() < 1e-6);

        // Precision = TP / (TP + FP) = 3 / 5 = 0.60
        assert!((m.precision() - 0.60).abs() < 1e-6);

        // Recall (Catch Rate) = TP / (TP + FN) = 3 / 4 = 0.75
        assert!((m.recall() - 0.75).abs() < 1e-6);

        // Specificity = TN / (TN + FP) = 4 / 6 = 0.6667
        assert!((m.specificity() - (4.0 / 6.0)).abs() < 1e-6);

        // Balanced Accuracy = (Recall + Specificity) / 2 = (0.75 + 4/6) / 2 = 0.70833
        assert!((m.balanced_accuracy() - ((0.75 + 4.0 / 6.0) / 2.0)).abs() < 1e-6);

        // False Negative Rate = FN / (TP + FN) = 1 / 4 = 0.25 (missed defect rate)
        assert!((m.false_negative_rate() - 0.25).abs() < 1e-6);
        assert!(((1.0 - m.recall()) - m.false_negative_rate()).abs() < 1e-6);

        // False Positive Rate = FP / (TN + FP) = 2 / 6 = 0.3333 (false alarm rate)
        assert!((m.false_positive_rate() - (2.0 / 6.0)).abs() < 1e-6);
        assert!(((1.0 - m.specificity()) - m.false_positive_rate()).abs() < 1e-6);

        // False Accept Rate = FN / (TN + FN) = 1 / (4 + 1) = 0.20 (defect rate in autonomous passes)
        assert!((m.false_accept_rate() - 0.20).abs() < 1e-6);

        // Verify FAR != FNR to prevent any confusion
        assert!((m.false_accept_rate() - m.false_negative_rate()).abs() > 0.01);
    }

    #[test]
    fn test_verification_polarity_evaluation() {
        // Two samples:
        // 1. Defect sample (actual_is_defect = true)
        // 2. Clean sample (actual_is_defect = false)

        // Direction A: CleanProbability (0.90 clean, 0.20 clean)
        let samples_clean = vec![
            (true, 0.20),  // defect with low clean probability -> flagged as defect
            (false, 0.90), // clean with high clean probability -> passed as clean
        ];
        let vm_clean = evaluate_verification_with_polarity(
            &samples_clean,
            0.50,
            VerificationPolarity::CleanProbability,
        );
        assert_eq!(vm_clean.confusion_matrix.true_positives, 1);
        assert_eq!(vm_clean.confusion_matrix.true_negatives, 1);
        assert_eq!(vm_clean.false_negative_rate, 0.0);
        assert_eq!(vm_clean.false_positive_rate, 0.0);
        assert_eq!(vm_clean.accuracy, 1.0);

        // Direction B: DefectProbability (0.80 defect, 0.10 defect)
        let samples_defect = vec![
            (true, 0.80),  // defect with high defect probability -> flagged as defect
            (false, 0.10), // clean with low defect probability -> passed as clean
        ];
        let vm_defect = evaluate_verification_with_polarity(
            &samples_defect,
            0.50,
            VerificationPolarity::DefectProbability,
        );
        assert_eq!(vm_defect.confusion_matrix.true_positives, 1);
        assert_eq!(vm_defect.confusion_matrix.true_negatives, 1);
        assert_eq!(vm_defect.false_negative_rate, 0.0);
        assert_eq!(vm_defect.false_positive_rate, 0.0);
        assert_eq!(vm_defect.accuracy, 1.0);
    }
}
