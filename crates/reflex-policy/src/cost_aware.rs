use crate::policy::Policy;
use reflex_core::{DecisionResponse, Observation, ReflexAction};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostAwarePolicy<P: Policy> {
    pub inner: P,
    pub max_acceptable_cost: f64,
    pub high_cost_action: ReflexAction,
}

impl<P: Policy> CostAwarePolicy<P> {
    pub fn new(inner: P, max_acceptable_cost: f64) -> Self {
        Self {
            inner,
            max_acceptable_cost,
            high_cost_action: ReflexAction::Escalate,
        }
    }
}

impl<P: Policy> Policy for CostAwarePolicy<P> {
    fn name(&self) -> &str {
        "cost_aware"
    }

    fn decide(&self, response: &DecisionResponse, observation: &Observation) -> ReflexAction {
        if response.cost_estimate > self.max_acceptable_cost {
            return self.high_cost_action.clone();
        }
        self.inner.decide(response, observation)
    }
}
