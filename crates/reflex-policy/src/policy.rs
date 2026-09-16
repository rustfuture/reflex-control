use reflex_core::{DecisionResponse, Observation, ReflexAction};

pub trait Policy: Send + Sync {
    /// Maps a probabilistic decision and its context to a concrete deterministic action
    fn decide(&self, response: &DecisionResponse, observation: &Observation) -> ReflexAction;

    /// Policy name / identifier
    fn name(&self) -> &str;
}
