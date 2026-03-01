use std::net::SocketAddr;

use soulseek_protocol::{
    connection::{PeerInitConnection, ServerConnection},
    error::Result,
    peer::{FileSearchResponse, PeerMessage},
    peer_init::PeerInitMessage,
    server::{FileSearchRequest, LoginRequest, ServerMessage, SetWaitPort},
};
use tokio::{net::TcpListener, net::lookup_host, sync::mpsc};

const LISTEN_PORT: u16 = 2234;
const SEARCH_TOKEN: u32 = 12345;

#[tokio::main]
async fn main() -> Result<()> {
    let addr = lookup_host("server.slsknet.org:2416")
        .await?
        .next()
        .expect("Could not resolve server address");

    let mut conn = ServerConnection::connect(addr).await?;

    let login = std::env::var("SOULSEEK_USERNAME").unwrap_or_default();
    let password = std::env::var("SOULSEEK_PASSWORD").unwrap_or_default();
    let req = LoginRequest::new(login, password);
    conn.send_raw(&req.encode()).await?;

    match conn.recv().await? {
        ServerMessage::Login(resp) => println!("Login: {:?}", resp),
        other => println!("Unexpected login response: {:?}", other),
    }

    // Announce our listen port so peers can connect back to us.
    let port_msg = SetWaitPort::new(LISTEN_PORT.into());
    conn.send_raw(&port_msg.encode()).await?;

    // Start listening for incoming peer connections before sending the search,
    // so we don't miss responses that arrive quickly.
    let listen_addr: SocketAddr = format!("0.0.0.0:{}", LISTEN_PORT).parse().unwrap();
    let listener = TcpListener::bind(listen_addr).await?;
    println!("Listening for peer connections on port {}", LISTEN_PORT);

    // Channel to collect search results from the peer acceptor task.
    let (tx, mut rx) = mpsc::unbounded_channel::<FileSearchResponse>();

    // Spawn a task that accepts incoming peer connections and decodes
    // FileSearchResponse messages, forwarding matching ones over the channel.
    let acceptor_tx = tx.clone();
    tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, peer_addr)) => {
                    let tx = acceptor_tx.clone();
                    tokio::spawn(async move {
                        if let Err(e) = handle_peer(stream, peer_addr, tx).await {
                            eprintln!("Peer {} error: {}", peer_addr, e);
                        }
                    });
                }
                Err(e) => {
                    eprintln!("Listener error: {}", e);
                    break;
                }
            }
        }
    });

    // Send the search request.
    let search = FileSearchRequest {
        token: SEARCH_TOKEN,
        query: "pink floyd".into(),
    };
    conn.send_raw(&search.encode()).await?;
    println!("Search sent, waiting for results...");

    // Drive the server message loop and the result collector concurrently.
    // We stop after collecting a few results for demonstration purposes.
    let mut result_count = 0;
    const MAX_RESULTS: usize = 5;

    loop {
        tokio::select! {
            // Keep the server connection alive and handle any server-pushed messages.
            msg = conn.recv() => {
                match msg? {
                    ServerMessage::ConnectToPeer(c) => {
                        // The server tells us to connect directly to a peer.
                        // We only handle "P" (peer) type connections here.
                        if c.conn_type == "P" {
                            let tx = tx.clone();
                            tokio::spawn(async move {
                                // ip is stored as a u32 in host byte order on the wire.
                                let ip = std::net::Ipv4Addr::from(c.ip.to_be_bytes());
                                if let Err(e) = connect_to_peer(ip, c.port, &c.username, c.token, tx).await {
                                    eprintln!("Direct peer connect error: {}", e);
                                }
                            });
                        }
                    }
                    // Ignore other server messages (RoomList, PrivilegedUsers, etc.)
                    _ => {}
                }
            }

            // Print each search result as it arrives.
            Some(resp) = rx.recv() => {
                if resp.token == SEARCH_TOKEN {
                    println!(
                        "[{}] {} result(s) from {}",
                        resp.token,
                        resp.results.len(),
                        resp.username
                    );
                    for file in &resp.results {
                        println!("  {} ({:?})", file.filename, file);
                    }
                    result_count += 1;
                    if result_count >= MAX_RESULTS {
                        println!("Collected {} peer responses, stopping.", MAX_RESULTS);
                        break;
                    }
                }
            }
        }
    }

    Ok(())
}

/// Handle an inbound peer TCP connection: read the PeerInit handshake,
/// upgrade to a PeerConnection, then read messages until the peer closes.
async fn handle_peer(
    stream: tokio::net::TcpStream,
    peer_addr: SocketAddr,
    tx: mpsc::UnboundedSender<FileSearchResponse>,
) -> Result<()> {
    let mut init_conn = PeerInitConnection::from_stream(stream);

    match init_conn.recv().await? {
        PeerInitMessage::PeerInit(init) => {
            if init.conn_type != "P" {
                return Ok(());
            }
            println!("Peer init from {} ({})", init.username, peer_addr);
        }
        PeerInitMessage::PierceFirewall(_) => {
            // Indirect connection — not handled in this simple example.
            return Ok(());
        }
    }

    let mut peer_conn = init_conn.into_peer_connection();
    loop {
        match peer_conn.recv().await {
            Ok(PeerMessage::FileSearchResponse(raw)) => {
                match FileSearchResponse::decode_compressed(&raw) {
                    Ok(resp) => {
                        let _ = tx.send(resp);
                    }
                    Err(e) => eprintln!("Decode error from {}: {}", peer_addr, e),
                }
            }
            Ok(_) => {}
            Err(_) => break, // Peer closed the connection.
        }
    }

    Ok(())
}

/// Initiate a direct outbound peer connection when the server sends us a
/// ConnectToPeer message, perform the PeerInit handshake, then read results.
async fn connect_to_peer(
    ip: std::net::Ipv4Addr,
    port: u32,
    username: &str,
    _token: u32,
    tx: mpsc::UnboundedSender<FileSearchResponse>,
) -> Result<()> {
    use soulseek_protocol::peer_init::PeerInit;

    let addr: SocketAddr = format!("{}:{}", ip, port).parse().unwrap();
    let stream = tokio::net::TcpStream::connect(addr).await?;
    let mut init_conn = PeerInitConnection::from_stream(stream);

    // We initiate, so we send the PeerInit message.
    let our_username = std::env::var("SOULSEEK_USERNAME").unwrap_or_default();
    let peer_init = PeerInit::new(our_username, "P");
    init_conn.send_raw(&peer_init.encode()).await?;

    let mut peer_conn = init_conn.into_peer_connection();
    loop {
        match peer_conn.recv().await {
            Ok(PeerMessage::FileSearchResponse(raw)) => {
                match FileSearchResponse::decode_compressed(&raw) {
                    Ok(resp) => {
                        println!(
                            "Direct result from {}: {} file(s)",
                            username,
                            resp.results.len()
                        );
                        let _ = tx.send(resp);
                    }
                    Err(e) => eprintln!("Decode error from {}: {}", username, e),
                }
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }

    Ok(())
}
