use thiserror::Error;

#[derive(Error, Debug)]
pub enum TelemetryError {
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Record not found: {0}")]
    NotFound(String),

    #[error("Lock error: {0}")]
    LockError(String),
}
