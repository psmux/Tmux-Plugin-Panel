/// TUI rendering with ratatui.
///
/// Draws all tabs, the overlay dialogs, status bar, and keybinding footer.
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Clear, List, ListItem, Paragraph, Tabs, Wrap,
    },
    Frame,
};

use crate::app::{App, ConfirmDialog, Tab};
use crate::config::{SettingCategory, TmuxConfig};
use crate::registry::Category;

// ── Color palette (tmux default) ────────────────────────────────────────

const BG: Color = Color::Rgb(0, 0, 0);              // Terminal black
const BG_DARK: Color = Color::Rgb(0, 0, 0);         // Pure black
const BG_LIGHT: Color = Color::Rgb(28, 28, 28);     // Very dark gray
const BG_HIGHLIGHT: Color = Color::Rgb(0, 56, 0);   // Dark green selection
const ACCENT: Color = Color::Rgb(0, 154, 0);        // tmux green
const ACCENT2: Color = Color::Rgb(85, 255, 85);     // Bright green
const TEXT: Color = Color::Rgb(208, 208, 208);       // Terminal light gray
const TEXT_DIM: Color = Color::Rgb(118, 118, 118);   // Medium gray
const TEXT_DARK: Color = Color::Rgb(58, 58, 58);     // Dark gray
const GREEN: Color = Color::Rgb(0, 175, 0);         // Green
const RED: Color = Color::Rgb(204, 0, 0);           // Terminal red
const YELLOW: Color = Color::Rgb(204, 170, 0);      // Terminal amber
const BLUE: Color = Color::Rgb(85, 170, 255);       // Terminal cyan-blue

// ── Main draw ───────────────────────────────────────────────────────────

/// Ensure scroll_offset keeps the selected index visible.
/// `lines_per_item` is how many terminal rows each list row occupies (usually 2).
fn ensure_scroll_visible(
    selected: usize,
    scroll_offset: &mut usize,
    visible_height: usize,
    lines_per_item: usize,
) {
    let items_visible = if lines_per_item > 0 {
        visible_height / lines_per_item
    } else {
        visible_height
    };
    if items_visible == 0 {
        return;
    }
    // Scroll down if selection is below viewport
    if selected >= *scroll_offset + items_visible {
        *scroll_offset = selected.saturating_sub(items_visible - 1);
    }
    // Scroll up if selection is above viewport
    if selected < *scroll_offset {
        *scroll_offset = selected;
    }
}

pub fn draw(f: &mut Frame, app: &mut App) {
    let size = f.area();

    // Overall layout: header(2) | divider(1) | tabs(3) | body | status(1) | footer(1)
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),  // header (taller for breathing room)
            Constraint::Length(1),  // divider line
            Constraint::Length(3),  // tabs
            Constraint::Min(10),   // body
            Constraint::Length(1),  // status
            Constraint::Length(1),  // footer
        ])
        .split(size);

    draw_header(f, outer[0], app);
    draw_header_divider(f, outer[1]);
    draw_tabs(f, outer[2], app);

    // Store layout regions for mouse hit-testing
    let r = outer[2];
    app.layout.tabs_area = Some((r.x, r.y, r.width, r.height));
    let r = outer[3];
    app.layout.body_area = Some((r.x, r.y, r.width, r.height));

    match app.tab {
        Tab::Dashboard => draw_dashboard_tab(f, outer[3], app),
        Tab::Browse => draw_browse_tab(f, outer[3], app),
        Tab::Installed => draw_installed_tab(f, outer[3], app),
        Tab::Config => draw_config_tab(f, outer[3], app),
    }

    draw_status(f, outer[4], app);
    draw_footer(f, outer[5], app);

    // Draw confirmation overlay
    if let Some(dialog) = &app.confirm {
        draw_confirm_dialog(f, size, dialog);
    }
}

// ── Header ──────────────────────────────────────────────────────────────

fn draw_header(f: &mut Frame, area: Rect, app: &App) {
    let mut spans = vec![
        Span::styled(" ", Style::default().bg(BG_DARK)),
        Span::styled(" TPP ", Style::default().fg(Color::Black).bg(ACCENT).bold()),
        Span::styled("  Tmux Plugin Panel", Style::default().fg(TEXT_DIM)),
    ];

    // Show detected multiplexers on the right
    if !app.detected_muxes.is_empty() {
        let mux_info: String = app
            .detected_muxes
            .iter()
            .map(|m| format!("{} {}", m.name, m.version))
            .collect::<Vec<_>>()
            .join(" · ");
        // Pad to push to right
        let used = 26; // approximate length of left side
        let pad = (area.width as usize).saturating_sub(used + mux_info.len() + 2);
        spans.push(Span::styled(
            " ".repeat(pad),
            Style::default().bg(BG_DARK),
        ));
        spans.push(Span::styled(mux_info, Style::default().fg(ACCENT2)));
    }

    let header = Paragraph::new(vec![
        Line::from(""), // top padding
        Line::from(spans),
    ])
    .style(Style::default().bg(BG_DARK));
    f.render_widget(header, area);
}

/// Thin divider line between header and tabs
fn draw_header_divider(f: &mut Frame, area: Rect) {
    let divider_str = "─".repeat(area.width as usize);
    let divider = Paragraph::new(Span::styled(
        divider_str,
        Style::default().fg(TEXT_DARK),
    ))
    .style(Style::default().bg(BG_DARK));
    f.render_widget(divider, area);
}

// ── Tab bar ─────────────────────────────────────────────────────────────

