use crate::id::DecisionId;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Success,
    Failure,
    Partial,
    Unknown,
}

impl Outcome {
    pub fn is_success(&self) -> bool {
        matches!(self, Outcome::Success)
    }

    pub fn is_failure(&self) -> bool {
        matches!(self, Outcome::Failure)
    }

    pub fn is_resolved(&self) -> bool {
        matches!(self, Outcome::Success | Outcome::Failure)
    }
}

impl fmt::Display for Outcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Outcome::Success => write!(f, "success"),
            Outcome::Failure => write!(f, "failure"),
            Outcome::Partial => write!(f, "partial"),
            Outcome::Unknown => write!(f, "unknown"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeSource {
    Ci,
    Tests,
    Verifier,
    Human,
    Runtime,
    Custom(String),
}

impl fmt::Display for OutcomeSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OutcomeSource::Ci => write!(f, "ci"),
            OutcomeSource::Tests => write!(f, "tests"),
            OutcomeSource::Verifier => write!(f, "verifier"),
            OutcomeSource::Human => write!(f, "human"),
            OutcomeSource::Runtime => write!(f, "runtime"),
            OutcomeSource::Custom(s) => write!(f, "custom({s})"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OutcomeRecord {
    pub decision_id: DecisionId,
    pub outcome: Outcome,
    pub source: OutcomeSource,
    pub verified_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
}

impl OutcomeRecord {
    pub fn new(decision_id: DecisionId, outcome: Outcome, source: OutcomeSource) -> Self {
        Self {
            decision_id,
            outcome,
            source,
            verified_at: Utc::now(),
            details: None,
        }
    }

    pub fn with_details<S: Into<String>>(mut self, details: S) -> Self {
        self.details = Some(details.into());
        self
    }
}
