use ratatui::widgets::ListState;

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
#[allow(dead_code)]
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
                write!(f, "{pct}%  ({downloaded}/{total} B)")
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
    pub username: String,
    pub filename: String,
    pub size: u64,
    pub status: DownloadStatus,
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

    // Search tab state
    pub search_input: String,
    pub search_input_mode: SearchInputMode,
    pub search_results: Vec<SearchResult>,
    pub search_list_state: ListState,
    pub selected_for_download: Vec<usize>,

    // Downloads tab state
    pub downloads: Vec<Download>,
    pub download_list_state: ListState,

    // Log pane
    pub log_messages: Vec<String>,
    pub log_scroll: u16,
}

impl App {
    pub fn new() -> Self {
        let mut search_list_state = ListState::default();
        search_list_state.select(None);

        let mut download_list_state = ListState::default();
        download_list_state.select(None);

        Self {
            active_tab: ActiveTab::Search,
            exit: false,
            search_input: String::new(),
            search_input_mode: SearchInputMode::Normal,
            search_results: Vec::new(),
            search_list_state,
            selected_for_download: Vec::new(),
            downloads: Vec::new(),
            download_list_state,
            log_messages: vec![
                "Welcome to slsk-rs — a Soulseek TUI client.".into(),
                "Press '/' to start a search, Tab to switch tabs.".into(),
            ],
            log_scroll: 0,
        }
    }

    pub fn push_log(&mut self, msg: impl Into<String>) {
        self.log_messages.push(msg.into());
        // Keep log from growing unbounded
        if self.log_messages.len() > 500 {
            self.log_messages.drain(0..100);
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

    /// Toggle the currently highlighted result in/out of the download queue.
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

    /// Move all selected results to the downloads list.
    pub fn enqueue_selected_downloads(&mut self) {
        if self.selected_for_download.is_empty() {
            self.push_log("No files selected for download.");
            return;
        }
        let mut count = 0;
        for &idx in &self.selected_for_download {
            if let Some(result) = self.search_results.get(idx) {
                self.downloads.push(Download {
                    username: result.username.clone(),
                    filename: result.filename.clone(),
                    size: result.size,
                    status: DownloadStatus::Queued,
                });
                count += 1;
            }
        }
        self.selected_for_download.clear();
        if self.download_list_state.selected().is_none() && !self.downloads.is_empty() {
            self.download_list_state.select(Some(0));
        }
        self.push_log(format!("Enqueued {count} file(s) for download."));
    }

    pub fn set_search_results(&mut self, results: Vec<SearchResult>) {
        self.search_results = results;
        self.selected_for_download.clear();
        if self.search_results.is_empty() {
            self.search_list_state.select(None);
        } else {
            self.search_list_state.select(Some(0));
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
