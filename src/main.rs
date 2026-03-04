/// tppanel — Tmux Plugin Panel
///
/// A full-fledged TUI alternative to TPM (tmux plugin manager).
/// Browse, install, remove, and update tmux plugins and themes.
mod app;
mod config;
mod detect;
mod github;
mod plugins;
mod registry;
mod ui;

use std::io;
use std::time::Duration;

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers, MouseEventKind, MouseButton, EnableMouseCapture, DisableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::*;

use app::{App, Tab};
use registry::Category;

#[tokio::main]
async fn main() -> Result<()> {
    // ── Terminal setup ─────────────────────────────────────────────
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // ── App init ───────────────────────────────────────────────────
    let mut app = App::new();
    app.load_config();
    app.load_registry();

    // Auto-navigate based on binary name (tmuxplugins → Browse, tmuxthemes → Themes)
    if let Some(name) = std::env::current_exe().ok().and_then(|p| p.file_stem().map(|s| s.to_string_lossy().to_lowercase())) {
        if name == "tmuxthemes" {
            app.tab = Tab::Browse;
            app.browse_category_index = 2; // Theme category
            app.browse_category = Some(registry::Category::Theme);
            app.refresh_browse();
        } else if name == "tmuxplugins" {
            app.tab = Tab::Browse;
        }
    }

    // ── Main loop ──────────────────────────────────────────────────
    let result = run_app(&mut terminal, &mut app).await;

    // ── Restore terminal ───────────────────────────────────────────
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;

    if let Err(e) = result {
        eprintln!("Error: {:?}", e);
    }

    Ok(())
}

async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<()> {
    loop {
        // Draw
        terminal.draw(|f| ui::draw(f, app))?;

        if !app.running {
            break;
        }

        // ── Handle preview: launch in isolated session (no permanent install) ──
        if let Some((repo, _cfg_clone, detected)) = app.preview_pending.take() {
            let cfg_snapshot = match app.config {
                Some(ref c) => c.clone(),
                None => {
                    app.set_status_err("No config file — press 'c' to create one first");
                    continue;
                }
            };

            app.set_status(&format!("Launching preview of {}…", repo));
            terminal.draw(|f| ui::draw(f, app))?;

            // Restore terminal for the preview subprocess
            // Restore terminal for the preview subprocess
            disable_raw_mode()?;
            execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
            terminal.show_cursor()?;

            let result = plugins::preview_plugin(&repo, &cfg_snapshot, &detected);

            // Re-enter TUI
            enable_raw_mode()?;
            execute!(terminal.backend_mut(), EnterAlternateScreen, EnableMouseCapture)?;
            terminal.hide_cursor()?;
            terminal.clear()?;

            if result.success {
                app.set_status(&result.message);
            } else {
                app.set_status_err(&result.message);
            }
            continue;
        }

        // Poll events (16ms ≈ 60fps, non-blocking for smooth UI)
        if event::poll(Duration::from_millis(50))? {
            match event::read()? {
                Event::Key(key) => {
                // Only handle key press events (not release/repeat on Windows)
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                // ── Confirmation dialog active ─────────────────────
                if app.confirm.is_some() {
                    handle_confirm_input(app, key.code).await;
                    continue;
                }

                // ── Search mode / Settings edit mode ───────────
                if is_search_editing(app) {
                    handle_search_input(app, key.code, key.modifiers).await;
                    continue;
                }

                if app.tab == Tab::Config && app.settings_editing.is_some() {
                    handle_settings_edit_input(app, key.code);
                    continue;
                }

                // ── Normal mode ────────────────────────────────────
                handle_normal_input(app, key.code, key.modifiers).await;
                }
                Event::Mouse(mouse) => {
                    handle_mouse_event(app, mouse).await;
                }
                _ => {}
            }
        }
    }
    Ok(())
}

fn is_search_editing(app: &App) -> bool {
    match app.tab {
        Tab::Browse => app.browse_search_editing,
        _ => false,
    }
}

// ── Mouse event handler ─────────────────────────────────────────────────

fn hit_test(x: u16, y: u16, region: &Option<(u16, u16, u16, u16)>) -> bool {
    if let Some((rx, ry, rw, rh)) = region {
        x >= *rx && x < rx + rw && y >= *ry && y < ry + rh
    } else {
        false
    }
}

