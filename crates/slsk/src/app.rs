use ratatui::widgets::ListState;

use crate::events::NetCommand;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveTab {
    Search,
    Downloads,
}

impl ActiveTab {
    pub fn next(self) -> Self {
        match self {
            Self::Search => Self::Downloads,
            Self::Downloads => Self::Search,
        }
    }

    pub fn previous(self) -> Self {
        match self {
            Self::Search => Self::Downloads,
            Self::Downloads => Self::Search,
        }
    }

    pub fn index(self) -> usize {
        match self {
            Self::Search => 0,
            Self::Downloads => 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadStatus {
    Queued,
    InProgress { downloaded: u64, total: u64 },
    Done,
    Failed(String),
}

impl std::fmt::Display for DownloadStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Queued => write!(f, "Queued"),
            Self::InProgress { downloaded, total } => {
                let pct = if *total > 0 {
                    (*downloaded as f64 / *total as f64 * 100.0) as u64
                } else {
                    0
                };
                let dl_kb = downloaded / 1024;
                let tot_kb = total / 1024;
                write!(f, "{pct}%  ({dl_kb}/{tot_kb} KB)")
            }
            Self::Done => write!(f, "Done"),
            Self::Failed(reason) => write!(f, "Failed: {reason}"),
        }
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct SearchResult {
    pub username: String,
    pub filename: String,
    pub size: u64,
    pub extension: String,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Download {
    /// Stable identifier used to match progress events.
    pub id: usize,
    pub username: String,
    pub filename: String,
    pub size: u64,
    pub status: DownloadStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginStatus {
    Connecting,
    LoggedIn,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchInputMode {
    /// Navigating the results list
    Normal,
    /// Typing in the search box
    Editing,
}

pub struct App {
    pub active_tab: ActiveTab,
    pub exit: bool,

    // Network
    pub login_status: LoginStatus,
    pub net_tx: tokio::sync::mpsc::UnboundedSender<NetCommand>,
    pub search_token_counter: u32,
    pub current_search_token: Option<u32>,

    // Search tab state
    pub search_input: String,
    pub search_input_mode: SearchInputMode,
    pub search_results: Vec<SearchResult>,
    pub search_list_state: ListState,
    pub selected_for_download: Vec<usize>,

    // Downloads tab state
    pub downloads: Vec<Download>,
    pub download_id_counter: usize,
    pub download_list_state: ListState,

    // Log pane
    pub log_messages: Vec<String>,
    pub log_scroll: u16,
}

impl App {
    pub fn new(net_tx: tokio::sync::mpsc::UnboundedSender<NetCommand>) -> Self {
        let mut search_list_state = ListState::default();
        search_list_state.select(None);

        let mut download_list_state = ListState::default();
        download_list_state.select(None);

        Self {
            active_tab: ActiveTab::Search,
            exit: false,
            login_status: LoginStatus::Connecting,
            net_tx,
            search_token_counter: 1,
            current_search_token: None,
            search_input: String::new(),
            search_input_mode: SearchInputMode::Normal,
            search_results: Vec::new(),
            search_list_state,
            selected_for_download: Vec::new(),
            downloads: Vec::new(),
            download_id_counter: 0,
            download_list_state,
            log_messages: vec![
                "Welcome to slsk-rs — a Soulseek TUI client.".into(),
                "Connecting to the Soulseek network…".into(),
            ],
            log_scroll: 0,
        }
    }

    pub fn push_log(&mut self, msg: impl Into<String>) {
        self.log_messages.push(msg.into());
        if self.log_messages.len() > 500 {
            self.log_messages.drain(0..100);
        }
    }

    // ── Network helpers ───────────────────────────────────────────────────────

    /// Issue a real search over the network.
    pub fn send_search(&mut self, query: String) {
        let token = self.search_token_counter;
        self.search_token_counter = self.search_token_counter.wrapping_add(1);
        self.current_search_token = Some(token);
        self.search_results.clear();
        self.search_list_state.select(None);
        self.selected_for_download.clear();

        let _ = self.net_tx.send(NetCommand::Search { token, query });
    }

    /// Enqueue all selected results as real downloads.
    pub fn enqueue_selected_downloads(&mut self) {
        if self.selected_for_download.is_empty() {
            self.push_log("No files selected for download.");
            return;
        }

        let indices: Vec<usize> = self.selected_for_download.drain(..).collect();
        let mut count = 0;

        for idx in indices {
            let Some(result) = self.search_results.get(idx) else {
                continue;
            };

            let id = self.download_id_counter;
            self.download_id_counter += 1;

            self.downloads.push(Download {
                id,
                username: result.username.clone(),
                filename: result.filename.clone(),
                size: result.size,
                status: DownloadStatus::Queued,
            });

            let _ = self.net_tx.send(NetCommand::Download {
                id,
                username: result.username.clone(),
                filename: result.filename.clone(),
                size: result.size,
            });

            count += 1;
        }

        if self.download_list_state.selected().is_none() && !self.downloads.is_empty() {
            self.download_list_state.select(Some(0));
        }

        if count > 0 {
            self.push_log(format!("Enqueued {count} file(s) for download."));
        }
    }

    // ── Network event handlers ────────────────────────────────────────────────

    pub fn on_search_results(&mut self, token: u32, results: Vec<SearchResult>) {
        if Some(token) != self.current_search_token {
            return;
        }
        let prev_len = self.search_results.len();
        self.search_results.extend(results);
        if prev_len == 0 && !self.search_results.is_empty() {
            self.search_list_state.select(Some(0));
        }
    }

    pub fn on_download_progress(&mut self, id: usize, downloaded: u64, total: u64) {
        if let Some(dl) = self.downloads.iter_mut().find(|d| d.id == id) {
            dl.status = DownloadStatus::InProgress { downloaded, total };
        }
    }

    pub fn on_download_done(&mut self, id: usize) {
        if let Some(dl) = self.downloads.iter_mut().find(|d| d.id == id) {
            dl.status = DownloadStatus::Done;
        }
    }

    pub fn on_download_failed(&mut self, id: usize, reason: String) {
        if let Some(dl) = self.downloads.iter_mut().find(|d| d.id == id) {
            dl.status = DownloadStatus::Failed(reason);
        }
    }

    // ── Search helpers ────────────────────────────────────────────────────────

    pub fn search_next(&mut self) {
        if self.search_results.is_empty() {
            return;
        }
        let i = match self.search_list_state.selected() {
            Some(i) => (i + 1) % self.search_results.len(),
            None => 0,
        };
        self.search_list_state.select(Some(i));
    }

    pub fn search_previous(&mut self) {
        if self.search_results.is_empty() {
            return;
        }
        let i = match self.search_list_state.selected() {
            Some(i) if i == 0 => self.search_results.len() - 1,
            Some(i) => i - 1,
            None => 0,
        };
        self.search_list_state.select(Some(i));
    }

    pub fn toggle_selected_for_download(&mut self) {
        let Some(idx) = self.search_list_state.selected() else {
            return;
        };
        if let Some(pos) = self.selected_for_download.iter().position(|&x| x == idx) {
            self.selected_for_download.remove(pos);
            let filename = self.search_results[idx].filename.clone();
            self.push_log(format!("Dequeued: {filename}"));
        } else {
            self.selected_for_download.push(idx);
            let filename = self.search_results[idx].filename.clone();
            self.push_log(format!("Queued for download: {filename}"));
        }
    }

    // ── Downloads helpers ─────────────────────────────────────────────────────

    pub fn download_next(&mut self) {
        if self.downloads.is_empty() {
            return;
        }
        let i = match self.download_list_state.selected() {
            Some(i) => (i + 1) % self.downloads.len(),
            None => 0,
        };
        self.download_list_state.select(Some(i));
    }

    pub fn download_previous(&mut self) {
        if self.downloads.is_empty() {
            return;
        }
        let i = match self.download_list_state.selected() {
            Some(i) if i == 0 => self.downloads.len() - 1,
            Some(i) => i - 1,
            None => 0,
        };
        self.download_list_state.select(Some(i));
    }

    pub fn remove_selected_download(&mut self) {
        let Some(idx) = self.download_list_state.selected() else {
            return;
        };
        if idx < self.downloads.len() {
            let filename = self.downloads[idx].filename.clone();
            self.downloads.remove(idx);
            self.push_log(format!("Removed download: {filename}"));
            let new_sel = if self.downloads.is_empty() {
                None
            } else {
                Some(idx.saturating_sub(1).min(self.downloads.len() - 1))
            };
            self.download_list_state.select(new_sel);
        }
    }

    // ── Log helpers ───────────────────────────────────────────────────────────

    pub fn log_scroll_down(&mut self) {
        self.log_scroll = self.log_scroll.saturating_add(1);
    }

    pub fn log_scroll_up(&mut self) {
        self.log_scroll = self.log_scroll.saturating_sub(1);
    }
}