fn draw_tabs(f: &mut Frame, area: Rect, app: &mut App) {
    let titles: Vec<Line> = Tab::ALL
        .iter()
        .map(|t| {
            let style = if *t == app.tab {
                Style::default().fg(ACCENT).bold()
            } else {
                Style::default().fg(TEXT_DIM)
            };
            Line::from(Span::styled(t.label(), style))
        })
        .collect();

    // Calculate tab hit regions for mouse clicks
    let mut tab_rects = Vec::new();
    let mut x_offset = area.x + 1; // account for block border
    let divider_width = 3u16; // " │ "
    for tab in Tab::ALL {
        let label_width = tab.label().len() as u16;
        tab_rects.push((x_offset, area.y, label_width, area.height));
        x_offset += label_width + divider_width;
    }
    app.layout.tab_rects = tab_rects;

    let tabs = Tabs::new(titles)
        .select(app.tab.index())
        .highlight_style(Style::default().fg(ACCENT).bold().underlined())
        .divider(Span::styled(" │ ", Style::default().fg(TEXT_DARK)))
        .block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(TEXT_DARK))
                .style(Style::default().bg(BG_DARK)),
        );
    f.render_widget(tabs, area);
}

// ── Dashboard tab ───────────────────────────────────────────────────────

fn draw_dashboard_tab(f: &mut Frame, area: Rect, app: &App) {
    use crate::app::DashboardItem;

    let block = Block::default()
        .borders(Borders::NONE)
        .style(Style::default().bg(BG));
    let inner = block.inner(area);
    f.render_widget(block, area);

    // Split: left(info panel) | center(action cards) | right(quick ref)
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(40),
            Constraint::Length(2),
        ])
        .split(inner);

    let center = cols[1];

    // Vertical layout: welcome | cards | system info | tips
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),   // welcome banner
            Constraint::Length(1),   // spacer
            Constraint::Min(12),    // action cards
            Constraint::Length(1),   // spacer
            Constraint::Length(6),   // system info
            Constraint::Length(3),   // quick reference
        ])
        .split(center);

    // ── Welcome banner ────────────────────────────────
    let welcome_lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  Welcome to ", Style::default().fg(TEXT)),
            Span::styled("Tmux Plugin Panel", Style::default().fg(ACCENT).bold()),
            Span::styled(" — your all-in-one app store", Style::default().fg(TEXT)),
        ]),
        Line::from(Span::styled(
            "  Browse plugins, apply themes, configure settings, and reset to defaults.",
            Style::default().fg(TEXT_DIM),
        )),
    ];
    let welcome = Paragraph::new(welcome_lines).style(Style::default().bg(BG));
    f.render_widget(welcome, rows[0]);

    // ── Action cards ──────────────────────────────────
    let cards_area = rows[2];
    let items: Vec<ListItem> = DashboardItem::ALL
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let is_sel = i == app.dashboard_selected;
            let bg = if is_sel { BG_HIGHLIGHT } else { BG_LIGHT };

            let icon_style = if is_sel {
                Style::default().fg(ACCENT2).bold()
            } else {
                Style::default().fg(ACCENT)
            };
            let label_style = if is_sel {
                Style::default().fg(ACCENT2).bold()
            } else {
                Style::default().fg(TEXT).bold()
            };
            let desc_style = Style::default().fg(TEXT_DIM);

            let pointer = if is_sel { "▶ " } else { "  " };
            let pointer_style = if is_sel {
                Style::default().fg(ACCENT2).bold()
            } else {
                Style::default().fg(TEXT_DARK)
            };

            let line1 = Line::from(vec![
                Span::styled(pointer, pointer_style),
                Span::styled(format!("{}  ", item.icon()), icon_style),
                Span::styled(item.label(), label_style),
            ]);
            let line2 = Line::from(vec![
                Span::styled("     ", Style::default()),
                Span::styled(item.description(), desc_style),
            ]);

            ListItem::new(vec![line1, line2]).style(Style::default().bg(bg))
        })
        .collect();

    let cards = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(TEXT_DARK))
                .title(Span::styled(" Quick Actions ", Style::default().fg(ACCENT).bold()))
                .style(Style::default().bg(BG)),
        );
    f.render_widget(cards, cards_area);

    // ── System info ───────────────────────────────────
    let mut info_lines: Vec<Line> = Vec::new();

    // Multiplexer info
    if !app.detected_muxes.is_empty() {
        let mux_info: String = app
            .detected_muxes
            .iter()
            .map(|m| format!("{} v{}", m.name, m.version))
            .collect::<Vec<_>>()
            .join("  ·  ");
        info_lines.push(Line::from(vec![
            Span::styled("  System: ", Style::default().fg(TEXT_DIM)),
            Span::styled(mux_info, Style::default().fg(GREEN)),
        ]));
    } else {
        info_lines.push(Line::from(Span::styled(
            "  System: No multiplexer detected — install tmux or PSMux",
            Style::default().fg(YELLOW),
        )));
    }

    // Config info
    if let Some(cfg) = &app.config {
        info_lines.push(Line::from(vec![
            Span::styled("  Config: ", Style::default().fg(TEXT_DIM)),
            Span::styled(
                format!("[{}] {}", cfg.type_label(), cfg.display_path()),
                Style::default().fg(TEXT),
            ),
            Span::styled(
                format!("  ·  {} plugins installed", cfg.plugins.len()),
                Style::default().fg(GREEN),
            ),
        ]));
    } else {
        info_lines.push(Line::from(Span::styled(
            "  Config: None found — select 'Configure Settings' to create one",
            Style::default().fg(YELLOW),
        )));
    }

    // Registry info
    info_lines.push(Line::from(vec![
        Span::styled("  Registry: ", Style::default().fg(TEXT_DIM)),
        Span::styled(
            format!("{} plugins available", app.registry.len()),
            Style::default().fg(TEXT),
        ),
        Span::styled(
            format!("  ·  {} shown with current filter", app.browse_list.len()),
            Style::default().fg(TEXT_DIM),
        ),
    ]));

    let info_block = Paragraph::new(info_lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(TEXT_DARK))
                .title(Span::styled(" System Info ", Style::default().fg(ACCENT).bold()))
                .style(Style::default().bg(BG)),
        );
    f.render_widget(info_block, rows[4]);

    // ── Quick reference ───────────────────────────────
    let quick_ref = Paragraph::new(vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  ↑↓", Style::default().fg(ACCENT).bold()),
            Span::styled(" Navigate   ", Style::default().fg(TEXT_DIM)),
            Span::styled("Enter", Style::default().fg(ACCENT).bold()),
            Span::styled(" Select   ", Style::default().fg(TEXT_DIM)),
            Span::styled("Tab", Style::default().fg(ACCENT).bold()),
            Span::styled(" Switch Tab   ", Style::default().fg(TEXT_DIM)),
            Span::styled("q", Style::default().fg(ACCENT).bold()),
            Span::styled(" Quit   ", Style::default().fg(TEXT_DIM)),
            Span::styled("?", Style::default().fg(ACCENT).bold()),
            Span::styled(" Help", Style::default().fg(TEXT_DIM)),
        ]),
    ])
    .style(Style::default().bg(BG));
    f.render_widget(quick_ref, rows[5]);
}

