//! # soulseek-protocol
//!
//! A pure-Rust implementation of the [Soulseek](https://www.slsknet.org/) P2P
//! file sharing protocol, based on the Nicotine+ protocol documentation.
//!
//! ## Crate layout
//!
//! | Module | Description |
//! |---|---|
//! | [`codec`] | Low-level byte-level encode/decode primitives |
//! | [`types`] | Shared types (enums, file attributes, user stats) |
//! | [`error`] | Error and Result types |
//! | [`server`] | Server messages (code 1–1003) |
//! | [`peer_init`] | Peer-init messages (PierceFirewall, PeerInit) |
//! | [`peer`] | Peer messages over P connections |
//! | [`shared_files`] | zlib-compressed shared file list helpers |
//! | [`file`] | File connection handshake (F connections) |
//! | [`distributed`] | Distributed search network messages (D connections) |
//! | [`connection`] | Async Tokio connection wrappers |
//!
//! ## Quick start
//!
//! ```rust,no_run
//! use soulseek_protocol::{
//!     connection::ServerConnection,
//!     server::{LoginRequest, ServerMessage},
//! };
//! use std::net::SocketAddr;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let addr: SocketAddr = "server.slsknet.org:2416".parse()?;
//!     let mut conn = ServerConnection::connect(addr).await?;
//!
//!     // Send login
//!     let req = LoginRequest::new("myuser", "mypassword");
//!     conn.send_raw(&req.encode()).await?;
//!
//!     // Read response
//!     match conn.recv().await? {
//!         ServerMessage::Login(resp) => println!("Login result: {:?}", resp),
//!         other => println!("Unexpected: {:?}", other),
//!     }
//!     Ok(())
//! }
//! ```
//!
//! ## Encoding / decoding without a live connection
//!
//! Every message type exposes `encode() -> Vec<u8>` and/or
//! `decode(body: &[u8]) -> Result<Self>`, so you can use the codec
//! without an async runtime or even a real network.
//!
//! ```rust
//! use soulseek_protocol::server::{LoginRequest, login_hash};
//!
//! let req = LoginRequest::new("alice", "hunter2");
//! let bytes = req.encode();
//! // bytes is a fully framed server message ready to send over TCP
//! assert!(bytes.len() > 8);
//!
//! // The MD5 hash helper is also public:
//! let hash = login_hash("alice", "hunter2");
//! assert_eq!(hash.len(), 32);
//! ```

pub mod codec;
pub mod connection;
pub mod distributed;
pub mod error;
pub mod file;
pub mod peer;
pub mod peer_init;
pub mod server;
pub mod shared_files;
pub mod types;
