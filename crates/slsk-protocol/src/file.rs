//! File connection messages — sent over an "F" TCP connection.
//!
//! There are no message codes here; the two messages are identified solely
//! by their position in the connection handshake.

use crate::{codec::*, error::Result};
use std::io::{Read, Write};

// ── FileTransferInit ──────────────────────────────────────────────────────────

/// Sent by the uploader to the downloader as the first message over an F
/// connection, to indicate which queued transfer token this connection
/// corresponds to.
#[derive(Debug, Clone)]
pub struct FileTransferInit {
    /// Matches the token in the corresponding `TransferRequest` peer message.
    pub token: u32,
}

impl FileTransferInit {
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(4);
        write_u32_le(&mut buf, self.token).unwrap();
        buf
    }

    pub fn decode(r: &mut impl Read) -> Result<Self> {
        Ok(Self {
            token: read_u32_le(r)?,
        })
    }
}

// ── FileOffset ────────────────────────────────────────────────────────────────

/// Sent by the downloader right after receiving `FileTransferInit`, specifying
/// how many bytes of the file have already been downloaded (0 for fresh
/// downloads). The uploader then seeks to this offset before sending data.
///
/// Note: Soulseek NS has a bug where it sends an offset of −1 when more than
/// 2 GB have been downloaded. Implementations should handle this gracefully.
#[derive(Debug, Clone)]
pub struct FileOffset {
    pub offset: u64,
}

impl FileOffset {
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(8);
        write_u64_le(&mut buf, self.offset).unwrap();
        buf
    }

    pub fn decode(r: &mut impl Read) -> Result<Self> {
        Ok(Self {
            offset: read_u64_le(r)?,
        })
    }
}

// ── Async helpers (Tokio) ─────────────────────────────────────────────────────

/// Perform the full F-connection handshake from the **uploader's** perspective:
///
/// 1. Write `FileTransferInit` with `token`.
/// 2. Read `FileOffset` from the downloader.
///
/// Returns the byte offset to seek to before streaming file data.
pub async fn uploader_handshake(
    stream: &mut (impl tokio::io::AsyncWriteExt + tokio::io::AsyncReadExt + Unpin),
    token: u32,
) -> std::io::Result<u64> {
    use tokio::io::AsyncWriteExt;
    let init = FileTransferInit { token };
    stream.write_all(&init.encode()).await?;

    use tokio::io::AsyncReadExt;
    let mut buf = [0u8; 8];
    stream.read_exact(&mut buf).await?;
    Ok(u64::from_le_bytes(buf))
}

/// Perform the full F-connection handshake from the **downloader's** perspective:
///
/// 1. Read `FileTransferInit` (token is used to match the pending transfer).
/// 2. Write `FileOffset` — how many bytes we already have (0 for new download).
///
/// Returns the token from the init message.
pub async fn downloader_handshake(
    stream: &mut (impl tokio::io::AsyncWriteExt + tokio::io::AsyncReadExt + Unpin),
    bytes_already_downloaded: u64,
) -> std::io::Result<u32> {
    use tokio::io::AsyncReadExt;
    let mut buf = [0u8; 4];
    stream.read_exact(&mut buf).await?;
    let token = u32::from_le_bytes(buf);

    use tokio::io::AsyncWriteExt;
    let offset = FileOffset {
        offset: bytes_already_downloaded,
    };
    stream.write_all(&offset.encode()).await?;

    Ok(token)
}