async fn handle_mouse_event(app: &mut App, mouse: crossterm::event::MouseEvent) {
    let x = mouse.column;
    let y = mouse.row;

    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            // ── Confirmation dialog: handle button clicks ──────
            if app.confirm.is_some() {
                if let Some(ref mut dialog) = app.confirm {
                    let center_x = 25u16;
                    dialog.confirm_selected = x > center_x;
                }
                return;
            }

            // ── Tab bar clicks ─────────────────────────────────
            if hit_test(x, y, &app.layout.tabs_area) {
                for (i, rect) in app.layout.tab_rects.iter().enumerate() {
                    if x >= rect.0 && x < rect.0 + rect.2 && y >= rect.1 && y < rect.1 + rect.3 {
                        app.tab = Tab::from_index(i);
                        app.detail_readme = None;
                        app.detail_scroll_offset = 0;
                        return;
                    }
                }
            }

            // ── Dashboard clicks (precise content area) ────────
            if app.tab == Tab::Dashboard {
                if let Some((cx, cy, cw, ch)) = app.layout.dashboard_cards_area {
                    if x >= cx && x < cx + cw && y >= cy && y < cy + ch {
                        let item_idx = ((y - cy) / 2) as usize;
                        let len = app::DashboardItem::ALL.len();
                        if item_idx < len {
                            app.dashboard_selected = item_idx;
                            handle_dashboard_enter(app);
                        }
                    }
                }
                return;
            }

            // ── Category sidebar clicks (Browse tab, precise content area) ──
            if app.tab == Tab::Browse {
                if let Some((cx, cy, cw, ch)) = app.layout.sidebar_content_area {
                    if x >= cx && x < cx + cw && y >= cy && y < cy + ch {
                        let cat_idx = (y - cy) as usize;
                        let max_cats = Category::ALL.len() + 1; // +1 for "All"
                        if cat_idx < max_cats {
                            app.browse_category_index = cat_idx;
                            app.browse_category = if cat_idx == 0 {
                                None
                            } else {
                                Some(Category::ALL[cat_idx - 1])
                            };
                            app.refresh_browse();
                        }
                        return;
                    }
                }
            }

            // ── Plugin list clicks (precise content area, Browse/Installed only) ──
            if matches!(app.tab, Tab::Browse | Tab::Installed) {
                if let Some((cx, cy, cw, ch)) = app.layout.list_content_area {
                    if x >= cx && x < cx + cw && y >= cy && y < cy + ch {
                        let item_idx = ((y - cy) / 2) as usize;
                        let scroll = match app.tab {
                            Tab::Browse => app.browse_scroll_offset,
                            Tab::Installed => app.installed_scroll_offset,
                            _ => 0,
                        };
                        let actual_idx = scroll + item_idx;
                        let len = app.current_list_len();
                        if actual_idx < len {
                            let sel = app.selected_mut();
                            *sel = actual_idx;
                            app.detail_scroll_offset = 0;
                        }
                        return;
                    }
                }
            }

            // ── Detail panel / Action button clicks (Browse/Installed only) ──
            if matches!(app.tab, Tab::Browse | Tab::Installed) {
                if hit_test(x, y, &app.layout.detail_area) {
                    if let Some((dx, dy, _dw, _)) = app.layout.detail_area {
                        let relative_y = y.saturating_sub(dy);
                        // Action buttons at y offset 6-8
                        if relative_y >= 6 && relative_y <= 8 {
                            let relative_x = x.saturating_sub(dx);
                            if let Some(repo) = app.selected_repo() {
                                let is_installed = app.installed_repos.contains(&repo);
                                if is_installed {
                                    if relative_x >= 2 && relative_x < 13 {
                                        handle_mouse_update(app, &repo);
                                    } else if relative_x >= 14 && relative_x < 28 {
                                        handle_mouse_uninstall(app, &repo);
                                    } else if relative_x >= 29 && relative_x < 40 {
                                        handle_mouse_preview(app, &repo);
                                    } else if relative_x >= 41 && relative_x < 52 {
                                        handle_mouse_readme(app, &repo).await;
                                    } else if relative_x >= 53 && app.is_theme_plugin(&repo) {
                                        handle_mouse_activate_theme(app, &repo);
                                    }
                                } else {
                                    if relative_x >= 2 && relative_x < 14 {
                                        handle_mouse_install(app, &repo);
                                    } else if relative_x >= 15 && relative_x < 26 {
                                        handle_mouse_preview(app, &repo);
                                    } else if relative_x >= 27 && relative_x < 38 {
                                        handle_mouse_readme(app, &repo).await;
                                    } else if relative_x >= 39 && app.is_theme_plugin(&repo) {
                                        handle_mouse_activate_theme(app, &repo);
                                    }
                                }
                            }
                        }
                    }
                    return;
                }
            }

            // ── Config tab list clicks (precise settings content area) ──
            if app.tab == Tab::Config {
                if let Some((cx, cy, cw, ch)) = app.layout.settings_content_area {
                    if x >= cx && x < cx + cw && y >= cy && y < cy + ch {
                        let item_idx = ((y - cy) / 2) as usize + app.settings_scroll_offset;
                        let len = app.filtered_settings().len();
                        if item_idx < len {
                            app.settings_selected = item_idx;
                        }
                    }
                }
            }
        }

        MouseEventKind::Down(MouseButton::Right) => {
            if matches!(app.tab, Tab::Browse | Tab::Installed) {
                if let Some(repo) = app.selected_repo() {
                    if app.installed_repos.contains(&repo) {
                        handle_mouse_uninstall(app, &repo);
                    }
                }
            }
        }

        MouseEventKind::ScrollUp => {
            if hit_test(x, y, &app.layout.detail_area) {
                app.detail_scroll_offset = app.detail_scroll_offset.saturating_sub(3);
            } else {
                app.move_selection(-3);
            }
        }

        MouseEventKind::ScrollDown => {
            if hit_test(x, y, &app.layout.detail_area) {
                app.detail_scroll_offset = app.detail_scroll_offset.saturating_add(3);
            } else {
                app.move_selection(3);
            }
        }

        MouseEventKind::Moved | MouseEventKind::Drag(_) => {}

        _ => {}
    }
}

