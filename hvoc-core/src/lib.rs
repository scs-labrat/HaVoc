pub mod canon;
pub mod error;
pub mod object;

pub use error::CoreError;
pub use object::*;

/// Opaque author identifier — the string encoding of a Veilid public key.
pub type AuthorId = String;

/// Content-addressed object identifier — hex-encoded BLAKE3 hash.
pub type ObjectId = String;
