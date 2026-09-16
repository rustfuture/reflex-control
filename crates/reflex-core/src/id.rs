use crate::error::CoreError;
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DecisionId(String);

impl DecisionId {
    pub fn generate() -> Self {
        Self(Uuid::new_v4().to_string())
    }

    pub fn new<S: Into<String>>(id: S) -> Result<Self, CoreError> {
        let val = id.into();
        if val.trim().is_empty() {
            return Err(CoreError::InvalidId("ID cannot be empty".to_string()));
        }
        Ok(Self(val))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for DecisionId {
    fn default() -> Self {
        Self::generate()
    }
}

impl fmt::Display for DecisionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<Uuid> for DecisionId {
    fn from(uuid: Uuid) -> Self {
        Self(uuid.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decision_id_valid() {
        let id = DecisionId::generate();
        assert!(!id.as_str().is_empty());

        let custom = DecisionId::new("custom-123").unwrap();
        assert_eq!(custom.as_str(), "custom-123");
    }

    #[test]
    fn test_decision_id_empty() {
        assert!(DecisionId::new("   ").is_err());
    }
}