fn handle_mouse_update(app: &mut App, repo: &str) {
    // Clone the plugin data to avoid borrow conflict
    let plugin = app
        .installed_list
        .iter()
        .find(|p| p.repo.as_deref() == Some(repo))
        .cloned();
    if let Some(plugin) = plugin {
        app.set_status(&format!("Updating {}...", repo));
        let result = plugins::update_plugin(&plugin);
        if result.success {
            app.set_status(&result.message);
            app.refresh_installed();
        } else {
            app.set_status_err(&result.message);
        }
    }
}

fn handle_mouse_uninstall(app: &mut App, repo: &str) {
    let name = repo.split('/').last().unwrap_or(repo).to_string();
    app.confirm = Some(app::ConfirmDialog {
        title: "Remove Plugin".to_string(),
        message: format!("Remove '{}' and delete its files?\n\nRepo: {}", name, repo),
        repo: repo.to_string(),
        action: app::ConfirmAction::RemovePlugin,
        confirm_selected: false,
    });
}

fn handle_mouse_preview(app: &mut App, repo: &str) {
    if let Some(ref cfg) = app.config {
        let cfg_clone = cfg.clone();
        let detected = app.detected_muxes.clone();
        app.set_status(&format!("Launching preview of {}...", repo));
        app.preview_pending = Some((repo.to_string(), cfg_clone, detected));
    } else {
        app.set_status_err("No config file — press 'c' to create one first");
    }
}

fn handle_mouse_install(app: &mut App, repo: &str) {
    // Show install confirmation
    let name = repo.split('/').last().unwrap_or(repo).to_string();
    app.confirm = Some(app::ConfirmDialog {
        title: "Install Plugin".to_string(),
        message: format!("Install '{}' from {}?", name, repo),
        repo: repo.to_string(),
        action: app::ConfirmAction::InstallPlugin,
        confirm_selected: true, // default to Confirm for install
    });
}

fn handle_mouse_activate_theme(app: &mut App, repo: &str) {
    // Compat check: don't allow activating themes incompatible with current config
    if let Some(err) = app.compat_error_message(repo) {
        app.set_status_err(&err);
        return;
    }
    let name = repo.split('/').last().unwrap_or(repo).to_string();
    app.confirm = Some(app::ConfirmDialog {
        title: "Activate Theme".to_string(),
        message: format!(
            "Activate '{}' as your theme?\n\nThis will deactivate any other theme.",
            name
        ),
        repo: repo.to_string(),
        action: app::ConfirmAction::ActivateTheme,
        confirm_selected: true,
    });
}

async fn handle_mouse_readme(app: &mut App, repo: &str) {
    if app.detail_readme.is_none() {
        app.detail_readme_loading = true;
        app.set_status(&format!("Fetching README for {}...", repo));
        match github::get_repo_readme(repo).await {
            Ok(readme) => {
                app.detail_readme = Some(readme);
                app.detail_scroll_offset = 0;
                app.set_status("README loaded");
            }
            Err(e) => {
                app.set_status_err(&format!("Failed to fetch README: {}", e));
            }
        }
        app.detail_readme_loading = false;
    }
}

fn handle_settings_edit_input(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Esc => {
            app.settings_editing = None;
            app.settings_edit_buffer.clear();
            app.set_status("Edit cancelled");
        }
        KeyCode::Enter => {
            finish_settings_edit(app);
        }
        KeyCode::Backspace => {
            app.settings_edit_buffer.pop();
        }
        KeyCode::Char(c) => {
            app.settings_edit_buffer.push(c);
        }
        _ => {}
    }
}

// ── Search mode input handler ───────────────────────────────────────────

