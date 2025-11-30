//! Error types for wRPC transport

use std::fmt;

/// Result type for wRPC operations
pub type Result<T> = std::result::Result<T, Error>;

/// Error type for wRPC transport operations
#[derive(Debug)]
pub enum Error {
    /// Serialization/deserialization error
    Serialization(String),
    /// Transport error (HTTP/fetch)
    Transport(String),
    /// Protocol error
    Protocol(String),
    /// Function not found
    NotFound { instance: String, function: String },
    /// Worker error
    Worker(worker::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Serialization(msg) => write!(f, "serialization error: {}", msg),
            Error::Transport(msg) => write!(f, "transport error: {}", msg),
            Error::Protocol(msg) => write!(f, "protocol error: {}", msg),
            Error::NotFound { instance, function } => {
                write!(f, "function not found: {}/{}", instance, function)
            }
            Error::Worker(e) => write!(f, "worker error: {}", e),
        }
    }
}

impl std::error::Error for Error {}

impl From<worker::Error> for Error {
    fn from(e: worker::Error) -> Self {
        Error::Worker(e)
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Serialization(e.to_string())
    }
}

impl From<Error> for worker::Error {
    fn from(e: Error) -> Self {
        worker::Error::RustError(e.to_string())
    }
}
