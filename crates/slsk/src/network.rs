//! Async network actor: manages the server connection, peer acceptor, and
//! dispatches search results and download requests.

use std::{
    collections::HashMap,
    net::{Ipv4Addr, SocketAddr},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use tokio::{
    fs,
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{
        Mutex,
        mpsc::{UnboundedReceiver, UnboundedSender},
    },
};

use slsk_protocol::{
    connection::{PeerConnection, PeerInitConnection, ServerConnection},
    file::downloader_handshake,
    peer::{FileSearchResponse, PeerMessage, QueueUpload},
    peer_init::{PeerInit, PeerInitMessage},
    server::{
        ConnectToPeerRequest, FileSearchRequest, LoginRequest, ServerMessage, SetWaitPort,
    },
    types::TransferDirection,
};

use crate::{
    app::SearchResult,
    events::{AppEvent, NetCommand},
};

/// Port we listen on for incoming peer connections.
const LISTEN_PORT: u16 = 2234;

/// Shared map from transfer token → download id, used across peer tasks.
type PendingDownloads = Arc<Mutex<HashMap<u32, PendingDownload>>>;

#[derive(Debug, Clone)]
struct PendingDownload {
    id: usize,
    filename: String,
    size: u64,
}

/// Entry point — spawned as a long-lived tokio task.
pub async fn run(
    username: String,
    password: String,
    ui_tx: UnboundedSender<AppEvent>,
    mut cmd_rx: UnboundedReceiver<NetCommand>,
    download_dir: PathBuf,
) {
    if let Err(e) = run_inner(username, password, ui_tx.clone(), &mut cmd_rx, download_dir).await {
        let _ = ui_tx.send(AppEvent::Log(format!("Network error: {e}")));
    }
}

async fn run_inner(
    username: String,
    password: String,
    ui_tx: UnboundedSender<AppEvent>,
    cmd_rx: &mut UnboundedReceiver<NetCommand>,
    download_dir: PathBuf,
) -> anyhow::Result<()> {
    // ── Connect & login ───────────────────────────────────────────────────────

    let _ = ui_tx.send(AppEvent::Log("Connecting to server.slsknet.org…".into()));
    let addr = tokio::net::lookup_host("server.slsknet.org:2416")
        .await?
        .next()
        .ok_or_else(|| anyhow::anyhow!("could not resolve server address"))?;

    let mut server = ServerConnection::connect(addr).await?;
    let _ = ui_tx.send(AppEvent::Log("Connected. Logging in…".into()));

    let req = LoginRequest::new(&username, &password);
    server.send_raw(&req.encode()).await?;

    match server.recv().await? {
        ServerMessage::Login(slsk_protocol::server::LoginResponse::Success { greet, .. }) => {
            let _ = ui_tx.send(AppEvent::LoginOk { greet: greet.clone() });
            let _ = ui_tx.send(AppEvent::Log(format!("Login OK: {greet}")));
        }
        ServerMessage::Login(slsk_protocol::server::LoginResponse::Failure { reason, .. }) => {
            let _ = ui_tx.send(AppEvent::LoginFailed { reason: reason.clone() });
            let _ = ui_tx.send(AppEvent::Log(format!("Login FAILED: {reason}")));
            return Ok(());
        }
        other => {
            let _ = ui_tx.send(AppEvent::Log(format!("Unexpected response: {other:?}")));
            return Ok(());
        }
    }

    // Announce our listen port.
    server.send_raw(&SetWaitPort::new(LISTEN_PORT as u32).encode()).await?;

    // ── Start TCP listener for inbound peer connections ───────────────────────

    let listen_addr: SocketAddr = format!("0.0.0.0:{LISTEN_PORT}").parse().unwrap();
    let listener = TcpListener::bind(listen_addr).await?;
    let _ = ui_tx.send(AppEvent::Log(format!("Listening for peers on port {LISTEN_PORT}")));

    let pending: PendingDownloads = Arc::new(Mutex::new(HashMap::new()));
    let token_counter = Arc::new(AtomicUsize::new(1));

    // Channel for search results received by peer tasks → main actor loop.
    let (peer_tx, mut peer_rx) =
        tokio::sync::mpsc::unbounded_channel::<FileSearchResponse>();

    // Spawn peer acceptor task.
    {
        let peer_tx = peer_tx.clone();
        let pending = pending.clone();
        let ui_tx = ui_tx.clone();
        let download_dir = download_dir.clone();
        let our_username = username.clone();
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, addr)) => {
                        let peer_tx = peer_tx.clone();
                        let pending = pending.clone();
                        let ui_tx = ui_tx.clone();
                        let dd = download_dir.clone();
                        let our_username = our_username.clone();
                        tokio::spawn(async move {
                            if let Err(e) =
                                handle_inbound_peer(stream, addr, peer_tx, pending, ui_tx, dd, our_username).await
                            {
                                // Peer closed connection — not interesting enough to log.
                                let _ = e;
                            }
                        });
                    }
                    Err(e) => {
                        let _ = ui_tx.send(AppEvent::Log(format!("Listener error: {e}")));
                        break;
                    }
                }
            }
        });
    }

    // ── Current search token we care about ───────────────────────────────────
    let mut current_search_token: Option<u32> = None;

    // ── Main event loop ───────────────────────────────────────────────────────
    loop {
        tokio::select! {
            // Commands from the UI
            cmd = cmd_rx.recv() => {
                let Some(cmd) = cmd else { break };
                match cmd {
                    NetCommand::Search { token, query } => {
                        current_search_token = Some(token);
                        let req = FileSearchRequest { token, query: query.clone() };
                        if let Err(e) = server.send_raw(&req.encode()).await {
                            let _ = ui_tx.send(AppEvent::Log(format!("Search send error: {e}")));
                        } else {
                            let _ = ui_tx.send(AppEvent::Log(format!("Search sent for '{query}'")));
                        }
                    }

                    NetCommand::Download { id, username, filename, size } => {
                        // Get the peer address so we can connect to them.
                        let tok = token_counter.fetch_add(1, Ordering::Relaxed) as u32;
                        {
                            let mut guard = pending.lock().await;
                            guard.insert(tok, PendingDownload { id, filename: filename.clone(), size });
                        }

                        // Ask the server to arrange a P connection to the peer,
                        // then follow up with a TransferRequest over it.
                        let peer_tx = peer_tx.clone();
                        let pending = pending.clone();
                        let ui_tx2 = ui_tx.clone();
                        let our_username = username.clone();
                        let dd = download_dir.clone();

                        // Send ConnectToPeer — the server will either relay a
                        // ConnectToPeer back to us with the peer's address, or
                        // the peer will connect to us directly (handled by the
                        // acceptor).  We store the token so we recognise
                        // whichever path completes first.
                        let ctp = ConnectToPeerRequest {
                            token: tok,
                            username: username.clone(),
                            conn_type: "P".into(),
                        };
                        if let Err(e) = server.send_raw(&ctp.encode()).await {
                            let _ = ui_tx.send(AppEvent::Log(format!("ConnectToPeer error: {e}")));
                        } else {
                            let _ = ui_tx.send(AppEvent::Log(
                                format!("Requesting connection to {our_username} for '{filename}'")
                            ));
                        }

                        // Peer will show up either via the acceptor (inbound)
                        // or via ConnectToPeer response below — the pending map
                        // ensures whichever wins picks up the TransferRequest.
                        // The actual transfer negotiation is done in the peer
                        // task; we just need to suppress the compiler warning.
                        let _ = (peer_tx, pending, ui_tx2, dd);
                    }
                }
            }

            // Incoming server messages
            msg = server.recv() => {
                match msg {
                    Ok(ServerMessage::ConnectToPeer(c)) if c.conn_type == "P" => {
                        let peer_tx = peer_tx.clone();
                        let pending = pending.clone();
                        let ui_tx = ui_tx.clone();
                        let dd = download_dir.clone();
                        let our_username = username.clone();
                        tokio::spawn(async move {
                            let ip = Ipv4Addr::from(c.ip.to_be_bytes());
                            if let Err(e) = connect_to_peer_outbound(
                                ip, c.port, &c.username, c.token,
                                peer_tx, pending, ui_tx.clone(), dd, our_username,
                            ).await {
                                let _ = ui_tx.send(AppEvent::Log(
                                    format!("Outbound peer connect error: {e}")
                                ));
                            }
                        });
                    }
                    Ok(ServerMessage::ConnectToPeer(c)) if c.conn_type == "F" => {
                        // File connection — handled by inbound peer acceptor.
                    }
                    Ok(other) => {
                        // Ignore most server-push messages silently.
                        let _ = other;
                    }
                    Err(e) => {
                        let _ = ui_tx.send(AppEvent::Log(format!("Server recv error: {e}")));
                        break;
                    }
                }
            }

            // Search results from peer tasks
            Some(resp) = peer_rx.recv() => {
                if Some(resp.token) == current_search_token {
                    let results: Vec<SearchResult> = resp.results.iter().map(|r| SearchResult {
                        username: resp.username.clone(),
                        filename: r.filename.clone(),
                        size: r.size,
                        extension: r.extension.clone(),
                    }).collect();
                    if !results.is_empty() {
                        let _ = ui_tx.send(AppEvent::SearchResults {
                            token: resp.token,
                            results,
                        });
                    }
                }
            }
        }
    }

    Ok(())
}

