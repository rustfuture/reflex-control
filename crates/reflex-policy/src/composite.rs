use crate::policy::Policy;
use reflex_core::{DecisionResponse, Observation, ReflexAction};

pub struct CompositePolicy {
    policies: Vec<Box<dyn Policy>>,
}

impl CompositePolicy {
    pub fn new() -> Self {
        Self {
            policies: Vec::new(),
        }
    }

    pub fn with_policy<P: Policy + 'static>(mut self, policy: P) -> Self {
        self.policies.push(Box::new(policy));
        self
    }

    pub fn push<P: Policy + 'static>(&mut self, policy: P) {
        self.policies.push(Box::new(policy));
    }
}

impl Default for CompositePolicy {
    fn default() -> Self {
        Self::new()
    }
}

impl Policy for CompositePolicy {
    fn name(&self) -> &str {
        "composite"
    }

    fn decide(&self, response: &DecisionResponse, observation: &Observation) -> ReflexAction {
        for policy in &self.policies {
            let action = policy.decide(response, observation);
            // Non-accept actions take priority
            if !action.is_accept() {
                return action;
            }
        }
        ReflexAction::Accept
    }
}
