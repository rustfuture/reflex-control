use reflex_core::{
    DeterministicEvidence, EvidenceVector, ReflexAction, SIGNAL_EVIDENCE_SUPPORTS_CLAIM,
    SIGNAL_FAILURE_IS_TRANSIENT, SIGNAL_IMPLEMENTATION_MATCHES_REQUEST,
    SIGNAL_INDEPENDENT_VERIFICATION_NEEDED, SIGNAL_OBJECTIVE_SATISFIED,
    SIGNAL_REQUIRED_WORK_REMAINING, SIGNAL_REQUIREMENTS_ARE_AMBIGUOUS, SIGNAL_RETRY_LIKELY_TO_HELP,
    SIGNAL_SECURITY_RISK, SIGNAL_UNEXPECTED_SCOPE_CHANGE, SIGNAL_WORKER_OUT_OF_SCOPE,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// The output of a decision composer combining evidence signals into an action
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComposerDecision {
    pub action: ReflexAction,
    pub confidence: f64,
    pub reasoning: Vec<String>,
    pub risk_score: f64,
}

impl ComposerDecision {
    pub fn new(
        action: ReflexAction,
        confidence: f64,
        risk_score: f64,
        reasoning: Vec<String>,
    ) -> Self {
        Self {
            action,
            confidence: confidence.clamp(0.0, 1.0),
            risk_score: risk_score.clamp(0.0, 1.0),
            reasoning,
        }
    }
}

/// Common trait for all decision composers
pub trait DecisionComposer: Send + Sync {
    fn name(&self) -> &str;
    fn compose(&self, evidence: &EvidenceVector) -> ComposerDecision;
}

// ─────────────────────────────────────────────────────────────────────────────
// A. RuleBasedComposer
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleComposerConfig {
    pub security_risk_escalate_threshold: f64,
    pub security_risk_verify_threshold: f64,
    pub transient_retry_threshold: f64,
    pub max_retries: usize,
    pub out_of_scope_escalate_threshold: f64,
    pub ambiguity_escalate_threshold: f64,
    pub work_remaining_continue_threshold: f64,
    pub verification_needed_threshold: f64,
}

impl Default for RuleComposerConfig {
    fn default() -> Self {
        Self {
            security_risk_escalate_threshold: 0.60,
            security_risk_verify_threshold: 0.30,
            transient_retry_threshold: 0.45,
            max_retries: 3,
            out_of_scope_escalate_threshold: 0.60,
            ambiguity_escalate_threshold: 0.65,
            work_remaining_continue_threshold: 0.50,
            verification_needed_threshold: 0.40,
        }
    }
}

pub struct RuleBasedComposer {
    config: RuleComposerConfig,
}

impl Default for RuleBasedComposer {
    fn default() -> Self {
        Self::new(RuleComposerConfig::default())
    }
}

impl RuleBasedComposer {
    pub fn new(config: RuleComposerConfig) -> Self {
        Self { config }
    }
}

impl DecisionComposer for RuleBasedComposer {
    fn name(&self) -> &str {
        "rule_based"
    }

    fn compose(&self, evidence: &EvidenceVector) -> ComposerDecision {
        let det: &DeterministicEvidence = &evidence.deterministic;
        let mut reasons = Vec::new();

        let sec_risk = evidence.get_prob_or(SIGNAL_SECURITY_RISK, 0.05);
        let transient_prob = evidence
            .get_prob(SIGNAL_FAILURE_IS_TRANSIENT)
            .or_else(|| evidence.get_prob(SIGNAL_RETRY_LIKELY_TO_HELP))
            .unwrap_or(0.20);
        let out_of_scope = evidence.get_prob_or(SIGNAL_WORKER_OUT_OF_SCOPE, 0.10);
        let ambiguity = evidence.get_prob_or(SIGNAL_REQUIREMENTS_ARE_AMBIGUOUS, 0.10);
        let work_remaining = evidence.get_prob_or(SIGNAL_REQUIRED_WORK_REMAINING, 0.10);
        let obj_satisfied = evidence.get_prob_or(SIGNAL_OBJECTIVE_SATISFIED, 0.80);
        let impl_matches = evidence.get_prob_or(SIGNAL_IMPLEMENTATION_MATCHES_REQUEST, 0.80);
        let verif_needed = evidence.get_prob_or(SIGNAL_INDEPENDENT_VERIFICATION_NEEDED, 0.20);
        let unexpected_scope = evidence.get_prob_or(SIGNAL_UNEXPECTED_SCOPE_CHANGE, 0.10);

        // Calculate composite risk score
        let risk_score = (sec_risk * 0.45
            + (if det.security_sensitive_files_changed {
                0.25
            } else {
                0.0
            })
            + (if det.unexpected_files_changed {
                0.15
            } else {
                0.0
            })
            + unexpected_scope * 0.15)
            .clamp(0.0, 1.0);

        // RULE 1: Severe Security Hazard -> Escalate immediately
        if sec_risk >= self.config.security_risk_escalate_threshold
            || (det.security_sensitive_files_changed
                && sec_risk >= self.config.security_risk_verify_threshold)
        {
            reasons.push(format!(
                "Security hazard detected (risk={:.2}, sensitive_files={})",
                sec_risk, det.security_sensitive_files_changed
            ));
            return ComposerDecision::new(ReflexAction::Escalate, sec_risk, risk_score, reasons);
        }

        // RULE 2: Deterministic Failure / Error Handling
        let is_failed = det.tests_passed == Some(false)
            || det.ci_passed == Some(false)
            || (det.exit_code.is_some() && det.exit_code != Some(0))
            || det.tool_error
            || det.timeout;

        if is_failed {
            reasons.push("Deterministic failure or execution error observed".to_string());

            if transient_prob >= self.config.transient_retry_threshold
                && det.retry_count < self.config.max_retries
            {
                reasons.push(format!(
                    "Failure is transient (p={:.2}) with retry budget {}/{}",
                    transient_prob, det.retry_count, self.config.max_retries
                ));
                return ComposerDecision::new(
                    ReflexAction::Retry,
                    transient_prob,
                    risk_score,
                    reasons,
                );
            }

            if out_of_scope >= self.config.out_of_scope_escalate_threshold
                || ambiguity >= self.config.ambiguity_escalate_threshold
            {
                reasons.push(format!(
                    "Failure compounded by scope or ambiguity (out_of_scope={out_of_scope:.2}, ambiguity={ambiguity:.2})"
                ));
                return ComposerDecision::new(
                    ReflexAction::Escalate,
                    out_of_scope.max(ambiguity),
                    risk_score,
                    reasons,
                );
            }

            if det.retry_count >= self.config.max_retries {
                reasons.push(format!(
                    "Retry budget exhausted ({}/{})",
                    det.retry_count, self.config.max_retries
                ));
                return ComposerDecision::new(ReflexAction::Escalate, 0.90, risk_score, reasons);
            }

            reasons.push("Non-transient failure requiring verification inspection".to_string());
            return ComposerDecision::new(ReflexAction::Verify, 0.75, risk_score, reasons);
        }

        // RULE 3: Scope Violations & Ambiguity
        if out_of_scope >= self.config.out_of_scope_escalate_threshold
            || ambiguity >= self.config.ambiguity_escalate_threshold
        {
            reasons.push(format!(
                "Worker out of scope ({out_of_scope:.2}) or instructions ambiguous ({ambiguity:.2})"
            ));
            return ComposerDecision::new(
                ReflexAction::Escalate,
                out_of_scope.max(ambiguity),
                risk_score,
                reasons,
            );
        }

        if det.unexpected_files_changed || unexpected_scope >= 0.60 {
            reasons.push(format!(
                "Unexpected scope modification (unexpected_files={}, scope_prob={:.2})",
                det.unexpected_files_changed, unexpected_scope
            ));
            return ComposerDecision::new(ReflexAction::Verify, 0.80, risk_score, reasons);
        }

        // RULE 4: Objective Completeness / Work Remaining
        if work_remaining >= self.config.work_remaining_continue_threshold
            || !det.worker_completed
            || (obj_satisfied < 0.35 && impl_matches < 0.35)
        {
            reasons.push(format!(
                "Required work still remaining (p={:.2}, completed={})",
                work_remaining, det.worker_completed
            ));
            return ComposerDecision::new(
                ReflexAction::Continue,
                work_remaining.max(0.70),
                risk_score,
                reasons,
            );
        }

        // RULE 5: Independent Verification Gate
        if verif_needed >= self.config.verification_needed_threshold
            || sec_risk >= self.config.security_risk_verify_threshold
        {
            reasons.push(format!(
                "Verification gate triggered (verif_needed={verif_needed:.2}, sec_risk={sec_risk:.2})"
            ));
            return ComposerDecision::new(
                ReflexAction::Verify,
                verif_needed.max(sec_risk),
                risk_score,
                reasons,
            );
        }

        // RULE 6: Clean Autonomous Pass
        reasons.push(format!(
            "Clean execution: tests pass, objective satisfied ({obj_satisfied:.2}), low risk ({risk_score:.2})"
        ));

        let pass_action =
            if work_remaining < 0.15 && (obj_satisfied >= 0.85 || det.tests_passed == Some(true)) {
                ReflexAction::Terminate
            } else {
                ReflexAction::Accept
            };

        ComposerDecision::new(
            pass_action,
            obj_satisfied.min(impl_matches),
            risk_score,
            reasons,
        )
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// B. LearnedComposer Interface & Linear Classifier Baseline
// ─────────────────────────────────────────────────────────────────────────────

pub trait LearnedClassifier: Send + Sync {
    fn predict_action_scores(&self, features: &[f64]) -> HashMap<ReflexAction, f64>;
}

/// Lightweight, dependency-free linear softmax learned composer baseline
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinearSoftmaxComposer {
    pub action_names: Vec<ReflexAction>,
    // weights[action_idx][feature_idx]
    pub weights: Vec<Vec<f64>>,
    pub biases: Vec<f64>,
}

impl Default for LinearSoftmaxComposer {
    fn default() -> Self {
        Self::create_calibrated_baseline()
    }
}

impl LinearSoftmaxComposer {
    pub fn create_calibrated_baseline() -> Self {
        let action_names = vec![
            ReflexAction::Accept,
            ReflexAction::Retry,
            ReflexAction::Verify,
            ReflexAction::Escalate,
            ReflexAction::Terminate,
            ReflexAction::Continue,
        ];

        let num_features = 22;
        let mut weights = vec![vec![0.0; num_features]; action_names.len()];
        let mut biases = vec![0.0; action_names.len()];

        // Action 0: Accept (feats: tests_passed, obj_satisfied, clean signals)
        weights[0][0] = -1.5; // failure_is_transient -> negative for accept
        weights[0][5] = -3.0; // security_risk -> strongly negative
        weights[0][6] = 2.0; // implementation_matches_request
        weights[0][7] = -2.5; // required_work_remaining -> negative
        weights[0][8] = 2.0; // objective_satisfied
        weights[0][10] = -2.0; // independent_verification_needed
        weights[0][11] = 2.5; // tests_passed
        weights[0][12] = -3.0; // tests_failed
        weights[0][13] = 2.0; // ci_passed
        weights[0][14] = -3.0; // ci_failed
        biases[0] = 0.5;

        // Action 1: Retry (feats: failure_is_transient, retry_likely_to_help, tests_failed)
        weights[1][0] = 3.5; // failure_is_transient -> strongly positive
        weights[1][9] = 3.0; // retry_likely_to_help -> strongly positive
        weights[1][12] = 2.0; // tests_failed
        weights[1][14] = 2.0; // ci_failed
        weights[1][16] = -2.0; // high retry count -> negative for retry
        weights[1][5] = -2.0; // security risk -> don't retry, escalate
        biases[1] = -0.5;

        // Action 2: Verify (feats: verif_needed, unexpected_files, moderate risk)
        weights[2][4] = 2.5; // unexpected_scope_change
        weights[2][5] = 2.0; // security_risk moderate
        weights[2][10] = 3.0; // independent_verification_needed
        weights[2][18] = 3.0; // unexpected_files_changed
        weights[2][19] = 2.0; // security_sensitive_files_changed
        biases[2] = 0.0;

        // Action 3: Escalate (feats: worker_out_of_scope, requirements_ambiguous, security_risk)
        weights[3][1] = 3.5; // requirements_are_ambiguous
        weights[3][2] = 3.5; // worker_out_of_scope
        weights[3][5] = 4.0; // security_risk
        weights[3][16] = 2.5; // exhausted retries
        weights[3][19] = 2.5; // security_sensitive_files_changed
        biases[3] = -1.0;

        // Action 4: Terminate (feats: worker_completed, tests_passed, obj_satisfied)
        weights[4][7] = -3.0; // required_work_remaining -> strongly negative
        weights[4][8] = 2.5; // objective_satisfied
        weights[4][11] = 2.5; // tests_passed
        weights[4][21] = 2.0; // worker_completed
        biases[4] = 0.2;

        // Action 5: Continue (feats: required_work_remaining, incomplete)
        weights[5][7] = 4.0; // required_work_remaining -> strongly positive
        weights[5][8] = -3.0; // objective_satisfied -> negative
        weights[5][21] = -3.0; // worker_completed -> negative
        biases[5] = -0.5;

        Self {
            action_names,
            weights,
            biases,
        }
    }

    /// Trains or fine-tunes weights via multinomial logistic regression (SGD)
    pub fn fit_sgd(&mut self, samples: &[(&EvidenceVector, ReflexAction)], lr: f64, epochs: usize) {
        let n_classes = self.action_names.len();
        for _ in 0..epochs {
            for (ev, target_action) in samples {
                let feats = ev.to_feature_vector();
                let target_idx = match self.action_names.iter().position(|a| a == target_action) {
                    Some(idx) => idx,
                    None => continue,
                };

                let mut logits = Vec::with_capacity(n_classes);
                for (idx, w_row) in self.weights.iter().enumerate() {
                    let mut dot = self.biases[idx];
                    for (f_idx, &f_val) in feats.iter().enumerate() {
                        if f_idx < w_row.len() {
                            dot += w_row[f_idx] * f_val;
                        }
                    }
                    logits.push(dot);
                }

                let max_logit = logits.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
                let exps: Vec<f64> = logits.iter().map(|&l| (l - max_logit).exp()).collect();
                let sum_exp: f64 = exps.iter().sum();
                let probs: Vec<f64> = exps.iter().map(|&e| e / sum_exp).collect();

                for (i, &prob_i) in probs.iter().enumerate().take(n_classes) {
                    let grad = prob_i - (if i == target_idx { 1.0 } else { 0.0 });
                    self.biases[i] -= lr * grad;
                    for (f_idx, &f_val) in feats.iter().enumerate() {
                        if f_idx < self.weights[i].len() {
                            self.weights[i][f_idx] -= lr * grad * f_val;
                        }
                    }
                }
            }
        }
    }
}

impl LearnedClassifier for LinearSoftmaxComposer {
    fn predict_action_scores(&self, features: &[f64]) -> HashMap<ReflexAction, f64> {
        let mut logits = Vec::with_capacity(self.action_names.len());
        for (idx, w_row) in self.weights.iter().enumerate() {
            let mut dot = self.biases[idx];
            for (f_idx, &f_val) in features.iter().enumerate() {
                if f_idx < w_row.len() {
                    dot += w_row[f_idx] * f_val;
                }
            }
            logits.push(dot);
        }

        // Softmax with numerical stability
        let max_logit = logits.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let exps: Vec<f64> = logits.iter().map(|&l| (l - max_logit).exp()).collect();
        let sum_exp: f64 = exps.iter().sum();

        let mut scores = HashMap::new();
        for (i, action) in self.action_names.iter().enumerate() {
            scores.insert(action.clone(), exps[i] / sum_exp);
        }
        scores
    }
}

impl DecisionComposer for LinearSoftmaxComposer {
    fn name(&self) -> &str {
        "learned_linear"
    }

    fn compose(&self, evidence: &EvidenceVector) -> ComposerDecision {
        let features = evidence.to_feature_vector();
        let scores = self.predict_action_scores(&features);

        let mut best_action = ReflexAction::Accept;
        let mut best_prob = 0.0;

        for (action, &prob) in &scores {
            if prob > best_prob {
                best_prob = prob;
                best_action = action.clone();
            }
        }

        let sec_risk = evidence.get_prob_or(SIGNAL_SECURITY_RISK, 0.05);
        let risk_score = (sec_risk * 0.5
            + (if evidence.deterministic.security_sensitive_files_changed {
                0.3
            } else {
                0.0
            })
            + (1.0 - best_prob) * 0.2)
            .clamp(0.0, 1.0);

        ComposerDecision::new(
            best_action,
            best_prob,
            risk_score,
            vec![format!(
                "Learned classifier selected action with probability {:.2}",
                best_prob
            )],
        )
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// C. GuardedHybridComposer (Simplest Effective Hybrid Architecture)
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardedHybridConfig {
    /// Hard ceiling: tasks at or above this security probability are unconditionally escalated
    pub hard_security_escalate_threshold: f64,
    /// Hard ceiling: security sensitive files changed + security risk above this threshold
    pub sensitive_files_security_threshold: f64,
    /// Transient retry probability threshold
    pub transient_retry_threshold: f64,
    /// Maximum allowable retries
    pub max_retries: usize,
    /// Out of scope threshold for hard escalation
    pub out_of_scope_threshold: f64,
    /// Requirements ambiguity threshold for hard escalation
    pub ambiguity_threshold: f64,
    /// Work remaining threshold for Continue action
    pub work_remaining_continue_threshold: f64,
    /// Clean execution quality index threshold for Autonomous Accept (0.0 - 1.0)
    pub clean_quality_accept_threshold: f64,
}

impl Default for GuardedHybridConfig {
    fn default() -> Self {
        Self {
            hard_security_escalate_threshold: 0.50,
            sensitive_files_security_threshold: 0.25,
            transient_retry_threshold: 0.45,
            max_retries: 2,
            out_of_scope_threshold: 0.50,
            ambiguity_threshold: 0.85,
            work_remaining_continue_threshold: 0.35,
            clean_quality_accept_threshold: 0.38,
        }
    }
}

/// The Guarded Hybrid Architecture combines non-negotiable deterministic & security safety rules
/// with a calibrated multi-signal clean execution scoring model.
/// Hard rules have absolute veto power; learned/calibrated scoring operates strictly inside the safe envelope.
pub struct GuardedHybridComposer {
    config: GuardedHybridConfig,
}

impl Default for GuardedHybridComposer {
    fn default() -> Self {
        Self::new(GuardedHybridConfig::default())
    }
}

impl GuardedHybridComposer {
    pub fn new(config: GuardedHybridConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &GuardedHybridConfig {
        &self.config
    }

    /// Computes the multi-signal Clean Execution Quality Index in [0.0, 1.0]
    pub fn compute_clean_quality_index(evidence: &EvidenceVector) -> f64 {
        let det = &evidence.deterministic;
        let obj_satisfied = evidence.get_prob_or(SIGNAL_OBJECTIVE_SATISFIED, 0.50);
        let impl_matches = evidence.get_prob_or(SIGNAL_IMPLEMENTATION_MATCHES_REQUEST, 0.50);
        let ev_supports = evidence.get_prob_or(SIGNAL_EVIDENCE_SUPPORTS_CLAIM, 0.50);
        let verif_needed = evidence.get_prob_or(SIGNAL_INDEPENDENT_VERIFICATION_NEEDED, 0.30);
        let sec_risk = evidence.get_prob_or(SIGNAL_SECURITY_RISK, 0.05);
        let out_of_scope = evidence.get_prob_or(SIGNAL_WORKER_OUT_OF_SCOPE, 0.10);

        let tests_bonus = if det.tests_passed == Some(true) && det.ci_passed == Some(true) {
            0.30
        } else if det.tests_passed == Some(true) {
            0.20
        } else {
            0.0
        };

        // Linear combination of positive indicators minus suspicion penalties
        let positive =
            obj_satisfied * 0.30 + impl_matches * 0.35 + ev_supports * 0.15 + tests_bonus;
        let penalties = verif_needed * 0.15 + sec_risk * 0.45 + out_of_scope * 0.30;
        let raw_score = positive - penalties;

        raw_score.clamp(0.0, 1.0)
    }
}

impl DecisionComposer for GuardedHybridComposer {
    fn name(&self) -> &str {
        "guarded_hybrid"
    }

    fn compose(&self, evidence: &EvidenceVector) -> ComposerDecision {
        let det: &DeterministicEvidence = &evidence.deterministic;
        let mut reasons = Vec::new();

        let sec_risk = evidence.get_prob_or(SIGNAL_SECURITY_RISK, 0.05);
        let transient_prob = evidence
            .get_prob(SIGNAL_FAILURE_IS_TRANSIENT)
            .or_else(|| evidence.get_prob(SIGNAL_RETRY_LIKELY_TO_HELP))
            .unwrap_or(0.20);
        let out_of_scope = evidence.get_prob_or(SIGNAL_WORKER_OUT_OF_SCOPE, 0.10);
        let ambiguity = evidence.get_prob_or(SIGNAL_REQUIREMENTS_ARE_AMBIGUOUS, 0.10);
        let work_remaining = evidence.get_prob_or(SIGNAL_REQUIRED_WORK_REMAINING, 0.10);
        let unexpected_scope = evidence.get_prob_or(SIGNAL_UNEXPECTED_SCOPE_CHANGE, 0.10);

        // ─────────────────────────────────────────────────────────────────────
        // LAYER 1: HARD SAFETY VETO RULES (Inviolable Guardrails)
        // ─────────────────────────────────────────────────────────────────────

        // RULE 1: Severe Security Hazard -> Inviolable Frontier Escalation
        if sec_risk >= self.config.hard_security_escalate_threshold
            || (det.security_sensitive_files_changed
                && sec_risk >= self.config.sensitive_files_security_threshold)
        {
            reasons.push(format!(
                "Hard Security Rule Veto: sec_risk={:.2}, sensitive_files={}",
                sec_risk, det.security_sensitive_files_changed
            ));
            return ComposerDecision::new(ReflexAction::Escalate, sec_risk, 0.95, reasons);
        }

        // RULE 2: Deterministic Failure / Error Handling
        let is_failed = det.tests_passed == Some(false)
            || det.ci_passed == Some(false)
            || (det.exit_code.is_some() && det.exit_code != Some(0))
            || det.tool_error
            || det.timeout;

        if is_failed {
            if transient_prob >= self.config.transient_retry_threshold
                && det.retry_count < self.config.max_retries
            {
                reasons.push(format!(
                    "Transient Error Recovery: transient_prob={:.2}, retry={}/{}",
                    transient_prob, det.retry_count, self.config.max_retries
                ));
                return ComposerDecision::new(ReflexAction::Retry, transient_prob, 0.20, reasons);
            }

            if det.retry_count >= self.config.max_retries {
                reasons.push(format!(
                    "Retry budget exhausted ({}/{}) -> Hard Escalation",
                    det.retry_count, self.config.max_retries
                ));
                return ComposerDecision::new(ReflexAction::Escalate, 0.95, 0.90, reasons);
            }

            reasons.push(format!(
                "Non-transient execution failure (transient={transient_prob:.2}) -> Hard Escalation"
            ));
            return ComposerDecision::new(ReflexAction::Escalate, 0.95, 0.90, reasons);
        }

        // RULE 3: Scope Violations & Ambiguity Hard Veto
        if out_of_scope >= self.config.out_of_scope_threshold
            || (det.unexpected_files_changed && out_of_scope >= 0.30)
            || (ambiguity >= self.config.ambiguity_threshold && !det.worker_completed)
        {
            reasons.push(format!(
                "Hard Scope/Ambiguity Veto: out_of_scope={:.2}, unexpected_files={}, ambiguity={:.2}",
                out_of_scope, det.unexpected_files_changed, ambiguity
            ));
            return ComposerDecision::new(
                ReflexAction::Escalate,
                out_of_scope.max(ambiguity),
                0.90,
                reasons,
            );
        }

        if det.unexpected_files_changed || unexpected_scope >= 0.58 {
            reasons.push("Unexpected Scope Expansion -> Verification Gate".to_string());
            return ComposerDecision::new(ReflexAction::Verify, 0.80, 0.60, reasons);
        }

        // ─────────────────────────────────────────────────────────────────────
        // LAYER 2: MULTI-STEP PROGRESSION (Autonomous Continue)
        // ─────────────────────────────────────────────────────────────────────
        if work_remaining >= self.config.work_remaining_continue_threshold
            && !det.worker_completed
            && det.tests_passed != Some(false)
            && sec_risk < 0.20
        {
            reasons.push(format!(
                "Multi-step progress in order: work_remaining={work_remaining:.2}, worker_completed=false"
            ));
            return ComposerDecision::new(ReflexAction::Continue, work_remaining, 0.15, reasons);
        }

        // ─────────────────────────────────────────────────────────────────────
        // LAYER 3: CALIBRATED QUALITY INDEX (Autonomous Accept vs Verification)
        // ─────────────────────────────────────────────────────────────────────
        let quality = Self::compute_clean_quality_index(evidence);

        if quality >= self.config.clean_quality_accept_threshold
            && sec_risk < 0.20
            && out_of_scope < 0.25
            && det.tests_passed == Some(true)
            && !det.unexpected_files_changed
            && !det.security_sensitive_files_changed
        {
            reasons.push(format!(
                "High Quality Clean Pass: quality={:.2} >= {:.2}, sec_risk={:.2}",
                quality, self.config.clean_quality_accept_threshold, sec_risk
            ));
            let action = if det.worker_completed && work_remaining < 0.20 {
                ReflexAction::Terminate
            } else {
                ReflexAction::Accept
            };
            let comp_risk = ((1.0 - quality) * 0.20 + sec_risk * 0.30).clamp(0.0, 1.0);
            return ComposerDecision::new(action, quality, comp_risk, reasons);
        }

        reasons.push(format!(
            "Borderline Quality ({:.2} < {:.2}) -> Verification Gate",
            quality, self.config.clean_quality_accept_threshold
        ));
        let comp_risk = ((1.0 - quality) * 0.40 + sec_risk * 0.40).clamp(0.0, 1.0);
        ComposerDecision::new(ReflexAction::Verify, 1.0 - quality, comp_risk, reasons)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reflex_core::SemanticEvidence;

    #[test]
    fn test_rule_based_composer_transient_retry() {
        let composer = RuleBasedComposer::default();
        let det = DeterministicEvidence {
            tests_passed: Some(false),
            retry_count: 1,
            ..Default::default()
        };
        let ev = EvidenceVector::new(det).with_semantic(SemanticEvidence::new(
            SIGNAL_FAILURE_IS_TRANSIENT,
            0.92,
            "mock",
            10,
        ));

        let dec = composer.compose(&ev);
        assert_eq!(dec.action, ReflexAction::Retry);
        assert!(dec.confidence >= 0.90);
    }

    #[test]
    fn test_rule_based_composer_security_escalate() {
        let composer = RuleBasedComposer::default();
        let det = DeterministicEvidence {
            tests_passed: Some(true),
            security_sensitive_files_changed: true,
            ..Default::default()
        };
        let ev = EvidenceVector::new(det).with_semantic(SemanticEvidence::new(
            SIGNAL_SECURITY_RISK,
            0.85,
            "mock",
            10,
        ));

        let dec = composer.compose(&ev);
        assert_eq!(dec.action, ReflexAction::Escalate);
    }

    #[test]
    fn test_linear_softmax_composer_prediction() {
        let composer = LinearSoftmaxComposer::default();
        let det = DeterministicEvidence {
            tests_passed: Some(true),
            ci_passed: Some(true),
            ..Default::default()
        };
        let ev = EvidenceVector::new(det).with_semantic(SemanticEvidence::new(
            SIGNAL_OBJECTIVE_SATISFIED,
            0.95,
            "mock",
            10,
        ));

        let dec = composer.compose(&ev);
        assert!(dec.action.is_autonomous_pass());
    }

    #[test]
    fn test_guarded_hybrid_hard_security_veto() {
        let composer = GuardedHybridComposer::default();
        let det = DeterministicEvidence {
            tests_passed: Some(true),
            ci_passed: Some(true),
            ..Default::default()
        };
        // High security risk MUST trigger hard veto even if objective satisfied is 0.99
        let ev = EvidenceVector::new(det)
            .with_semantic(SemanticEvidence::new(
                SIGNAL_OBJECTIVE_SATISFIED,
                0.99,
                "mock",
                10,
            ))
            .with_semantic(SemanticEvidence::new(
                SIGNAL_SECURITY_RISK,
                0.75,
                "mock",
                10,
            ));

        let dec = composer.compose(&ev);
        assert_eq!(dec.action, ReflexAction::Escalate);
        assert!(dec
            .reasoning
            .iter()
            .any(|r| r.contains("Hard Security Rule Veto")));
    }

    #[test]
    fn test_guarded_hybrid_clean_autonomous_accept() {
        let composer = GuardedHybridComposer::default();
        let det = DeterministicEvidence {
            tests_passed: Some(true),
            ci_passed: Some(true),
            worker_completed: true,
            ..Default::default()
        };
        let ev = EvidenceVector::new(det)
            .with_semantic(SemanticEvidence::new(
                SIGNAL_OBJECTIVE_SATISFIED,
                0.92,
                "mock",
                10,
            ))
            .with_semantic(SemanticEvidence::new(
                SIGNAL_IMPLEMENTATION_MATCHES_REQUEST,
                0.90,
                "mock",
                10,
            ))
            .with_semantic(SemanticEvidence::new(
                SIGNAL_EVIDENCE_SUPPORTS_CLAIM,
                0.88,
                "mock",
                10,
            ))
            .with_semantic(SemanticEvidence::new(
                SIGNAL_INDEPENDENT_VERIFICATION_NEEDED,
                0.35,
                "mock",
                10,
            ))
            .with_semantic(SemanticEvidence::new(
                SIGNAL_SECURITY_RISK,
                0.04,
                "mock",
                10,
            ));

        let dec = composer.compose(&ev);
        assert!(dec.action.is_autonomous_pass());
        assert!(dec.risk_score < 0.25);
    }

    #[test]
    fn test_guarded_hybrid_multi_step_continue() {
        let composer = GuardedHybridComposer::default();
        let det = DeterministicEvidence {
            tests_passed: Some(true),
            worker_completed: false,
            ..Default::default()
        };
        let ev = EvidenceVector::new(det)
            .with_semantic(SemanticEvidence::new(
                SIGNAL_REQUIRED_WORK_REMAINING,
                0.70,
                "mock",
                10,
            ))
            .with_semantic(SemanticEvidence::new(
                SIGNAL_SECURITY_RISK,
                0.05,
                "mock",
                10,
            ));

        let dec = composer.compose(&ev);
        assert_eq!(dec.action, ReflexAction::Continue);
    }
}
