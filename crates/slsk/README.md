# slsk

A terminal UI client for the [Soulseek](https://www.slsknet.org/) P2P file-sharing network, built with [ratatui](https://ratatui.rs/) and [tokio](https://tokio.rs/).

## Features

- Search the Soulseek network and browse results with rich metadata (bitrate, sample rate, bit depth, duration, queue status)
- Select multiple files and download them in parallel
- Live download progress tracking with queue position updates
- Scrollable log pane with focus and fullscreen modes
- Async network actor fully decoupled from the UI event loop

## Requirements

- Rust 1.85+ (edition 2024)
- A Soulseek account — register at [slsknet.org](https://www.slsknet.org/)

## Installation

```sh
# From the workspace root
cargo install --path crates/slsk
```

## Configuration

Credentials are read from environment variables:

```sh
export SOULSEEK_USERNAME=youruser
export SOULSEEK_PASSWORD=yourpassword
```

## Running

```sh
cargo run -p slsk
# or, after installation:
slsk
```

Downloaded files are saved to `~/Downloads/slsk-rs/` (or `./slsk-rs/` as a fallback).

## Keybindings

### Global

| Key | Action |
|---|---|
| `q` | Quit |
| `Tab` / `Shift+Tab` | Switch between Search and Downloads tabs |
| `l` | Focus / unfocus the log pane |
| `PageUp` / `PageDown` | Scroll the log pane |

### Search tab

| Key | Action |
|---|---|
| `/` | Enter search mode |
| `Enter` | Confirm search (in search mode) |
| `Esc` | Cancel search (in search mode) |
| `↑` / `↓` or `k` / `j` | Navigate results |
| `Space` | Toggle selection for download |
| `Enter` | Enqueue all selected files for download |

### Downloads tab

| Key | Action |
|---|---|
| `↑` / `↓` or `k` / `j` | Navigate downloads |
| `d` | Remove selected download |

### Log pane (when focused with `l`)

| Key | Action |
|---|---|
| `↑` / `↓` or `k` / `j` | Scroll log |
| `f` | Toggle fullscreen |
| `l` | Unfocus |

## License

MIT
