mod app;
mod events;
mod network;
mod ui;

use std::{path::PathBuf, time::Duration};

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use tokio::sync::mpsc::unbounded_channel;

use app::{App, LoginStatus, SearchInputMode};
use events::{AppEvent, NetCommand};

fn download_dir() -> PathBuf {
    dirs::download_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("slsk-rs")
}

fn credentials() -> (String, String) {
    let username = std::env::var("SOULSEEK_USERNAME").unwrap_or_default();
    let password = std::env::var("SOULSEEK_PASSWORD").unwrap_or_default();
    (username, password)
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let (username, password) = credentials();

    // Channel: network → UI events
    let (event_tx, mut event_rx) = unbounded_channel::<AppEvent>();
    // Channel: UI → network commands
    let (cmd_tx, cmd_rx) = unbounded_channel::<NetCommand>();

    // Spawn the network actor.
    let dd = download_dir();
    tokio::spawn(network::run(username, password, event_tx, cmd_rx, dd));

    // Initialise ratatui.
    let mut terminal = ratatui::init();
    let mut app = App::new(cmd_tx);

    let result = run(&mut terminal, &mut app, &mut event_rx).await;

    ratatui::restore();
    result
}

async fn run(
    terminal: &mut ratatui::DefaultTerminal,
    app: &mut App,
    event_rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
) -> std::io::Result<()> {
    loop {
        terminal.draw(|frame| ui::draw(frame, app))?;

        // Drain all pending network events (non-blocking).
        while let Ok(ev) = event_rx.try_recv() {
            handle_app_event(app, ev);
        }

        // Poll for a terminal keyboard event with a short timeout so the
        // network events above are re-checked regularly.
        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                match app.search_input_mode {
                    SearchInputMode::Editing => handle_editing_key(app, key.code),
                    SearchInputMode::Normal => {
                        if handle_global_key(app, key.code, key.modifiers) {
                            break;
                        }
                    }
                }
            }
        }

        if app.exit {
            break;
        }
    }

    Ok(())
}

// ── Network event → App state ─────────────────────────────────────────────────

fn handle_app_event(app: &mut App, ev: AppEvent) {
    match ev {
        AppEvent::LoginOk { greet } => {
            app.login_status = LoginStatus::LoggedIn;
            app.push_log(format!("Logged in: {greet}"));
        }
        AppEvent::LoginFailed { reason } => {
            app.login_status = LoginStatus::Failed;
            app.push_log(format!("Login failed: {reason}"));
        }
        AppEvent::SearchResults { token, results } => {
            let count = results.len();
            app.on_search_results(token, results);
            app.push_log(format!(
                "+{count} result(s)  (total: {})",
                app.search_results.len()
            ));
        }
        AppEvent::DownloadProgress {
            id,
            downloaded,
            total,
        } => {
            app.on_download_progress(id, downloaded, total);
        }
        AppEvent::DownloadDone { id } => {
            app.on_download_done(id);
            if let Some(dl) = app.downloads.iter().find(|d| d.id == id) {
                app.push_log(format!("Download complete: {}", dl.filename));
            }
        }
        AppEvent::DownloadFailed { id, reason } => {
            app.on_download_failed(id, reason.clone());
            app.push_log(format!("Download failed (id={id}): {reason}"));
        }
        AppEvent::TransferDenied { id, reason } => {
            app.on_download_failed(id, format!("denied: {reason}"));
            app.push_log(format!("Transfer denied (id={id}): {reason}"));
        }
        AppEvent::QueuePosition { id, position } => {
            app.on_queue_position(id, position);
        }
        AppEvent::Log(msg) => {
            app.push_log(msg);
        }
    }
}

// ── Keyboard handlers ─────────────────────────────────────────────────────────

fn handle_editing_key(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Esc => {
            app.search_input_mode = SearchInputMode::Normal;
            app.push_log("Search cancelled.");
        }
        KeyCode::Enter => {
            let query = app.search_input.trim().to_string();
            if query.is_empty() {
                app.push_log("Search query is empty.");
            } else {
                app.push_log(format!("Searching for: {query}"));
                app.send_search(query);
            }
            app.search_input_mode = SearchInputMode::Normal;
        }
        KeyCode::Backspace => {
            app.search_input.pop();
        }
        KeyCode::Char(c) => {
            app.search_input.push(c);
        }
        _ => {}
    }
}

/// Returns `true` when the application should quit.
fn handle_global_key(app: &mut App, code: KeyCode, _modifiers: KeyModifiers) -> bool {
    match code {
        KeyCode::Char('q') | KeyCode::Char('Q') => {
            app.exit = true;
            return true;
        }

        KeyCode::Tab => {
            app.active_tab = app.active_tab.next();
        }
        KeyCode::BackTab => {
            app.active_tab = app.active_tab.previous();
        }

        KeyCode::Down | KeyCode::Char('j') => match app.active_tab {
            app::ActiveTab::Search => app.search_next(),
            app::ActiveTab::Downloads => app.download_next(),
        },
        KeyCode::Up | KeyCode::Char('k') => match app.active_tab {
            app::ActiveTab::Search => app.search_previous(),
            app::ActiveTab::Downloads => app.download_previous(),
        },

        KeyCode::Char('/') if matches!(app.active_tab, app::ActiveTab::Search) => {
            app.search_input_mode = SearchInputMode::Editing;
        }
        KeyCode::Char(' ') if matches!(app.active_tab, app::ActiveTab::Search) => {
            app.toggle_selected_for_download();
        }
        KeyCode::Enter if matches!(app.active_tab, app::ActiveTab::Search) => {
            app.enqueue_selected_downloads();
        }

        KeyCode::Char('d') if matches!(app.active_tab, app::ActiveTab::Downloads) => {
            app.remove_selected_download();
        }

        KeyCode::PageDown => app.log_scroll_down(),
        KeyCode::PageUp => app.log_scroll_up(),

        _ => {}
    }

    false
}
