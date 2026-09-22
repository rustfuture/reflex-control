use crate::metrics::{CalibrationMetrics, DecisionOutcomePair};
use crate::optimizer::{OptimizationConstraints, OptimizationResult, ThresholdOptimizer};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatasetSplit<T> {
    pub train: Vec<T>,
    pub val: Vec<T>,
    pub test: Vec<T>,
}

impl<T> DatasetSplit<T> {
    pub fn total_samples(&self) -> usize {
        self.train.len() + self.val.len() + self.test.len()
    }
}

/// Simple pseudo-random shuffle for deterministic dataset partitioning
fn deterministic_shuffle<T>(items: &mut [T], mut seed: u64) {
    if items.len() <= 1 {
        return;
    }
    for i in (1..items.len()).rev() {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let j = (seed % (i as u64 + 1)) as usize;
        items.swap(i, j);
    }
}

/// Partitions items into stratified Train (e.g. 50%), Validation (25%), and Test (25%) splits.
/// Stratification ensures that positive/defect ratios remain consistent across splits.
pub fn split_stratified<T, F>(
    items: Vec<T>,
    train_ratio: f64,
    val_ratio: f64,
    _test_ratio: f64,
    is_positive: F,
    seed: u64,
) -> DatasetSplit<T>
where
    F: Fn(&T) -> bool,
{
    let mut positives = Vec::new();
    let mut negatives = Vec::new();

    for item in items {
        if is_positive(&item) {
            positives.push(item);
        } else {
            negatives.push(item);
        }
    }

    deterministic_shuffle(&mut positives, seed);
    deterministic_shuffle(&mut negatives, seed.wrapping_add(101));

    let split_subset = |mut group: Vec<T>| -> (Vec<T>, Vec<T>, Vec<T>) {
        let n = group.len() as f64;
        let train_end = (n * train_ratio).round() as usize;
        let val_end = (train_end + (n * val_ratio).round() as usize).min(group.len());

        let rest = group.split_off(train_end);
        let mut val_and_test = rest;
        let test = val_and_test.split_off(val_end.saturating_sub(train_end));
        let val = val_and_test;
        let train = group;

        (train, val, test)
    };

    let (pos_train, pos_val, pos_test) = split_subset(positives);
    let (neg_train, neg_val, neg_test) = split_subset(negatives);

    let mut train = pos_train;
    train.extend(neg_train);
    deterministic_shuffle(&mut train, seed.wrapping_add(202));

    let mut val = pos_val;
    val.extend(neg_val);
    deterministic_shuffle(&mut val, seed.wrapping_add(303));

    let mut test = pos_test;
    test.extend(neg_test);
    deterministic_shuffle(&mut test, seed.wrapping_add(404));

    DatasetSplit { train, val, test }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SplitEvaluationReport {
    pub train_samples: usize,
    pub val_samples: usize,
    pub test_samples: usize,
    pub train_val_optimization: OptimizationResult,
    pub frozen_threshold: f64,
    pub test_metrics: CalibrationMetrics,
}

impl DatasetSplit<DecisionOutcomePair> {
    /// Selects the optimal threshold exclusively on Train + Validation,
    /// and then evaluates the frozen threshold on the test partition.
    pub fn evaluate_frozen_pipeline(
        &self,
        constraints: &OptimizationConstraints,
    ) -> SplitEvaluationReport {
        // Step 1: Combine Train and Validation to tune threshold
        let mut train_val = self.train.clone();
        train_val.extend(self.val.clone());

        let opt_res = ThresholdOptimizer::optimize(&train_val, constraints);
        let frozen_threshold = opt_res.recommended_threshold;

        // Step 2: Evaluate the frozen threshold on the test partition.
        let test_metrics = CalibrationMetrics::compute(&self.test, frozen_threshold);

        SplitEvaluationReport {
            train_samples: self.train.len(),
            val_samples: self.val.len(),
            test_samples: self.test.len(),
            train_val_optimization: opt_res,
            frozen_threshold,
            test_metrics,
        }
    }

    /// Evaluates a pre-selected frozen threshold directly on the test partition.
    pub fn evaluate_test_with_frozen_threshold(&self, frozen_threshold: f64) -> CalibrationMetrics {
        CalibrationMetrics::compute(&self.test, frozen_threshold)
    }
}

use crate::metrics::AnnotatedDecisionPair;

impl DatasetSplit<AnnotatedDecisionPair> {
    pub fn to_pair_split(&self) -> DatasetSplit<DecisionOutcomePair> {
        DatasetSplit {
            train: self.train.iter().map(|a| a.to_pair()).collect(),
            val: self.val.iter().map(|a| a.to_pair()).collect(),
            test: self.test.iter().map(|a| a.to_pair()).collect(),
        }
    }

    pub fn evaluate_frozen_pipeline(
        &self,
        constraints: &OptimizationConstraints,
    ) -> SplitEvaluationReport {
        self.to_pair_split().evaluate_frozen_pipeline(constraints)
    }

    pub fn evaluate_test_with_frozen_threshold(&self, frozen_threshold: f64) -> CalibrationMetrics {
        self.to_pair_split()
            .evaluate_test_with_frozen_threshold(frozen_threshold)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reflex_core::{Outcome, ReflexAction};

    #[test]
    fn test_stratified_split() {
        let mut items = Vec::new();
        // 100 successes, 20 defects
        for _ in 0..100 {
            items.push(DecisionOutcomePair::new(
                0.95,
                ReflexAction::Accept,
                Outcome::Success,
            ));
        }
        for _ in 0..20 {
            items.push(DecisionOutcomePair::new(
                0.60,
                ReflexAction::Verify,
                Outcome::Failure,
            ));
        }

        let split = split_stratified(items, 0.50, 0.25, 0.25, |p| p.outcome.is_failure(), 42);

        assert_eq!(split.total_samples(), 120);
        assert!(!split.train.is_empty());
        assert!(!split.val.is_empty());
        assert!(!split.test.is_empty());

        // Defect ratio should be preserved across all splits
        let test_defects = split.test.iter().filter(|p| p.outcome.is_failure()).count();
        assert!((4..=6).contains(&test_defects));
    }

    #[test]
    fn test_frozen_threshold_pipeline() {
        let mut items = Vec::new();
        for _ in 0..200 {
            items.push(DecisionOutcomePair::new(
                0.97,
                ReflexAction::Accept,
                Outcome::Success,
            ));
        }
        for _ in 0..20 {
            items.push(DecisionOutcomePair::new(
                0.80,
                ReflexAction::Verify,
                Outcome::Failure,
            ));
        }

        let split = split_stratified(items, 0.50, 0.25, 0.25, |p| p.outcome.is_failure(), 42);

        let constraints = OptimizationConstraints::default();
        let report = split.evaluate_frozen_pipeline(&constraints);

        assert!(report.frozen_threshold > 0.0);
        assert_eq!(report.test_samples, split.test.len());
        assert!(report.test_metrics.total_samples > 0);
    }
}
