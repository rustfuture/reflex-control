//! Maps the outcome of an agent runtime's verification gate onto Reflex Control's
//! Accept / Retry / Escalate decision. Fully offline: semantic signals come from the
//! mock evidence provider and no runtime crate is linked; `RuntimeTaskResult` mirrors
//! the fields such a runtime reports (exit status, verification result, changed files,
//! remaining retry budget).

use reflex_core::{
    DeterministicEvidence, EvidenceVector, ReflexAction, RiskLevel, ALL_ATOMIC_SIGNALS,
    SIGNAL_EVIDENCE_SUPPORTS_CLAIM, SIGNAL_IMPLEMENTATION_MATCHES_REQUEST,
    SIGNAL_UNEXPECTED_SCOPE_CHANGE, SIGNAL_WORKER_OUT_OF_SCOPE,
};
use reflex_policy::composer::{DecisionComposer, GuardedHybridComposer};
use reflex_policy::risk_defer::RiskAbstentionPolicy;
use reflex_provider::{AtomicEvidenceProvider, MockEvidenceProvider};
use std::fmt;

/// Retry budget the runtime grants a task in total.
const MAX_RETRIES: usize = 2;

/// Runtime-like result of one task attempt.
#[derive(Debug, Clone)]
pub struct RuntimeTaskResult {
    /// Exit status of the last command, if one ran.
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    /// Outcome of the verification command after the last edit; `None` if it never ran.
    pub verification_passed: Option<bool>,
    pub changed_files: Vec<String>,
    pub retries_left: usize,
    /// Free-text summary/log excerpt handed to the semantic evidence provider.
    pub summary: String,
}

/// The three-way outcome the orchestrator acts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateDecision {
    Accept,
    Retry,
    Escalate,
}

impl fmt::Display for GateDecision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            GateDecision::Accept => "accept",
            GateDecision::Retry => "retry",
            GateDecision::Escalate => "escalate",
        })
    }
}

fn is_sensitive_path(path: &str) -> bool {
    let p = path.to_lowercase();
    [
        ".env",
        "secret",
        "credential",
        "auth",
        "migration",
        ".pem",
        ".key",
        "id_rsa",
    ]
    .iter()
    .any(|needle| p.contains(needle))
}

/// Maps the runtime result into reflex-core deterministic evidence.
pub fn to_deterministic(result: &RuntimeTaskResult) -> DeterministicEvidence {
    DeterministicEvidence {
        tests_passed: result.verification_passed,
        exit_code: result.exit_code,
        timeout: result.timed_out,
        retry_count: MAX_RETRIES.saturating_sub(result.retries_left),
        files_changed: result.changed_files.len(),
        security_sensitive_files_changed: result.changed_files.iter().any(|p| is_sensitive_path(p)),
        worker_completed: result.exit_code == Some(0) && !result.timed_out,
        ..Default::default()
    }
}

/// Collapses Reflex actions to the three outcomes. Anything that is not a clean
/// autonomous pass or a budgeted retry goes to escalation (fail closed).
pub fn to_gate_decision(action: &ReflexAction) -> GateDecision {
    match action {
        ReflexAction::Accept | ReflexAction::Terminate => GateDecision::Accept,
        ReflexAction::Retry => GateDecision::Retry,
        _ => GateDecision::Escalate,
    }
}