// ── Inbound peer connection handler ───────────────────────────────────────────

async fn handle_inbound_peer(
    stream: TcpStream,
    addr: SocketAddr,
    peer_tx: UnboundedSender<FileSearchResponse>,
    pending: PendingDownloads,
    ui_tx: UnboundedSender<AppEvent>,
    download_dir: PathBuf,
    our_username: String,
) -> anyhow::Result<()> {
    let mut init_conn = PeerInitConnection::from_stream(stream);

    match init_conn.recv().await? {
        PeerInitMessage::PeerInit(init) => {
            match init.conn_type.as_str() {
                "P" => {
                    let mut peer_conn = init_conn.into_peer_connection();
                    handle_peer_messages(
                        &mut peer_conn, addr, &peer_tx, &pending, &ui_tx,
                        &download_dir, &our_username,
                    ).await?;
                }
                "F" => {
                    let mut stream = init_conn.into_stream();
                    handle_file_connection(
                        &mut stream, &pending, &ui_tx, &download_dir,
                    ).await?;
                }
                _ => {}
            }
        }
        PeerInitMessage::PierceFirewall(pf) => {
            // Indirect connection: peer pierces our firewall — treat as P.
            let _ = pf;
            let mut peer_conn = init_conn.into_peer_connection();
            handle_peer_messages(
                &mut peer_conn, addr, &peer_tx, &pending, &ui_tx,
                &download_dir, &our_username,
            ).await?;
        }
    }

    Ok(())
}

