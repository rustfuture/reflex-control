use thiserror::Error;

#[derive(Error, Debug, Clone, PartialEq)]
pub enum CoreError {
    #[error("Invalid probability value '{value}', must be between 0.0 and 1.0")]
    InvalidProbability { value: f64 },

    #[error("Invalid confidence value '{value}', must be between 0.0 and 1.0")]
    InvalidConfidence { value: f64 },

    #[error("Invalid identifier: {0}")]
    InvalidId(String),

    #[error("Validation error: {0}")]
    ValidationError(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),
}
