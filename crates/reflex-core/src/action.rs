use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReflexAction {
    Accept,
    Retry,
    Escalate,
    Verify,
    Reject,
    Terminate,
    Continue,
    DeferToSmallReasoner,
    DeferToFrontier,
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

    pub fn is_retry(&self) -> bool {
        matches!(self, ReflexAction::Retry)
    }

    pub fn is_terminate(&self) -> bool {
        matches!(self, ReflexAction::Terminate)
    }

    pub fn is_continue(&self) -> bool {
        matches!(self, ReflexAction::Continue)
    }

    pub fn is_defer(&self) -> bool {
        matches!(
            self,
            ReflexAction::DeferToSmallReasoner | ReflexAction::DeferToFrontier
        )
    }

    pub fn is_autonomous_pass(&self) -> bool {
        matches!(self, ReflexAction::Accept | ReflexAction::Terminate)
    }
}

impl FromStr for ReflexAction {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let normalized = s.trim().to_lowercase();
        Ok(match normalized.as_str() {
            "accept" => ReflexAction::Accept,
            "retry" => ReflexAction::Retry,
            "escalate" => ReflexAction::Escalate,
            "verify" => ReflexAction::Verify,
            "reject" => ReflexAction::Reject,
            "terminate" => ReflexAction::Terminate,
            "continue" => ReflexAction::Continue,
            "defer_to_small_reasoner" | "defer_small" => ReflexAction::DeferToSmallReasoner,
            "defer_to_frontier" | "defer_frontier" => ReflexAction::DeferToFrontier,
            other => ReflexAction::Custom(other.to_string()),
        })
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
            ReflexAction::Terminate => write!(f, "terminate"),
            ReflexAction::Continue => write!(f, "continue"),
            ReflexAction::DeferToSmallReasoner => write!(f, "defer_to_small_reasoner"),
            ReflexAction::DeferToFrontier => write!(f, "defer_to_frontier"),
            ReflexAction::Custom(s) => write!(f, "custom({s})"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reflex_action_display_and_parse() {
        let actions = [
            ReflexAction::Accept,
            ReflexAction::Retry,
            ReflexAction::Escalate,
            ReflexAction::Verify,
            ReflexAction::Terminate,
            ReflexAction::Continue,
            ReflexAction::DeferToSmallReasoner,
            ReflexAction::DeferToFrontier,
        ];

        for action in actions {
            let str_repr = action.to_string();
            let parsed: ReflexAction = str_repr.parse().unwrap();
            assert_eq!(action, parsed);
        }
    }

    #[test]
    fn test_action_classification_helpers() {
        assert!(ReflexAction::Accept.is_autonomous_pass());
        assert!(ReflexAction::Terminate.is_autonomous_pass());
        assert!(!ReflexAction::Verify.is_autonomous_pass());
        assert!(ReflexAction::DeferToSmallReasoner.is_defer());
        assert!(ReflexAction::DeferToFrontier.is_defer());
        assert!(!ReflexAction::Accept.is_defer());
    }
}
