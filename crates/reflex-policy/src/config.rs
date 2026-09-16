use crate::policy::Policy;
use crate::risk_aware::RiskAwarePolicy;
use crate::threshold::ThresholdPolicy;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationConfig {
    #[serde(default = "default_accept_threshold")]
    pub accept_threshold: f64,
    #[serde(default = "default_escalate_threshold")]
    pub escalate_threshold: f64,
}

fn default_accept_threshold() -> f64 {
    0.90
}

fn default_escalate_threshold() -> f64 {
    0.65
}

impl Default for VerificationConfig {
    fn default() -> Self {
        Self {
            accept_threshold: 0.90,
            escalate_threshold: 0.65,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RisksConfig {
    #[serde(default = "default_true")]
    pub critical_always_escalate: bool,
    #[serde(default = "default_true")]
    pub high_always_verify: bool,
}

fn default_true() -> bool {
    true
}

impl Default for RisksConfig {
    fn default() -> Self {
        Self {
            critical_always_escalate: true,
            high_always_verify: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PolicyConfig {
    #[serde(default)]
    pub verification: VerificationConfig,
    #[serde(default)]
    pub risks: RisksConfig,
}

impl PolicyConfig {
    pub fn build_policy(&self) -> Box<dyn Policy> {
        let threshold = ThresholdPolicy::new(
            self.verification.accept_threshold,
            self.verification.escalate_threshold,
        );

        let risk_aware = RiskAwarePolicy::new(threshold)
            .with_critical_escalate(self.risks.critical_always_escalate)
            .with_high_verify(self.risks.high_always_verify);

        Box::new(risk_aware)
    }
}
