use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReflexAction {
    Accept,
    Retry,
    Escalate,
    Verify,
    Reject,
    Custom(String),
}

impl ReflexAction {
    pub fn is_accept(&self) -> bool {
        matches!(self, ReflexAction::Accept)
    }

    pub fn is_verify(&self) -> bool {
        matches!(self, ReflexAction::Verify)
    }

    pub fn is_escalate(&self) -> bool {
        matches!(self, ReflexAction::Escalate)
    }
}

impl fmt::Display for ReflexAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReflexAction::Accept => write!(f, "accept"),
            ReflexAction::Retry => write!(f, "retry"),
            ReflexAction::Escalate => write!(f, "escalate"),
            ReflexAction::Verify => write!(f, "verify"),
            ReflexAction::Reject => write!(f, "reject"),
            ReflexAction::Custom(s) => write!(f, "custom({s})"),
        }
    }
}
