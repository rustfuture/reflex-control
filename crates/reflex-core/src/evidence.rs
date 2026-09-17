use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// Canonical atomic semantic signal names
pub const SIGNAL_FAILURE_IS_TRANSIENT: &str = "failure_is_transient";
pub const SIGNAL_REQUIREMENTS_ARE_AMBIGUOUS: &str = "requirements_are_ambiguous";
pub const SIGNAL_WORKER_OUT_OF_SCOPE: &str = "worker_out_of_scope";
pub const SIGNAL_EVIDENCE_SUPPORTS_CLAIM: &str = "evidence_supports_claim";
pub const SIGNAL_UNEXPECTED_SCOPE_CHANGE: &str = "unexpected_scope_change";
pub const SIGNAL_SECURITY_RISK: &str = "security_risk";
pub const SIGNAL_IMPLEMENTATION_MATCHES_REQUEST: &str = "implementation_matches_request";
pub const SIGNAL_REQUIRED_WORK_REMAINING: &str = "required_work_remaining";
pub const SIGNAL_OBJECTIVE_SATISFIED: &str = "objective_satisfied";
pub const SIGNAL_RETRY_LIKELY_TO_HELP: &str = "retry_likely_to_help";
pub const SIGNAL_INDEPENDENT_VERIFICATION_NEEDED: &str = "independent_verification_needed";

pub const ALL_ATOMIC_SIGNALS: &[&str] = &[
    SIGNAL_FAILURE_IS_TRANSIENT,
    SIGNAL_REQUIREMENTS_ARE_AMBIGUOUS,
    SIGNAL_WORKER_OUT_OF_SCOPE,
    SIGNAL_EVIDENCE_SUPPORTS_CLAIM,
    SIGNAL_UNEXPECTED_SCOPE_CHANGE,
    SIGNAL_SECURITY_RISK,
    SIGNAL_IMPLEMENTATION_MATCHES_REQUEST,
    SIGNAL_REQUIRED_WORK_REMAINING,
    SIGNAL_OBJECTIVE_SATISFIED,
    SIGNAL_RETRY_LIKELY_TO_HELP,
    SIGNAL_INDEPENDENT_VERIFICATION_NEEDED,
];

/// A single atomic semantic evidence signal returned by an evidence engine (e.g. TypeSafe Jev)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SemanticEvidence {
    pub name: String,
    pub probability: f64,
    pub source: String,
    pub latency_ms: u64,
}

impl SemanticEvidence {
    pub fn new(
        name: impl Into<String>,
        probability: f64,
        source: impl Into<String>,
        latency_ms: u64,
    ) -> Self {
        Self {
            name: name.into(),
            probability: probability.clamp(0.0, 1.0),
            source: source.into(),
            latency_ms,
        }
    }
}

/// Deterministic sensors for hard facts that must never be guessed or inferred by LLMs
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeterministicEvidence {
    pub tests_passed: Option<bool>,
    pub ci_passed: Option<bool>,
    pub exit_code: Option<i32>,
    pub retry_count: usize,
    pub files_changed: usize,
    pub unexpected_files_changed: bool,
    pub git_diff_size: usize,
    pub security_sensitive_files_changed: bool,
    pub tool_error: bool,
    pub timeout: bool,
    pub worker_completed: bool,
}

impl Default for DeterministicEvidence {
    fn default() -> Self {
        Self {
            tests_passed: None,
            ci_passed: None,
            exit_code: None,
            retry_count: 0,
            files_changed: 0,
            unexpected_files_changed: false,
            git_diff_size: 0,
            security_sensitive_files_changed: false,
            tool_error: false,
            timeout: false,
            worker_completed: true,
        }
    }
}

/// Composite evidence vector combining semantic inference with deterministic measurements
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct EvidenceVector {
    pub semantic: HashMap<String, SemanticEvidence>,
    pub deterministic: DeterministicEvidence,
}

impl EvidenceVector {
    pub fn new(deterministic: DeterministicEvidence) -> Self {
        Self {
            semantic: HashMap::new(),
            deterministic,
        }
    }

    pub fn with_semantic(mut self, evidence: SemanticEvidence) -> Self {
        self.semantic.insert(evidence.name.clone(), evidence);
        self
    }

    pub fn add_semantic(&mut self, evidence: SemanticEvidence) {
        self.semantic.insert(evidence.name.clone(), evidence);
    }

    pub fn get_prob(&self, name: &str) -> Option<f64> {
        self.semantic.get(name).map(|e| e.probability)
    }

    pub fn get_prob_or(&self, name: &str, default: f64) -> f64 {
        self.get_prob(name).unwrap_or(default)
    }