// ── Browse tab ──────────────────────────────────────────────────────────

fn draw_browse_tab(f: &mut Frame, area: Rect, app: &mut App) {
    // Split: sidebar(20) | list | detail
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(20),
            Constraint::Percentage(35),
            Constraint::Min(30),
        ])
        .split(area);

    draw_category_sidebar(f, cols[0], app);
    // Adjust scroll to keep selection visible (2 lines per item, minus search bar)
    let list_inner_h = cols[1].height.saturating_sub(4) as usize; // border + search bar
    ensure_scroll_visible(app.browse_selected, &mut app.browse_scroll_offset, list_inner_h, 2);
    draw_plugin_list(f, cols[1], app, &app.browse_list, app.browse_selected, app.browse_scroll_offset, true);
    draw_detail_panel(f, cols[2], app);

    // Store layout regions for mouse
    let r = cols[0];
    app.layout.sidebar_area = Some((r.x, r.y, r.width, r.height));
    let r = cols[1];
    app.layout.list_area = Some((r.x, r.y, r.width, r.height));
    let r = cols[2];
    app.layout.detail_area = Some((r.x, r.y, r.width, r.height));
}

fn draw_category_sidebar(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(TEXT_DARK))
        .style(Style::default().bg(BG_DARK));
    let inner = block.inner(area);
    f.render_widget(block, area);

    // Title + categories
    let mut items: Vec<ListItem> = Vec::new();

    // "All" entry
    let all_style = if app.browse_category_index == 0 {
        Style::default().fg(ACCENT).bold().bg(BG_HIGHLIGHT)
    } else {
        Style::default().fg(TEXT_DIM)
    };
    items.push(ListItem::new(Line::from(Span::styled(
        " 📦 All",
        all_style,
    ))));

    for (i, cat) in Category::ALL.iter().enumerate() {
        let style = if app.browse_category_index == i + 1 {
            Style::default().fg(ACCENT).bold().bg(BG_HIGHLIGHT)
        } else {
            Style::default().fg(TEXT_DIM)
        };
        items.push(ListItem::new(Line::from(Span::styled(
            format!(" {} {}", cat.icon(), cat.label()),
            style,
        ))));
    }

    let list = List::new(items).block(
        Block::default()
            .title(Span::styled(" CATEGORIES ", Style::default().fg(ACCENT).bold()))
            .borders(Borders::NONE)
            .style(Style::default().bg(BG_DARK)),
    );
    f.render_widget(list, inner);
}

fn draw_plugin_list(
    f: &mut Frame,
    area: Rect,
    app: &App,
    plugins: &[crate::registry::RegistryPlugin],
    selected: usize,
    scroll_offset: usize,
    show_search: bool,
) {
    let block = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(TEXT_DARK))
        .style(Style::default().bg(BG));

    let inner = block.inner(area);
    f.render_widget(block, area);

    // Search bar + list
    let layout = if show_search {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(1)])
            .split(inner)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(0), Constraint::Min(1)])
            .split(inner)
    };

    if show_search {
        let search_style = if app.browse_search_editing {
            Style::default().fg(ACCENT)
        } else {
            Style::default().fg(TEXT_DIM)
        };
        let search_text = if app.browse_search.is_empty() {
            "  / Search plugins...".to_string()
        } else {
            format!("  / {}", app.browse_search)
        };
        let search_bar = Paragraph::new(search_text)
            .style(search_style)
            .block(
                Block::default()
                    .borders(Borders::BOTTOM)
                    .border_style(Style::default().fg(TEXT_DARK))
                    .style(Style::default().bg(BG_LIGHT)),
            );
        f.render_widget(search_bar, layout[0]);
    }

    // Plugin items
    let list_area = layout[1];
    let visible_height = list_area.height as usize;

    let items: Vec<ListItem> = plugins
        .iter()
        .enumerate()
        .skip(scroll_offset)
        .take(visible_height / 2) // 2 lines per item
        .map(|(i, p)| {
            let is_sel = i == selected;
            let is_inst = app.installed_repos.contains(p.repo.as_str());

            let name_style = if is_sel {
                Style::default().fg(ACCENT).bold()
            } else {
                Style::default().fg(TEXT).bold()
            };
            let stars = format!(" ★{}", p.stars);
            let status = if is_inst { " ●" } else { " ○" };
            let status_color = if is_inst { GREEN } else { TEXT_DIM };

            // Compat badge
            let badge = p.compat_badge();
            let badge_color = if p.compat.len() >= 2 { GREEN }
                else if p.compat.contains(&crate::registry::Compat::PSMux) { BLUE }
                else { TEXT_DIM };

            let line1 = Line::from(vec![
                Span::styled(format!(" {}", p.name), name_style),
                Span::styled(format!(" {}", badge), Style::default().fg(badge_color)),
                Span::styled(stars, Style::default().fg(YELLOW)),
                Span::styled(status, Style::default().fg(status_color)),
            ]);

            let desc_style = Style::default().fg(TEXT_DIM);
            let desc_text = if p.description.len() > (list_area.width as usize - 4) {
                format!(" {:.width$}…", p.description, width = list_area.width as usize - 5)
            } else {
                format!(" {}", p.description)
            };
            let line2 = Line::from(Span::styled(desc_text, desc_style));

            let bg = if is_sel {
                BG_HIGHLIGHT
            } else {
                BG
            };

            ListItem::new(vec![line1, line2])
                .style(Style::default().bg(bg))
        })
        .collect();

    let list = List::new(items);
    f.render_widget(list, list_area);
}

