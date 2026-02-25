<p align="center">
  <img src="https://img.shields.io/badge/Rust-🦀-orange?style=for-the-badge" alt="Rust">
  <img src="https://img.shields.io/badge/TUI-ratatui-blue?style=for-the-badge" alt="Ratatui">
  <img src="https://img.shields.io/badge/License-MIT-green?style=for-the-badge" alt="MIT License">
</p>

<h1 align="center">⌨️ tppanel — Tmux Plugin Panel</h1>

<p align="center">
  <b>A full-featured TUI plugin manager for tmux — the modern alternative to TPM.</b><br>
  Browse, install, remove, update, and theme your tmux — all from a beautiful terminal interface.
</p>

<p align="center">
  <code>cargo install --path .</code>&nbsp;&nbsp;→&nbsp;&nbsp;<code>tppanel</code>
</p>

<p align="center">
  <img src="screenshot.png" alt="tppanel screenshot" width="900">
</p>

---

## Why tppanel?

[TPM](https://github.com/tmux-plugins/tpm) works, but it's a shell script with no UI, no search, no browsing. **tppanel** gives you a complete graphical plugin manager — think "app store for tmux" — right inside your terminal:

- **Browse** a curated registry of 40+ plugins, sorted by category and stars
- **Search** GitHub for any tmux plugin in real-time
- **One-key install/remove/update** — no editing config files manually
- **Theme gallery** — preview and switch tmux themes instantly
- **Settings editor** — toggle tmux options (mouse, status bar, keybindings) without memorizing `set -g` syntax
- **Auto-detection** — finds tmux (and PSMux) installations, versions, and config files automatically
- **Config management** — creates, parses, and updates `tmux.conf` / `psmux.conf` for you
- **Offline fallback** — ships with an embedded plugin registry; works without internet

## Features at a Glance

| Tab | What it does |
|-----|-------------|
| **Browse** | Category sidebar + plugin list + detail panel. Search, filter by compat, install with Enter. |
| **Installed** | See all installed plugins. Update one (`u`), update all (`U`), remove (`x`), clean orphans (`C`). |
| **Themes** | Dedicated gallery for tmux themes. Install, activate, switch — one keypress. |
| **Config** | Full settings editor. Toggle booleans, cycle choices, edit values inline. Grouped by category. |

## Quick Start

### Prerequisites

- **Rust** toolchain (1.70+): [rustup.rs](https://rustup.rs)
- **tmux** installed on your system (or PSMux)
- **git** (for cloning plugins)

### Install

```bash
# Clone and build
git clone https://github.com/marlocarlo/Tmux-Plugin-Panel.git
cd Tmux-Plugin-Panel
cargo build --release

# Run it
./target/release/tppanel
```

Or install directly:

```bash
cargo install --path .
tppanel
```

### First Run

1. **tppanel** auto-detects your tmux installation and config file
2. If no `tmux.conf` exists, press `c` to create one
3. Browse plugins → press `Enter` to install
4. Press `R` to reload tmux with your new plugins

## Keybindings

| Key | Action |
|-----|--------|
| `q` | Quit |
| `Tab` / `Shift+Tab` | Switch tabs |
| `1`–`4` | Jump to tab |
| `↑` `↓` / `j` `k` | Navigate list |
| `←` `→` / `h` `l` | Switch category (Browse) / settings group (Config) |
| `Enter` | Install plugin / toggle setting / fetch README |
| `x` / `d` | Remove plugin (with confirmation) |
| `u` | Update selected plugin |
| `U` | Update **all** plugins |
| `C` | Clean orphaned plugins |
| `/` | Search plugins |
| `f` | Toggle compat filter (tmux / psmux) |
| `r` | Rescan config |
| `R` | Reload tmux config (`tmux source-file`) |
| `c` | Create config / cycle configs |
| `J` / `K` | Scroll README detail |
| `?` | Show help |

## Architecture

```
src/
├── main.rs      # Entry point, terminal setup, event loop
├── app.rs       # Application state machine (tabs, selections, data)
├── ui.rs        # TUI rendering with ratatui (all 4 tabs + overlays)
├── registry.rs  # Plugin registry — remote fetch, local cache, embedded fallback
├── plugins.rs   # Git-based install/remove/update engine
├── themes.rs    # Theme management (install, activate, switch)
├── config.rs    # tmux.conf / psmux.conf parser & editor
├── detect.rs    # Auto-detection of tmux/PSMux binaries & configs
└── github.rs    # GitHub API client for search & repo info
```

## Plugin Registry

tppanel ships with a curated registry of popular tmux plugins covering:

- ⭐ **Essential** — TPM, tmux-sensible, tmux-256color
- 🎨 **Themes** — Catppuccin, Dracula, Nord, Tokyo Night, Rose Pine, and more
- 💾 **Session** — tmux-resurrect, tmux-continuum
- 🧭 **Navigation** — vim-tmux-navigator, tmux-fzf
- 📊 **Status Bar** — tmux-cpu, tmux-battery, tmux-net-speed
- 📋 **Clipboard** — tmux-yank
- 🔧 **Utility** — tmux-fingers, tmux-open, tmux-logging

The registry is fetched from GitHub on startup and cached locally. An embedded copy is compiled into the binary as a fallback.

### Custom Registry

Point tppanel at your own registry by hosting a JSON file matching the schema in `registry.json` and modifying the `REGISTRY_URL` constant.

## Tech Stack

- **[Rust](https://www.rust-lang.org/)** — safe, fast, no runtime
- **[ratatui](https://ratatui.rs/)** — modern terminal UI framework
- **[crossterm](https://github.com/crossterm-rs/crossterm)** — cross-platform terminal manipulation
- **[tokio](https://tokio.rs/)** — async runtime for GitHub API calls
- **[reqwest](https://docs.rs/reqwest/)** — HTTP client

## Configuration

tppanel uses the standard TPM plugin syntax in your tmux config:

```bash
# ~/.tmux.conf
set -g @plugin 'tmux-plugins/tmux-sensible'
set -g @plugin 'catppuccin/tmux'
set -g @plugin 'tmux-plugins/tmux-resurrect'
```

Plugins are installed to `~/.tmux/plugins/` by default.

## Environment Variables

| Variable | Purpose |
|----------|---------|
| `GITHUB_TOKEN` or `GH_TOKEN` | Authenticate GitHub API requests (higher rate limits) |

## License

MIT — see [Cargo.toml](Cargo.toml).

## Contributing

Contributions welcome! Feel free to open issues or submit pull requests.

1. Fork the repo
2. Create a feature branch
3. Make your changes
4. Submit a PR

---

<p align="center">
  Built with 🦀 Rust and ❤️ for the terminal
</p>
