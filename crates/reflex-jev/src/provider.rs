use crate::client::JevClient;
use crate::config::JevConfig;
use crate::types::JevDecisionRequest;
use async_trait::async_trait;
use reflex_core::{Decision, DecisionRequest, DecisionResponse};
use reflex_provider::{DecisionProvider, ProviderError};
use std::time::Instant;

pub struct JevProvider {
    client: JevClient,
}

impl JevProvider {
    pub fn new(config: JevConfig) -> Self {
        Self {
            client: JevClient::new(config),
        }
    }
}

#[async_trait]
impl DecisionProvider for JevProvider {
    fn name(&self) -> &str {
        "jev"
    }

    async fn choice(&self, request: &DecisionRequest) -> Result<DecisionResponse, ProviderError> {
        let req = JevDecisionRequest {
            prompt: request.observation.context.clone(),
            task_id: request.observation.task_id.clone(),
            candidates: request.options.clone(),
            temperature: 0.0,
        };

        let start = Instant::now();
        let resp = self.client.post_decision("decide/choice", &req).await?;
        let latency_ms = resp
            .latency_ms
            .unwrap_or(start.elapsed().as_millis() as u64);
        let cost_estimate = resp.cost_usd.unwrap_or(0.0002);

        let probabilities = resp
            .probabilities
            .into_iter()
            .map(|c| (c.candidate, c.probability))
            .collect();

        let decision = Decision::new(resp.selected, probabilities, resp.confidence);

        Ok(DecisionResponse::new(
            request.id.clone(),
            decision,
            "jev",
            latency_ms,
            cost_estimate,
        ))
    }

    async fn score(&self, request: &DecisionRequest) -> Result<DecisionResponse, ProviderError> {
        let req = JevDecisionRequest {
            prompt: request.observation.context.clone(),
            task_id: request.observation.task_id.clone(),
            candidates: vec!["score".to_string()],
            temperature: 0.0,
        };

        let start = Instant::now();
        let resp = self.client.post_decision("decide/score", &req).await?;
        let latency_ms = resp
            .latency_ms
            .unwrap_or(start.elapsed().as_millis() as u64);
        let cost_estimate = resp.cost_usd.unwrap_or(0.0001);

        let decision = Decision::new(
            resp.selected,
            resp.probabilities
                .into_iter()
                .map(|p| (p.candidate, p.probability))
                .collect(),
            resp.confidence,
        );

        Ok(DecisionResponse::new(
            request.id.clone(),
            decision,
            "jev",
            latency_ms,
            cost_estimate,
        ))
    }

    async fn probability(
        &self,
        request: &DecisionRequest,
    ) -> Result<DecisionResponse, ProviderError> {
        let req = JevDecisionRequest {
            prompt: request.observation.context.clone(),
            task_id: request.observation.task_id.clone(),
            candidates: vec!["true".to_string(), "false".to_string()],
            temperature: 0.0,
        };

        let start = Instant::now();
        let resp = self
            .client
            .post_decision("decide/probability", &req)
            .await?;
        let latency_ms = resp
            .latency_ms
            .unwrap_or(start.elapsed().as_millis() as u64);
        let cost_estimate = resp.cost_usd.unwrap_or(0.0001);

        let probabilities: Vec<(String, f64)> = resp
            .probabilities
            .into_iter()
            .map(|p| (p.candidate, p.probability))
            .collect();

        let decision = Decision::new(resp.selected, probabilities, resp.confidence);

        Ok(DecisionResponse::new(
            request.id.clone(),
            decision,
            "jev",
            latency_ms,
            cost_estimate,
        ))
    }
}