// ── Detail panel ────────────────────────────────────────────────────────

fn draw_detail_panel(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::NONE)
        .style(Style::default().bg(BG));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let repo = match app.selected_repo() {
        Some(r) => r,
        None => {
            let msg = Paragraph::new("← Select a plugin to view details")
                .style(Style::default().fg(TEXT_DIM))
                .block(Block::default().style(Style::default().bg(BG)));
            f.render_widget(msg, inner);
            return;
        }
    };

    let is_installed = app.installed_repos.contains(&repo);

    // Try to find info from registry
    let (name, desc, stars, category, compat_badge) = if let Some(rp) = app.get_registry_plugin(&repo) {
        (rp.name.clone(), rp.description.clone(), rp.stars, rp.category.label().to_string(), rp.compat_badge().to_string())
    } else {
        (repo.split('/').last().unwrap_or(&repo).to_string(), String::new(), 0, String::new(), String::new())
    };

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),  // name
            Constraint::Length(1),  // repo
            Constraint::Length(2),  // description
            Constraint::Length(1),  // meta
            Constraint::Length(3),  // action buttons + shortcuts
            Constraint::Length(1),  // separator
            Constraint::Min(3),    // readme
        ])
        .split(inner);

    // Name + active theme badge
    let is_active_theme = app.active_theme.as_deref() == Some(&repo);
    let is_theme = app.is_theme_plugin(&repo);
    let is_compat = app.is_plugin_compatible(&repo);
    let mut name_spans = vec![
        Span::styled(format!("  {}", name), Style::default().fg(ACCENT).bold()),
    ];
    if is_active_theme {
        name_spans.push(Span::styled("  ● ACTIVE", Style::default().fg(GREEN).bold()));
    }
    let name_line = Paragraph::new(Line::from(name_spans))
        .style(Style::default().bg(BG));
    f.render_widget(name_line, layout[0]);

    // Repo
    let repo_line = Paragraph::new(Line::from(Span::styled(
        format!("    {}", repo),
        Style::default().fg(TEXT_DIM),
    )))
    .style(Style::default().bg(BG));
    f.render_widget(repo_line, layout[1]);

    // Description
    let desc_p = Paragraph::new(desc.clone())
        .wrap(Wrap { trim: true })
        .style(Style::default().fg(TEXT).bg(BG));
    f.render_widget(desc_p, layout[2]);

    // Meta line
    let meta = format!("  ★ {} stars   Category: {}   Compat: {}", stars, category, compat_badge);
    let meta_p = Paragraph::new(Line::from(Span::styled(
        meta,
        Style::default().fg(TEXT_DIM),
    )))
    .style(Style::default().bg(BG));
    f.render_widget(meta_p, layout[3]);

    // Action buttons — clear, visible, styled
    let (action_line, shortcut_line) = if is_installed {
        let mut btns = vec![
            Span::styled("  ", Style::default().bg(BG)),
            Span::styled(" ⟳ Update ", Style::default().fg(Color::Black).bg(YELLOW).bold()),
            Span::styled("  ", Style::default().bg(BG)),
            Span::styled(" ✕ Uninstall ", Style::default().fg(Color::White).bg(RED).bold()),
            Span::styled("  ", Style::default().bg(BG)),
            Span::styled(" ▶ Preview ", Style::default().fg(Color::Black).bg(BLUE).bold()),
            Span::styled("  ", Style::default().bg(BG)),
            Span::styled(" ⓘ README ", Style::default().fg(Color::Black).bg(Color::Cyan).bold()),
        ];
        let mut shortcuts_txt = "     u          x/d            p            Enter".to_string();
        if is_theme && !is_active_theme && is_compat {
            btns.push(Span::styled("  ", Style::default().bg(BG)));
            btns.push(Span::styled(" ★ Activate ", Style::default().fg(Color::Black).bg(Color::Rgb(255, 165, 0)).bold()));
            shortcuts_txt.push_str("       a");
        } else if is_theme && !is_active_theme && !is_compat {
            btns.push(Span::styled("  ", Style::default().bg(BG)));
            btns.push(Span::styled(" ✕ Incompatible ", Style::default().fg(Color::DarkGray).bg(Color::Rgb(60, 60, 60))));
        }
        (
            Line::from(btns),
            Line::from(vec![
                Span::styled(shortcuts_txt, Style::default().fg(TEXT_DARK)),
            ]),
        )
    } else {
        let mut btns = vec![
            Span::styled("  ", Style::default().bg(BG)),
            Span::styled(" ⬇ Install ", Style::default().fg(Color::Black).bg(GREEN).bold()),
            Span::styled("  ", Style::default().bg(BG)),
            Span::styled(" ▶ Preview ", Style::default().fg(Color::Black).bg(BLUE).bold()),
            Span::styled("  ", Style::default().bg(BG)),
            Span::styled(" ⓘ README ", Style::default().fg(Color::Black).bg(Color::Cyan).bold()),
        ];
        let mut shortcuts_txt = "    Enter         p            Enter(2nd)".to_string();
        if is_theme && is_compat {
            btns.push(Span::styled("  ", Style::default().bg(BG)));
            btns.push(Span::styled(" ★ Activate ", Style::default().fg(Color::Black).bg(Color::Rgb(255, 165, 0)).bold()));
            shortcuts_txt.push_str("       a");
        } else if is_theme && !is_compat {
            btns.push(Span::styled("  ", Style::default().bg(BG)));
            btns.push(Span::styled(" ✕ Incompatible ", Style::default().fg(Color::DarkGray).bg(Color::Rgb(60, 60, 60))));
        }
        (
            Line::from(btns),
            Line::from(vec![
                Span::styled(shortcuts_txt, Style::default().fg(TEXT_DARK)),
            ]),
        )
    };
    let action_p = Paragraph::new(vec![action_line, Line::from(""), shortcut_line])
        .style(Style::default().bg(BG));
    f.render_widget(action_p, layout[4]);

    // Separator
    let sep = Paragraph::new("─".repeat(area.width as usize))
        .style(Style::default().fg(TEXT_DARK).bg(BG));
    f.render_widget(sep, layout[5]);

    // Readme content
    let readme_area = layout[6];
    if app.detail_readme_loading {
        let loading = Paragraph::new("  Loading README...")
            .style(Style::default().fg(TEXT_DIM).bg(BG));
        f.render_widget(loading, readme_area);
    } else if let Some(readme) = &app.detail_readme {
        // Render README as simple wrapped text
        let lines: Vec<Line> = readme
            .lines()
            .skip(app.detail_scroll_offset)
            .map(|l| {
                let style = if l.starts_with('#') {
                    Style::default().fg(ACCENT).bold()
                } else if l.starts_with("```") {
                    Style::default().fg(BLUE)
                } else if l.starts_with('-') || l.starts_with('*') {
                    Style::default().fg(TEXT)
                } else {
                    Style::default().fg(TEXT)
                };
                Line::from(Span::styled(format!("  {}", l), style))
            })
            .collect();

        let readme_p = Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .style(Style::default().bg(BG));
        f.render_widget(readme_p, readme_area);
    } else {
        let hint = Paragraph::new(format!(
            "  Press Enter to fetch README\n  Repository: https://github.com/{}",
            repo
        ))
        .style(Style::default().fg(TEXT_DIM).bg(BG));
        f.render_widget(hint, readme_area);
    }
}

