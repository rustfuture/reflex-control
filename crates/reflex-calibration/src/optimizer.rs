use crate::metrics::DecisionOutcomePair;
use crate::stats::{wilson_score_interval, ConfidenceInterval};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationConstraints {
    pub current_threshold: f64,
    pub max_false_accept_rate: f64,
    pub max_false_negative_rate: f64,
    pub min_coverage: f64,
}

impl Default for OptimizationConstraints {
    fn default() -> Self {
        Self {
            current_threshold: 0.90,
            max_false_accept_rate: 0.01,
            max_false_negative_rate: 0.01,
            min_coverage: 0.50,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationResult {
    pub current_threshold: f64,
    pub current_coverage: f64,
    pub current_false_accept_rate: f64,
    pub current_false_negative_rate: f64,
    pub recommended_threshold: f64,
    pub expected_coverage: f64,
    pub expected_frontier_calls_avoided_pct: f64,
    pub expected_cost_reduction_pct: f64,
    pub expected_false_accept_rate: f64,
    pub expected_false_negative_rate: f64,
    pub far_ci: ConfidenceInterval,
    pub fnr_ci: ConfidenceInterval,
    pub coverage_ci: ConfidenceInterval,
    pub calls_avoided_ci: ConfidenceInterval,
    pub is_feasible: bool,
    pub is_statistically_proven: bool,
    pub explanation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParetoPoint {
    pub threshold: f64,
    pub coverage_pct: f64,
    pub calls_avoided_pct: f64,
    pub cost_reduction_pct: f64,
    pub false_accept_rate: f64,
    pub false_negative_rate: f64,
    pub far_ci: ConfidenceInterval,
    pub fnr_ci: ConfidenceInterval,
    pub is_target_met: bool,
    pub is_statistically_proven: bool,
    pub is_pareto_optimal: bool,
}

pub struct ThresholdOptimizer;

impl ThresholdOptimizer {
    /// Evaluate metrics for a specific threshold cutoff tau
    /// Returns: (coverage, far, fnr, calls_avoided_pct, cost_reduction_pct, accepted_failed, accepted_total, total_actual_failures, calls_avoided)
    pub fn evaluate_cutoff(
        pairs: &[DecisionOutcomePair],
        tau: f64,
    ) -> (f64, f64, f64, f64, f64, usize, usize, usize, usize) {
        let resolved_pairs: Vec<_> = pairs
            .iter()
            .filter(|pair| pair.outcome.is_resolved())
            .collect();
        if resolved_pairs.is_empty() {
            return (0.0, 0.0, 0.0, 0.0, 0.0, 0, 0, 0, 0);
        }
        let n = resolved_pairs.len() as f64;
        let mut accepted_total = 0usize;
        let mut accepted_failed = 0usize;
        let mut total_actual_failures = 0usize;
        let mut escalated_total = 0usize;
        let mut cheap_verified_resolved = 0usize;

        for p in resolved_pairs {
            let actual_success = p.is_success();
            if p.outcome.is_failure() {
                total_actual_failures += 1;
            }

            let is_accepted = p.confidence >= tau;
            if is_accepted {
                accepted_total += 1;
                if !actual_success {
                    accepted_failed += 1;
                }
            } else if p.confidence >= 0.65 {
                // Tier 2: fast cheap verifier ($0.005)
                if actual_success {
                    // Fast verifier successfully confirms clean outcome, avoiding frontier LLM
                    cheap_verified_resolved += 1;
                } else {
                    // Fast verifier catches defect and escalates to frontier model
                    escalated_total += 1;
                }
            } else {
                // Low confidence: immediate frontier escalation
                escalated_total += 1;
            }
        }

        let coverage = accepted_total as f64 / n;
        let far = if accepted_total > 0 {
            accepted_failed as f64 / accepted_total as f64
        } else {
            0.0
        };
        let fnr = if total_actual_failures > 0 {
            accepted_failed as f64 / total_actual_failures as f64
        } else {
            0.0
        };

        // Mathematically correct frontier calls avoided:
        // Tasks accepted directly (no verifier) + tasks resolved clean by cheap verifier.
        // Defective tasks in cheap verify tier escalate to frontier model.
        let calls_avoided = accepted_total + cheap_verified_resolved;
        let calls_avoided_pct = (calls_avoided as f64 / n) * 100.0;

        let baseline_cost = n * 0.02;
        let reflex_cost = (accepted_total as f64 * 0.0001)
            + (cheap_verified_resolved as f64 * (0.0001 + 0.005))
            + (escalated_total as f64 * (0.0001 + 0.02));
        let cost_reduction_pct = if baseline_cost > 0.0 {
            ((baseline_cost - reflex_cost) / baseline_cost) * 100.0
        } else {
            0.0
        };

        (
            coverage,
            far,
            fnr,
            calls_avoided_pct,
            cost_reduction_pct,
            accepted_failed,
            accepted_total,
            total_actual_failures,
            calls_avoided,
        )
    }

    pub fn optimize(
        pairs: &[DecisionOutcomePair],
        constraints: &OptimizationConstraints,
    ) -> OptimizationResult {
        if pairs.is_empty() || pairs.iter().all(|pair| !pair.outcome.is_resolved()) {
            let default_ci = ConfidenceInterval::default();
            return OptimizationResult {
                current_threshold: constraints.current_threshold,
                current_coverage: 0.0,
                current_false_accept_rate: 0.0,
                current_false_negative_rate: 0.0,
                recommended_threshold: constraints.current_threshold,
                expected_coverage: 0.0,
                expected_frontier_calls_avoided_pct: 0.0,
                expected_cost_reduction_pct: 0.0,
                expected_false_accept_rate: 0.0,
                expected_false_negative_rate: 0.0,
                far_ci: default_ci,
                fnr_ci: default_ci,
                coverage_ci: default_ci,
                calls_avoided_ci: default_ci,
                is_feasible: false,
                is_statistically_proven: false,
                explanation: if pairs.is_empty() {
                    "Dataset is empty; cannot optimize threshold.".to_string()
                } else {
                    "Dataset contains no resolved success/failure outcomes; cannot optimize threshold."
                        .to_string()
                },
            };
        }

        let (curr_cov, curr_far, curr_fnr, _, _, _, _, _, _) =
            Self::evaluate_cutoff(pairs, constraints.current_threshold);

        // Search candidate thresholds from 0.500 to 0.995 in steps of 0.001
        let mut best_threshold = constraints.current_threshold;
        let mut best_coverage = 0.0;
        let mut best_far = 1.0;
        let mut best_fnr = 1.0;
        let mut found_feasible = false;

        let mut step = 500;
        while step <= 995 {
            let tau = step as f64 / 1000.0;
            let (cov, far, fnr, _, _, _, accepted_total, total_actual_failures, _) =
                Self::evaluate_cutoff(pairs, tau);

            let has_safety_denominators = accepted_total > 0 && total_actual_failures > 0;
            let satisfies_safety = has_safety_denominators
                && far <= constraints.max_false_accept_rate
                && fnr <= constraints.max_false_negative_rate;
            let satisfies_coverage = cov >= constraints.min_coverage;

            if satisfies_safety && satisfies_coverage {
                // Feasible: prioritize maximizing coverage while keeping FAR and FNR below target
                if !found_feasible
                    || cov > best_coverage
                    || (cov == best_coverage && (far + fnr) < (best_far + best_fnr))
                {
                    found_feasible = true;
                    best_threshold = tau;
                    best_coverage = cov;
                    best_far = far;
                    best_fnr = fnr;
                }
            } else if !found_feasible {
                // Best effort safety first: satisfy FAR & FNR if possible
                let is_better = (satisfies_safety && cov > best_coverage)
                    || (has_safety_denominators
                        && far <= constraints.max_false_accept_rate
                        && (cov > best_coverage || best_far > constraints.max_false_accept_rate));
                if is_better {
                    best_threshold = tau;
                    best_coverage = cov;
                    best_far = far;
                    best_fnr = fnr;
                }
            }
            step += 1;
        }

        let (
            exp_cov,
            exp_far,
            exp_fnr,
            exp_calls_avoided,
            exp_cost_red,
            exp_accepted_failed,
            exp_accepted_total,
            exp_total_actual_failures,
            exp_calls_avoided_count,
        ) = Self::evaluate_cutoff(pairs, best_threshold);

        let far_ci = wilson_score_interval(exp_accepted_failed, exp_accepted_total, 0.95);
        let fnr_ci = wilson_score_interval(exp_accepted_failed, exp_total_actual_failures, 0.95);
        let resolved_count = pairs
            .iter()
            .filter(|pair| pair.outcome.is_resolved())
            .count();
        let coverage_ci = wilson_score_interval(exp_accepted_total, resolved_count, 0.95);
        let calls_avoided_ci = wilson_score_interval(exp_calls_avoided_count, resolved_count, 0.95);

        let is_statistically_proven = far_ci
            .is_upper_bound_proven(constraints.max_false_accept_rate)
            && fnr_ci.is_upper_bound_proven(constraints.max_false_negative_rate);

        let far_observed = format_rate(exp_far, exp_accepted_total);
        let fnr_observed = format_rate(exp_fnr, exp_total_actual_failures);
        let explanation = if found_feasible {
            let proven_tag = if is_statistically_proven {
                format!(
                    "Statistically proven <{:.1}% at 95% confidence (FAR upper: {:.2}%, FNR upper: {:.2}%).",
                    constraints.max_false_accept_rate * 100.0,
                    far_ci.upper * 100.0,
                    fnr_ci.upper * 100.0
                )
            } else {
                format!(
                    "Early signal only: sample size (N={}) yields 95% CI upper bound of {:.2}% FAR (insufficient to formally prove <{:.1}%).",
                    exp_accepted_total,
                    far_ci.upper * 100.0,
                    constraints.max_false_accept_rate * 100.0
                )
            };
            format!(
                "Optimal calibrated threshold is {:.3}. Observed FAR = {}, observed FNR = {}, with {:.1}% coverage ({:.1}% verifier calls avoided). {}",
                best_threshold,
                far_observed,
                fnr_observed,
                exp_cov * 100.0,
                exp_calls_avoided,
                proven_tag
            )
        } else {
            format!(
                "Recommended best-effort safety threshold is {:.3} (observed FAR: {}, observed FNR: {}, Coverage: {:.1}%).",
                best_threshold,
                far_observed,
                fnr_observed,
                exp_cov * 100.0
            )
        };

        OptimizationResult {
            current_threshold: constraints.current_threshold,
            current_coverage: curr_cov,
            current_false_accept_rate: curr_far,
            current_false_negative_rate: curr_fnr,
            recommended_threshold: best_threshold,
            expected_coverage: exp_cov,
            expected_frontier_calls_avoided_pct: exp_calls_avoided,
            expected_cost_reduction_pct: exp_cost_red,
            expected_false_accept_rate: exp_far,
            expected_false_negative_rate: exp_fnr,
            far_ci,
            fnr_ci,
            coverage_ci,
            calls_avoided_ci,
            is_feasible: found_feasible,
            is_statistically_proven,
            explanation,
        }
    }

    /// Computes the Cost vs Risk Pareto Frontier across candidate thresholds
    pub fn pareto_frontier(pairs: &[DecisionOutcomePair]) -> Vec<ParetoPoint> {
        if pairs.is_empty() || pairs.iter().all(|pair| !pair.outcome.is_resolved()) {
            return Vec::new();
        }

        let candidate_steps = [
            0.70, 0.75, 0.80, 0.82, 0.85, 0.88, 0.90, 0.92, 0.94, 0.95, 0.96, 0.97, 0.98,
        ];

        let mut points: Vec<ParetoPoint> = candidate_steps
            .iter()
            .map(|&tau| {
                let (cov, far, fnr, calls_avoided, cost_red, acc_failed, acc_total, total_fail, _) =
                    Self::evaluate_cutoff(pairs, tau);
                let far_ci = wilson_score_interval(acc_failed, acc_total, 0.95);
                let fnr_ci = wilson_score_interval(acc_failed, total_fail, 0.95);
                let is_statistically_proven =
                    far_ci.is_upper_bound_proven(0.01) && fnr_ci.is_upper_bound_proven(0.01);
                let is_target_met = far_ci.sample_size > 0
                    && fnr_ci.sample_size > 0
                    && far <= 0.01
                    && fnr <= 0.01
                    && calls_avoided >= 40.0;
                ParetoPoint {
                    threshold: tau,
                    coverage_pct: cov * 100.0,
                    calls_avoided_pct: calls_avoided,
                    cost_reduction_pct: cost_red,
                    false_accept_rate: far,
                    false_negative_rate: fnr,
                    far_ci,
                    fnr_ci,
                    is_target_met,
                    is_statistically_proven,
                    is_pareto_optimal: false,
                }
            })
            .collect();

        // Mark non-dominated points: point A dominates point B if
        // Cost Reduction is >= and Risk (FAR+FNR) is <= (with at least one strictly better)
        for i in 0..points.len() {
            let mut is_dominated = false;
            for j in 0..points.len() {
                if i != j {
                    let cost_i = points[i].cost_reduction_pct;
                    let cost_j = points[j].cost_reduction_pct;
                    let risk_i = points[i].false_accept_rate + points[i].false_negative_rate;
                    let risk_j = points[j].false_accept_rate + points[j].false_negative_rate;

                    if cost_j >= cost_i && risk_j <= risk_i && (cost_j > cost_i || risk_j < risk_i)
                    {
                        is_dominated = true;
                        break;
                    }
                }
            }
            points[i].is_pareto_optimal = !is_dominated;
        }

        points
    }
}

fn format_rate(rate: f64, denominator: usize) -> String {
    if denominator > 0 {
        format!("{:.2}%", rate * 100.0)
    } else {
        "N/A (n=0)".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reflex_core::{Outcome, ReflexAction};

    #[test]
    fn test_optimizer() {
        let mut pairs = Vec::new();
        // 80 high confidence successes
        for _ in 0..80 {
            pairs.push(DecisionOutcomePair::new(
                0.96,
                ReflexAction::Accept,
                Outcome::Success,
            ));
        }
        // 1 high confidence failure
        pairs.push(DecisionOutcomePair::new(
            0.92,
            ReflexAction::Accept,
            Outcome::Failure,
        ));
        // 19 low confidence failures
        for _ in 0..19 {
            pairs.push(DecisionOutcomePair::new(
                0.60,
                ReflexAction::Verify,
                Outcome::Failure,
            ));
        }

        let constraints = OptimizationConstraints {
            current_threshold: 0.85,
            max_false_accept_rate: 0.02,
            max_false_negative_rate: 0.05,
            min_coverage: 0.70,
        };

        let res = ThresholdOptimizer::optimize(&pairs, &constraints);
        assert!(res.is_feasible);
        assert!(res.expected_coverage >= 0.70);
        assert!(res.expected_false_accept_rate <= 0.02);
    }

    #[test]
    fn test_pareto_frontier() {
        let mut pairs = Vec::new();
        for _ in 0..100 {
            pairs.push(DecisionOutcomePair::new(
                0.95,
                ReflexAction::Accept,
                Outcome::Success,
            ));
        }
        pairs.push(DecisionOutcomePair::new(
            0.70,
            ReflexAction::Verify,
            Outcome::Failure,
        ));

        let frontier = ThresholdOptimizer::pareto_frontier(&pairs);
        assert!(!frontier.is_empty());
        assert!(frontier.iter().any(|p| p.is_pareto_optimal));
    }

    #[test]
    fn unresolved_outcomes_cannot_prove_a_safe_threshold() {
        let pairs = vec![
            DecisionOutcomePair::new(0.99, ReflexAction::Accept, Outcome::Unknown),
            DecisionOutcomePair::new(0.99, ReflexAction::Accept, Outcome::Partial),
        ];

        let result = ThresholdOptimizer::optimize(&pairs, &OptimizationConstraints::default());
        assert!(!result.is_feasible);
        assert!(!result.is_statistically_proven);
        assert_eq!(result.far_ci.sample_size, 0);
        assert!(result.explanation.contains("no resolved"));
        assert_eq!(ThresholdOptimizer::pareto_frontier(&pairs).len(), 0);
    }

    #[test]
    fn no_observed_failures_do_not_count_as_a_measured_zero_fnr() {
        let pairs = vec![
            DecisionOutcomePair::new(0.99, ReflexAction::Accept, Outcome::Success),
            DecisionOutcomePair::new(0.90, ReflexAction::Accept, Outcome::Success),
        ];

        let result = ThresholdOptimizer::optimize(&pairs, &OptimizationConstraints::default());
        assert!(!result.is_feasible);
        assert_eq!(result.fnr_ci.sample_size, 0);
        assert!(result.explanation.contains("observed FNR: N/A (n=0)"));
        assert!(ThresholdOptimizer::pareto_frontier(&pairs)
            .iter()
            .all(|point| !point.is_target_met));
    }
}
