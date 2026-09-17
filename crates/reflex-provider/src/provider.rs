use crate::error::ProviderError;
use async_trait::async_trait;
use reflex_core::{DecisionRequest, DecisionResponse, SemanticEvidence};
use serde::{Deserialize, Serialize};

#[async_trait]
pub trait DecisionProvider: Send + Sync {
    /// Identifier for this provider (e.g. "mock", "jev", "openai")
    fn name(&self) -> &str;

    /// Select from categorical options
    async fn choice(&self, request: &DecisionRequest) -> Result<DecisionResponse, ProviderError>;

    /// Return a continuous score [0.0, 1.0]
    async fn score(&self, request: &DecisionRequest) -> Result<DecisionResponse, ProviderError>;

    /// Return binary or calibrated probability [0.0, 1.0]
    async fn probability(
        &self,
        request: &DecisionRequest,
    ) -> Result<DecisionResponse, ProviderError>;

    /// General dispatch based on request.decision_type
    async fn evaluate(&self, request: &DecisionRequest) -> Result<DecisionResponse, ProviderError> {
        match request.decision_type {
            reflex_core::DecisionType::Choice => self.choice(request).await,
            reflex_core::DecisionType::Score => self.score(request).await,
            reflex_core::DecisionType::Probability => self.probability(request).await,
            reflex_core::DecisionType::Custom(_) => self.choice(request).await,
        }
    }
}

/// Evaluation response from an atomic evidence provider
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvidenceEvaluationResponse {
    pub signals: Vec<SemanticEvidence>,
    pub latency_ms: u64,
    pub cost_estimate: f64,
    pub input_tokens: usize,
}

/// Provider of narrow, atomic semantic evidence signals
#[async_trait]
pub trait AtomicEvidenceProvider: Send + Sync {
    /// Provider identifier (e.g. "jev-atomic", "mock-atomic")
    fn name(&self) -> &str;

    /// Evaluate a set of narrow semantic questions over the observation context in parallel
    async fn evaluate_evidence(
        &self,
        context: &str,
        requested_signals: &[&str],
    ) -> Result<EvidenceEvaluationResponse, ProviderError>;
}