// ── Installed tab ───────────────────────────────────────────────────────

fn draw_installed_tab(f: &mut Frame, area: Rect, app: &mut App) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(1)])
        .split(area);

    // Actions bar
    let actions = Paragraph::new(Line::from(vec![
        Span::styled(" [", Style::default().fg(TEXT_DARK)),
        Span::styled("U", Style::default().fg(YELLOW).bold()),
        Span::styled("]pdate All  [", Style::default().fg(TEXT_DARK)),
        Span::styled("C", Style::default().fg(RED).bold()),
        Span::styled("]lean Orphaned  [", Style::default().fg(TEXT_DARK)),
        Span::styled("R", Style::default().fg(BLUE).bold()),
        Span::styled("]eload tmux  [", Style::default().fg(TEXT_DARK)),
        Span::styled("S", Style::default().fg(GREEN).bold()),
        Span::styled("]ource plugins", Style::default().fg(TEXT_DARK)),
    ]))
    .style(Style::default().bg(BG_LIGHT));
    f.render_widget(actions, layout[0]);

    // Split: list | detail
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Min(30)])
        .split(layout[1]);

    // Installed list
    let block = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(TEXT_DARK))
        .style(Style::default().bg(BG));
    let list_inner = block.inner(cols[0]);
    f.render_widget(block, cols[0]);

    if app.installed_list.is_empty() {
        let msg = Paragraph::new("  No plugins installed yet.\n\n  Browse the catalog and install some!")
            .style(Style::default().fg(TEXT_DIM).bg(BG));
        f.render_widget(msg, list_inner);
    } else {
        let visible = list_inner.height as usize;
        ensure_scroll_visible(app.installed_selected, &mut app.installed_scroll_offset, visible, 2);
        let items: Vec<ListItem> = app
            .installed_list
            .iter()
            .enumerate()
            .skip(app.installed_scroll_offset)
            .take(visible / 2)
            .map(|(i, p)| {
                let is_sel = i == app.installed_selected;
                let name_style = if is_sel {
                    Style::default().fg(ACCENT).bold()
                } else {
                    Style::default().fg(TEXT).bold()
                };

                let is_active = p.repo.as_deref() == app.active_theme.as_deref()
                    && app.active_theme.is_some();

                let mut spans = vec![
                    Span::styled(format!(" {}", p.display_name()), name_style),
                    Span::styled(" ●", Style::default().fg(GREEN)),
                ];
                if is_active {
                    spans.push(Span::styled(" ACTIVE", Style::default().fg(Color::Rgb(255, 165, 0)).bold()));
                }

                let line1 = Line::from(spans);
                let line2 = Line::from(Span::styled(
                    format!(" {}", p.description()),
                    Style::default().fg(TEXT_DIM),
                ));

                let bg = if is_sel { BG_HIGHLIGHT } else { BG };
                ListItem::new(vec![line1, line2]).style(Style::default().bg(bg))
            })
            .collect();

        f.render_widget(List::new(items), list_inner);
    }

    // Detail panel
    draw_detail_panel(f, cols[1], app);

    // Store layout regions for mouse
    let r = cols[0];
    app.layout.list_area = Some((r.x, r.y, r.width, r.height));
    let r = cols[1];
    app.layout.detail_area = Some((r.x, r.y, r.width, r.height));
    app.layout.sidebar_area = None; // Installed tab has no sidebar
}

