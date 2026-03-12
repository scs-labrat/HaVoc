use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("canonical serialization failed: {0}")]
    Canon(String),

    #[error("object ID mismatch: expected {expected}, got {actual}")]
    IdMismatch { expected: String, actual: String },

    #[error("invalid signature on object {object_id}")]
    BadSignature { object_id: String },

    #[error("validation error: {0}")]
    Validation(String),
}