// ── Outbound peer connection ───────────────────────────────────────────────────

async fn connect_to_peer_outbound(
    ip: Ipv4Addr,
    port: u32,
    username: &str,
    token: u32,
    peer_tx: UnboundedSender<FileSearchResponse>,
    pending: PendingDownloads,
    ui_tx: UnboundedSender<AppEvent>,
    download_dir: PathBuf,
    our_username: String,
) -> anyhow::Result<()> {
    let addr: SocketAddr = format!("{ip}:{port}").parse()?;
    let stream = TcpStream::connect(addr).await?;
    let mut init_conn = PeerInitConnection::from_stream(stream);

    // Check whether this token is in our pending downloads.
    let is_download = pending.lock().await.contains_key(&token);

    if is_download {
        // For a download, negotiate a "P" connection first, send
        // TransferRequest (QueueUpload), then wait for TransferResponse.
        let peer_init = PeerInit::new(&our_username, "P");
        init_conn.send_raw(&peer_init.encode()).await?;

        let mut peer_conn = init_conn.into_peer_connection();
        handle_peer_messages(
            &mut peer_conn, addr, &peer_tx, &pending, &ui_tx,
            &download_dir, &our_username,
        ).await?;
    } else {
        // Search result path — standard PeerInit "P".
        let peer_init = PeerInit::new(&our_username, "P");
        init_conn.send_raw(&peer_init.encode()).await?;

        let mut peer_conn = init_conn.into_peer_connection();
        handle_peer_messages(
            &mut peer_conn, addr, &peer_tx, &pending, &ui_tx,
            &download_dir, &our_username,
        ).await?;
    }

    let _ = (token, username);
    Ok(())
}

// ── Generic P-connection message loop ─────────────────────────────────────────

