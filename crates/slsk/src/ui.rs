use ratatui::{
    Frame,
    layout::{Constraint, Layout, Margin, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span, Text},
    widgets::{
        Block, Borders, List, ListItem, Paragraph, Scrollbar, ScrollbarOrientation,
        ScrollbarState, Tabs, Wrap,
    },
};

use crate::app::{ActiveTab, App, DownloadStatus, LoginStatus, SearchInputMode};

pub fn draw(frame: &mut Frame, app: &mut App) {
    let [tabs_area, content_area, log_area] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Fill(1),
        Constraint::Length(8),
    ])
    .areas(frame.area());

    render_tabs(frame, app, tabs_area);
    render_log(frame, app, log_area);

    match app.active_tab {
        ActiveTab::Search => render_search_tab(frame, app, content_area),
        ActiveTab::Downloads => render_downloads_tab(frame, app, content_area),
    }
}

fn render_tabs(frame: &mut Frame, app: &App, area: Rect) {
    let (status_text, status_color) = match app.login_status {
        LoginStatus::Connecting => ("⧖ Connecting", Color::Yellow),
        LoginStatus::LoggedIn => ("● Online", Color::Green),
        LoginStatus::Failed => ("✗ Login failed", Color::Red),
    };

    let titles = ["Search", "Downloads"];
    let tabs = Tabs::new(titles)
        .block(
            Block::bordered()
                .title(Line::from(vec![
                    " slsk-rs  ".bold(),
                    Span::styled(status_text, Style::default().fg(status_color)),
                    " ".into(),
                ]))
                .title_bottom(
                    Line::from(vec![
                        " Tab ".into(),
                        "◄►".cyan().bold(),
                        "  Quit ".into(),
                        "q".cyan().bold(),
                        " ".into(),
                    ])
                    .centered(),
                ),
        )
        .select(app.active_tab.index())
        .style(Style::default().fg(Color::DarkGray))
        .highlight_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        )
        .divider(" | ");

    frame.render_widget(tabs, area);
}

fn render_search_tab(frame: &mut Frame, app: &mut App, area: Rect) {
    let [input_area, results_area, hints_area] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .areas(area);

    render_search_input(frame, app, input_area);
    render_search_results(frame, app, results_area);
    render_search_hints(frame, app, hints_area);
}

fn render_search_input(frame: &mut Frame, app: &App, area: Rect) {
    let (border_style, title) = match app.search_input_mode {
        SearchInputMode::Editing => (
            Style::default().fg(Color::Cyan),
            " Search (editing — Enter to search, Esc to cancel) ",
        ),
        SearchInputMode::Normal => (Style::default().fg(Color::DarkGray), " Search (press / to type) "),
    };

    let input = Paragraph::new(app.search_input.as_str())
        .block(
            Block::bordered()
                .title(title)
                .border_style(border_style),
        )
        .style(Style::default().fg(Color::White));

    frame.render_widget(input, area);

    if app.search_input_mode == SearchInputMode::Editing {
        let x = area.x + 1 + app.search_input.len() as u16;
        let y = area.y + 1;
        // Keep cursor within the box
        if x < area.x + area.width - 1 {
            frame.set_cursor_position((x, y));
        }
    }
}

fn render_search_results(frame: &mut Frame, app: &mut App, area: Rect) {
    let items: Vec<ListItem> = app
        .search_results
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let is_queued = app.selected_for_download.contains(&i);

            // ── Row 1: checkbox + filename ─────────────────────────────────
            let mark_span = if is_queued {
                Span::styled("[x] ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))
            } else {
                Span::styled("[ ] ", Style::default().fg(Color::DarkGray))
            };

            let filename = r.filename.rsplit(['/', '\\']).next().unwrap_or(&r.filename);
            let filename_style = if is_queued {
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
            };

            let row1 = Line::from(vec![
                mark_span,
                Span::styled(filename, filename_style),
            ]);

            // ── Row 2: metadata ────────────────────────────────────────────
            let size_mb = r.size as f64 / (1024.0 * 1024.0);
            let speed_kbps = r.avg_speed / 1024;

            // Bitrate / format
            let audio_info = match (r.bitrate, r.sample_rate, r.bit_depth) {
                (Some(br), Some(sr), Some(bd)) => {
                    let vbr = if r.is_vbr { " VBR" } else { "" };
                    format!("{br} kbps{vbr} · {sr} Hz · {bd}-bit")
                }
                (Some(br), Some(sr), None) => {
                    let vbr = if r.is_vbr { " VBR" } else { "" };
                    format!("{br} kbps{vbr} · {sr} Hz")
                }
                (Some(br), None, None) => {
                    let vbr = if r.is_vbr { " VBR" } else { "" };
                    format!("{br} kbps{vbr}")
                }
                _ => String::new(),
            };

            // Duration
            let dur_str = r.duration.map(|s| {
                if s >= 3600 {
                    format!("{}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
                } else {
                    format!("{}:{:02}", s / 60, s % 60)
                }
            });

            // Slot / queue
            let (slot_text, slot_color) = if r.slot_free {
                ("▶ ready", Color::Green)
            } else if r.queue_length == 0 {
                ("▶ ready", Color::Green)
            } else {
                let s = format!("⧖ queue: {}", r.queue_length);
                (Box::leak(s.into_boxed_str()) as &str, Color::Yellow)
            };

            let mut meta_spans: Vec<Span> = vec![
                Span::raw("    "),
                Span::styled(
                    format!("{:.1} MB", size_mb),
                    Style::default().fg(Color::Cyan),
                ),
            ];
            if !audio_info.is_empty() {
                meta_spans.push(Span::styled("  ·  ", Style::default().fg(Color::DarkGray)));
                meta_spans.push(Span::styled(audio_info, Style::default().fg(Color::Magenta)));
            }
            if let Some(d) = dur_str {
                meta_spans.push(Span::styled("  ·  ", Style::default().fg(Color::DarkGray)));
                meta_spans.push(Span::styled(d, Style::default().fg(Color::Blue)));
            }
            meta_spans.push(Span::styled("  ·  ", Style::default().fg(Color::DarkGray)));
            meta_spans.push(Span::styled(
                format!("{speed_kbps} KB/s"),
                Style::default().fg(Color::Cyan),
            ));
            meta_spans.push(Span::styled("  ·  ", Style::default().fg(Color::DarkGray)));
            meta_spans.push(Span::styled(
                slot_text,
                Style::default().fg(slot_color),
            ));
            meta_spans.push(Span::styled("  ·  ", Style::default().fg(Color::DarkGray)));
            meta_spans.push(Span::styled(
                r.username.as_str(),
                Style::default().fg(Color::DarkGray),
            ));

            let row2 = Line::from(meta_spans);

            ListItem::new(Text::from(vec![row1, row2]))
        })
        .collect();

    let result_count = app.search_results.len();
    let title = if result_count == 0 {
        " Results ".to_string()
    } else {
        format!(" Results ({result_count}) ")
    };

    let list = List::new(items)
        .block(Block::bordered().title(title))
        .highlight_style(Style::default().bg(Color::DarkGray))
        .highlight_symbol("▶ ");

    frame.render_stateful_widget(list, area, &mut app.search_list_state);
}

