use crate::policy::Policy;
use reflex_core::{DecisionResponse, Observation, ReflexAction, RiskLevel};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskAwarePolicy<P: Policy> {
    pub inner: P,
    pub critical_always_escalate: bool,
    pub high_always_verify: bool,
}

impl<P: Policy> RiskAwarePolicy<P> {
    pub fn new(inner: P) -> Self {
        Self {
            inner,
            critical_always_escalate: true,
            high_always_verify: true,
        }
    }

    pub fn with_critical_escalate(mut self, enabled: bool) -> Self {
        self.critical_always_escalate = enabled;
        self
    }

    pub fn with_high_verify(mut self, enabled: bool) -> Self {
        self.high_always_verify = enabled;
        self
    }
}

impl<P: Policy> Policy for RiskAwarePolicy<P> {
    fn name(&self) -> &str {
        "risk_aware"
    }

    fn decide(&self, response: &DecisionResponse, observation: &Observation) -> ReflexAction {
        // Enforce safety invariant: High risk requires mandatory verification,
        // Critical risk requires escalation or verification, even if confidence is 1.0.
        match observation.risk_level {
            RiskLevel::Critical => {
                if self.critical_always_escalate {
                    ReflexAction::Escalate
                } else {
                    ReflexAction::Verify
                }
            }
            RiskLevel::High => {
                if self.high_always_verify {
                    ReflexAction::Verify
                } else {
                    self.inner.decide(response, observation)
                }
            }
            RiskLevel::Medium | RiskLevel::Low => {
                // Delegate to underlying policy
                self.inner.decide(response, observation)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::threshold::ThresholdPolicy;
    use reflex_core::{Decision, DecisionId};

    #[test]
    fn test_high_risk_bypasses_high_confidence_accept() {
        let base = ThresholdPolicy::new(0.80, 0.50);
        let policy = RiskAwarePolicy::new(base);

        let high_conf = DecisionResponse::new(
            DecisionId::generate(),
            Decision::new("true", vec![], 0.999),
            "mock",
            5,
            0.0001,
        );

        // Low risk -> Accepted because confidence > 0.80
        let low_obs = Observation::new("read cache").with_risk(RiskLevel::Low);
        assert_eq!(policy.decide(&high_conf, &low_obs), ReflexAction::Accept);

        // High risk -> Mandatory Verify despite 0.999 confidence
        let high_obs = Observation::new("delete database table").with_risk(RiskLevel::High);
        assert_eq!(policy.decide(&high_conf, &high_obs), ReflexAction::Verify);

        // Critical risk -> Mandatory Escalate
        let crit_obs = Observation::new("drop production root key").with_risk(RiskLevel::Critical);
        assert_eq!(policy.decide(&high_conf, &crit_obs), ReflexAction::Escalate);
    }
}