// ── Settings tab (was Config) ────────────────────────────────────────────

fn draw_config_tab(f: &mut Frame, area: Rect, app: &mut App) {
    let block = Block::default()
        .borders(Borders::NONE)
        .style(Style::default().bg(BG));
    let inner = block.inner(area);
    f.render_widget(block, area);

    match &app.config {
        None => {
            draw_no_config(f, inner, app);
        }
        Some(_) => {
            let cfg = app.config.clone().unwrap();
            // Split: settings category sidebar | settings list | detection info
            let cols = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Length(18),
                    Constraint::Min(40),
                    Constraint::Length(32),
                ])
                .split(inner);

            draw_settings_sidebar(f, cols[0], app);
            draw_settings_list(f, cols[1], app, &cfg);
            draw_detection_panel(f, cols[2], app);
        }
    }
}

fn draw_no_config(f: &mut Frame, area: Rect, app: &App) {
    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "  No config file found.",
            Style::default().fg(TEXT_DIM),
        )),
        Line::from(""),
    ];
    if !app.detected_muxes.is_empty() {
        lines.push(Line::from(Span::styled(
            "  Detected multiplexers:",
            Style::default().fg(TEXT),
        )));
        for m in &app.detected_muxes {
            lines.push(Line::from(Span::styled(
                format!("    • {} ({})", m.name, m.version),
                Style::default().fg(GREEN),
            )));
        }
        lines.push(Line::from(""));
    }
    lines.push(Line::from(Span::styled(
        "  Press 'c' to create a default config file.",
        Style::default().fg(ACCENT),
    )));
    let msg = Paragraph::new(lines).style(Style::default().fg(TEXT_DIM));
    f.render_widget(msg, area);
}

fn draw_settings_sidebar(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(TEXT_DARK))
        .style(Style::default().bg(BG_DARK));
    let sb_inner = block.inner(area);
    f.render_widget(block, area);

    let mut items: Vec<ListItem> = Vec::new();

    // "All Settings"
    let all_style = if app.settings_category_index == 0 {
        Style::default().fg(ACCENT).bold().bg(BG_HIGHLIGHT)
    } else {
        Style::default().fg(TEXT_DIM)
    };
    items.push(ListItem::new(Line::from(Span::styled(
        " ⚙ All Settings",
        all_style,
    ))));

    for (i, cat) in SettingCategory::ALL.iter().enumerate() {
        let style = if app.settings_category_index == i + 1 {
            Style::default().fg(ACCENT).bold().bg(BG_HIGHLIGHT)
        } else {
            Style::default().fg(TEXT_DIM)
        };
        items.push(ListItem::new(Line::from(Span::styled(
            format!(" {} {}", cat.icon(), cat.label()),
            style,
        ))));
    }

    let list = List::new(items).block(
        Block::default()
            .title(Span::styled(" SETTINGS ", Style::default().fg(ACCENT).bold()))
            .borders(Borders::NONE)
            .style(Style::default().bg(BG_DARK)),
    );
    f.render_widget(list, sb_inner);
}