pub async fn decide(
    result: &RuntimeTaskResult,
    task_risk: RiskLevel,
) -> Result<(GateDecision, ReflexAction), Box<dyn std::error::Error>> {
    let mut evidence = EvidenceVector::new(to_deterministic(result));
    // The mock answers 0.5 for scope/match signals it has no heuristic for, which
    // trips the scope veto. Pin those to benign values so that the security,
    // transient, objective and work-remaining heuristics (keyed on the summary
    // text) drive the outcome.
    let provider = MockEvidenceProvider::new()
        .with_signal(SIGNAL_WORKER_OUT_OF_SCOPE, 0.05)
        .with_signal(SIGNAL_UNEXPECTED_SCOPE_CHANGE, 0.05)
        .with_signal(SIGNAL_IMPLEMENTATION_MATCHES_REQUEST, 0.90)
        .with_signal(SIGNAL_EVIDENCE_SUPPORTS_CLAIM, 0.90);
    for sig in provider
        .evaluate_evidence(&result.summary, ALL_ATOMIC_SIGNALS)
        .await?
        .signals
    {
        evidence.add_semantic(sig);
    }

    let composer = GuardedHybridComposer::default();
    let composed = composer.compose(&evidence);
    let (action, _risk) = RiskAbstentionPolicy::default().evaluate(&composed, &evidence, task_risk);
    Ok((to_gate_decision(&action), action))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cases = [
        (
            "verified fix",
            RiskLevel::Low,
            RuntimeTaskResult {
                exit_code: Some(0),
                timed_out: false,
                verification_passed: Some(true),
                changed_files: vec!["src/lib.rs".into()],
                retries_left: 2,
                summary: "tests passed, success".into(),
            },
        ),
        (
            "provider timeout, budget left",
            RiskLevel::Low,
            RuntimeTaskResult {
                exit_code: None,
                timed_out: true,
                verification_passed: Some(false),
                changed_files: vec!["src/lib.rs".into()],
                retries_left: 2,
                summary: "request timeout".into(),
            },
        ),
        (
            "verification failing, budget exhausted",
            RiskLevel::Low,
            RuntimeTaskResult {
                exit_code: Some(101),
                timed_out: false,
                verification_passed: Some(false),
                changed_files: vec!["src/lib.rs".into()],
                retries_left: 0,
                summary: "assertion failed".into(),
            },
        ),
        (
            "green tests, secret file touched",
            RiskLevel::Low,
            RuntimeTaskResult {
                exit_code: Some(0),
                timed_out: false,
                verification_passed: Some(true),
                changed_files: vec![".env".into()],
                retries_left: 2,
                summary: "tests passed, success, credential leak".into(),
            },
        ),
    ];

    println!("=== Reflex Control: agent-runtime gate (offline, mock evidence) ===");
    for (name, risk, result) in &cases {
        let (decision, action) = decide(result, *risk).await?;
        println!("{name:<40} -> {decision} (reflex action: {action})");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok_result() -> RuntimeTaskResult {
        RuntimeTaskResult {
            exit_code: Some(0),
            timed_out: false,
            verification_passed: Some(true),
            changed_files: vec!["src/lib.rs".into()],
            retries_left: 2,
            summary: "tests passed, success".into(),
        }
    }

    #[test]
    fn maps_fields_into_deterministic_evidence() {
        let mut r = ok_result();
        r.retries_left = 1;
        r.changed_files.push("config/secrets.toml".into());
        let det = to_deterministic(&r);
        assert_eq!(det.tests_passed, Some(true));
        assert_eq!(det.retry_count, 1);
        assert_eq!(det.files_changed, 2);
        assert!(det.security_sensitive_files_changed);
        assert!(det.worker_completed);
    }

    #[tokio::test]
    async fn verified_clean_run_is_accepted() {
        let (d, _) = decide(&ok_result(), RiskLevel::Low).await.unwrap();
        assert_eq!(d, GateDecision::Accept);
    }

    #[tokio::test]
    async fn transient_failure_with_budget_retries_then_escalates_when_exhausted() {
        let mut r = ok_result();
        r.exit_code = None;
        r.timed_out = true;
        r.verification_passed = Some(false);
        r.summary = "request timeout".into();
        assert_eq!(
            decide(&r, RiskLevel::Low).await.unwrap().0,
            GateDecision::Retry
        );
        r.retries_left = 0;
        assert_eq!(
            decide(&r, RiskLevel::Low).await.unwrap().0,
            GateDecision::Escalate
        );
    }

    #[tokio::test]
    async fn security_signal_or_critical_risk_escalates_despite_green_verification() {
        let mut r = ok_result();
        r.summary = "tests passed, success, credential leak".into();
        assert_eq!(
            decide(&r, RiskLevel::Low).await.unwrap().0,
            GateDecision::Escalate
        );
        assert_eq!(
            decide(&ok_result(), RiskLevel::Critical).await.unwrap().0,
            GateDecision::Escalate
        );
    }

    #[tokio::test]
    async fn missing_verification_is_never_accepted() {
        let mut r = ok_result();
        r.verification_passed = None;
        assert_ne!(
            decide(&r, RiskLevel::Low).await.unwrap().0,
            GateDecision::Accept
        );
    }
}
