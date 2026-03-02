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
    peer::{FileSearchResponse, PeerMessage, QueueUpload, TransferResponse},
    peer_init::{PeerInit, PeerInitMessage, PierceFirewall},
    server::{
        ConnectToPeerRequest, FileSearchRequest, GetPeerAddressRequest, LoginRequest,
        ServerMessage, SetWaitPort,
    },
    types::{FileAttributeType, TransferDirection},
};

use crate::{
    app::SearchResult,
    events::{AppEvent, NetCommand},
};

const LISTEN_PORT: u16 = 2234;

// ── Shared state ──────────────────────────────────────────────────────────────

/// A download we have requested but whose F-connection hasn't arrived yet.
#[derive(Debug, Clone)]
struct PendingDownload {
    id: usize,
    username: String,
    filename: String,
    size: u64,
}

/// Keyed by our own CTP token. Used to:
/// - look up the download when the peer PierceFirewalls back to us (pf.token == our token)
/// - look up the download when we send an outbound ConnectToPeer and get a CTP back
///   from the server (c.token == our token in that case too).
type ByCtpToken = Arc<Mutex<HashMap<u32, PendingDownload>>>;

/// Keyed by peer username. Used when the server sends *us* a ConnectToPeer
/// (c.token is the *peer's* token, not ours) — we match by username instead.
/// A single P-connection can serve multiple queued files from the same peer,
/// so we keep a Vec and pop the first entry each time.
type ByUsername = Arc<Mutex<HashMap<String, Vec<PendingDownload>>>>;

/// Keyed by the transfer token that appears inside the F-connection handshake
/// (set by the peer in their TransferRequest). Populated once we accept a
/// TransferRequest; consumed by handle_file_connection.
type ByTransferToken = Arc<Mutex<HashMap<u32, PendingDownload>>>;