fn draw_settings_list(f: &mut Frame, area: Rect, app: &mut App, cfg: &TmuxConfig) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(1)])
        .split(area);

    // Header
    let config_count_label = if app.all_configs.len() > 1 {
        format!(
            "  ·  Config {} of {} (press 'c' to cycle)",
            app.active_config_index + 1,
            app.all_configs.len(),
        )
    } else {
        String::new()
    };

    let header_text = vec![
        Line::from(vec![
            Span::styled("  📄 ", Style::default().fg(ACCENT)),
            Span::styled(
                format!("[{}] ", cfg.type_label()),
                Style::default().fg(ACCENT2).bold(),
            ),
            Span::styled(cfg.display_path(), Style::default().fg(ACCENT).bold()),
            Span::styled(config_count_label, Style::default().fg(TEXT_DIM)),
        ]),
        Line::from(Span::styled(
            "  ←/→ categories  ↑/↓ navigate  Enter toggle/edit  Bksp reset  D reset-all  Ctrl+D factory-reset",
            Style::default().fg(TEXT_DARK),
        )),
    ];
    let header = Paragraph::new(header_text).style(Style::default().bg(BG_LIGHT));
    f.render_widget(header, layout[0]);

    // Settings list
    let list_area = layout[1];
    let visible_height = list_area.height as usize;
    ensure_scroll_visible(app.settings_selected, &mut app.settings_scroll_offset, visible_height, 2);
    let filtered = app.filtered_settings();

    if filtered.is_empty() {
        let msg = Paragraph::new("  No settings in this category.")
            .style(Style::default().fg(TEXT_DIM).bg(BG));
        f.render_widget(msg, list_area);
        return;
    }

    let items: Vec<ListItem> = filtered
        .iter()
        .enumerate()
        .skip(app.settings_scroll_offset)
        .take(visible_height / 2) // 2 lines per item
        .map(|(i, s)| {
            let is_sel = i == app.settings_selected;
            let is_editing = app.settings_editing == Some(i);
            let bg = if is_sel { BG_HIGHLIGHT } else { BG };

            let name_style = if is_sel {
                Style::default().fg(ACCENT).bold()
            } else {
                Style::default().fg(TEXT).bold()
            };

            // Value display
            let val_display = if is_editing {
                format!("{}▏", app.settings_edit_buffer)
            } else if s.value.is_empty() {
                format!("{} (default)", s.default)
            } else {
                s.value.clone()
            };

            let val_style = if is_editing {
                Style::default().fg(ACCENT).bold()
            } else if s.is_default() {
                Style::default().fg(TEXT_DIM)
            } else {
                Style::default().fg(GREEN)
            };

            // Toggle indicator for bools
            let toggle = match s.stype {
                crate::config::SettingType::Bool => {
                    let on = s.is_bool_on() || (s.value.is_empty() && matches!(s.default.as_str(), "on" | "yes" | "true"));
                    if on {
                        Span::styled(" ● ON ", Style::default().fg(GREEN).bold())
                    } else {
                        Span::styled(" ○ OFF", Style::default().fg(TEXT_DIM))
                    }
                }
                crate::config::SettingType::Choice => {
                    Span::styled(" ▾", Style::default().fg(BLUE))
                }
                _ => Span::raw(""),
            };

            // Category badge (only when showing All)
            let cat_badge = if app.settings_category_index == 0 {
                Span::styled(
                    format!(" [{}]", s.category.label()),
                    Style::default().fg(TEXT_DARK),
                )
            } else {
                Span::raw("")
            };

            let modified_marker = if !s.is_default() {
                Span::styled(" ✎", Style::default().fg(YELLOW))
            } else {
                Span::raw("")
            };

            let line1 = Line::from(vec![
                Span::styled(format!("  {} ", s.label), name_style),
                toggle,
                modified_marker,
                cat_badge,
            ]);

            let line2 = if is_sel {
                Line::from(vec![
                    Span::styled("    ", Style::default()),
                    Span::styled(val_display, val_style),
                    Span::styled(
                        format!("  — {}", s.description),
                        Style::default().fg(TEXT_DIM),
                    ),
                ])
            } else {
                Line::from(vec![
                    Span::styled("    ", Style::default()),
                    Span::styled(val_display, val_style),
                ])
            };

            ListItem::new(vec![line1, line2]).style(Style::default().bg(bg))
        })
        .collect();

    let list = List::new(items).style(Style::default().bg(BG));
    f.render_widget(list, list_area);
}

fn draw_detection_panel(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::LEFT)
        .border_style(Style::default().fg(TEXT_DARK))
        .title(Span::styled(" System ", Style::default().fg(ACCENT).bold()))
        .style(Style::default().bg(BG_DARK));
    let det_inner = block.inner(area);
    f.render_widget(block, area);

    let mut lines: Vec<Line> = Vec::new();

    // Platform info
    if let Some(report) = &app.detection_report {
        let os_label = match report.platform.os {
            "windows" => "Windows",
            "macos" => "macOS",
            "linux" => "Linux",
            other => other,
        };
        lines.push(Line::from(vec![
            Span::styled(" OS: ", Style::default().fg(TEXT_DIM)),
            Span::styled(os_label, Style::default().fg(TEXT).bold()),
        ]));
        if report.platform.is_wsl {
            lines.push(Line::from(Span::styled(
                " WSL: Yes",
                Style::default().fg(BLUE).bold(),
            )));
        }
        lines.push(Line::from(""));

        // Detected multiplexers
        lines.push(Line::from(Span::styled(
            " ─── Multiplexers ───",
            Style::default().fg(ACCENT).bold(),
        )));
        if report.multiplexers.is_empty() {
            lines.push(Line::from(Span::styled(
                "  None found",
                Style::default().fg(RED),
            )));
        } else {
            for m in &report.multiplexers {
                let path_str = m
                    .binary_path
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| m.binary.clone());
                lines.push(Line::from(Span::styled(
                    format!("  ● {} v{}", m.name, m.version),
                    Style::default().fg(GREEN),
                )));
                let home = dirs::home_dir().unwrap_or_default();
                let display_path = path_str.replace(&home.display().to_string(), "~");
                lines.push(Line::from(Span::styled(
                    format!("    {}", display_path),
                    Style::default().fg(TEXT_DIM),
                )));
            }
        }
        lines.push(Line::from(""));

        // Config files — with selection indicators
        lines.push(Line::from(Span::styled(
            " ─── Config Files ───",
            Style::default().fg(ACCENT).bold(),
        )));
        if app.all_configs.is_empty() {
            lines.push(Line::from(Span::styled(
                "  None found",
                Style::default().fg(TEXT_DIM),
            )));
            lines.push(Line::from(Span::styled(
                "  'c' to create one",
                Style::default().fg(ACCENT),
            )));
        } else {
            for (i, cfg) in app.all_configs.iter().enumerate() {
                let is_active = i == app.active_config_index;
                let marker = if is_active { "▶" } else { "○" };
                let style = if is_active {
                    Style::default().fg(ACCENT).bold()
                } else {
                    Style::default().fg(TEXT_DIM)
                };
                let home = dirs::home_dir().unwrap_or_default();
                let display = cfg.display_path()
                    .replace(&home.display().to_string(), "~");
                lines.push(Line::from(vec![
                    Span::styled(format!("  {} ", marker), style),
                    Span::styled(
                        format!("[{}] ", cfg.type_label()),
                        if is_active {
                            Style::default().fg(ACCENT2).bold()
                        } else {
                            Style::default().fg(TEXT_DARK)
                        },
                    ),
                ]));
                lines.push(Line::from(Span::styled(
                    format!("    {}", display),
                    if is_active {
                        Style::default().fg(TEXT)
                    } else {
                        Style::default().fg(TEXT_DARK)
                    },
                )));
                if is_active {
                    lines.push(Line::from(Span::styled(
                        format!("    {} plugin(s)", cfg.plugins.len()),
                        Style::default().fg(GREEN),
                    )));
                }
            }
            if app.all_configs.len() > 1 {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    " 'c' to cycle configs",
                    Style::default().fg(ACCENT),
                )));
            }
        }
        lines.push(Line::from(""));

        // Searched paths (compact — show count and a few)
        let missing: Vec<_> = report.config_locations.iter().filter(|c| !c.exists).collect();
        if !missing.is_empty() {
            lines.push(Line::from(Span::styled(
                format!(" ─── Searched ({}) ───", missing.len()),
                Style::default().fg(TEXT_DARK),
            )));
            for c in missing.iter().take(8) {
                let home = dirs::home_dir().unwrap_or_default();
                let display = c.path.display().to_string()
                    .replace(&home.display().to_string(), "~");
                lines.push(Line::from(Span::styled(
                    format!("  ○ {}", display),
                    Style::default().fg(TEXT_DARK),
                )));
            }
            if missing.len() > 8 {
                lines.push(Line::from(Span::styled(
                    format!("  … +{} more", missing.len() - 8),
                    Style::default().fg(TEXT_DARK),
                )));
            }
        }
    } else {
        lines.push(Line::from(Span::styled(
            " Detection not run",
            Style::default().fg(TEXT_DIM),
        )));
    }

    let para = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .style(Style::default().bg(BG_DARK));
    f.render_widget(para, det_inner);
}

