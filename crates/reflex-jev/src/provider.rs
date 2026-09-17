use crate::client::JevClient;
use crate::config::JevConfig;
use crate::types::{Answer, QuestionSpec, SystemOneRequest};
use async_trait::async_trait;
use reflex_core::{Decision, DecisionRequest, DecisionResponse};
use reflex_provider::{DecisionProvider, ProviderError};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Instant;

/// Supported prompt/question formulation variants
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum FormulationVariant {
    /// Baseline / Concise criteria
    #[default]
    A,
    /// Explicit criteria clarity / Inverted polarity / Direct defect detection
    B,
    /// Domain role / Execution pipeline / Inspection necessity
    C,
}

/// Active formulation configuration for each decision type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct FormulationConfig {
    pub routing: FormulationVariant,
    pub retry: FormulationVariant,
    pub termination: FormulationVariant,
    pub verification: FormulationVariant,
}

impl FormulationConfig {
    pub fn all(variant: FormulationVariant) -> Self {
        Self {
            routing: variant,
            retry: variant,
            termination: variant,
            verification: variant,
        }
    }

    pub fn all_a() -> Self {
        Self::all(FormulationVariant::A)
    }

    pub fn all_b() -> Self {
        Self::all(FormulationVariant::B)
    }

    pub fn all_c() -> Self {
        Self::all(FormulationVariant::C)
    }
}

pub struct JevProvider {
    client: JevClient,
    formulations: FormulationConfig,
}

impl JevProvider {
    pub fn new(config: JevConfig) -> Self {
        Self {
            client: JevClient::new(config),
            formulations: FormulationConfig::default(),
        }
    }

    pub fn new_with_formulations(config: JevConfig, formulations: FormulationConfig) -> Self {
        Self {
            client: JevClient::new(config),
            formulations,
        }
    }

    pub fn client(&self) -> &JevClient {
        &self.client
    }

    pub fn formulations(&self) -> &FormulationConfig {
        &self.formulations
    }

    pub fn set_formulations(&mut self, formulations: FormulationConfig) {
        self.formulations = formulations;
    }

    pub fn with_formulations(mut self, formulations: FormulationConfig) -> Self {
        self.formulations = formulations;
        self
    }
}

#[async_trait]
impl DecisionProvider for JevProvider {
    fn name(&self) -> &str {
        "jev"
    }

