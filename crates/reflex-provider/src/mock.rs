use crate::error::ProviderError;
use crate::provider::DecisionProvider;
use async_trait::async_trait;
use reflex_core::{Decision, DecisionRequest, DecisionResponse};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

#[derive(Clone)]
pub struct MockProvider {
    name: String,
    fixed_decision: Option<Decision>,
    fixed_probability: Option<f64>,
    simulated_latency_ms: u64,
    simulated_cost: f64,
    fail_next_calls: Arc<AtomicUsize>,
}

impl Default for MockProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl MockProvider {
    pub fn new() -> Self {
        Self {
            name: "mock".to_string(),
            fixed_decision: None,
            fixed_probability: None,
            simulated_latency_ms: 5,
            simulated_cost: 0.0001,
            fail_next_calls: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    pub fn with_fixed_decision(mut self, decision: Decision) -> Self {
        self.fixed_decision = Some(decision);
        self
    }

    pub fn with_fixed_probability(mut self, prob: f64) -> Self {
        self.fixed_probability = Some(prob.clamp(0.0, 1.0));
        self
    }

    pub fn with_latency(mut self, latency_ms: u64) -> Self {
        self.simulated_latency_ms = latency_ms;
        self
    }

    pub fn with_cost(mut self, cost: f64) -> Self {
        self.simulated_cost = cost;
        self
    }

    pub fn fail_next(self, count: usize) -> Self {
        self.fail_next_calls.store(count, Ordering::SeqCst);
        self
    }

    fn check_failure(&self) -> Result<(), ProviderError> {
        let remaining = self.fail_next_calls.load(Ordering::SeqCst);
        if remaining > 0 {
            self.fail_next_calls.fetch_sub(1, Ordering::SeqCst);
            return Err(ProviderError::Unavailable(
                "Simulated mock provider failure".to_string(),
            ));
        }
        Ok(())
    }

    fn compute_heuristic_prob(&self, context: &str) -> f64 {
        if let Some(p) = self.fixed_probability {
            return p;
        }
        // Deterministic pseudo-probability from hash of context
        let hash = context.bytes().fold(5381u64, |acc, b| {
            acc.wrapping_mul(33).wrapping_add(b as u64)
        });
        let norm = (hash % 1000) as f64 / 1000.0;
        // Bias slightly towards high confidence for typical tasks
        (norm * 0.4 + 0.6).clamp(0.0, 1.0)
    }
}

#[async_trait]
impl DecisionProvider for MockProvider {
    fn name(&self) -> &str {
        &self.name
    }

    async fn choice(&self, request: &DecisionRequest) -> Result<DecisionResponse, ProviderError> {
        self.check_failure()?;
        if self.simulated_latency_ms > 0 {
            tokio::time::sleep(tokio::time::Duration::from_millis(
                self.simulated_latency_ms,
            ))
            .await;
        }

        if let Some(ref d) = self.fixed_decision {
            return Ok(DecisionResponse::new(
                request.id.clone(),
                d.clone(),
                &self.name,
                self.simulated_latency_ms,
                self.simulated_cost,
            ));
        }

        let options = if request.options.is_empty() {
            vec!["accept".to_string(), "reject".to_string()]
        } else {
            request.options.clone()
        };

        let selected = options[0].clone();
        let p_top = self.compute_heuristic_prob(&request.observation.context);
        let rem = if options.len() > 1 {
            (1.0 - p_top) / (options.len() - 1) as f64
        } else {
            0.0
        };

        let mut probs = Vec::new();
        probs.push((selected.clone(), p_top));
        for opt in options.iter().skip(1) {
            probs.push((opt.clone(), rem));
        }

        let decision = Decision::new(selected, probs, p_top);

        Ok(DecisionResponse::new(
            request.id.clone(),
            decision,
            &self.name,
            self.simulated_latency_ms,
            self.simulated_cost,
        ))
    }

    async fn score(&self, request: &DecisionRequest) -> Result<DecisionResponse, ProviderError> {
        self.check_failure()?;
        if self.simulated_latency_ms > 0 {
            tokio::time::sleep(tokio::time::Duration::from_millis(
                self.simulated_latency_ms,
            ))
            .await;
        }

        let prob = self.compute_heuristic_prob(&request.observation.context);
        let decision = Decision::new(
            format!("{prob:.4}"),
            vec![("score".to_string(), prob)],
            prob,
        );

        Ok(DecisionResponse::new(
            request.id.clone(),
            decision,
            &self.name,
            self.simulated_latency_ms,
            self.simulated_cost,
        ))
    }

    async fn probability(
        &self,
        request: &DecisionRequest,
    ) -> Result<DecisionResponse, ProviderError> {
        self.check_failure()?;
        if self.simulated_latency_ms > 0 {
            tokio::time::sleep(tokio::time::Duration::from_millis(
                self.simulated_latency_ms,
            ))
            .await;
        }

        let p_true = self.compute_heuristic_prob(&request.observation.context);
        let selected = p_true >= 0.5;
        let decision = Decision::binary(selected, p_true);

        Ok(DecisionResponse::new(
            request.id.clone(),
            decision,
            &self.name,
            self.simulated_latency_ms,
            self.simulated_cost,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reflex_core::Observation;

    #[tokio::test]
    async fn test_mock_provider_success() {
        let provider = MockProvider::new().with_fixed_probability(0.92);
        let req = DecisionRequest::probability(Observation::new("Verify user identity"));
        let res = provider.probability(&req).await.unwrap();

        assert_eq!(res.decision.selected, "true");
        assert_eq!(res.decision.confidence, 0.92);
        assert_eq!(res.provider, "mock");
    }

    #[tokio::test]
    async fn test_mock_provider_failure() {
        let provider = MockProvider::new().fail_next(1);
        let req = DecisionRequest::probability(Observation::new("Test"));
        assert!(provider.probability(&req).await.is_err());
        // Subsequent call succeeds
        assert!(provider.probability(&req).await.is_ok());
    }
}
