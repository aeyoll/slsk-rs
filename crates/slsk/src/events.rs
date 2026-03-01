use crate::app::SearchResult;

/// Events emitted by the async network layer and consumed by the UI loop.
#[derive(Debug)]
pub enum AppEvent {
    /// Login succeeded.
    LoginOk { greet: String },
    /// Login failed.
    LoginFailed { reason: String },
    /// A batch of search results arrived for the given token.
    SearchResults {
        token: u32,
        results: Vec<SearchResult>,
    },
    /// A download's byte progress was updated.
    DownloadProgress {
        id: usize,
        downloaded: u64,
        total: u64,
    },
    /// A download completed successfully.
    DownloadDone { id: usize },
    /// A download failed.
    DownloadFailed { id: usize, reason: String },
    /// A peer connection was rejected or the peer queue-denied the transfer.
    TransferDenied { id: usize, reason: String },
    /// Our position in the peer's upload queue was updated.
    QueuePosition { id: usize, position: u32 },
    /// Generic log message from the network layer.
    Log(String),
}

/// Commands the UI sends to the network actor.
#[derive(Debug)]
pub enum NetCommand {
    /// Issue a file search.
    Search { token: u32, query: String },
    /// Enqueue a file download.
    Download {
        id: usize,
        username: String,
        filename: String,
        size: u64,
    },
}
