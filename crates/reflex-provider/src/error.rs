use thiserror::Error;

#[derive(Error, Debug)]
pub enum ProviderError {
    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),

    #[error("Rate limit exceeded: {0}")]
    RateLimited(String),

    #[error("Request timed out: {0}")]
    Timeout(String),

    #[error("Network failure: {0}")]
    Network(String),

    #[error("Malformed provider response: {0}")]
    MalformedResponse(String),

    #[error("Provider unavailable: {0}")]
    Unavailable(String),

    #[error("Invalid request parameters: {0}")]
    InvalidRequest(String),

    #[error("Internal provider error: {0}")]
    Internal(String),
}