    async fn choice(&self, request: &DecisionRequest) -> Result<DecisionResponse, ProviderError> {
        let is_retry = request
            .options
            .iter()
            .any(|o| o.eq_ignore_ascii_case("retry"));

        let (instructions, criteria) = if is_retry {
            match self.formulations.retry {
                FormulationVariant::A => {
                    let mut c = HashMap::new();
                    c.insert(
                        "retry".to_string(),
                        "Transient failure suitable for retry".to_string(),
                    );
                    c.insert(
                        "escalate".to_string(),
                        "High risk, critical issue, or defect requiring frontier LLM".to_string(),
                    );
                    c.insert(
                        "terminate".to_string(),
                        "Task is complete or cannot make progress".to_string(),
                    );
                    (
                        "Evaluate the agent task context and select the optimal decision action."
                            .to_string(),
                        c,
                    )
                }
                FormulationVariant::B => {
                    let mut c = HashMap::new();
                    c.insert(
                        "retry".to_string(),
                        "Transient, recoverable failure such as rate limit (HTTP 429), network timeout, or temporary resource lock that will likely succeed on immediate retry."
                            .to_string(),
                    );
                    c.insert(
                        "escalate".to_string(),
                        "Non-transient logic failure, persistent code defect, security violation, or unrecoverable assertion error requiring frontier model or human debugging."
                            .to_string(),
                    );
                    c.insert(
                        "terminate".to_string(),
                        "Task has completed successfully or reached a permanent terminal state where no further execution attempts should be made."
                            .to_string(),
                    );
                    (
                        "Determine the post-execution recovery action for this agent task."
                            .to_string(),
                        c,
                    )
                }
                FormulationVariant::C => {
                    let mut c = HashMap::new();
                    c.insert(
                        "retry".to_string(),
                        "Re-run the same operation because the failure is temporary or flaky."
                            .to_string(),
                    );
                    c.insert(
                        "escalate".to_string(),
                        "Transfer control to a frontier LLM or supervisor because error requires deep code reasoning."
                            .to_string(),
                    );
                    c.insert(
                        "terminate".to_string(),
                        "End workflow execution because the task succeeded or is permanently unrecoverable."
                            .to_string(),
                    );
                    (
                        "Select recovery protocol for the observed execution outcome.".to_string(),
                        c,
                    )
                }
            }
        } else {
            // Routing
            match self.formulations.routing {
                FormulationVariant::A => {
                    let mut c = HashMap::new();
                    c.insert(
                        "accept".to_string(),
                        "Task output is safe, high quality, and verified clean".to_string(),
                    );
                    c.insert(
                        "verify".to_string(),
                        "Borderline confidence or requires automated check".to_string(),
                    );
                    c.insert(
                        "escalate".to_string(),
                        "High risk, critical issue, or defect requiring frontier LLM".to_string(),
                    );
                    (
                        "Evaluate the agent task context and select the optimal decision action."
                            .to_string(),
                        c,
                    )
                }
                FormulationVariant::B => {
                    let mut c = HashMap::new();
                    c.insert(
                        "accept".to_string(),
                        "Output is verified clean, low-risk, standard routine operation, proceed directly without additional verification or escalation."
                            .to_string(),
                    );
                    c.insert(
                        "verify".to_string(),
                        "Output is plausible but high-risk or ambiguous, requiring automated verification gate or test pass before proceeding."
                            .to_string(),
                    );
                    c.insert(
                        "escalate".to_string(),
                        "Task failed, encountered critical error, high-severity defect, or cannot be safely handled by System-1 worker; requires System-2 frontier reasoning."
                            .to_string(),
                    );
                    (
                        "Determine the routing action for this agent task based on safety, risk level, and completion confidence."
                            .to_string(),
                        c,
                    )
                }
                FormulationVariant::C => {
                    let mut c = HashMap::new();
                    c.insert(
                        "accept".to_string(),
                        "No intervention needed; task succeeded cleanly within safety parameters."
                            .to_string(),
                    );
                    c.insert(
                        "verify".to_string(),
                        "Inspection required; task has elevated risk, ambiguous output, or missing verification."
                            .to_string(),
                    );
                    c.insert(
                        "escalate".to_string(),
                        "Failure or critical hazard; task exceeded worker capacity and needs human or frontier model escalation."
                            .to_string(),
                    );
                    (
                        "Classify execution state into the appropriate handling pipeline: autonomous pass, check gate, or senior model intervention."
                            .to_string(),
                        c,
                    )
                }
            }
        };

        let mut questions = HashMap::new();
        questions.insert(
            "decision".to_string(),
            QuestionSpec::Choice {
                instructions,
                criteria,
            },
        );

        let sys1_req = SystemOneRequest {
            model: self.client.config().model.clone(),
            state: request.observation.context.clone(),
            questions,
        };

        let start = Instant::now();
        let resp = self.client.execute_system_one(&sys1_req).await?;
        let latency_ms = start.elapsed().as_millis() as u64;

        let cost_estimate = resp
            .usage
            .as_ref()
            .map(|u| (u.input_tokens as f64 / 1_000_000.0) * 0.042)
            .unwrap_or(0.0);

        let answer = resp.answers.get("decision").ok_or_else(|| {
            ProviderError::MalformedResponse(
                "Missing 'decision' answer from Jev response".to_string(),
            )
        })?;

        match answer {
            Answer::Choice {
                choice,
                confidence,
                probabilities,
            } => {
                let prob_vec: Vec<(String, f64)> =
                    probabilities.iter().map(|(k, v)| (k.clone(), *v)).collect();

                let decision = Decision::new(choice.clone(), prob_vec, *confidence);
                Ok(DecisionResponse::new(
                    request.id.clone(),
                    decision,
                    "jev-live",
                    latency_ms,
                    cost_estimate,
                ))
            }
            _ => Err(ProviderError::MalformedResponse(
                "Expected Choice answer from Jev API, got different primitive".to_string(),
            )),
        }
    }