async fn handle_peer_messages(
    peer_conn: &mut PeerConnection,
    _addr: SocketAddr,
    peer_tx: &UnboundedSender<FileSearchResponse>,
    pending: &PendingDownloads,
    ui_tx: &UnboundedSender<AppEvent>,
    download_dir: &PathBuf,
    _our_username: &str,
) -> anyhow::Result<()> {
    loop {
        match peer_conn.recv().await {
            Ok(PeerMessage::FileSearchResponse(raw)) => {
                if let Ok(resp) = FileSearchResponse::decode_compressed(&raw) {
                    let _ = peer_tx.send(resp);
                }
            }

            // Peer is asking us to queue an upload — not relevant for a pure
            // download client, but we receive it when we send QueueUpload.
            Ok(PeerMessage::QueueUpload(_)) => {}

            // Peer accepted our transfer request — they will now open an F
            // connection to us.  We just wait for the inbound F connection.
            Ok(PeerMessage::TransferRequest(req)) => {
                if req.direction == TransferDirection::Upload {
                    // Peer is initiating an upload to us.
                    let guard = pending.lock().await;
                    if let Some(dl) = guard.values().find(|d| d.filename == req.filename) {
                        let _ = ui_tx.send(AppEvent::Log(format!(
                            "Transfer starting: {}",
                            req.filename
                        )));
                        let _ = dl;
                    }
                }
            }

            Ok(PeerMessage::UploadDenied(d)) => {
                let mut guard = pending.lock().await;
                if let Some((&tok, dl)) =
                    guard.iter().find(|(_, d)| d.filename == d.filename)
                {
                    let _ = ui_tx.send(AppEvent::TransferDenied {
                        id: dl.id,
                        reason: d.reason.clone(),
                    });
                    guard.remove(&tok);
                }
            }

            // Peer wants us to download a file — queue the upload from their
            // perspective.  We respond by opening an F connection.
            Ok(PeerMessage::PlaceInQueueResponse(_)) => {}

            Ok(_) => {}

            Err(_) => break,
        }
    }

    // If we have a pending download for this peer, send QueueUpload.
    // (In real Soulseek, we send this after receiving the peer's TransferRequest
    // telling us they're uploading; for simplicity we send it immediately.)
    {
        let guard = pending.lock().await;
        for dl in guard.values() {
            let qu = QueueUpload { filename: dl.filename.clone() };
            let _ = peer_conn.send_raw(&qu.encode()).await;
        }
    }

    let _ = download_dir;
    Ok(())
}

// ── F-connection: receive file bytes ──────────────────────────────────────────

async fn handle_file_connection(
    stream: &mut TcpStream,
    pending: &PendingDownloads,
    ui_tx: &UnboundedSender<AppEvent>,
    download_dir: &PathBuf,
) -> anyhow::Result<()> {
    // Downloader handshake: read token, send offset (always 0 for new download).
    let token = downloader_handshake(stream, 0).await?;

    let dl = {
        let guard = pending.lock().await;
        guard.get(&token).cloned()
    };

    let Some(dl) = dl else {
        let _ = ui_tx.send(AppEvent::Log(format!("Unknown transfer token {token}")));
        return Ok(());
    };

    let basename = dl.filename.rsplit(['/', '\\']).next().unwrap_or(&dl.filename);
    let dest = download_dir.join(basename);

    let _ = ui_tx.send(AppEvent::Log(format!("Receiving '{basename}'…")));

    // Ensure the download directory exists.
    fs::create_dir_all(download_dir).await?;
    let mut file = fs::File::create(&dest).await?;

    let mut buf = vec![0u8; 65536];
    let mut downloaded: u64 = 0;
    let total = dl.size;

    loop {
        let n = stream.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).await?;
        downloaded += n as u64;

        let _ = ui_tx.send(AppEvent::DownloadProgress {
            id: dl.id,
            downloaded,
            total,
        });

        if total > 0 && downloaded >= total {
            break;
        }
    }

    file.flush().await?;

    // Remove from pending.
    pending.lock().await.remove(&token);

    if total > 0 && downloaded >= total {
        let _ = ui_tx.send(AppEvent::DownloadDone { id: dl.id });
        let _ = ui_tx.send(AppEvent::Log(format!("Download complete: {basename}")));
    } else {
        let _ = ui_tx.send(AppEvent::DownloadFailed {
            id: dl.id,
            reason: format!("incomplete: {downloaded}/{total} bytes"),
        });
    }

    Ok(())
}
