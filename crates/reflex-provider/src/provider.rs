use crate::error::ProviderError;
use async_trait::async_trait;
use reflex_core::{DecisionRequest, DecisionResponse};

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
