use crate::client::JevClient;
use crate::config::JevConfig;
use crate::types::{Answer, QuestionSpec, SystemOneRequest};
use async_trait::async_trait;
use reflex_core::{
    SemanticEvidence, SIGNAL_EVIDENCE_SUPPORTS_CLAIM, SIGNAL_FAILURE_IS_TRANSIENT,
    SIGNAL_IMPLEMENTATION_MATCHES_REQUEST, SIGNAL_INDEPENDENT_VERIFICATION_NEEDED,
    SIGNAL_OBJECTIVE_SATISFIED, SIGNAL_REQUIRED_WORK_REMAINING, SIGNAL_REQUIREMENTS_ARE_AMBIGUOUS,
    SIGNAL_RETRY_LIKELY_TO_HELP, SIGNAL_SECURITY_RISK, SIGNAL_UNEXPECTED_SCOPE_CHANGE,
    SIGNAL_WORKER_OUT_OF_SCOPE,
};
use reflex_provider::{AtomicEvidenceProvider, EvidenceEvaluationResponse, ProviderError};
use std::collections::HashMap;
use std::time::Instant;

/// Official TypeSafe Jev implementation of the AtomicEvidenceProvider trait.
/// Evaluates narrow, decoupled semantic questions in parallel via a single SystemOne request.
pub struct JevAtomicEvidenceProvider {
    client: JevClient,
}

impl JevAtomicEvidenceProvider {
    pub fn new(config: JevConfig) -> Self {
        Self {
            client: JevClient::new(config),
        }
    }

    pub fn client(&self) -> &JevClient {
        &self.client
    }

    pub fn question_for_signal(signal_name: &str) -> QuestionSpec {
        let instructions = match signal_name {
            SIGNAL_FAILURE_IS_TRANSIENT => {
                "Is the encountered failure or error transient and likely to succeed on immediate retry (such as network timeout, rate limit 429, socket reset, temporary connection failure)?"
            }
            SIGNAL_REQUIREMENTS_ARE_AMBIGUOUS => {
                "Are the task specifications, user prompt, or requirements ambiguous, conflicting, or missing critical parameters?"
            }
            SIGNAL_WORKER_OUT_OF_SCOPE => {
                "Is the worker operating beyond its capability, touching unrelated systems, or clearly out of its depth?"
            }
            SIGNAL_EVIDENCE_SUPPORTS_CLAIM => {
                "Does the provided context, test output, or execution log actually support the claim of successful completion?"
            }
            SIGNAL_UNEXPECTED_SCOPE_CHANGE => {
                "Did this task introduce unexpected changes, modify unrequested files, or exceed the intended scope boundaries?"
            }
            SIGNAL_SECURITY_RISK => {
                "Does this execution or code change introduce any security vulnerability, privilege escalation, secret leak, or hazardous command?"
            }
            SIGNAL_IMPLEMENTATION_MATCHES_REQUEST => {
                "Does the completed work directly and faithfully match what the user requested?"
            }
            SIGNAL_REQUIRED_WORK_REMAINING => {
                "Is any required subtask, verification, test assertion, or objective still unfinished or pending?"
            }
            SIGNAL_OBJECTIVE_SATISFIED => {
                "Has the core objective and intended functionality been successfully achieved and verified?"
            }
            SIGNAL_RETRY_LIKELY_TO_HELP => {
                "Is immediately retrying this exact action likely to succeed without altering the code or parameters?"
            }
            SIGNAL_INDEPENDENT_VERIFICATION_NEEDED => {
                "Does this execution outcome require independent verification by a stronger model or human reviewer before acceptance?"
            }
            _ => "Assess the probability [0.0 - 1.0] that this condition holds true based on the provided context.",
        };

        QuestionSpec::Noul {
            instructions: instructions.to_string(),
        }
    }
}

#[async_trait]
impl AtomicEvidenceProvider for JevAtomicEvidenceProvider {
    fn name(&self) -> &str {
        "jev-atomic"
    }

    async fn evaluate_evidence(
        &self,
        context: &str,
        requested_signals: &[&str],
    ) -> Result<EvidenceEvaluationResponse, ProviderError> {
        let mut questions = HashMap::new();
        for &sig in requested_signals {
            questions.insert(sig.to_string(), Self::question_for_signal(sig));
        }

        let sys1_req = SystemOneRequest {
            model: self.client.config().model.clone(),
            state: context.to_string(),
            questions,
        };

        let start = Instant::now();
        let resp = self.client.execute_system_one(&sys1_req).await?;
        let latency_ms = start.elapsed().as_millis() as u64;

        let input_tokens = resp.usage.as_ref().map(|u| u.input_tokens).unwrap_or(0);
        let cost_estimate = (input_tokens as f64 / 1_000_000.0) * 0.042;

        let mut signals = Vec::new();
        for (sig_name, ans) in resp.answers {
            let prob = match ans {
                Answer::Noul { noul } => noul.clamp(0.0, 1.0),
                Answer::Choice { confidence, .. } => confidence.clamp(0.0, 1.0),
                Answer::Score { score, .. } => (score / 4.0).clamp(0.0, 1.0),
            };

            signals.push(SemanticEvidence::new(
                sig_name,
                prob,
                "jev-systemone",
                latency_ms,
            ));
        }

        Ok(EvidenceEvaluationResponse {
            signals,
            latency_ms,
            cost_estimate,
            input_tokens,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reflex_core::ALL_ATOMIC_SIGNALS;

    #[test]
    fn test_question_spec_generation_for_all_signals() {
        for &sig in ALL_ATOMIC_SIGNALS {
            let q = JevAtomicEvidenceProvider::question_for_signal(sig);
            match q {
                QuestionSpec::Noul { instructions } => {
                    assert!(!instructions.is_empty(), "Question empty for {sig}");
                }
                _ => panic!("Expected Noul question for atomic evidence"),
            }
        }
    }
}
