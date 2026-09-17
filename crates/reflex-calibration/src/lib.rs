pub mod buckets;
pub mod eval;
pub mod metrics;
pub mod optimizer;
pub mod split;
pub mod stats;

pub use buckets::{CalibrationBucket, CalibrationCurve};
pub use eval::*;
pub use metrics::{
    compute_disaggregated_metrics, AnnotatedDecisionPair, CalibrationMetrics, DecisionOutcomePair,
    SliceMetric,
};
pub use optimizer::{OptimizationConstraints, OptimizationResult, ParetoPoint, ThresholdOptimizer};
pub use split::{split_stratified, DatasetSplit, SplitEvaluationReport};
pub use stats::{
    bootstrap_metric_ci, clopper_pearson_zero_upper_bound, wilson_score_interval,
    ConfidenceInterval,
};
