//! Async TCP connection helpers built on Tokio.
//!
//! Provides typed wrappers around raw TCP streams for each Soulseek
//! connection type (Server, Peer, PeerInit, Distributed).

use std::net::SocketAddr;

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
};

use crate::{
    distributed::DistribMessage,
    error::{Error, Result},
    peer::PeerMessage,
    peer_init::PeerInitMessage,
    server::ServerMessage,
};

// ── ServerConnection ──────────────────────────────────────────────────────────

/// Authenticated, framed connection to the Soulseek server.
pub struct ServerConnection {
    stream: BufReader<TcpStream>,
}

impl ServerConnection {
    pub async fn connect(addr: SocketAddr) -> Result<Self> {
        let stream = TcpStream::connect(addr).await?;
        Ok(Self {
            stream: BufReader::new(stream),
        })
    }

    /// Send a raw pre-encoded frame (e.g. from `LoginRequest::encode()`).
    pub async fn send_raw(&mut self, frame: &[u8]) -> Result<()> {
        self.stream.get_mut().write_all(frame).await?;
        Ok(())
    }

    /// Read the next message from the server.
    pub async fn recv(&mut self) -> Result<ServerMessage> {
        // Read 4-byte total length
        let mut len_buf = [0u8; 4];
        self.stream.read_exact(&mut len_buf).await?;
        let total = u32::from_le_bytes(len_buf) as usize;
        if total < 4 {
            return Err(Error::Protocol(format!(
                "server frame too short: {}",
                total
            )));
        }
        let mut buf = vec![0u8; total];
        self.stream.read_exact(&mut buf).await?;
        let code = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
        ServerMessage::decode(code, &buf[4..])
    }
}

// ── PeerInitConnection ────────────────────────────────────────────────────────

/// Raw TCP stream used before a peer connection type is established.
pub struct PeerInitConnection {
    stream: TcpStream,
}

impl PeerInitConnection {
    pub fn from_stream(stream: TcpStream) -> Self {
        Self { stream }
    }

    pub async fn send_raw(&mut self, frame: &[u8]) -> Result<()> {
        self.stream.write_all(frame).await?;
        Ok(())
    }

    pub async fn recv(&mut self) -> Result<PeerInitMessage> {
        let mut len_buf = [0u8; 4];
        self.stream.read_exact(&mut len_buf).await?;
        let total = u32::from_le_bytes(len_buf) as usize;
        if total == 0 {
            return Err(Error::Protocol("empty peer-init frame".into()));
        }
        let mut buf = vec![0u8; total];
        self.stream.read_exact(&mut buf).await?;
        PeerInitMessage::decode(buf[0], &buf[1..])
    }

    /// Upgrade this init connection into a typed peer connection.
    pub fn into_peer_connection(self) -> PeerConnection {
        PeerConnection {
            stream: self.stream,
        }
    }

    /// Upgrade into a distributed connection.
    pub fn into_distributed_connection(self) -> DistributedConnection {
        DistributedConnection {
            stream: self.stream,
        }
    }

    /// Access the raw stream (e.g. for F connections, which have no code framing).
    pub fn into_stream(self) -> TcpStream {
        self.stream
    }
}

// ── PeerConnection ────────────────────────────────────────────────────────────

/// Established P-type peer connection.
pub struct PeerConnection {
    stream: TcpStream,
}

impl PeerConnection {
    pub async fn send_raw(&mut self, frame: &[u8]) -> Result<()> {
        self.stream.write_all(frame).await?;
        Ok(())
    }

    pub async fn recv(&mut self) -> Result<PeerMessage> {
        let mut len_buf = [0u8; 4];
        self.stream.read_exact(&mut len_buf).await?;
        let total = u32::from_le_bytes(len_buf) as usize;
        if total < 4 {
            return Err(Error::Protocol(format!("peer frame too short: {}", total)));
        }
        let mut buf = vec![0u8; total];
        self.stream.read_exact(&mut buf).await?;
        let code = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
        PeerMessage::decode(code, &buf[4..])
    }
}

// ── DistributedConnection ─────────────────────────────────────────────────────

/// Established D-type distributed search connection.
pub struct DistributedConnection {
    stream: TcpStream,
}

impl DistributedConnection {
    pub async fn send_raw(&mut self, frame: &[u8]) -> Result<()> {
        self.stream.write_all(frame).await?;
        Ok(())
    }

    pub async fn recv(&mut self) -> Result<DistribMessage> {
        let mut len_buf = [0u8; 4];
        self.stream.read_exact(&mut len_buf).await?;
        let total = u32::from_le_bytes(len_buf) as usize;
        if total == 0 {
            return Err(Error::Protocol("empty distributed frame".into()));
        }
        let mut buf = vec![0u8; total];
        self.stream.read_exact(&mut buf).await?;
        DistribMessage::decode(buf[0], &buf[1..])
    }
}
