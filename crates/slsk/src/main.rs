mod app;
mod ui;

use std::io;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};

use app::{App, SearchInputMode};

fn run(terminal: &mut ratatui::DefaultTerminal) -> io::Result<()> {
    let mut app = App::new();

    loop {
        terminal.draw(|frame| ui::draw(frame, &mut app))?;

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                match app.search_input_mode {
                    SearchInputMode::Editing => handle_editing_key(&mut app, key.code),
                    SearchInputMode::Normal => {
                        if handle_global_key(&mut app, key.code, key.modifiers) {
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
                perform_mock_search(app, &query);
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

        // Tab switching
        KeyCode::Tab => {
            app.active_tab = app.active_tab.next();
        }
        KeyCode::BackTab => {
            app.active_tab = app.active_tab.previous();
        }

        // Per-tab navigation
        KeyCode::Down | KeyCode::Char('j') => match app.active_tab {
            app::ActiveTab::Search => app.search_next(),
            app::ActiveTab::Downloads => app.download_next(),
        },
        KeyCode::Up | KeyCode::Char('k') => match app.active_tab {
            app::ActiveTab::Search => app.search_previous(),
            app::ActiveTab::Downloads => app.download_previous(),
        },

        // Search tab specific
        KeyCode::Char('/') if matches!(app.active_tab, app::ActiveTab::Search) => {
            app.search_input_mode = SearchInputMode::Editing;
        }
        KeyCode::Char(' ') if matches!(app.active_tab, app::ActiveTab::Search) => {
            app.toggle_selected_for_download();
        }
        KeyCode::Enter if matches!(app.active_tab, app::ActiveTab::Search) => {
            app.enqueue_selected_downloads();
        }

        // Downloads tab specific
        KeyCode::Char('d') if matches!(app.active_tab, app::ActiveTab::Downloads) => {
            app.remove_selected_download();
        }

        // Log scrolling (available on all tabs)
        KeyCode::PageDown => app.log_scroll_down(),
        KeyCode::PageUp => app.log_scroll_up(),

        _ => {}
    }

    false
}

/// Simulated search — replace with real Soulseek protocol calls.
fn perform_mock_search(app: &mut App, query: &str) {
    use app::SearchResult;

    let results = vec![
        SearchResult {
            username: "peer_alpha".into(),
            filename: format!("Music/{query} - Track 01.mp3"),
            size: 8_432_640,
            extension: "mp3".into(),
        },
        SearchResult {
            username: "peer_alpha".into(),
            filename: format!("Music/{query} - Track 02.mp3"),
            size: 7_120_000,
            extension: "mp3".into(),
        },
        SearchResult {
            username: "peer_beta".into(),
            filename: format!("Shared/{query} (FLAC).flac"),
            size: 42_000_000,
            extension: "flac".into(),
        },
        SearchResult {
            username: "peer_gamma".into(),
            filename: format!("Downloads/{query} Live.mp3"),
            size: 9_800_000,
            extension: "mp3".into(),
        },
        SearchResult {
            username: "peer_delta".into(),
            filename: format!("Music/{query} Remaster.flac"),
            size: 55_000_000,
            extension: "flac".into(),
        },
    ];

    let count = results.len();
    app.set_search_results(results);
    app.push_log(format!("Found {count} result(s) for '{query}'."));
}

fn main() -> io::Result<()> {
    let mut terminal = ratatui::init();
    let result = run(&mut terminal);
    ratatui::restore();
    result
}