    /// Fixed feature schema names for learned composers
    pub fn feature_names() -> &'static [&'static str] {
        &[
            // Semantic signals (0..11)
            SIGNAL_FAILURE_IS_TRANSIENT,
            SIGNAL_REQUIREMENTS_ARE_AMBIGUOUS,
            SIGNAL_WORKER_OUT_OF_SCOPE,
            SIGNAL_EVIDENCE_SUPPORTS_CLAIM,
            SIGNAL_UNEXPECTED_SCOPE_CHANGE,
            SIGNAL_SECURITY_RISK,
            SIGNAL_IMPLEMENTATION_MATCHES_REQUEST,
            SIGNAL_REQUIRED_WORK_REMAINING,
            SIGNAL_OBJECTIVE_SATISFIED,
            SIGNAL_RETRY_LIKELY_TO_HELP,
            SIGNAL_INDEPENDENT_VERIFICATION_NEEDED,
            // Deterministic signals (11..21)
            "tests_passed_bool",
            "tests_failed_bool",
            "ci_passed_bool",
            "ci_failed_bool",
            "exit_code_zero",
            "retry_count_norm",
            "files_changed_norm",
            "unexpected_files_changed",
            "security_sensitive_files_changed",
            "tool_error_or_timeout",
            "worker_completed_bool",
        ]
    }

    /// Converts evidence vector into a dense feature vector of length 22 for learned models
    pub fn to_feature_vector(&self) -> Vec<f64> {
        let mut feats = Vec::with_capacity(22);

        // 1. Semantic signals (default 0.5 if missing)
        feats.push(self.get_prob_or(SIGNAL_FAILURE_IS_TRANSIENT, 0.5));
        feats.push(self.get_prob_or(SIGNAL_REQUIREMENTS_ARE_AMBIGUOUS, 0.5));
        feats.push(self.get_prob_or(SIGNAL_WORKER_OUT_OF_SCOPE, 0.5));
        feats.push(self.get_prob_or(SIGNAL_EVIDENCE_SUPPORTS_CLAIM, 0.5));
        feats.push(self.get_prob_or(SIGNAL_UNEXPECTED_SCOPE_CHANGE, 0.5));
        feats.push(self.get_prob_or(SIGNAL_SECURITY_RISK, 0.5));
        feats.push(self.get_prob_or(SIGNAL_IMPLEMENTATION_MATCHES_REQUEST, 0.5));
        feats.push(self.get_prob_or(SIGNAL_REQUIRED_WORK_REMAINING, 0.5));
        feats.push(self.get_prob_or(SIGNAL_OBJECTIVE_SATISFIED, 0.5));
        feats.push(self.get_prob_or(SIGNAL_RETRY_LIKELY_TO_HELP, 0.5));
        feats.push(self.get_prob_or(SIGNAL_INDEPENDENT_VERIFICATION_NEEDED, 0.5));

        // 2. Deterministic sensors
        let det = &self.deterministic;
        feats.push(if det.tests_passed == Some(true) {
            1.0
        } else {
            0.0
        });
        feats.push(if det.tests_passed == Some(false) {
            1.0
        } else {
            0.0
        });
        feats.push(if det.ci_passed == Some(true) {
            1.0
        } else {
            0.0
        });
        feats.push(if det.ci_passed == Some(false) {
            1.0
        } else {
            0.0
        });
        feats.push(if det.exit_code == Some(0) || det.exit_code.is_none() {
            1.0
        } else {
            0.0
        });
        feats.push((det.retry_count as f64 / 5.0).clamp(0.0, 1.0));
        feats.push((det.files_changed as f64 / 20.0).clamp(0.0, 1.0));
        feats.push(if det.unexpected_files_changed {
            1.0
        } else {
            0.0
        });
        feats.push(if det.security_sensitive_files_changed {
            1.0
        } else {
            0.0
        });
        feats.push(if det.tool_error || det.timeout {
            1.0
        } else {
            0.0
        });
        feats.push(if det.worker_completed { 1.0 } else { 0.0 });

        feats
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_evidence_vector_feature_mapping() {
        let det = DeterministicEvidence {
            tests_passed: Some(true),
            ci_passed: Some(true),
            exit_code: Some(0),
            retry_count: 1,
            files_changed: 3,
            unexpected_files_changed: false,
            git_diff_size: 140,
            security_sensitive_files_changed: false,
            tool_error: false,
            timeout: false,
            worker_completed: true,
        };

        let vec = EvidenceVector::new(det)
            .with_semantic(SemanticEvidence::new(
                SIGNAL_SECURITY_RISK,
                0.05,
                "test",
                200,
            ))
            .with_semantic(SemanticEvidence::new(
                SIGNAL_FAILURE_IS_TRANSIENT,
                0.90,
                "test",
                200,
            ));

        assert_eq!(vec.get_prob(SIGNAL_SECURITY_RISK), Some(0.05));
        assert_eq!(vec.get_prob(SIGNAL_FAILURE_IS_TRANSIENT), Some(0.90));
        assert_eq!(vec.get_prob_or(SIGNAL_WORKER_OUT_OF_SCOPE, 0.5), 0.5);

        let feats = vec.to_feature_vector();
        assert_eq!(feats.len(), EvidenceVector::feature_names().len());
        assert_eq!(feats[0], 0.90); // failure_is_transient
        assert_eq!(feats[5], 0.05); // security_risk
        assert_eq!(feats[11], 1.0); // tests_passed_bool
        assert_eq!(feats[12], 0.0); // tests_failed_bool
    }
}
