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
        let is_termination = request.options.iter().any(|o| o == "terminate");
        let decision = if is_termination {
            let sel = if p_true >= 0.5 {
                "terminate"
            } else {
                "continue"
            };
            let conf = if p_true >= 0.5 { p_true } else { 1.0 - p_true };
            Decision::new(
                sel,
                vec![
                    ("terminate".to_string(), p_true),
                    ("continue".to_string(), 1.0 - p_true),
                ],
                conf,
            )
        } else if request
            .options
            .iter()
            .any(|o| o == "clean" || o == "defect")
        {
            let sel = if p_true >= 0.5 { "clean" } else { "defect" };
            let conf = if p_true >= 0.5 { p_true } else { 1.0 - p_true };
            Decision::new(
                sel,
                vec![
                    ("clean".to_string(), p_true),
                    ("defect".to_string(), 1.0 - p_true),
                ],
                conf,
            )
        } else {
            let selected = p_true >= 0.5;
            Decision::binary(selected, p_true)
        };

        Ok(DecisionResponse::new(
            request.id.clone(),
            decision,
            &self.name,
            self.simulated_latency_ms,
            self.simulated_cost,
        ))
    }
}

/// Mock atomic evidence provider for local testing and simulations
#[derive(Clone)]
pub struct MockEvidenceProvider {
    name: String,
    fixed_signals: std::collections::HashMap<String, f64>,
    simulated_latency_ms: u64,
}

impl Default for MockEvidenceProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl MockEvidenceProvider {
    pub fn new() -> Self {
        Self {
            name: "mock-evidence".to_string(),
            fixed_signals: std::collections::HashMap::new(),
            simulated_latency_ms: 5,
        }
    }

    pub fn with_signal(mut self, name: impl Into<String>, prob: f64) -> Self {
        self.fixed_signals.insert(name.into(), prob.clamp(0.0, 1.0));
        self
    }
}

#[async_trait]
impl crate::provider::AtomicEvidenceProvider for MockEvidenceProvider {
    fn name(&self) -> &str {
        &self.name
    }

    async fn evaluate_evidence(
        &self,
        context: &str,
        requested_signals: &[&str],
    ) -> Result<crate::provider::EvidenceEvaluationResponse, ProviderError> {
        let ctx_lower = context.to_lowercase();
        let mut signals = Vec::new();

        for &sig in requested_signals {
            let prob = if let Some(&p) = self.fixed_signals.get(sig) {
                p
            } else {
                match sig {
                    reflex_core::SIGNAL_FAILURE_IS_TRANSIENT => {
                        if ctx_lower.contains("429")
                            || ctx_lower.contains("timeout")
                            || ctx_lower.contains("rate limit")
                        {
                            0.90
                        } else {
                            0.10
                        }
                    }
                    reflex_core::SIGNAL_REQUIREMENTS_ARE_AMBIGUOUS => {
                        if ctx_lower.contains("unclear") || ctx_lower.contains("ambiguous") {
                            0.80
                        } else {
                            0.15
                        }
                    }
                    reflex_core::SIGNAL_SECURITY_RISK => {
                        if ctx_lower.contains("injection")
                            || ctx_lower.contains("vulnerability")
                            || ctx_lower.contains("leak")
                        {
                            0.85
                        } else {
                            0.05
                        }
                    }
                    reflex_core::SIGNAL_REQUIRED_WORK_REMAINING => {
                        if ctx_lower.contains("incomplete")
                            || ctx_lower.contains("pending")
                            || ctx_lower.contains("failing")
                        {
                            0.80
                        } else {
                            0.10
                        }
                    }
                    reflex_core::SIGNAL_OBJECTIVE_SATISFIED => {
                        if ctx_lower.contains("passed") || ctx_lower.contains("success") {
                            0.90
                        } else {
                            0.20
                        }
                    }
                    reflex_core::SIGNAL_INDEPENDENT_VERIFICATION_NEEDED => {
                        if ctx_lower.contains("verify") || ctx_lower.contains("high risk") {
                            0.75
                        } else {
                            0.20
                        }
                    }
                    _ => 0.50,
                }
            };

            signals.push(reflex_core::SemanticEvidence::new(
                sig,
                prob,
                &self.name,
                self.simulated_latency_ms,
            ));
        }

        Ok(crate::provider::EvidenceEvaluationResponse {
            signals,
            latency_ms: self.simulated_latency_ms,
            cost_estimate: 0.0,
            input_tokens: 100,
        })
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
