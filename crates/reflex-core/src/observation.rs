use crate::risk::RiskLevel;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct Observation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,

    pub context: String,

    #[serde(default)]
    pub risk_level: RiskLevel,

    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, serde_json::Value>,
}

impl Observation {
    pub fn new<C: Into<String>>(context: C) -> Self {
        Self {
            task_id: None,
            context: context.into(),
            risk_level: RiskLevel::Low,
            metadata: HashMap::new(),
        }
    }

    pub fn with_task_id<S: Into<String>>(mut self, task_id: S) -> Self {
        self.task_id = Some(task_id.into());
        self
    }

    pub fn with_risk(mut self, risk_level: RiskLevel) -> Self {
        self.risk_level = risk_level;
        self
    }

    pub fn with_meta<K: Into<String>, V: Into<serde_json::Value>>(
        mut self,
        key: K,
        value: V,
    ) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }
}