// ── Entry point ───────────────────────────────────────────────────────────────

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

    server.send_raw(&SetWaitPort::new(LISTEN_PORT as u32).encode()).await?;

    // ── TCP listener for inbound peer connections ─────────────────────────────

    let listen_addr: SocketAddr = format!("0.0.0.0:{LISTEN_PORT}").parse().unwrap();
    let listener = TcpListener::bind(listen_addr).await?;
    let _ = ui_tx.send(AppEvent::Log(format!("Listening for peers on port {LISTEN_PORT}")));

    // Three lookup tables for pending downloads (see type aliases above).
    let by_ctp:      ByCtpToken      = Arc::new(Mutex::new(HashMap::new()));
    let by_username: ByUsername      = Arc::new(Mutex::new(HashMap::new()));
    let by_transfer: ByTransferToken = Arc::new(Mutex::new(HashMap::new()));

    // Maps our CTP token → peer username, for CantConnectToPeer fallback.
    let ctp_to_peer: Arc<Mutex<HashMap<u32, String>>> = Arc::new(Mutex::new(HashMap::new()));

    let ctp_token_counter = Arc::new(AtomicUsize::new(1));

    // Channel: search-result peer tasks → main loop
    let (peer_tx, mut peer_rx) = tokio::sync::mpsc::unbounded_channel::<FileSearchResponse>();

    // Spawn the inbound acceptor.
    {
        let peer_tx      = peer_tx.clone();
        let by_ctp       = by_ctp.clone();
        let by_username  = by_username.clone();
        let by_transfer  = by_transfer.clone();
        let ui_tx        = ui_tx.clone();
        let download_dir = download_dir.clone();
        let our_username = username.clone();
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, addr)) => {
                        let peer_tx      = peer_tx.clone();
                        let by_ctp       = by_ctp.clone();
                        let by_username  = by_username.clone();
                        let by_transfer  = by_transfer.clone();
                        let ui_tx        = ui_tx.clone();
                        let dd           = download_dir.clone();
                        let our_username = our_username.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handle_inbound(
                                stream, addr,
                                peer_tx, by_ctp, by_username, by_transfer,
                                ui_tx, dd, our_username,
                            ).await {
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

    let mut current_search_token: Option<u32> = None;

    // ── Main event loop ───────────────────────────────────────────────────────
    loop {
        tokio::select! {
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

                    NetCommand::Download { id, username: peer, filename, size } => {
                        let ctp_tok = ctp_token_counter.fetch_add(1, Ordering::Relaxed) as u32;

                        let dl = PendingDownload { id, username: peer.clone(), filename: filename.clone(), size };

                        // Register under all lookup tables.
                        by_ctp.lock().await.insert(ctp_tok, dl.clone());
                        by_username.lock().await.entry(peer.clone()).or_default().push(dl);
                        ctp_to_peer.lock().await.insert(ctp_tok, peer.clone());

                        let _ = ui_tx.send(AppEvent::Log(
                            format!("Requesting P connection to {peer} for '{filename}'")
                        ));

                        let ctp = ConnectToPeerRequest {
                            token: ctp_tok,
                            username: peer.clone(),
                            conn_type: "P".into(),
                        };
                        if let Err(e) = server.send_raw(&ctp.encode()).await {
                            let _ = ui_tx.send(AppEvent::Log(format!("ConnectToPeer send error: {e}")));
                        }
                    }
                }
            }

            msg = server.recv() => {
                match msg {
                    // Server tells us to connect to the peer directly.
                    // c.token is the PEER's token, not ours — match by username.
                    Ok(ServerMessage::ConnectToPeer(c)) if c.conn_type == "P" => {
                        let queue_len = by_username.lock().await
                            .get(&c.username)
                            .map(|q| q.len())
                            .unwrap_or(0);
                        let _ = ui_tx.send(AppEvent::Log(format!(
                            "ConnectToPeer P → {} {}:{} (pending={})",
                            c.username,
                            Ipv4Addr::from(c.ip.to_be_bytes()),
                            c.port,
                            queue_len,
                        )));

                        // Always spawn a task: it reads search results when queue_len==0,
                        // or drains all queued downloads when queue_len>0.
                        // For downloads: later duplicate CTP responses (one per extra file
                        // requested to the same peer) will find an empty queue and become
                        // harmless read_search_results tasks.
                        {
                            let peer_tx      = peer_tx.clone();
                            let by_transfer  = by_transfer.clone();
                            let by_username  = by_username.clone();
                            let ui_tx        = ui_tx.clone();
                            let dd           = download_dir.clone();
                            let our_username = username.clone();
                            tokio::spawn(async move {
                                let ip = Ipv4Addr::from(c.ip.to_be_bytes());
                                let addr: SocketAddr = format!("{ip}:{}", c.port).parse().unwrap();
                                if let Err(e) = outbound_p_connect(
                                    addr, &c.username,
                                    peer_tx, by_username, by_transfer,
                                    ui_tx.clone(), dd, our_username,
                                ).await {
                                    let _ = ui_tx.send(AppEvent::Log(
                                        format!("Outbound connect error to {}: {e}", c.username)
                                    ));
                                }
                            });
                        }
                    }
                    Ok(ServerMessage::ConnectToPeer(c)) if c.conn_type == "F" => {
                        let ip = Ipv4Addr::from(c.ip.to_be_bytes());
                        let addr: SocketAddr = format!("{ip}:{}", c.port).parse().unwrap();
                        let _ = ui_tx.send(AppEvent::Log(format!(
                            "ConnectToPeer F → {} {addr} token={}", c.username, c.token
                        )));
                        let ctp_token    = c.token;
                        let by_transfer  = by_transfer.clone();
                        let ui_tx        = ui_tx.clone();
                        let dd           = download_dir.clone();
                        tokio::spawn(async move {
                            if let Err(e) = outbound_f_connect(
                                addr, ctp_token, by_transfer, ui_tx.clone(), dd,
                            ).await {
                                let _ = ui_tx.send(AppEvent::Log(
                                    format!("Outbound F-connect error: {e}")
                                ));
                            }
                        });
                    }
                    Ok(ServerMessage::ConnectToPeer(c)) => {
                        let _ = ui_tx.send(AppEvent::Log(format!(
                            "ConnectToPeer type='{}' (ignored): {}",
                            c.conn_type, c.username
                        )));
                    }

                    // Server couldn't broker the connection — try GetPeerAddress fallback.
                    // We deduplicate: only issue GetPeerAddress once per peer, not once
                    // per CTP token, so that N queued files don't cause N parallel connects.
                    Ok(ServerMessage::CantConnectToPeer(cant)) => {
                        let peer = ctp_to_peer.lock().await.remove(&cant.token);
                        let _ = ui_tx.send(AppEvent::Log(format!(
                            "CantConnectToPeer token={} peer={:?} — trying GetPeerAddress",
                            cant.token, peer
                        )));
                        if let Some(peer_name) = peer {
                            // Only send GetPeerAddress if this peer still has pending
                            // downloads (i.e. no other CTP response already claimed them).
                            let has_pending = by_username.lock().await
                                .get(&peer_name)
                                .map(|q| !q.is_empty())
                                .unwrap_or(false);
                            if has_pending {
                                let req = GetPeerAddressRequest { username: peer_name };
                                if let Err(e) = server.send_raw(&req.encode()).await {
                                    let _ = ui_tx.send(AppEvent::Log(format!("GetPeerAddress error: {e}")));
                                }
                            }
                        }
                    }

                    // GetPeerAddress response — attempt a cold direct connect.
                    // Drain the entire queue for this peer so one connection handles all files.
                    Ok(ServerMessage::GetPeerAddress(a)) => {
                        let queue_len = by_username.lock().await
                            .get(&a.username)
                            .map(|q| q.len())
                            .unwrap_or(0);
                        let _ = ui_tx.send(AppEvent::Log(format!(
                            "GetPeerAddress → {} {}:{} (pending={})",
                            a.username,
                            Ipv4Addr::from(a.ip.to_be_bytes()),
                            a.port,
                            queue_len,
                        )));
                        if a.port == 0 {
                            let _ = ui_tx.send(AppEvent::Log(format!(
                                "Peer {} is unreachable (port=0)", a.username
                            )));
                        } else if queue_len > 0 {
                            let peer_tx      = peer_tx.clone();
                            let by_transfer  = by_transfer.clone();
                            let by_username  = by_username.clone();
                            let ui_tx        = ui_tx.clone();
                            let dd           = download_dir.clone();
                            let our_username = username.clone();
                            tokio::spawn(async move {
                                let ip = Ipv4Addr::from(a.ip.to_be_bytes());
                                let addr: SocketAddr = format!("{ip}:{}", a.port).parse().unwrap();
                                if let Err(e) = outbound_p_connect(
                                    addr, &a.username,
                                    peer_tx, by_username, by_transfer,
                                    ui_tx.clone(), dd, our_username,
                                ).await {
                                    let _ = ui_tx.send(AppEvent::Log(
                                        format!("GetPeerAddress connect error to {}: {e}", a.username)
                                    ));
                                }
                            });
                        }
                    }

                    Ok(_) => {}
                    Err(e) => {
                        if matches!(&e, slsk_protocol::error::Error::Io(_)) {
                            let _ = ui_tx.send(AppEvent::Log(format!("Server connection lost: {e}")));
                            break;
                        }
                        let _ = ui_tx.send(AppEvent::Log(format!("Server decode error (ignored): {e}")));
                    }
                }
            }

            Some(resp) = peer_rx.recv() => {
                if Some(resp.token) == current_search_token {
                    let results: Vec<SearchResult> = resp.results.iter().map(|r| {
                        let mut bitrate = None;
                        let mut is_vbr = false;
                        let mut duration = None;
                        let mut sample_rate = None;
                        let mut bit_depth = None;
                        for attr in &r.attributes {
                            match attr.code {
                                FileAttributeType::Bitrate    => bitrate     = Some(attr.value),
                                FileAttributeType::Vbr        => is_vbr      = attr.value != 0,
                                FileAttributeType::Duration   => duration    = Some(attr.value),
                                FileAttributeType::SampleRate => sample_rate = Some(attr.value),
                                FileAttributeType::BitDepth   => bit_depth   = Some(attr.value),
                                _ => {}
                            }
                        }
                        SearchResult {
                            username:     resp.username.clone(),
                            filename:     r.filename.clone(),
                            size:         r.size,
                            extension:    r.extension.clone(),
                            slot_free:    resp.slot_free,
                            avg_speed:    resp.avg_speed,
                            queue_length: resp.queue_length,
                            bitrate,
                            is_vbr,
                            duration,
                            sample_rate,
                            bit_depth,
                        }
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

// ── Inbound connection handler ────────────────────────────────────────────────

async fn handle_inbound(
    stream: TcpStream,
    addr: SocketAddr,
    peer_tx: UnboundedSender<FileSearchResponse>,
    by_ctp: ByCtpToken,
    by_username: ByUsername,
    by_transfer: ByTransferToken,
    ui_tx: UnboundedSender<AppEvent>,
    download_dir: PathBuf,
    our_username: String,
) -> anyhow::Result<()> {
    let mut init_conn = PeerInitConnection::from_stream(stream);

    match init_conn.recv().await? {
        PeerInitMessage::PeerInit(init) => {
            let _ = ui_tx.send(AppEvent::Log(format!(
                "Inbound PeerInit from {addr}: type='{}'",
                init.conn_type
            )));
            match init.conn_type.as_str() {
                "P" => {
                    let downloads = by_username.lock().await
                        .remove(&init.username)
                        .unwrap_or_default();
                    let _ = ui_tx.send(AppEvent::Log(format!(
                        "Inbound PeerInit P from {}: {} download(s)",
                        init.username, downloads.len()
                    )));
                    let mut peer_conn = init_conn.into_peer_connection();
                    if downloads.is_empty() {
                        read_search_results(&mut peer_conn, &peer_tx).await;
                    } else {
                        let _ = download_p_session(
                            &mut peer_conn, downloads, &by_transfer, &ui_tx,
                        ).await;
                    }
                }
                "F" => {
                    let mut stream = init_conn.into_stream();
                    receive_file(&mut stream, &by_transfer, &ui_tx, &download_dir).await?;
                }
                _ => {}
            }
        }
        PeerInitMessage::PierceFirewall(pf) => {
            // pf.token == the CTP token we sent in our ConnectToPeerRequest.
            // Look it up to decide if this is a download connection, then drain
            // all remaining pending downloads for this peer from by_username.
            let first_dl = by_ctp.lock().await.remove(&pf.token);
            let downloads = if let Some(first) = first_dl {
                // Drain all remaining queued downloads for this peer.
                let mut rest = by_username.lock().await
                    .remove(&first.username)
                    .unwrap_or_default();
                // Exclude the one we already have from by_ctp to avoid duplicates.
                rest.retain(|d| d.id != first.id);
                let mut all = vec![first];
                all.append(&mut rest);
                all
            } else {
                Vec::new()
            };
            let _ = ui_tx.send(AppEvent::Log(format!(
                "Inbound PierceFirewall from {addr}: token={} download(s)={}",
                pf.token, downloads.len()
            )));

            // Reply with PierceFirewall so the peer knows the handshake is
            // complete and both sides can switch to P-message framing.
            let reply = PierceFirewall { token: pf.token };
            init_conn.send_raw(&reply.encode()).await?;

            let mut peer_conn = init_conn.into_peer_connection();
            if downloads.is_empty() {
                read_search_results(&mut peer_conn, &peer_tx).await;
            } else {
                let _ = download_p_session(
                    &mut peer_conn, downloads, &by_transfer, &ui_tx,
                ).await;
            }
        }
    }

    let _ = (addr, our_username);
    Ok(())
}

// ── Outbound P connection (we connect to the peer) ────────────────────────────

async fn outbound_p_connect(
    addr: SocketAddr,
    peer_username: &str,
    peer_tx: UnboundedSender<FileSearchResponse>,
    by_username: ByUsername,
    by_transfer: ByTransferToken,
    ui_tx: UnboundedSender<AppEvent>,
    download_dir: PathBuf,
    our_username: String,
) -> anyhow::Result<()> {
    let stream = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        TcpStream::connect(addr),
    )
    .await
    .map_err(|_| anyhow::anyhow!("TCP connect timeout to {addr}"))?
    .map_err(|e| anyhow::anyhow!("TCP connect to {addr}: {e}"))?;

    let mut init_conn = PeerInitConnection::from_stream(stream);
    let peer_init = PeerInit::new(&our_username, "P");
    init_conn.send_raw(&peer_init.encode()).await?;

    let mut peer_conn = init_conn.into_peer_connection();

    // Atomically drain the entire download queue for this peer so that no
    // other concurrently spawned task can steal entries.
    let downloads = by_username.lock().await
        .remove(peer_username)
        .unwrap_or_default();

    let _ = ui_tx.send(AppEvent::Log(format!(
        "Outbound P connected to {peer_username}, drained {} download(s) from queue",
        downloads.len()
    )));

    if downloads.is_empty() {
        read_search_results(&mut peer_conn, &peer_tx).await;
    } else {
        let _ = download_p_session(&mut peer_conn, downloads, &by_transfer, &ui_tx).await;
    }

    let _ = download_dir;
    Ok(())
}

// ── Outbound F connection (server-brokered file transfer) ─────────────────────

async fn outbound_f_connect(
    addr: SocketAddr,
    ctp_token: u32,
    by_transfer: ByTransferToken,
    ui_tx: UnboundedSender<AppEvent>,
    download_dir: PathBuf,
) -> anyhow::Result<()> {
    let stream = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        TcpStream::connect(addr),
    )
    .await
    .map_err(|_| anyhow::anyhow!("F-connect timeout to {addr}"))?
    .map_err(|e| anyhow::anyhow!("F-connect to {addr}: {e}"))?;

    let mut init_conn = PeerInitConnection::from_stream(stream);

    // Server-brokered connections use PierceFirewall (not PeerInit) so the
    // peer can match the token from their ConnectToPeerRequest.
    let pf = PierceFirewall { token: ctp_token };
    init_conn.send_raw(&pf.encode()).await?;

    // After PierceFirewall the peer switches straight to F protocol
    // (no PierceFirewall reply) and sends FileTransferInit as raw bytes.
    let mut stream = init_conn.into_stream();
    receive_file(&mut stream, &by_transfer, &ui_tx, &download_dir).await?;

    Ok(())
}

// ── Download P-session: QueueUpload → TransferRequest → TransferResponse ──────
//
// Sends QueueUpload for every pending download up front, then dispatches
// TransferRequest messages by matching the filename so that a single
// P-connection covers all queued files from this peer.

async fn download_p_session(
    peer_conn: &mut PeerConnection,
    downloads: Vec<PendingDownload>,
    by_transfer: &ByTransferToken,
    ui_tx: &UnboundedSender<AppEvent>,
) -> anyhow::Result<()> {
    // Send QueueUpload for every file at once.
    for dl in &downloads {
        let qu = QueueUpload { filename: dl.filename.clone() };
        peer_conn.send_raw(&qu.encode()).await?;
        let _ = ui_tx.send(AppEvent::Log(format!("QueueUpload sent: '{}'", dl.filename)));
    }

    // Build a lookup map so we can match TransferRequest/UploadDenied by filename.
    let mut by_filename: HashMap<String, PendingDownload> = downloads
        .into_iter()
        .map(|dl| (dl.filename.clone(), dl))
        .collect();

    // Read messages until the peer closes the connection or all files are handled.
    loop {
        if by_filename.is_empty() {
            break;
        }

        match peer_conn.recv().await {
            Ok(PeerMessage::PlaceInQueueResponse(piq)) => {
                // Match against a known filename; fall back to the first entry.
                let dl = by_filename.get(&piq.filename)
                    .or_else(|| by_filename.values().next());
                if let Some(dl) = dl {
                    let _ = ui_tx.send(AppEvent::QueuePosition {
                        id: dl.id,
                        position: piq.place,
                    });
                    let _ = ui_tx.send(AppEvent::Log(format!(
                        "Queue position for '{}': {}",
                        piq.filename, piq.place
                    )));
                }
            }

            Ok(PeerMessage::TransferRequest(req))
                if req.direction == TransferDirection::Upload =>
            {
                let transfer_token = req.token;

                // Match by filename, falling back to first entry if not found.
                let dl = by_filename.remove(&req.filename)
                    .or_else(|| by_filename.keys().next().cloned().and_then(|k| by_filename.remove(&k)));

                let Some(dl) = dl else {
                    // Unknown transfer; accept anyway so the connection stays alive.
                    let resp = TransferResponse::UploadAllowed { token: transfer_token };
                    let _ = peer_conn.send_raw(&resp.encode()).await;
                    continue;
                };

                let file_size = req.file_size.unwrap_or(dl.size);
                let resp = TransferResponse::UploadAllowed { token: transfer_token };
                peer_conn.send_raw(&resp.encode()).await?;

                let _ = ui_tx.send(AppEvent::Log(format!(
                    "TransferRequest accepted (token={transfer_token}) for '{}', waiting for F connection…",
                    dl.filename
                )));

                by_transfer.lock().await.insert(
                    transfer_token,
                    PendingDownload {
                        id: dl.id,
                        username: dl.username.clone(),
                        filename: dl.filename.clone(),
                        size: file_size,
                    },
                );
            }

            Ok(PeerMessage::UploadDenied(d)) => {
                if let Some(dl) = by_filename.remove(&d.filename).or_else(|| {
                    by_filename.keys().next().cloned().and_then(|k| by_filename.remove(&k))
                }) {
                    let _ = ui_tx.send(AppEvent::TransferDenied {
                        id: dl.id,
                        reason: d.reason.clone(),
                    });
                    let _ = ui_tx.send(AppEvent::Log(format!(
                        "Upload denied for '{}': {}",
                        dl.filename, d.reason
                    )));
                }
            }

            Ok(_) => {}
            Err(_) => break,
        }
    }

    Ok(())
}

// ── Search-only P-connection loop ─────────────────────────────────────────────

async fn read_search_results(
    peer_conn: &mut PeerConnection,
    peer_tx: &UnboundedSender<FileSearchResponse>,
) {
    loop {
        match peer_conn.recv().await {
            Ok(PeerMessage::FileSearchResponse(raw)) => {
                if let Ok(resp) = FileSearchResponse::decode_compressed(&raw) {
                    let _ = peer_tx.send(resp);
                }
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
}

// ── F-connection: stream file bytes to disk ───────────────────────────────────

async fn receive_file(
    stream: &mut TcpStream,
    by_transfer: &ByTransferToken,
    ui_tx: &UnboundedSender<AppEvent>,
    download_dir: &PathBuf,
) -> anyhow::Result<()> {
    let token = downloader_handshake(stream, 0).await?;
    let _ = ui_tx.send(AppEvent::Log(format!("F-connection: transfer token={token}")));

    let dl = by_transfer.lock().await.remove(&token);

    let Some(dl) = dl else {
        let _ = ui_tx.send(AppEvent::Log(format!(
            "F-connection: no pending download for token={token}"
        )));
        return Ok(());
    };

    let basename = dl.filename.rsplit(['/', '\\']).next().unwrap_or(&dl.filename);
    let dest = download_dir.join(basename);
    let _ = ui_tx.send(AppEvent::Log(format!("Receiving '{basename}'…")));

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

    if total > 0 && downloaded >= total {
        let _ = ui_tx.send(AppEvent::DownloadDone { id: dl.id });
        let _ = ui_tx.send(AppEvent::Log(format!("Download complete: {basename}")));
    } else {
        let _ = ui_tx.send(AppEvent::DownloadFailed {
            id: dl.id,
            reason: format!("incomplete ({downloaded}/{total} bytes)"),
        });
    }

    Ok(())
}
