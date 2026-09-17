use crate::composer::ComposerDecision;
use reflex_core::{EvidenceVector, ReflexAction, RiskLevel, SIGNAL_SECURITY_RISK};
use serde::{Deserialize, Serialize};

/// Configuration thresholds for the Risk & Deferral Layer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskDeferralConfig {
    /// Upper composite risk bound for granting autonomous accept without human/model review
    pub max_risk_for_autonomous_accept: f64,
    /// Upper composite risk bound for deferring to a small fast reasoning model (System-1.5)
    pub max_risk_for_small_reasoner: f64,
    /// Risk threshold above which frontier model or human escalation is mandatory
    pub mandatory_frontier_escalation_risk: f64,
}

impl Default for RiskDeferralConfig {
    fn default() -> Self {
        Self {
            max_risk_for_autonomous_accept: 0.28,
            max_risk_for_small_reasoner: 0.58,
            mandatory_frontier_escalation_risk: 0.70,
        }
    }
}

/// Evaluates composer decisions against risk budgets and policy guardrails
pub struct RiskAbstentionPolicy {
    config: RiskDeferralConfig,
}

impl Default for RiskAbstentionPolicy {
    fn default() -> Self {
        Self::new(RiskDeferralConfig::default())
    }
}

impl RiskAbstentionPolicy {
    pub fn new(config: RiskDeferralConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &RiskDeferralConfig {
        &self.config
    }

    /// Computes composite multi-factor risk index combining task level, semantic risk, and composer confidence
    pub fn compute_composite_risk(
        &self,
        composer_decision: &ComposerDecision,
        evidence: &EvidenceVector,
        task_risk: RiskLevel,
    ) -> f64 {
        let intrinsic_risk = match task_risk {
            RiskLevel::Low => 0.05,
            RiskLevel::Medium => 0.25,
            RiskLevel::High => 0.60,
            RiskLevel::Critical => 0.95,
        };

        let sec_risk = evidence.get_prob_or(SIGNAL_SECURITY_RISK, 0.05);
        let uncert = (1.0 - composer_decision.confidence).clamp(0.0, 1.0);
        let det_penalty = if evidence.deterministic.security_sensitive_files_changed {
            0.20
        } else if evidence.deterministic.unexpected_files_changed {
            0.10
        } else {
            0.0
        };

        (intrinsic_risk * 0.30
            + composer_decision.risk_score * 0.30
            + sec_risk * 0.25
            + uncert * 0.15
            + det_penalty)
            .clamp(0.0, 1.0)
    }

    /// Evaluates composer decision and returns final action (including deferral tiers)
    pub fn evaluate(
        &self,
        composer_decision: &ComposerDecision,
        evidence: &EvidenceVector,
        task_risk: RiskLevel,
    ) -> (ReflexAction, f64) {
        let sec_risk = evidence.get_prob_or(SIGNAL_SECURITY_RISK, 0.05);
        let composite_risk = self.compute_composite_risk(composer_decision, evidence, task_risk);

        // 1. Mandatory Safety Escalation
        if task_risk == RiskLevel::Critical
            || sec_risk >= self.config.mandatory_frontier_escalation_risk
            || (evidence.deterministic.security_sensitive_files_changed && sec_risk >= 0.40)
        {
            return (ReflexAction::DeferToFrontier, composite_risk);
        }

        // 2. Map actions with risk-sensitive triage
        let final_action = match &composer_decision.action {
            ReflexAction::Accept | ReflexAction::Terminate => {
                if composite_risk <= self.config.max_risk_for_autonomous_accept {
                    composer_decision.action.clone()
                } else if composite_risk <= self.config.max_risk_for_small_reasoner {
                    ReflexAction::DeferToSmallReasoner
                } else {
                    ReflexAction::DeferToFrontier
                }
            }
            ReflexAction::Verify => {
                if composite_risk <= self.config.max_risk_for_small_reasoner {
                    ReflexAction::DeferToSmallReasoner
                } else {
                    ReflexAction::DeferToFrontier
                }
            }
            ReflexAction::Escalate => ReflexAction::DeferToFrontier,
            ReflexAction::Retry => ReflexAction::Retry,
            ReflexAction::Continue => ReflexAction::Continue,
            ReflexAction::Reject => ReflexAction::Reject,
            ReflexAction::DeferToSmallReasoner => ReflexAction::DeferToSmallReasoner,
            ReflexAction::DeferToFrontier => ReflexAction::DeferToFrontier,
            ReflexAction::Custom(c) => ReflexAction::Custom(c.clone()),
        };

        (final_action, composite_risk)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reflex_core::{DeterministicEvidence, SemanticEvidence, SIGNAL_SECURITY_RISK};

    #[test]
    fn test_mandatory_frontier_escalation_on_critical_risk() {
        let policy = RiskAbstentionPolicy::default();
        let dec = ComposerDecision::new(ReflexAction::Accept, 0.99, 0.05, vec![]);
        let ev = EvidenceVector::default();

        let (action, _) = policy.evaluate(&dec, &ev, RiskLevel::Critical);
        assert_eq!(action, ReflexAction::DeferToFrontier);
    }

    #[test]
    fn test_autonomous_accept_when_risk_is_low() {
        let policy = RiskAbstentionPolicy::default();
        let dec = ComposerDecision::new(ReflexAction::Accept, 0.95, 0.05, vec![]);
        let ev = EvidenceVector::new(DeterministicEvidence {
            tests_passed: Some(true),
            ..Default::default()
        });

        let (action, _) = policy.evaluate(&dec, &ev, RiskLevel::Low);
        assert_eq!(action, ReflexAction::Accept);
    }

    #[test]
    fn test_defer_to_small_reasoner_when_risk_is_moderate() {
        let policy = RiskAbstentionPolicy::default();
        let dec = ComposerDecision::new(ReflexAction::Accept, 0.80, 0.35, vec![]);
        let ev = EvidenceVector::new(DeterministicEvidence::default()).with_semantic(
            SemanticEvidence::new(SIGNAL_SECURITY_RISK, 0.30, "mock", 10),
        );

        let (action, _) = policy.evaluate(&dec, &ev, RiskLevel::Medium);
        assert_eq!(action, ReflexAction::DeferToSmallReasoner);
    }
}
