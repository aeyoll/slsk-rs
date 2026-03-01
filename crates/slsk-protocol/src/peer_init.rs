//! Peer init messages — sent on new TCP connections before P/F/D traffic begins.
//! These use uint8 message codes and the standard length-prefixed frame.

use crate::{codec::*, error::Result};
use std::io::Cursor;

pub mod code {
    pub const PIERCE_FIREWALL: u8 = 0;
    pub const PEER_INIT: u8 = 1;
}

// ── PierceFirewall ────────────────────────────────────────────────────────────

/// Sent in response to an indirect (server-relayed) connection request.
/// The `token` is taken from the `ConnectToPeer` server message.
#[derive(Debug, Clone)]
pub struct PierceFirewall {
    pub token: u32,
}

impl PierceFirewall {
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        write_u32_le(&mut body, self.token).unwrap();
        frame_message_u8(code::PIERCE_FIREWALL, &body)
    }

    pub fn decode(body: &[u8]) -> Result<Self> {
        let mut cur = Cursor::new(body);
        Ok(Self {
            token: read_u32_le(&mut cur)?,
        })
    }
}

// ── PeerInit ──────────────────────────────────────────────────────────────────

/// Sent to initiate a direct P, F, or D connection.
/// `token` is always 0 today (historically used for spoofing prevention).
#[derive(Debug, Clone)]
pub struct PeerInit {
    pub username: String,
    /// "P", "F", or "D" — see [`crate::types::ConnectionType`]
    pub conn_type: String,
    pub token: u32,
}

impl PeerInit {
    pub fn new(username: impl Into<String>, conn_type: impl Into<String>) -> Self {
        Self {
            username: username.into(),
            conn_type: conn_type.into(),
            token: 0,
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        write_string(&mut body, &self.username).unwrap();
        write_string(&mut body, &self.conn_type).unwrap();
        write_u32_le(&mut body, self.token).unwrap();
        frame_message_u8(code::PEER_INIT, &body)
    }

    pub fn decode(body: &[u8]) -> Result<Self> {
        let mut cur = Cursor::new(body);
        Ok(Self {
            username: read_string(&mut cur)?,
            conn_type: read_string(&mut cur)?,
            token: read_u32_le(&mut cur)?,
        })
    }
}

// ── Top-level peer-init enum ──────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum PeerInitMessage {
    PierceFirewall(PierceFirewall),
    PeerInit(PeerInit),
}

impl PeerInitMessage {
    pub fn decode(code: u8, body: &[u8]) -> crate::error::Result<Self> {
        match code {
            code::PIERCE_FIREWALL => Ok(Self::PierceFirewall(PierceFirewall::decode(body)?)),
            code::PEER_INIT => Ok(Self::PeerInit(PeerInit::decode(body)?)),
            _ => Err(crate::error::Error::Protocol(format!(
                "unknown peer-init code {}",
                code
            ))),
        }
    }
}
