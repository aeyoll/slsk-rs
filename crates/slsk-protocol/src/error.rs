//! Error types for the soulseek-protocol crate.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("encoding error: {0}")]
    Encoding(String),

    #[error("protocol error: {0}")]
    Protocol(String),

    #[error("unknown message code: {0}")]
    UnknownCode(u32),

    #[error("unknown distributed code: {0}")]
    UnknownDistribCode(u8),

    #[error("zlib error: {0}")]
    Zlib(String),

    #[error("login rejected: {0}")]
    LoginRejected(String),
}

pub type Result<T> = std::result::Result<T, Error>;
