use crate::metrics::DecisionOutcomePair;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalibrationBucket {
    pub min_conf: f64,
    pub max_conf: f64,
    pub count: usize,
    pub mean_confidence: f64,
    pub observed_success_rate: f64,
    pub calibration_gap: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalibrationCurve {
    pub buckets: Vec<CalibrationBucket>,
}

impl CalibrationCurve {
    pub fn build(pairs: &[DecisionOutcomePair], num_buckets: usize) -> Self {
        let buckets_n = num_buckets.max(2);
        let step = 1.0 / buckets_n as f64;

        let mut counts = vec![0usize; buckets_n];
        let mut conf_sums = vec![0.0f64; buckets_n];
        let mut success_counts = vec![0usize; buckets_n];

        for pair in pairs {
            let mut idx = (pair.confidence / step).floor() as usize;
            if idx >= buckets_n {
                idx = buckets_n - 1;
            }
            counts[idx] += 1;
            conf_sums[idx] += pair.confidence;
            if pair.is_success() {
                success_counts[idx] += 1;
            }
        }

        let mut buckets = Vec::with_capacity(buckets_n);
        for i in 0..buckets_n {
            let min_conf = i as f64 * step;
            let max_conf = (i + 1) as f64 * step;
            let c = counts[i];
            let mean_conf = if c > 0 {
                conf_sums[i] / c as f64
            } else {
                (min_conf + max_conf) / 2.0
            };
            let obs_rate = if c > 0 {
                success_counts[i] as f64 / c as f64
            } else {
                0.0
            };
            let gap = (mean_conf - obs_rate).abs();

            buckets.push(CalibrationBucket {
                min_conf,
                max_conf,
                count: c,
                mean_confidence: mean_conf,
                observed_success_rate: obs_rate,
                calibration_gap: gap,
            });
        }

        Self { buckets }
    }
}
