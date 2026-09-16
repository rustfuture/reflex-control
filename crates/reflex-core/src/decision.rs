use crate::id::DecisionId;
use crate::observation::Observation;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionType {
    Choice,
    Score,
    Probability,
    Custom(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DecisionRequest {
    pub id: DecisionId,
    pub decision_type: DecisionType,
    pub observation: Observation,
    pub options: Vec<String>,
}

impl DecisionRequest {
    pub fn new(
        decision_type: DecisionType,
        observation: Observation,
        options: Vec<String>,
    ) -> Self {
        Self {
            id: DecisionId::generate(),
            decision_type,
            observation,
            options,
        }
    }

    pub fn choice<S: Into<String>>(observation: Observation, options: Vec<S>) -> Self {
        Self::new(
            DecisionType::Choice,
            observation,
            options.into_iter().map(|s| s.into()).collect(),
        )
    }

    pub fn probability(observation: Observation) -> Self {
        Self::new(
            DecisionType::Probability,
            observation,
            vec!["true".to_string(), "false".to_string()],
        )
    }

    pub fn score(observation: Observation) -> Self {
        Self::new(DecisionType::Score, observation, Vec::new())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Decision {
    pub selected: String,
    pub probabilities: Vec<(String, f64)>,
    pub confidence: f64,
}

impl Decision {
    pub fn new<S: Into<String>>(
        selected: S,
        probabilities: Vec<(String, f64)>,
        confidence: f64,
    ) -> Self {
        Self {
            selected: selected.into(),
            probabilities,
            confidence: confidence.clamp(0.0, 1.0),
        }
    }

    pub fn binary(selected: bool, probability_true: f64) -> Self {
        let p_true = probability_true.clamp(0.0, 1.0);
        let p_false = (1.0 - p_true).clamp(0.0, 1.0);
        let confidence = if p_true >= 0.5 { p_true } else { p_false };
        Self {
            selected: if selected {
                "true".to_string()
            } else {
                "false".to_string()
            },
            probabilities: vec![("true".to_string(), p_true), ("false".to_string(), p_false)],
            confidence,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DecisionResponse {
    pub request_id: DecisionId,
    pub decision: Decision,
    pub provider: String,
    pub latency_ms: u64,
    pub cost_estimate: f64,
}

impl DecisionResponse {
    pub fn new(
        request_id: DecisionId,
        decision: Decision,
        provider: impl Into<String>,
        latency_ms: u64,
        cost_estimate: f64,
    ) -> Self {
        Self {
            request_id,
            decision,
            provider: provider.into(),
            latency_ms,
            cost_estimate,
        }
    }
}