    async fn score(&self, request: &DecisionRequest) -> Result<DecisionResponse, ProviderError> {
        let mut questions = HashMap::new();
        questions.insert(
            "risk".to_string(),
            QuestionSpec::Score {
                instructions:
                    "Rate the execution risk and defect likelihood from 0 (lowest) to 4 (critical)"
                        .to_string(),
                criteria: vec![
                    "Minimal risk, fully compliant".to_string(),
                    "Low risk, standard operation".to_string(),
                    "Moderate risk, requires inspection".to_string(),
                    "High risk of defect or failure".to_string(),
                    "Critical failure or severe risk".to_string(),
                ],
            },
        );

        let sys1_req = SystemOneRequest {
            model: self.client.config().model.clone(),
            state: request.observation.context.clone(),
            questions,
        };

        let start = Instant::now();
        let resp = self.client.execute_system_one(&sys1_req).await?;
        let latency_ms = start.elapsed().as_millis() as u64;

        let cost_estimate = resp
            .usage
            .as_ref()
            .map(|u| (u.input_tokens as f64 / 1_000_000.0) * 0.042)
            .unwrap_or(0.0);

        let answer = resp.answers.get("risk").ok_or_else(|| {
            ProviderError::MalformedResponse("Missing 'risk' answer from Jev response".to_string())
        })?;

        match answer {
            Answer::Score {
                score,
                confidence,
                probabilities,
                ..
            } => {
                let prob_vec: Vec<(String, f64)> = probabilities
                    .as_ref()
                    .map(|p| p.iter().map(|(k, v)| (k.clone(), *v)).collect())
                    .unwrap_or_default();
                let decision = Decision::new(score.to_string(), prob_vec, *confidence);
                Ok(DecisionResponse::new(
                    request.id.clone(),
                    decision,
                    "jev-live",
                    latency_ms,
                    cost_estimate,
                ))
            }
            _ => Err(ProviderError::MalformedResponse(
                "Expected Score answer from Jev API".to_string(),
            )),
        }
    }