async fn handle_search_input(app: &mut App, code: KeyCode, _mods: KeyModifiers) {
    match code {
        KeyCode::Esc => {
            match app.tab {
                Tab::Browse => {
                    app.browse_search_editing = false;
                }
                _ => {}
            }
        }
        KeyCode::Enter => {
            match app.tab {
                Tab::Browse => {
                    app.browse_search_editing = false;
                    app.refresh_browse();
                }
                _ => {}
            }
        }
        KeyCode::Backspace => {
            match app.tab {
                Tab::Browse => {
                    app.browse_search.pop();
                    app.refresh_browse();
                }
                _ => {}
            }
        }
        KeyCode::Char(c) => {
            match app.tab {
                Tab::Browse => {
                    app.browse_search.push(c);
                    app.refresh_browse();
                }
                _ => {}
            }
        }
        _ => {}
    }
}

// ── Confirmation dialog handler ─────────────────────────────────────────

async fn handle_confirm_input(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Left | KeyCode::Right | KeyCode::Tab => {
            if let Some(ref mut dialog) = app.confirm {
                dialog.confirm_selected = !dialog.confirm_selected;
            }
        }
        KeyCode::Esc => {
            app.confirm = None;
        }
        KeyCode::Enter => {
            let dialog = app.confirm.take().unwrap();
            if dialog.confirm_selected {
                match dialog.action {
                    app::ConfirmAction::RemovePlugin => {
                        let repo = dialog.repo.clone();
                        if let Some(ref mut cfg) = app.config {
                            let result = plugins::remove_plugin(&repo, cfg);
                            if result.success {
                                app.set_status(&result.message);
                                app.refresh_installed();
                                app.refresh_browse();
                            } else {
                                app.set_status_err(&result.message);
                            }
                        }
                    }
                    app::ConfirmAction::InstallPlugin => {
                        let repo = dialog.repo.clone();
                        app.set_status(&format!("Installing {}...", repo));
                        if let Some(ref mut cfg) = app.config {
                            let result = plugins::install_plugin(&repo, cfg, None);
                            if result.success {
                                app.set_status(&result.message);
                                app.refresh_installed();
                                app.refresh_browse();
                            } else {
                                app.set_status_err(&result.message);
                            }
                        } else {
                            app.set_status_err("No config file found. Press 'c' to create one.");
                        }
                    }
                    app::ConfirmAction::ActivateTheme => {
                        let repo = dialog.repo.clone();
                        app.set_status(&format!("Activating theme {}...", repo));
                        if let Some(ref mut cfg) = app.config {
                            let detected = app.detected_muxes.clone();
                            let result = plugins::activate_theme(&repo, cfg, &detected);
                            if result.success {
                                app.set_status(&result.message);
                                app.refresh_installed();
                                app.refresh_browse();
                            } else {
                                app.set_status_err(&result.message);
                            }
                        } else {
                            app.set_status_err("No config file found. Press 'c' to create one.");
                        }
                    }
                    app::ConfirmAction::ResetEntireConfig => {
                        let reset_result = if let Some(ref mut cfg) = app.config {
                            config::reset_entire_config(cfg).map(|()| {
                                let reload_result = plugins::reload_config(cfg, &app.detected_muxes);
                                let bak = cfg.path.with_extension("conf.bak");
                                let backup_note = if bak.exists() {
                                    format!(" Backup saved to {}", bak.display())
                                } else {
                                    String::new()
                                };
                                let type_label = cfg.type_label().to_string();
                                (reload_result, type_label, backup_note)
                            })
                        } else {
                            Err(anyhow::anyhow!("No config"))
                        };
                        match reset_result {
                            Ok((reload_result, type_label, backup_note)) => {
                                app.refresh_installed();
                                app.refresh_settings();
                                app.refresh_browse();
                                if reload_result.success {
                                    app.set_status(&format!(
                                        "Config reset to {} defaults — all plugins removed. {}.{}",
                                        type_label,
                                        reload_result.message,
                                        backup_note,
                                    ));
                                } else {
                                    app.set_status(&format!(
                                        "Config reset to {} defaults — restart {} to apply.{}",
                                        type_label,
                                        type_label,
                                        backup_note,
                                    ));
                                }
                            }
                            Err(e) => {
                                app.set_status_err(&format!("Reset failed: {}", e));
                            }
                        }
                    }
                    app::ConfirmAction::ResetAllSettings => {
                        if let Some(ref mut cfg) = app.config {
                            match config::reset_all_settings(cfg) {
                                Ok(count) => {
                                    app.set_status(&format!("Reset {} settings to defaults", count));
                                    app.refresh_settings();
                                }
                                Err(e) => {
                                    app.set_status_err(&format!("Reset failed: {}", e));
                                }
                            }
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

// ── Normal mode input handler ───────────────────────────────────────────

async fn handle_normal_input(app: &mut App, code: KeyCode, mods: KeyModifiers) {
    match code {
        // ── Quit ────────────────────────────────────────────
        KeyCode::Char('q') => {
            app.running = false;
        }
        KeyCode::Char('c') if mods.contains(KeyModifiers::CONTROL) => {
            app.running = false;
        }

        // ── Tab switching ───────────────────────────────────
        KeyCode::Tab => {
            let next = (app.tab.index() + 1) % Tab::ALL.len();
            app.tab = Tab::from_index(next);
            app.detail_readme = None;
            app.detail_scroll_offset = 0;
        }
        KeyCode::BackTab => {
            let prev = if app.tab.index() == 0 {
                Tab::ALL.len() - 1
            } else {
                app.tab.index() - 1
            };
            app.tab = Tab::from_index(prev);
            app.detail_readme = None;
            app.detail_scroll_offset = 0;
        }
        KeyCode::Char('1') => { app.tab = Tab::Dashboard; app.detail_readme = None; }
        KeyCode::Char('2') => { app.tab = Tab::Browse; app.detail_readme = None; }
        KeyCode::Char('3') => { app.tab = Tab::Installed; app.detail_readme = None; }
        KeyCode::Char('4') => { app.tab = Tab::Config; app.detail_readme = None; }

        // ── Navigation ──────────────────────────────────────
        KeyCode::Up | KeyCode::Char('k') => {
            app.move_selection(-1);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            app.move_selection(1);
        }
        KeyCode::Home | KeyCode::Char('g') => {
            let sel = app.selected_mut();
            *sel = 0;
            let off = app.scroll_offset_mut();
            *off = 0;
        }
        KeyCode::End | KeyCode::Char('G') => {
            let len = app.current_list_len();
            if len > 0 {
                let sel = app.selected_mut();
                *sel = len - 1;
            }
        }
        KeyCode::PageUp => {
            app.move_selection(-10);
        }
        KeyCode::PageDown => {
            app.move_selection(10);
        }

        // ── Category sidebar (Browse tab) + Settings categories ────
        KeyCode::Left | KeyCode::Char('h') => {
            if app.tab == Tab::Browse {
                if app.browse_category_index > 0 {
                    app.browse_category_index -= 1;
                } else {
                    app.browse_category_index = Category::ALL.len(); // wrap to last
                }
                app.browse_category = if app.browse_category_index == 0 {
                    None
                } else {
                    Some(Category::ALL[app.browse_category_index - 1])
                };
                app.refresh_browse();
            } else if app.tab == Tab::Config {
                // Cycle settings categories left
                let max = crate::config::SettingCategory::ALL.len();
                if app.settings_category_index > 0 {
                    app.settings_category_index -= 1;
                } else {
                    app.settings_category_index = max;
                }
                app.settings_selected = 0;
                app.settings_scroll_offset = 0;
            }
        }
        KeyCode::Right | KeyCode::Char('l') => {
            if app.tab == Tab::Browse {
                let max = Category::ALL.len();
                if app.browse_category_index < max {
                    app.browse_category_index += 1;
                } else {
                    app.browse_category_index = 0; // wrap to All
                }
                app.browse_category = if app.browse_category_index == 0 {
                    None
                } else {
                    Some(Category::ALL[app.browse_category_index - 1])
                };
                app.refresh_browse();
            } else if app.tab == Tab::Config {
                // Cycle settings categories right
                let max = crate::config::SettingCategory::ALL.len();
                if app.settings_category_index < max {
                    app.settings_category_index += 1;
                } else {
                    app.settings_category_index = 0;
                }
                app.settings_selected = 0;
                app.settings_scroll_offset = 0;
            }
        }

        // ── Search ──────────────────────────────────────────
        KeyCode::Char('/') => {
            match app.tab {
                Tab::Browse => {
                    app.browse_search_editing = true;
                }
                _ => {}
            }
        }

        // ── Install (Enter) / Settings toggle / Dashboard action ─
        KeyCode::Enter => {
            if app.tab == Tab::Dashboard {
                handle_dashboard_enter(app);
            } else if app.tab == Tab::Config {
                handle_settings_enter(app);
            } else {
                handle_enter(app).await;
            }
        }

        // ── Reset entire config (Ctrl+D on Config tab) ──────
        KeyCode::Char('d') if mods.contains(KeyModifiers::CONTROL) => {
            if app.tab == Tab::Config && app.config.is_some() {
                let type_label = app.config.as_ref().unwrap().type_label().to_string();
                app.confirm = Some(app::ConfirmDialog {
                    title: "Factory Reset".to_string(),
                    message: format!(
                        "Reset your entire {} config to factory defaults?\n\n\
                         This will:\n\
                         • Remove ALL custom settings\n\
                         • Remove ALL plugin/source-file lines\n\
                         • Delete ALL installed plugin directories from disk\n\
                         • Replace the config with {} built-in defaults\n\
                         • Auto-reload the running {} session\n\n\
                         A backup of your current config will be saved as .conf.bak",
                        type_label, type_label, type_label
                    ),
                    repo: String::new(),
                    action: app::ConfirmAction::ResetEntireConfig,
                    confirm_selected: false,
                });
            }
        }

        // ── Remove (x/d) ───────────────────────────────────
        KeyCode::Char('x') | KeyCode::Char('d') => {
            if let Some(repo) = app.selected_repo() {
                if app.installed_repos.contains(&repo) {
                    let name = repo.split('/').last().unwrap_or(&repo).to_string();
                    app.confirm = Some(app::ConfirmDialog {
                        title: "Remove Plugin".to_string(),
                        message: format!("Remove '{}' and delete its files?\n\nRepo: {}", name, repo),
                        repo,
                        action: app::ConfirmAction::RemovePlugin,
                        confirm_selected: false,
                    });
                }
            }
        }

        // ── Update (u) ─────────────────────────────────────
        KeyCode::Char('u') => {
            if let Some(repo) = app.selected_repo() {
                if app.installed_repos.contains(&repo) {
                    app.set_status(&format!("Updating {}...", repo));
                    if let Some(plugin) = app
                        .installed_list
                        .iter()
                        .find(|p| p.repo.as_deref() == Some(&repo))
                    {
                        let result = plugins::update_plugin(plugin);
                        if result.success {
                            app.set_status(&result.message);
                            app.refresh_installed();
                        } else {
                            app.set_status_err(&result.message);
                        }
                    }
                }
            }
        }

        // ── Update All (U) ─────────────────────────────────
        KeyCode::Char('U') => {
            if app.tab == Tab::Installed && app.config.is_some() {
                app.set_status("Updating all plugins...");
                // Clone config ref to avoid borrow conflict
                let cfg_clone = app.config.clone().unwrap();
                let results = plugins::update_all_plugins(&cfg_clone);
                let ok = results.iter().filter(|r| r.success).count();
                let fail = results.len() - ok;
                app.set_status(&format!("Updated: {} ok, {} failed", ok, fail));
                app.refresh_installed();
            }
        }

        // ── Clean orphaned (C) ──────────────────────────────
        KeyCode::Char('C') => {
            if app.tab == Tab::Installed {
                if let Some(ref mut cfg) = app.config {
                    let results = plugins::clean_orphaned_plugins(cfg);
                    let ct = results.len();
                    app.set_status(&format!("Cleaned {} orphaned plugins", ct));
                }
                app.refresh_installed();
            }
        }

        // ── Activate theme (a) ──────────────────────────────
        KeyCode::Char('a') => {
            if matches!(app.tab, Tab::Browse | Tab::Installed) {
                if let Some(repo) = app.selected_repo() {
                    if app.is_theme_plugin(&repo) {
                        // Compat check: don't allow activating incompatible themes
                        if let Some(err) = app.compat_error_message(&repo) {
                            app.set_status_err(&err);
                        } else {
                            let name = repo.split('/').last().unwrap_or(&repo).to_string();
                            app.confirm = Some(app::ConfirmDialog {
                                title: "Activate Theme".to_string(),
                                message: format!(
                                    "Activate '{}' as your theme?\n\nThis will deactivate any other theme.",
                                    name
                                ),
                                repo,
                                action: app::ConfirmAction::ActivateTheme,
                                confirm_selected: true,
                            });
                        }
                    } else {
                        app.set_status("Not a theme plugin — use 'a' on theme-category plugins");
                    }
                }
            }
        }

        // ── Reload tmux/psmux config (R) ──────────────
        KeyCode::Char('R') => {
            if let Some(ref cfg) = app.config {
                let result = plugins::reload_config(cfg, &app.detected_muxes);
                if result.success {
                    app.set_status(&result.message);
                } else {
                    app.set_status_err(&result.message);
                }
            } else {
                app.set_status_err("No config file loaded to reload");
            }
        }

        // ── Reload config + rescan (r) ──────────────────────
        KeyCode::Char('r') => {
            app.load_config();
            app.set_status("Configuration reloaded");
        }
        // ── Create config if none exists (c) ────────────
        KeyCode::Char('c') => {
            if app.config.is_none() {
                // Determine which type to create based on detected multiplexers
                let config_type = if app.detected_muxes.iter().any(|m| {
                    m.name.to_lowercase().contains("psmux")
                }) {
                    "psmux"
                } else {
                    "tmux"
                };
                match config::create_default_config(config_type) {
                    Ok(cfg) => {
                        app.set_status(&format!("Created config: {}", cfg.display_path()));
                        app.load_config();
                    }
                    Err(e) => {
                        app.set_status_err(&format!("Failed to create config: {}", e));
                    }
                }
            } else if app.all_configs.len() > 1 {
                // Cycle through configs when multiple exist
                app.cycle_config();
            }
        }
        // ── Toggle compat filter (f) ──────────────────
        KeyCode::Char('f') => {
            app.toggle_compat_filter();
        }

        // ── Preview plugin/theme (p) ──────────────────────
        KeyCode::Char('p') => {
            if matches!(app.tab, Tab::Browse | Tab::Installed) {
                if let Some(repo) = app.selected_repo() {
                    if let Some(ref cfg) = app.config {
                        let cfg_clone = cfg.clone();
                        let detected = app.detected_muxes.clone();
                        app.set_status(&format!("Launching preview of {}...", repo));
                        // We need to temporarily leave the TUI for the preview
                        app.preview_pending = Some((repo, cfg_clone, detected));
                    } else {
                        app.set_status_err("No config file — press 'c' to create one first");
                    }
                }
            }
        }

        // ── Reset single setting (Backspace on Config tab) ──
        KeyCode::Backspace => {
            if app.tab == Tab::Config {
                let filtered = app.filtered_settings();
                let sel = app.settings_selected;
                if sel < filtered.len() {
                    let setting = filtered[sel].clone();
                    if !setting.is_default() {
                        if let Some(ref mut cfg) = app.config {
                            match config::reset_setting(cfg, &setting.key) {
                                Ok(()) => {
                                    app.set_status(&format!("{} → reset to default ({})", setting.label, setting.default));
                                    app.refresh_settings();
                                    app.settings_selected = sel;
                                }
                                Err(e) => {
                                    app.set_status_err(&format!("Reset failed: {}", e));
                                }
                            }
                        }
                    } else {
                        app.set_status(&format!("{} is already at default", setting.label));
                    }
                }
            }
        }

        // ── Reset all settings (D on Config tab) ────────────
        KeyCode::Char('D') => {
            if app.tab == Tab::Config {
                app.confirm = Some(app::ConfirmDialog {
                    title: "Reset All Settings".to_string(),
                    message: "Reset ALL settings to their defaults?\n\nPlugin lines will be preserved.".to_string(),
                    repo: String::new(),
                    action: app::ConfirmAction::ResetAllSettings,
                    confirm_selected: false,
                });
            }
        }

        // ── Detail readme scroll ────────────────────────────
        KeyCode::Char('J') => {
            app.detail_scroll_offset = app.detail_scroll_offset.saturating_add(3);
        }
        KeyCode::Char('K') => {
            app.detail_scroll_offset = app.detail_scroll_offset.saturating_sub(3);
        }

        // ── Help ────────────────────────────────────────────
        KeyCode::Char('?') => {
            app.set_status(
                "q:quit Tab:sw 1-4:tabs ↑↓:nav Enter:action x:rm u:upd p:preview /:srch Bksp:reset D:resetAll Ctrl+D:factoryReset R:reload",
            );
        }

        _ => {}
    }
}

/// Handle Enter on Dashboard — execute quick action.
fn handle_dashboard_enter(app: &mut App) {
    use app::DashboardItem;
    let item = DashboardItem::ALL[app.dashboard_selected];
    match item {
        DashboardItem::BrowsePlugins => {
            app.tab = Tab::Browse;
            app.detail_readme = None;
            app.set_status("Browse and install plugins — use ↑↓ to navigate, Enter to install");
        }
        DashboardItem::BrowseThemes => {
            app.tab = Tab::Browse;
            app.browse_category_index = 2; // Theme is Category::ALL[1], so index 2 (1-based in sidebar: 0=All)
            app.browse_category = Some(Category::Theme);
            app.refresh_browse();
            app.detail_readme = None;
            app.set_status("Browsing themes — Enter to install, p to preview");
        }
        DashboardItem::ConfigureSettings => {
            app.tab = Tab::Config;
            app.detail_readme = None;
            app.set_status("Settings — Enter to toggle/edit, Backspace to reset, ←→ categories");
        }
        DashboardItem::ResetToDefaults => {
            if app.config.is_some() {
                let type_label = app.config.as_ref().unwrap().type_label().to_string();
                app.confirm = Some(app::ConfirmDialog {
                    title: "Factory Reset".to_string(),
                    message: format!(
                        "Reset your entire {} config to factory defaults?\n\n\
                         This will:\n\
                         • Remove ALL custom settings\n\
                         • Remove ALL plugin/source-file lines\n\
                         • Delete ALL installed plugin directories from disk\n\
                         • Replace the config with {} built-in defaults\n\
                         • Auto-reload the running {} session\n\n\
                         A backup of your current config will be saved as .conf.bak",
                        type_label, type_label, type_label
                    ),
                    repo: String::new(),
                    action: app::ConfirmAction::ResetEntireConfig,
                    confirm_selected: false,
                });
            } else {
                app.set_status_err("No config file found. Press 'c' in Config tab to create one.");
            }
        }
        DashboardItem::ManageRegistries => {
            app.set_status("Registry sources are configured in registry_sources.json — see REGISTRY_FORMAT.md for details");
        }
    }
}

/// Handle Enter key — install plugin or fetch readme.
async fn handle_enter(app: &mut App) {
    if let Some(repo) = app.selected_repo() {
        if app.installed_repos.contains(&repo) {
            // Already installed — fetch readme
            if app.detail_readme.is_none() {
                app.detail_readme_loading = true;
                app.set_status(&format!("Fetching README for {}...", repo));
                match github::get_repo_readme(&repo).await {
                    Ok(readme) => {
                        app.detail_readme = Some(readme);
                        app.detail_scroll_offset = 0;
                        app.set_status("README loaded");
                    }
                    Err(e) => {
                        app.set_status_err(&format!("Failed to fetch README: {}", e));
                    }
                }
                app.detail_readme_loading = false;
            }
        } else {
            // Not installed — install it (with compat check)
            let compat_ok = {
                let rp = app.get_registry_plugin(&repo);
                if let (Some(rp), Some(filter)) = (rp, app.compat_filter) {
                    rp.is_compatible(filter)
                } else {
                    true // no registry info or no filter → allow
                }
            };
            if !compat_ok {
                let label = app.compat_filter.map(|c| c.label()).unwrap_or("your platform");
                app.set_status_err(&format!(
                    "'{}' is not compatible with {}. Toggle filter (f) to install anyway.",
                    repo, label
                ));
            } else {
                app.set_status(&format!("Installing {}...", repo));
                if let Some(ref mut cfg) = app.config {
                    let result = plugins::install_plugin(&repo, cfg, None);
                    if result.success {
                        app.set_status(&result.message);
                        app.refresh_installed();
                        app.refresh_browse();
                    } else {
                        app.set_status_err(&result.message);
                    }
                } else {
                    app.set_status_err("No config file found. Press 'c' to create one.");
                }
            }
        }
    }
}

/// Handle Enter on the Settings tab — toggle bool, cycle choice, or start editing.
fn handle_settings_enter(app: &mut App) {
    let filtered = app.filtered_settings();
    let sel = app.settings_selected;
    if sel >= filtered.len() {
        return;
    }

    // If currently editing, finish the edit
    if app.settings_editing.is_some() {
        finish_settings_edit(app);
        return;
    }

    let setting = filtered[sel].clone();

    match setting.stype {
        crate::config::SettingType::Bool => {
            // Toggle on/off
            let current = setting.display_value();
            let new_val = match current {
                "on" | "yes" | "true" | "1" => "off",
                _ => "on",
            };
            if let Some(ref mut cfg) = app.config {
                if let Err(e) = crate::config::set_setting(cfg, &setting.key, new_val) {
                    app.set_status_err(&format!("Failed: {}", e));
                    return;
                }
            }
            app.refresh_settings();
            app.settings_selected = sel;
            app.set_status(&format!("{} → {}", setting.label, new_val));
        }
        crate::config::SettingType::Choice => {
            // Cycle to next choice
            let current = setting.display_value().to_string();
            let choices = &setting.choices;
            if choices.is_empty() {
                return;
            }
            let cur_idx = choices.iter().position(|c| c == &current).unwrap_or(0);
            let next_idx = (cur_idx + 1) % choices.len();
            let new_val = &choices[next_idx];
            if let Some(ref mut cfg) = app.config {
                if let Err(e) = crate::config::set_setting(cfg, &setting.key, new_val) {
                    app.set_status_err(&format!("Failed: {}", e));
                    return;
                }
            }
            app.refresh_settings();
            app.settings_selected = sel;
            app.set_status(&format!("{} → {}", setting.label, new_val));
        }
        crate::config::SettingType::Int | crate::config::SettingType::String => {
            // Start inline editing
            app.settings_editing = Some(sel);
            app.settings_edit_buffer = if setting.value.is_empty() {
                setting.default.clone()
            } else {
                setting.value.clone()
            };
        }
    }
}

fn finish_settings_edit(app: &mut App) {
    if let Some(sel) = app.settings_editing.take() {
        let filtered = app.filtered_settings();
        if sel < filtered.len() {
            let key = filtered[sel].key.clone();
            let label = filtered[sel].label.clone();
            let new_val = app.settings_edit_buffer.clone();
            if let Some(ref mut cfg) = app.config {
                if let Err(e) = crate::config::set_setting(cfg, &key, &new_val) {
                    app.set_status_err(&format!("Failed: {}", e));
                    return;
                }
            }
            app.refresh_settings();
            app.settings_selected = sel;
            app.set_status(&format!("{} → {}", label, new_val));
        }
    }
    app.settings_edit_buffer.clear();
}
