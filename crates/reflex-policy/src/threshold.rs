use crate::policy::Policy;
use reflex_core::{DecisionResponse, Observation, ReflexAction};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThresholdPolicy {
    pub accept_threshold: f64,
    pub verify_threshold: f64,
}

impl Default for ThresholdPolicy {
    fn default() -> Self {
        Self {
            accept_threshold: 0.90,
            verify_threshold: 0.65,
        }
    }
}

impl ThresholdPolicy {
    pub fn new(accept_threshold: f64, verify_threshold: f64) -> Self {
        Self {
            accept_threshold: accept_threshold.clamp(0.0, 1.0),
            verify_threshold: verify_threshold.clamp(0.0, 1.0),
        }
    }
}

impl Policy for ThresholdPolicy {
    fn name(&self) -> &str {
        "threshold"
    }

    fn decide(&self, response: &DecisionResponse, _observation: &Observation) -> ReflexAction {
        let conf = response.decision.confidence;
        if conf >= self.accept_threshold {
            ReflexAction::Accept
        } else if conf >= self.verify_threshold {
            ReflexAction::Verify
        } else {
            ReflexAction::Escalate
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reflex_core::{Decision, DecisionId};

    #[test]
    fn test_threshold_routing() {
        let policy = ThresholdPolicy::new(0.90, 0.70);
        let obs = Observation::new("test");

        let high = DecisionResponse::new(
            DecisionId::generate(),
            Decision::new("true", vec![], 0.95),
            "mock",
            10,
            0.0001,
        );
        assert_eq!(policy.decide(&high, &obs), ReflexAction::Accept);

        let med = DecisionResponse::new(
            DecisionId::generate(),
            Decision::new("true", vec![], 0.75),
            "mock",
            10,
            0.0001,
        );
        assert_eq!(policy.decide(&med, &obs), ReflexAction::Verify);

        let low = DecisionResponse::new(
            DecisionId::generate(),
            Decision::new("true", vec![], 0.50),
            "mock",
            10,
            0.0001,
        );
        assert_eq!(policy.decide(&low, &obs), ReflexAction::Escalate);
    }
}
