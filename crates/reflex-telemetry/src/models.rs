use chrono::{DateTime, Utc};
use reflex_core::{DecisionId, Outcome, OutcomeSource, ReflexAction, RiskLevel};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionRecord {
    pub id: DecisionId,
    pub timestamp: DateTime<Utc>,
    pub provider: String,
    pub decision_type: String,
    pub context: String,
    pub task_id: Option<String>,
    pub selected: String,
    pub confidence: f64,
    pub probabilities_json: String,
    pub action: ReflexAction,
    pub risk_level: RiskLevel,
    pub latency_ms: u64,
    pub cost_estimate: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutcomeRecord {
    pub decision_id: DecisionId,
    pub result: Outcome,
    pub source: OutcomeSource,
    pub verified_at: DateTime<Utc>,
    pub details: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShadowRecord {
    pub id: String,
    pub task_id: Option<String>,
    pub timestamp: DateTime<Utc>,
    pub predicted_action: ReflexAction,
    pub confidence: f64,
    pub actual_action: String,
    pub final_outcome: Option<Outcome>,
    pub latency_ms: u64,
    pub cost_estimate: f64,
}