// ── Status bar ──────────────────────────────────────────────────────────

fn draw_status(f: &mut Frame, area: Rect, app: &App) {
    let style = if app.status.is_error {
        Style::default().fg(Color::White).bg(RED).bold()
    } else {
        Style::default().fg(Color::Black).bg(ACCENT).bold()
    };
    let status = Paragraph::new(Span::styled(
        format!(" {}", app.status.text),
        style,
    ));
    f.render_widget(status, area);
}

// ── Footer (keybind hints) ──────────────────────────────────────────────

fn draw_footer(f: &mut Frame, area: Rect, _app: &App) {
    let key_style = Style::default().fg(Color::White).bg(ACCENT).bold();
    let label_style = Style::default().fg(Color::Black).bg(ACCENT);
    let mouse_style = Style::default().fg(Color::Rgb(180, 255, 180)).bg(ACCENT);

    let hints = Line::from(vec![
        Span::styled(" q", key_style),
        Span::styled(" Quit ", label_style),
        Span::styled("Tab", key_style),
        Span::styled(" Next ", label_style),
        Span::styled("↑↓", key_style),
        Span::styled(" Nav ", label_style),
        Span::styled("Enter", key_style),
        Span::styled(" Install ", label_style),
        Span::styled("p", key_style),
        Span::styled(" Preview ", label_style),
        Span::styled("x", key_style),
        Span::styled(" Rm ", label_style),
        Span::styled("u", key_style),
        Span::styled(" Upd ", label_style),
        Span::styled("/", key_style),
        Span::styled(" Search ", label_style),
        Span::styled("f", key_style),
        Span::styled(" Filter ", label_style),
        Span::styled("a", key_style),
        Span::styled(" Activate ", label_style),
        Span::styled(" 🖱 Mouse", mouse_style),
        Span::styled(" ON ", mouse_style),
    ]);

    let footer = Paragraph::new(hints).style(Style::default().bg(ACCENT));
    f.render_widget(footer, area);
}

// ── Confirmation dialog (overlay) ───────────────────────────────────────

fn draw_confirm_dialog(f: &mut Frame, area: Rect, dialog: &ConfirmDialog) {
    let width = 50u16.min(area.width.saturating_sub(4));
    let height = 8u16.min(area.height.saturating_sub(4));
    let x = (area.width.saturating_sub(width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    let popup = Rect::new(x, y, width, height);

    f.render_widget(Clear, popup);

    let block = Block::default()
        .title(Span::styled(
            format!(" {} ", dialog.title),
            Style::default().fg(ACCENT).bold(),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(ACCENT))
        .style(Style::default().bg(BG_LIGHT));

    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(2), Constraint::Length(2)])
        .split(inner);

    // Message
    let msg = Paragraph::new(dialog.message.as_str())
        .wrap(Wrap { trim: true })
        .style(Style::default().fg(TEXT));
    f.render_widget(msg, layout[0]);

    // Buttons
    let cancel_style = if !dialog.confirm_selected {
        Style::default().fg(BG_DARK).bg(TEXT).bold()
    } else {
        Style::default().fg(TEXT_DIM)
    };
    let confirm_style = if dialog.confirm_selected {
        Style::default().fg(BG_DARK).bg(RED).bold()
    } else {
        Style::default().fg(TEXT_DIM)
    };

    let buttons = Line::from(vec![
        Span::raw("      "),
        Span::styled(" Cancel ", cancel_style),
        Span::raw("   "),
        Span::styled(" Confirm ", confirm_style),
    ]);
    let btn_p = Paragraph::new(buttons);
    f.render_widget(btn_p, layout[1]);
}