fn render_search_hints(frame: &mut Frame, app: &App, area: Rect) {
    let hints = match app.search_input_mode {
        SearchInputMode::Normal => {
            vec![
                Span::styled("/", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw(" search  "),
                Span::styled("↑↓ / j k", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw(" navigate  "),
                Span::styled("Space", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw(" toggle  "),
                Span::styled("Enter", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw(" download selected"),
            ]
        }
        SearchInputMode::Editing => {
            vec![
                Span::styled("Enter", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw(" confirm  "),
                Span::styled("Esc", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw(" cancel"),
            ]
        }
    };
    let _ = app; // suppress unused warning
    frame.render_widget(Paragraph::new(Line::from(hints)), area);
}

fn render_downloads_tab(frame: &mut Frame, app: &mut App, area: Rect) {
    let [list_area, hints_area] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .areas(area);

    render_download_list(frame, app, list_area);
    render_download_hints(frame, hints_area);
}

fn render_download_list(frame: &mut Frame, app: &mut App, area: Rect) {
    let items: Vec<ListItem> = app
        .downloads
        .iter()
        .map(|dl| {
            let status_style = match &dl.status {
                DownloadStatus::Queued => Style::default().fg(Color::Yellow),
                DownloadStatus::PeerQueued { .. } => Style::default().fg(Color::Yellow),
                DownloadStatus::InProgress { .. } => Style::default().fg(Color::Cyan),
                DownloadStatus::Done => Style::default().fg(Color::Green),
                DownloadStatus::Failed(_) => Style::default().fg(Color::Red),
            };

            let filename = dl
                .filename
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or(&dl.filename);

            let label = format!("{} — {} — {}", dl.username, filename, dl.status);
            ListItem::new(label).style(status_style)
        })
        .collect();

    let download_count = app.downloads.len();
    let title = if download_count == 0 {
        " Downloads ".to_string()
    } else {
        format!(" Downloads ({download_count}) ")
    };

    let list = List::new(items)
        .block(Block::bordered().title(title))
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

    frame.render_stateful_widget(list, area, &mut app.download_list_state);
}

fn render_download_hints(frame: &mut Frame, area: Rect) {
    let hints = vec![
        Span::styled("↑↓ / j k", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw(" navigate  "),
        Span::styled("d", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw(" remove selected"),
    ];
    frame.render_widget(Paragraph::new(Line::from(hints)), area);
}

fn render_log(frame: &mut Frame, app: &mut App, area: Rect) {
    let log_height = area.height.saturating_sub(2) as usize;
    let total = app.log_messages.len();

    let max_scroll = total.saturating_sub(log_height);
    if app.log_scroll as usize > max_scroll {
        app.log_scroll = max_scroll as u16;
    }

    let start = (total.saturating_sub(log_height)).saturating_sub(app.log_scroll as usize);
    let visible: Vec<Line> = app.log_messages[start..]
        .iter()
        .take(log_height)
        .map(|msg| Line::from(Span::raw(msg.as_str())))
        .collect();

    let log = Paragraph::new(Text::from(visible))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Log ")
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .wrap(Wrap { trim: false });

    frame.render_widget(log, area);

    if total > log_height {
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight);
        let mut scrollbar_state = ScrollbarState::new(max_scroll).position(max_scroll - app.log_scroll as usize);
        frame.render_stateful_widget(
            scrollbar,
            area.inner(Margin {
                horizontal: 0,
                vertical: 1,
            }),
            &mut scrollbar_state,
        );
    }
}
