use crate::metrics::DecisionOutcomePair;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationConstraints {
    pub current_threshold: f64,
    pub max_false_accept_rate: f64,
    pub min_coverage: f64,
}

impl Default for OptimizationConstraints {
    fn default() -> Self {
        Self {
            current_threshold: 0.90,
            max_false_accept_rate: 0.01,
            min_coverage: 0.60,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationResult {
    pub current_threshold: f64,
    pub current_coverage: f64,
    pub current_false_accept_rate: f64,
    pub recommended_threshold: f64,
    pub expected_coverage: f64,
    pub expected_false_accept_rate: f64,
    pub is_feasible: bool,
    pub explanation: String,
}

pub struct ThresholdOptimizer;

impl ThresholdOptimizer {
    pub fn optimize(
        pairs: &[DecisionOutcomePair],
        constraints: &OptimizationConstraints,
    ) -> OptimizationResult {
        if pairs.is_empty() {
            return OptimizationResult {
                current_threshold: constraints.current_threshold,
                current_coverage: 0.0,
                current_false_accept_rate: 0.0,
                recommended_threshold: constraints.current_threshold,
                expected_coverage: 0.0,
                expected_false_accept_rate: 0.0,
                is_feasible: false,
                explanation: "Dataset is empty; cannot optimize threshold.".to_string(),
            };
        }

        let n = pairs.len() as f64;

        let eval_threshold = |tau: f64| -> (f64, f64) {
            let mut accepted_total = 0;
            let mut accepted_failed = 0;

            for p in pairs {
                if p.confidence >= tau {
                    accepted_total += 1;
                    if !p.is_success() {
                        accepted_failed += 1;
                    }
                }
            }

            let cov = accepted_total as f64 / n;
            let far = if accepted_total > 0 {
                accepted_failed as f64 / accepted_total as f64
            } else {
                0.0
            };
            (cov, far)
        };

        let (curr_cov, curr_far) = eval_threshold(constraints.current_threshold);

        // Search candidate thresholds from 0.500 to 0.999 in steps of 0.001
        let mut best_threshold = constraints.current_threshold;
        let mut best_coverage = 0.0;
        let mut best_far = 1.0;
        let mut found_feasible = false;

        // Collect unique candidate cutoffs
        let mut step = 500;
        while step <= 999 {
            let tau = step as f64 / 1000.0;
            let (cov, far) = eval_threshold(tau);

            let satisfies_safety = far <= constraints.max_false_accept_rate;
            let satisfies_coverage = cov >= constraints.min_coverage;

            if satisfies_safety && satisfies_coverage {
                // Feasible: we want to maximize coverage or pick the lowest safe threshold
                if !found_feasible
                    || cov > best_coverage
                    || (cov == best_coverage && far < best_far)
                {
                    found_feasible = true;
                    best_threshold = tau;
                    best_coverage = cov;
                    best_far = far;
                }
            } else if !found_feasible {
                // If not yet feasible, prioritize satisfying safety first
                if far <= constraints.max_false_accept_rate && cov > best_coverage {
                    best_threshold = tau;
                    best_coverage = cov;
                    best_far = far;
                }
            }
            step += 1;
        }

        let (exp_cov, exp_far) = eval_threshold(best_threshold);

        let explanation = if found_feasible {
            format!(
                "Found optimal threshold {:.3} satisfying max FAR <= {:.2}% and coverage >= {:.1}%.",
                best_threshold,
                constraints.max_false_accept_rate * 100.0,
                constraints.min_coverage * 100.0
            )
        } else {
            format!(
                "Strict constraints could not both be met. Recommended best-effort safety threshold is {:.3} (FAR: {:.2}%, Coverage: {:.1}%).",
                best_threshold,
                exp_far * 100.0,
                exp_cov * 100.0
            )
        };

        OptimizationResult {
            current_threshold: constraints.current_threshold,
            current_coverage: curr_cov,
            current_false_accept_rate: curr_far,
            recommended_threshold: best_threshold,
            expected_coverage: exp_cov,
            expected_false_accept_rate: exp_far,
            is_feasible: found_feasible,
            explanation,
        }
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
            min_coverage: 0.70,
        };

        let res = ThresholdOptimizer::optimize(&pairs, &constraints);
        assert!(res.is_feasible);
        assert!(res.expected_coverage >= 0.70);
        assert!(res.expected_false_accept_rate <= 0.02);
    }
}
