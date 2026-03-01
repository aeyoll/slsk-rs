//! Distributed network messages — sent over a "D" TCP connection.
//! Uses uint8 message codes with the standard length-prefixed framing.

use crate::{codec::*, error::Result};
use std::io::Cursor;

pub mod code {
    pub const PING: u8 = 0; // DEPRECATED
    pub const SEARCH: u8 = 3;
    pub const BRANCH_LEVEL: u8 = 4;
    pub const BRANCH_ROOT: u8 = 5;
    pub const CHILD_DEPTH: u8 = 7; // OBSOLETE
    pub const EMBEDDED_MESSAGE: u8 = 93; // DEPRECATED
}

// ── DistribSearch ─────────────────────────────────────────────────────────────

/// Search request received from a parent node in the distributed network.
///
/// `identifier` must equal the code point of ASCII `'1'` (49).  
/// Clients should reject messages with any other value.
///
/// When acting as a branch root, the raw message bytes (including the
/// identifier) must be forwarded verbatim to all child peers.
#[derive(Debug, Clone)]
pub struct DistribSearch {
    /// Always 49 (ASCII '1')
    pub identifier: u32,
    pub username: String,
    pub token: u32,
    pub query: String,
}

impl DistribSearch {
    pub const REQUIRED_IDENTIFIER: u32 = 49; // '1'

    pub fn encode_raw(&self) -> Vec<u8> {
        let mut body = Vec::new();
        write_u32_le(&mut body, self.identifier).unwrap();
        write_string(&mut body, &self.username).unwrap();
        write_u32_le(&mut body, self.token).unwrap();
        write_string(&mut body, &self.query).unwrap();
        // Distributed messages use uint8 code framing
        frame_message_u8(code::SEARCH, &body)
    }

    pub fn decode(body: &[u8]) -> Result<Self> {
        let mut cur = Cursor::new(body);
        let identifier = read_u32_le(&mut cur)?;
        let username = read_string(&mut cur)?;
        let token = read_u32_le(&mut cur)?;
        let query = read_string(&mut cur)?;
        Ok(Self {
            identifier,
            username,
            token,
            query,
        })
    }

    pub fn is_valid_identifier(&self) -> bool {
        self.identifier == Self::REQUIRED_IDENTIFIER
    }
}

// ── DistribBranchLevel ────────────────────────────────────────────────────────

/// Our position in the distributed branch hierarchy (0 = branch root).
#[derive(Debug, Clone)]
pub struct DistribBranchLevel {
    pub level: i32,
}

impl DistribBranchLevel {
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        write_i32_le(&mut body, self.level).unwrap();
        frame_message_u8(code::BRANCH_LEVEL, &body)
    }

    pub fn decode(body: &[u8]) -> Result<Self> {
        Ok(Self {
            level: read_i32_le(&mut Cursor::new(body))?,
        })
    }
}

// ── DistribBranchRoot ─────────────────────────────────────────────────────────

/// Username of the root node of our branch.
///
/// Since early 2026, SoulseekQt sends this even when it is itself the branch
/// root — implementations must always send it regardless of branch status.
#[derive(Debug, Clone)]
pub struct DistribBranchRoot {
    pub root: String,
}

impl DistribBranchRoot {
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        write_string(&mut body, &self.root).unwrap();
        frame_message_u8(code::BRANCH_ROOT, &body)
    }

    pub fn decode(body: &[u8]) -> Result<Self> {
        Ok(Self {
            root: read_string(&mut Cursor::new(body))?,
        })
    }
}

// ── Top-level distributed message enum ───────────────────────────────────────

#[derive(Debug, Clone)]
pub enum DistribMessage {
    Search(DistribSearch),
    BranchLevel(DistribBranchLevel),
    BranchRoot(DistribBranchRoot),
    /// Catch-all for unknown / deprecated codes
    Unknown {
        code: u8,
        body: Vec<u8>,
    },
}

impl DistribMessage {
    pub fn decode(code: u8, body: &[u8]) -> Result<Self> {
        match code {
            code::SEARCH => Ok(Self::Search(DistribSearch::decode(body)?)),
            code::BRANCH_LEVEL => Ok(Self::BranchLevel(DistribBranchLevel::decode(body)?)),
            code::BRANCH_ROOT => Ok(Self::BranchRoot(DistribBranchRoot::decode(body)?)),
            _ => Ok(Self::Unknown {
                code,
                body: body.to_vec(),
            }),
        }
    }
}
