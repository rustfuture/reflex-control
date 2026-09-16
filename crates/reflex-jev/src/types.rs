use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JevDecisionRequest {
    pub prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub candidates: Vec<String>,
    #[serde(default = "default_temperature")]
    pub temperature: f64,
}

fn default_temperature() -> f64 {
    0.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JevCandidateScore {
    pub candidate: String,
    pub logprob: Option<f64>,
    pub probability: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JevDecisionResponse {
    pub id: String,
    pub selected: String,
    pub probabilities: Vec<JevCandidateScore>,
    pub confidence: f64,
    #[serde(default)]
    pub latency_ms: Option<u64>,
    #[serde(default)]
    pub cost_usd: Option<f64>,
}
