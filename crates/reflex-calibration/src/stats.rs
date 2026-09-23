use serde::{Deserialize, Serialize};

/// Represents an empirical or statistical confidence interval
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Default)]
pub struct ConfidenceInterval {
    pub point_estimate: f64,
    pub lower: f64,
    pub upper: f64,
    pub sample_size: usize,
    pub confidence_level: f64,
}

impl ConfidenceInterval {
    pub fn new(
        point_estimate: f64,
        lower: f64,
        upper: f64,
        sample_size: usize,
        confidence_level: f64,
    ) -> Self {
        Self {
            point_estimate,
            lower: lower.clamp(0.0, 1.0),
            upper: upper.clamp(0.0, 1.0),
            sample_size,
            confidence_level,
        }
    }

    /// Checks whether the upper confidence bound is strictly below a target threshold.
    /// E.g. for target < 1%, returns true only if upper < 0.01.
    pub fn is_upper_bound_proven(&self, target: f64) -> bool {
        self.sample_size > 0 && self.upper < target
    }

    /// Formats as percentage string with CI: "0.00% [95% CI: 0.00% – 1.82%]"
    pub fn format_pct(&self) -> String {
        if self.sample_size == 0 {
            return "N/A (n=0)".to_string();
        }
        format!(
            "{:.2}% [{:.0}% CI: {:.2}% – {:.2}%]",
            self.point_estimate * 100.0,
            self.confidence_level * 100.0,
            self.lower * 100.0,
            self.upper * 100.0
        )
    }
}

/// Computes the Wilson score interval for a binomial proportion k / n.
/// Handles zero observed errors correctly by producing a non-zero upper bound (e.g. 3.84 / (n + 3.84)).
pub fn wilson_score_interval(
    successes: usize,
    total: usize,
    confidence_level: f64,
) -> ConfidenceInterval {
    if total == 0 {
        return ConfidenceInterval {
            point_estimate: 0.0,
            lower: 0.0,
            upper: 1.0,
            sample_size: 0,
            confidence_level,
        };
    }

    let n = total as f64;
    let p = successes as f64 / n;

    // Normal quantile z for common confidence levels
    let z = match (confidence_level * 100.0).round() as i64 {
        90 => 1.644853,
        99 => 2.575829,
        _ => 1.959964, // 95% default
    };

    let z2 = z * z;
    let denom = 1.0 + z2 / n;
    let center = (p + z2 / (2.0 * n)) / denom;
    let margin = (z / denom) * ((p * (1.0 - p) / n) + (z2 / (4.0 * n * n))).sqrt();

    let lower = (center - margin).max(0.0);
    let upper = (center + margin).min(1.0);

    ConfidenceInterval::new(p, lower, upper, total, confidence_level)
}

/// Exact Clopper-Pearson upper bound for 0 observed errors in n samples:
/// Upper = 1 - alpha^(1/n)
pub fn clopper_pearson_zero_upper_bound(n: usize, alpha: f64) -> f64 {
    if n == 0 {
        return 1.0;
    }
    1.0 - alpha.powf(1.0 / n as f64)
}

/// Simple deterministic pseudo-random generator (Linear Congruential Generator)
/// for reproducible bootstrap sampling without external dependencies.
struct SimpleRng {
    state: u64,
}

impl SimpleRng {
    fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 { 0x853c49e6748fea9b } else { seed },
        }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }

    fn gen_index(&mut self, bound: usize) -> usize {
        (self.next_u64() % bound as u64) as usize
    }
}

/// Computes a non-parametric bootstrap confidence interval for a slice of f64 values (e.g. costs or ratios).
pub fn bootstrap_metric_ci(
    values: &[f64],
    reps: usize,
    confidence_level: f64,
    seed: u64,
) -> ConfidenceInterval {
    if values.is_empty() {
        return ConfidenceInterval {
            point_estimate: 0.0,
            lower: 0.0,
            upper: 0.0,
            sample_size: 0,
            confidence_level,
        };
    }

    let n = values.len();
    let point_estimate = values.iter().sum::<f64>() / n as f64;

    let mut rng = SimpleRng::new(seed);
    let mut bootstrap_means = Vec::with_capacity(reps);

    for _ in 0..reps {
        let mut sample_sum = 0.0;
        for _ in 0..n {
            let idx = rng.gen_index(n);
            sample_sum += values[idx];
        }
        bootstrap_means.push(sample_sum / n as f64);
    }

    bootstrap_means.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let alpha = 1.0 - confidence_level;
    let lower_idx = ((alpha / 2.0) * reps as f64).floor() as usize;
    let upper_idx = (((1.0 - alpha / 2.0) * reps as f64).ceil() as usize).min(reps - 1);

    let lower = bootstrap_means[lower_idx];
    let upper = bootstrap_means[upper_idx];

    ConfidenceInterval {
        point_estimate,
        lower,
        upper,
        sample_size: n,
        confidence_level,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wilson_zero_errors_bound() {
        // With 0 errors in 100 samples, point estimate is 0.0, but upper bound is ~3.7%
        let ci = wilson_score_interval(0, 100, 0.95);
        assert_eq!(ci.point_estimate, 0.0);
        assert_eq!(ci.lower, 0.0);
        assert!(ci.upper > 0.03 && ci.upper < 0.04);
        assert!(!ci.is_upper_bound_proven(0.01)); // Cannot claim < 1% with N=100!

        // With 0 errors in 500 samples, upper bound should be < 1%
        let ci_large = wilson_score_interval(0, 500, 0.95);
        assert_eq!(ci_large.point_estimate, 0.0);
        assert!(ci_large.upper < 0.01);
        assert!(ci_large.is_upper_bound_proven(0.01)); // Can claim < 1% with N=500!
    }

    #[test]
    fn zero_denominator_formats_as_not_available() {
        assert_eq!(wilson_score_interval(0, 0, 0.95).format_pct(), "N/A (n=0)");
    }

    #[test]
    fn test_clopper_pearson_zero_bound() {
        let bound_100 = clopper_pearson_zero_upper_bound(100, 0.05);
        assert!(bound_100 > 0.025 && bound_100 < 0.035);

        let bound_500 = clopper_pearson_zero_upper_bound(500, 0.05);
        assert!(bound_500 < 0.01);
    }

    #[test]
    fn test_bootstrap_ci() {
        let values = vec![0.80, 0.85, 0.90, 0.92, 0.88, 0.84, 0.86, 0.89];
        let ci = bootstrap_metric_ci(&values, 1000, 0.95, 42);
        assert!(ci.lower <= ci.point_estimate);
        assert!(ci.point_estimate <= ci.upper);
        assert_eq!(ci.sample_size, 8);
    }
}