    async fn probability(
        &self,
        request: &DecisionRequest,
    ) -> Result<DecisionResponse, ProviderError> {
        let is_termination = request.options.iter().any(|o| o == "terminate");
        let (question_key, instructions) = if is_termination {
            match self.formulations.termination {
                FormulationVariant::A => (
                    "should_terminate".to_string(),
                    "Has this agent task completed all its required objectives and should execution terminate now?"
                        .to_string(),
                ),
                FormulationVariant::B => (
                    "is_incomplete".to_string(),
                    "Is any required work, subtask, test, or objective still incomplete or pending for this agent?"
                        .to_string(),
                ),
                FormulationVariant::C => (
                    "is_finished".to_string(),
                    "Is this agent workflow fully concluded with all goals satisfied such that execution should stop immediately?"
                        .to_string(),
                ),
            }
        } else {
            // Verification
            match self.formulations.verification {
                FormulationVariant::A => (
                    "is_valid".to_string(),
                    "Is this automated task execution clean, correct, and valid without defects?"
                        .to_string(),
                ),
                FormulationVariant::B => (
                    "has_defect".to_string(),
                    "Does this execution contain a defect, correctness issue, error, or safety concern that requires independent verification?"
                        .to_string(),
                ),
                FormulationVariant::C => (
                    "needs_verification".to_string(),
                    "Does this task output require an automated verifier or frontier model check before it can be trusted?"
                        .to_string(),
                ),
            }
        };

        let mut questions = HashMap::new();
        questions.insert(question_key.clone(), QuestionSpec::Noul { instructions });

        let sys1_req = SystemOneRequest {
            model: self.client.config().model.clone(),
            state: request.observation.context.clone(),
            questions,
        };

        let start = Instant::now();
        let resp = self.client.execute_system_one(&sys1_req).await?;
        let latency_ms = start.elapsed().as_millis() as u64;

        let cost_estimate = resp
            .usage
            .as_ref()
            .map(|u| (u.input_tokens as f64 / 1_000_000.0) * 0.042)
            .unwrap_or(0.0);

        let answer = resp.answers.get(&question_key).ok_or_else(|| {
            ProviderError::MalformedResponse(format!(
                "Missing '{question_key}' answer from Jev response"
            ))
        })?;

        match answer {
            Answer::Noul { noul } => {
                let raw_p = (*noul).clamp(0.0, 1.0);
                let decision = if is_termination {
                    let p_term = match self.formulations.termination {
                        FormulationVariant::B => (1.0 - raw_p).clamp(0.0, 1.0),
                        _ => raw_p,
                    };
                    let sel = if p_term >= 0.5 {
                        "terminate"
                    } else {
                        "continue"
                    };
                    let conf = if p_term >= 0.5 { p_term } else { 1.0 - p_term };
                    Decision::new(
                        sel,
                        vec![
                            ("terminate".to_string(), p_term),
                            ("continue".to_string(), 1.0 - p_term),
                        ],
                        conf,
                    )
                } else {
                    // Verification
                    match self.formulations.verification {
                        FormulationVariant::A => {
                            let sel = if raw_p >= 0.5 { "clean" } else { "defect" };
                            let conf = if raw_p >= 0.5 { raw_p } else { 1.0 - raw_p };
                            Decision::new(
                                sel,
                                vec![
                                    ("clean".to_string(), raw_p),
                                    ("defect".to_string(), 1.0 - raw_p),
                                    ("is_valid".to_string(), raw_p),
                                ],
                                conf,
                            )
                        }
                        FormulationVariant::B => {
                            let sel = if raw_p >= 0.5 { "defect" } else { "clean" };
                            let conf = if raw_p >= 0.5 { raw_p } else { 1.0 - raw_p };
                            Decision::new(
                                sel,
                                vec![
                                    ("defect".to_string(), raw_p),
                                    ("clean".to_string(), 1.0 - raw_p),
                                    ("has_defect".to_string(), raw_p),
                                ],
                                conf,
                            )
                        }
                        FormulationVariant::C => {
                            let sel = if raw_p >= 0.5 { "defect" } else { "clean" };
                            let conf = if raw_p >= 0.5 { raw_p } else { 1.0 - raw_p };
                            Decision::new(
                                sel,
                                vec![
                                    ("defect".to_string(), raw_p),
                                    ("clean".to_string(), 1.0 - raw_p),
                                    ("needs_verification".to_string(), raw_p),
                                ],
                                conf,
                            )
                        }
                    }
                };

                Ok(DecisionResponse::new(
                    request.id.clone(),
                    decision,
                    "jev-live",
                    latency_ms,
                    cost_estimate,
                ))
            }
            _ => Err(ProviderError::MalformedResponse(
                "Expected Noul answer from Jev API".to_string(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_formulation_config_defaults() {
        let config = FormulationConfig::default();
        assert_eq!(config.routing, FormulationVariant::A);
        assert_eq!(config.retry, FormulationVariant::A);
        assert_eq!(config.termination, FormulationVariant::A);
        assert_eq!(config.verification, FormulationVariant::A);

        let config_b = FormulationConfig::all_b();
        assert_eq!(config_b.routing, FormulationVariant::B);
        assert_eq!(config_b.retry, FormulationVariant::B);
        assert_eq!(config_b.termination, FormulationVariant::B);
        assert_eq!(config_b.verification, FormulationVariant::B);
    }

    #[test]
    fn test_termination_polarity_inversion_logic() {
        // Variant B inverts raw_p: P(terminate) = 1.0 - raw_p
        let raw_p_incomplete: f64 = 0.85; // highly incomplete
        let p_term = (1.0 - raw_p_incomplete).clamp(0.0, 1.0);
        assert!((p_term - 0.15).abs() < 1e-6);
        let sel = if p_term >= 0.5 {
            "terminate"
        } else {
            "continue"
        };
        assert_eq!(sel, "continue");

        let raw_p_not_incomplete: f64 = 0.05; // almost fully complete
        let p_term2 = (1.0 - raw_p_not_incomplete).clamp(0.0, 1.0);
        assert!((p_term2 - 0.95).abs() < 1e-6);
        let sel2 = if p_term2 >= 0.5 {
            "terminate"
        } else {
            "continue"
        };
        assert_eq!(sel2, "terminate");
    }

    #[test]
    fn test_verification_polarity_mapping() {
        // Variant A: raw_p is clean
        let raw_clean = 0.92;
        let sel_a = if raw_clean >= 0.5 { "clean" } else { "defect" };
        assert_eq!(sel_a, "clean");

        // Variant B: raw_p is defect
        let raw_defect = 0.92;
        let sel_b = if raw_defect >= 0.5 { "defect" } else { "clean" };
        assert_eq!(sel_b, "defect");
    }
}
