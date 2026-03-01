# soulseek-protocol

A pure-Rust implementation of the [Soulseek](https://www.slsknet.org/) P2P file-sharing protocol, based on the [Nicotine+](https://nicotine-plus.org/) protocol documentation.

---

## Features

- Full codec (encode + decode) for all active server, peer, peer-init, file, and distributed messages
- Zero-copy where possible; no `unsafe`
- Async Tokio helpers for live TCP connections (`ServerConnection`, `PeerConnection`, etc.)
- zlib compression/decompression for shared file lists and search responses
- MD5 hash helpers for the login handshake
- Strongly-typed enums for status codes, transfer directions, file attributes, etc.

---

## Crate layout

```
src/
├── lib.rs            # Crate root + docs
├── codec.rs          # Read/write primitives + message framing
├── error.rs          # Error / Result types
├── types.rs          # Shared enums and structs
├── server.rs         # All server messages (codes 1–1003)
├── peer_init.rs      # PierceFirewall + PeerInit (uint8 framing)
├── peer.rs           # Peer messages over P connections (uint32 framing)
├── shared_files.rs   # zlib-encoded shared file list helpers
├── file.rs           # FileTransferInit + FileOffset (F connections)
├── distributed.rs    # Distributed search network (D connections)
└── connection.rs     # Async Tokio connection wrappers
```

---

## Quick start

Add to `Cargo.toml`:

```toml
[dependencies]
soulseek-protocol = { path = "." }
tokio = { version = "1", features = ["full"] }
```

### Login to the server

```rust
use soulseek_protocol::{
    connection::ServerConnection,
    error::Result,
    server::{LoginRequest, ServerMessage, SetWaitPort},
};
use tokio::net::lookup_host;

#[tokio::main]
async fn main() -> Result<()> {
    let addr = lookup_host("server.slsknet.org:2416")
        .await?
        .next()
        .expect("Could not resolve server address");
    let mut conn = ServerConnection::connect(addr).await?;

    // Send login message
    let req = LoginRequest::new("myuser", "mypassword");
    conn.send_raw(&req.encode()).await?;

    // Read login response
    match conn.recv().await? {
        ServerMessage::Login(resp) => println!("{:?}", resp),
        other => println!("Unexpected: {:?}", other),
    }

    // Announce our listen port
    let port_msg = SetWaitPort::new(2234);
    conn.send_raw(&port_msg.encode()).await?;

    Ok(())
}
```

### Search for files

```rust
use soulseek_protocol::server::{FileSearchRequest, ServerMessage};

let search = FileSearchRequest { token: 12345, query: "pink floyd".into() };
conn.send_raw(&search.encode()).await?;
```

### Download a file (full flow)

```rust
use soulseek_protocol::{
    peer::{QueueUpload, PeerMessage, TransferResponse},
    peer_init::{PeerInit, PeerInitMessage},
    file,
};

// 1. Queue upload on peer
let queue = QueueUpload { filename: "\\music\\pink_floyd\\wish_you_were_here.flac".into() };
peer_conn.send_raw(&queue.encode()).await?;

// 2. Peer sends TransferRequest — accept it
let msg = peer_conn.recv().await?;
if let PeerMessage::TransferRequest(req) = msg {
    let accept = TransferResponse::UploadAllowed { token: req.token };
    peer_conn.send_raw(&accept.encode()).await?;
}

// 3. Peer opens F connection; perform handshake
let bytes_we_have = 0u64;
let token = file::downloader_handshake(&mut f_stream, bytes_we_have).await?;

// 4. Read file data until connection closes
tokio::io::copy(&mut f_stream, &mut output_file).await?;
```

---

## Protocol coverage

### Server messages

| Code | Name | Status |
|------|------|--------|
| 1 | Login | ✅ |
| 2 | SetWaitPort | ✅ |
| 3 | GetPeerAddress | ✅ |
| 5 | WatchUser | ✅ |
| 6 | UnwatchUser | ✅ |
| 7 | GetUserStatus | ✅ |
| 13 | SayChatroom | ✅ |
| 14 | JoinRoom | ✅ |
| 15 | LeaveRoom | ✅ |
| 16 | UserJoinedRoom | ✅ |
| 17 | UserLeftRoom | ✅ |
| 18 | ConnectToPeer | ✅ |
| 22 | MessageUser | ✅ |
| 23 | MessageAcked | ✅ |
| 26 | FileSearch | ✅ |
| 28 | SetStatus | ✅ |
| 35 | SharedFoldersFiles | ✅ |
| 36 | GetUserStats | ✅ |
| 41 | Relogged | ✅ |
| 42 | UserSearch | ✅ |
| 51 | AddThingILike | ✅ |
| 52 | RemoveThingILike | ✅ |
| 54 | Recommendations | ✅ |
| 56 | GlobalRecommendations | ✅ |
| 57 | UserInterests | ✅ |
| 64 | RoomList | ✅ |
| 66 | AdminMessage | ✅ |
| 69 | PrivilegedUsers | ✅ |
| 71 | HaveNoParent | ✅ |
| 83 | ParentMinSpeed | ✅ |
| 84 | ParentSpeedRatio | ✅ |
| 92 | CheckPrivileges | ✅ |
| 93 | EmbeddedMessage | ✅ |
| 100 | AcceptChildren | ✅ |
| 102 | PossibleParents | ✅ |
| 103 | WishlistSearch | ✅ |
| 104 | WishlistInterval | ✅ |
| 110 | SimilarUsers | ✅ |
| 113–116 | Room Tickers | ✅ |
| 117–118 | Hate list | ✅ |
| 120 | RoomSearch | ✅ |
| 121 | SendUploadSpeed | ✅ |
| 123 | GivePrivileges | ✅ |
| 126 | BranchLevel | ✅ |
| 127 | BranchRoot | ✅ |
| 130 | ResetDistributed | ✅ |
| 133–148 | Private room management | ✅ |
| 149 | MessageUsers | ✅ |
| 150–152 | Global room | ✅ |
| 160 | ExcludedSearchPhrases | ✅ |
| 1001 | CantConnectToPeer | ✅ |
| 1003 | CantCreateRoom | ✅ |

### Peer init messages

| Code | Name | Status |
|------|------|--------|
| 0 | PierceFirewall | ✅ |
| 1 | PeerInit | ✅ |

### Peer messages

| Code | Name | Status |
|------|------|--------|
| 4 | GetSharedFileList | ✅ |
| 5 | SharedFileListResponse (zlib) | ✅ |
| 9 | FileSearchResponse (zlib) | ✅ |
| 15 | UserInfoRequest | ✅ |
| 16 | UserInfoResponse | ✅ |
| 36 | FolderContentsRequest | ✅ |
| 37 | FolderContentsResponse (zlib) | ✅ |
| 40 | TransferRequest | ✅ |
| 41 | TransferResponse (upload + legacy download) | ✅ |
| 43 | QueueUpload | ✅ |
| 44 | PlaceInQueueResponse | ✅ |
| 46 | UploadFailed | ✅ |
| 50 | UploadDenied | ✅ |
| 51 | PlaceInQueueRequest | ✅ |

### File connection

| Message | Status |
|---------|--------|
| FileTransferInit | ✅ |
| FileOffset | ✅ |
| Async handshake helpers | ✅ |

### Distributed messages

| Code | Name | Status |
|------|------|--------|
| 3 | DistribSearch | ✅ |
| 4 | DistribBranchLevel | ✅ |
| 5 | DistribBranchRoot | ✅ |

---

## Wire format notes

- All integers are **little-endian**
- Strings are **uint32-length-prefixed** byte strings
- Server and peer (P) messages use **uint32** message codes
- Peer-init and distributed (D) messages use **uint8** message codes
- File (F) connections have **no message codes** — messages are position-based
- Shared file lists and search responses are **zlib-compressed**
- Login uses a **MD5 hex digest** of `username + password` concatenated

---

## References

- [Nicotine+ protocol documentation](https://nicotine-plus.org/doc/SLSKPROTOCOL.html)
- [Soulfind](https://github.com/slskd/soulfind) — open source Soulseek server
- [slskd](https://github.com/slskd/slskd) — .NET Soulseek client

---

## License

MIT
