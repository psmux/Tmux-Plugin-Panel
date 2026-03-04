/// Application state machine.
///
/// Holds the entire TUI state: active tab, selected items, search queries,
/// loaded data, and pending operations.
use crate::config::{ConfigSetting, SettingCategory, TmuxConfig};
use crate::detect::{DetectedMux, DetectionReport};
use crate::plugins::InstalledPlugin;
use crate::registry::{self, Category, Compat, RegistryPlugin};

/// Which tab is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Dashboard,
    Browse,
    Installed,
    Config,
}

impl Tab {
    pub const ALL: &'static [Tab] = &[
        Tab::Dashboard,
        Tab::Browse,
        Tab::Installed,
        Tab::Config,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            Tab::Dashboard => " ⌂ Home ",
            Tab::Browse => " ☰ Browse ",
            Tab::Installed => " ● Installed ",
            Tab::Config => " ⚙ Config ",
        }
    }

    pub fn index(&self) -> usize {
        match self {
            Tab::Dashboard => 0,
            Tab::Browse => 1,
            Tab::Installed => 2,
            Tab::Config => 3,
        }
    }

    pub fn from_index(i: usize) -> Tab {
        match i {
            0 => Tab::Dashboard,
            1 => Tab::Browse,
            2 => Tab::Installed,
            3 => Tab::Config,
            _ => Tab::Dashboard,
        }
    }
}

/// Dashboard quick-action items.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DashboardItem {
    BrowsePlugins,
    BrowseThemes,
    ConfigureSettings,
    ResetToDefaults,
    ManageRegistries,
}

impl DashboardItem {
    pub const ALL: &'static [DashboardItem] = &[
        DashboardItem::BrowsePlugins,
        DashboardItem::BrowseThemes,
        DashboardItem::ConfigureSettings,
        DashboardItem::ResetToDefaults,
        DashboardItem::ManageRegistries,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            DashboardItem::BrowsePlugins => "Browse & Install Plugins",
            DashboardItem::BrowseThemes => "Browse & Install Themes",
            DashboardItem::ConfigureSettings => "Configure Settings",
            DashboardItem::ResetToDefaults => "Reset to Defaults",
            DashboardItem::ManageRegistries => "Manage Plugin Sources",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            DashboardItem::BrowsePlugins => "📦",
            DashboardItem::BrowseThemes => "🎨",
            DashboardItem::ConfigureSettings => "⚙",
            DashboardItem::ResetToDefaults => "🔄",
            DashboardItem::ManageRegistries => "📋",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            DashboardItem::BrowsePlugins => "Search, discover, and install plugins from the registry",
            DashboardItem::BrowseThemes => "Browse themes — use the Theme category filter in Browse",
            DashboardItem::ConfigureSettings => "Toggle mouse, status bar, prefix key, and more",
            DashboardItem::ResetToDefaults => "Restore all settings to factory defaults",
            DashboardItem::ManageRegistries => "Add or remove plugin repository sources",
        }
    }
}

/// Which pane has focus in a split view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Sidebar,
    List,
    Detail,
    Search,
}

/// What action a confirmation dialog is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmAction {
    RemovePlugin,
    InstallPlugin,
    ActivateTheme,
    ResetEntireConfig,
    ResetAllSettings,
}

/// Active confirmation dialog.
#[derive(Debug, Clone)]
pub struct ConfirmDialog {
    pub title: String,
    pub message: String,
    pub repo: String,
    pub action: ConfirmAction,
    pub confirm_selected: bool, // false = Cancel highlighted, true = Confirm
}

/// Status bar message with optional severity.
#[derive(Debug, Clone)]
pub struct StatusMessage {
    pub text: String,
    pub is_error: bool,
}

/// Cached layout regions for mouse hit-testing.
/// These are updated every frame by ui::draw().
#[derive(Debug, Clone, Default)]
pub struct LayoutRegions {
    pub tabs_area: Option<(u16, u16, u16, u16)>,       // (x, y, w, h)
    pub sidebar_area: Option<(u16, u16, u16, u16)>,     // category sidebar
    pub list_area: Option<(u16, u16, u16, u16)>,        // plugin/item list
    pub detail_area: Option<(u16, u16, u16, u16)>,      // detail panel
    pub action_buttons_area: Option<(u16, u16, u16, u16)>, // action buttons in detail
    pub body_area: Option<(u16, u16, u16, u16)>,        // full body (for Dashboard)
    pub tab_rects: Vec<(u16, u16, u16, u16)>,           // individual tab rects
    // Precise content areas for accurate mouse hit-testing (set by rendering code)
    pub list_content_area: Option<(u16, u16, u16, u16)>,
    pub sidebar_content_area: Option<(u16, u16, u16, u16)>,
    pub dashboard_cards_area: Option<(u16, u16, u16, u16)>,
    pub settings_content_area: Option<(u16, u16, u16, u16)>,
}

/// The full application state.
pub struct App {
    pub running: bool,
    pub tab: Tab,
    pub focus: Focus,
    pub config: Option<TmuxConfig>,

    // ── Multiplexer detection ───────────────────────
    pub detected_muxes: Vec<DetectedMux>,
    pub all_configs: Vec<TmuxConfig>,
    pub active_config_index: usize,

    // ── Dynamic registry ────────────────────────────
    pub registry: Vec<RegistryPlugin>,
    pub compat_filter: Option<Compat>,

    // ── Browse tab ──────────────────────────────────
    pub browse_search: String,
    pub browse_search_editing: bool,
    pub browse_category: Option<Category>,
    pub browse_category_index: usize, // 0 = All, 1..=7 = categories
    pub browse_list: Vec<RegistryPlugin>,
    pub browse_selected: usize,
    pub browse_scroll_offset: usize,

    // ── Installed tab ───────────────────────────────
    pub installed_list: Vec<InstalledPlugin>,
    pub installed_selected: usize,
    pub installed_scroll_offset: usize,
    pub active_theme: Option<String>, // repo of the currently active theme


    // ── Config/Settings tab ──────────────────────────
    pub config_scroll_offset: usize,
    pub settings_list: Vec<ConfigSetting>,
    pub settings_selected: usize,
    pub settings_scroll_offset: usize,
    pub settings_category_index: usize, // 0=All, 1..=6 = SettingCategory
    pub settings_editing: Option<usize>, // index into filtered settings if editing
    pub settings_edit_buffer: String,
    pub detection_report: Option<DetectionReport>,

    // ── Detail readme ───────────────────────────────
    pub detail_readme: Option<String>,
    pub detail_readme_loading: bool,
    pub detail_scroll_offset: usize,

    // ── Dialogs / Status ────────────────────────────
    pub confirm: Option<ConfirmDialog>,
    pub status: StatusMessage,
    pub installed_repos: std::collections::HashSet<String>,

    // ── Dashboard tab ────────────────────────────────
    pub dashboard_selected: usize,

    // ── Preview pending (repo, config_clone, detected_muxes) ────
    pub preview_pending: Option<(String, crate::config::TmuxConfig, Vec<crate::detect::DetectedMux>)>,

    // ── Orphan tracking (display only — user cleans manually) ────
    pub _orphan_count: usize,

    // ── Layout regions for mouse hit-testing ────────
    pub layout: LayoutRegions,
}

impl App {
    pub fn new() -> Self {
        // Load embedded registry immediately for instant Browse tab
        let registry = registry::load_embedded();

        // Auto-detect compat filter based on platform
        let compat_filter = if cfg!(target_os = "windows") {
            Some(Compat::PSMux)
        } else {
            Some(Compat::Tmux)
        };

        let browse_list = registry::search_registry(&registry, "", None, compat_filter);

        App {
            running: true,
            tab: Tab::Dashboard,
            focus: Focus::List,
            config: None,

            detected_muxes: Vec::new(),
            all_configs: Vec::new(),
            active_config_index: 0,

            registry,
            compat_filter,

            browse_search: String::new(),
            browse_search_editing: false,
            browse_category: None,
            browse_category_index: 0,
            browse_list,
            browse_selected: 0,
            browse_scroll_offset: 0,

            installed_list: Vec::new(),
            installed_selected: 0,
            installed_scroll_offset: 0,
            active_theme: None,

            config_scroll_offset: 0,
            settings_list: Vec::new(),
            settings_selected: 0,
            settings_scroll_offset: 0,
            settings_category_index: 0,
            settings_editing: None,
            settings_edit_buffer: String::new(),
            detection_report: None,

            detail_readme: None,
            detail_readme_loading: false,
            detail_scroll_offset: 0,

            confirm: None,
            dashboard_selected: 0,

            status: StatusMessage {
                text: "Welcome! Navigate with ↑↓ and press Enter to get started.".to_string(),
                is_error: false,
            },
            installed_repos: std::collections::HashSet::new(),
            preview_pending: None,
            _orphan_count: 0,
            layout: LayoutRegions::default(),
        }
    }

    pub fn load_config(&mut self) {
        // Run full detection sweep
        let report = crate::detect::detect_all();
        self.detected_muxes = report.multiplexers.clone();
        self.detection_report = Some(report);

        // Find all config files
        self.all_configs = crate::config::find_configs();
        self.active_config_index = 0;
        self.config = self.all_configs.first().cloned();

        // Check for orphaned plugins (on disk but not in config).
        // Don't auto-delete — just report count so the user can decide.
        if let Some(ref cfg) = self.config {
            let orphans = crate::plugins::find_orphaned_plugins(cfg);
            if !orphans.is_empty() {
                // Store count for status display; user can press 'C' in Installed tab to clean
                self._orphan_count = orphans.len();
            }
        }

        // Repair broken @plugin entries missing their activation lines
        // (e.g. source-file for psmux themes, run-shell for scripts).
        if let Some(ref mut cfg) = self.config {
            let repaired = crate::config::repair_missing_activation_lines(cfg);
            if repaired > 0 {
                // Re-parse config to pick up the new lines
                if let Ok(refreshed) = crate::config::parse_config(&cfg.path, &cfg.config_type) {
                    *cfg = refreshed;
                    // Also update the all_configs entry
                    if self.active_config_index < self.all_configs.len() {
                        self.all_configs[self.active_config_index] = cfg.clone();
                    }
                }
            }
        }

        self.refresh_installed();
        self.refresh_settings();

        // Build status message
        let mux_names: Vec<String> = self
            .detected_muxes
            .iter()
            .map(|m| format!("{} ({})", m.name, m.version))
            .collect();

        if let Some(cfg) = &self.config {
            let mux_info = if mux_names.is_empty() {
                "no multiplexer binary found".to_string()
            } else {
                mux_names.join(", ")
            };
            self.set_status(&format!(
                "{}  ·  {} plugins  ·  {} config(s)  ·  {}",
                cfg.type_label(),
                cfg.plugins.len(),
                self.all_configs.len(),
                mux_info,
            ));
        } else if !self.detected_muxes.is_empty() {
            let mux_info = mux_names.join(", ");
            self.set_status(&format!(
                "No config file found  ·  Detected: {}  ·  Press 'c' to create one",
                mux_info
            ));
        } else {
            self.set_status_err(
                "No multiplexer found. Install tmux (Linux/macOS) or PSMux (Windows).",
            );
        }
    }

    /// Switch to the next config when multiple configs are found.
    pub fn cycle_config(&mut self) {
        if self.all_configs.len() <= 1 {
            return;
        }
        self.active_config_index = (self.active_config_index + 1) % self.all_configs.len();
        self.config = Some(self.all_configs[self.active_config_index].clone());
        self.refresh_installed();
        self.refresh_settings();
        if let Some(cfg) = &self.config {
            self.set_status(&format!(
                "Switched to {} config: {}  ·  {} plugins",
                cfg.type_label(),
                cfg.display_path(),
                cfg.plugins.len(),
            ));
        }
    }

    pub fn refresh_installed(&mut self) {
        if let Some(cfg) = &self.config {
            self.installed_list = crate::plugins::scan_installed_plugins(cfg);
            self.installed_repos = self
                .installed_list
                .iter()
                .filter_map(|p| p.repo.clone())
                .collect();
            // Also add short names so Browse tab can detect installed-by-name
            for p in &self.installed_list {
                if let Some(repo) = &p.repo {
                    if let Some(short) = repo.split('/').last() {
                        self.installed_repos.insert(short.to_string());
                    }
                }
            }
            // Detect active theme from config plugins
            self.active_theme = self.detect_active_theme();
        }
    }

    /// Detect the currently active theme by checking:
    /// 1. Config @plugin entries that map to theme-category registry entries
    /// 2. Config source-file lines that reference a theme plugin's plugin.conf
    fn detect_active_theme(&self) -> Option<String> {
        let registry = crate::registry::embedded_registry();
        if let Some(cfg) = &self.config {
            // Check @plugin declarations first
            for plugin_entry in &cfg.plugins {
                if let Some(rp) = registry.iter().find(|rp| rp.repo == plugin_entry.repo) {
                    if rp.category == Category::Theme {
                        return Some(rp.repo.clone());
                    }
                }
            }
            // Check source-file lines that reference theme plugin.conf
            for line in &cfg.lines {
                let lt = line.trim();
                if lt.starts_with("source-file") {
                    for rp in registry.iter().filter(|rp| rp.category == Category::Theme) {
                        let theme_name = rp.repo.split('/').last().unwrap_or(&rp.repo);
                        if lt.contains(theme_name) {
                            return Some(rp.repo.clone());
                        }
                    }
                }
            }
        }
        None
    }

    /// Check if a repo is a theme plugin.
    /// Matches both full repo paths ("psmux-plugins/psmux-theme-catppuccin")
    /// and short names ("psmux-theme-catppuccin").
    pub fn is_theme_plugin(&self, repo: &str) -> bool {
        let registry = crate::registry::embedded_registry();
        registry.iter()
            .find(|rp| rp.repo == repo || rp.repo.split('/').last() == Some(repo))
            .map(|rp| rp.category == Category::Theme)
            .unwrap_or(false)
    }

    /// Check if a plugin is compatible with the currently loaded config.
    /// Returns true if the plugin is compatible or if we can't determine compat.
    pub fn is_plugin_compatible(&self, repo: &str) -> bool {
        let required = if let Some(ref cfg) = self.config {
            if cfg.config_type == "psmux" {
                Compat::PSMux
            } else {
                Compat::Tmux
            }
        } else {
            return true; // no config = can't judge
        };
        if let Some(rp) = self.get_registry_plugin(repo) {
            rp.is_compatible(required)
        } else {
            true // not in registry = assume compatible
        }
    }

    /// Get incompatibility reason string, or None if compatible.
    pub fn compat_error_message(&self, repo: &str) -> Option<String> {
        if self.is_plugin_compatible(repo) {
            return None;
        }
        let cfg = self.config.as_ref()?;
        let plugin_name = repo.split('/').last().unwrap_or(repo);
        let other = if cfg.config_type == "psmux" { "tmux" } else { "psmux" };
        Some(format!(
            "'{}' is {}-only and not compatible with {}.",
            plugin_name, other, cfg.type_label(),
        ))
    }

    pub fn refresh_settings(&mut self) {
        if let Some(cfg) = &self.config {
            self.settings_list = crate::config::parse_settings(cfg);
            self.settings_selected = 0;
            self.settings_scroll_offset = 0;
            self.settings_editing = None;
        }
    }

    /// Get settings filtered by current category.
    pub fn filtered_settings(&self) -> Vec<&ConfigSetting> {
        if self.settings_category_index == 0 {
            self.settings_list.iter().collect()
        } else {
            let cat = SettingCategory::ALL[self.settings_category_index - 1];
            self.settings_list.iter().filter(|s| s.category == cat).collect()
        }
    }

    pub fn refresh_browse(&mut self) {
        self.browse_list = registry::search_registry(
            &self.registry,
            &self.browse_search,
            self.browse_category,
            self.compat_filter,
        );
        self.browse_selected = 0;
        self.browse_scroll_offset = 0;
    }

    /// Load registry (embedded). Remote fetch may be added later.
    pub fn load_registry(&mut self) {
        self.registry = registry::load_registry();
        self.refresh_browse();
        self.set_status(&format!(
            "Registry loaded: {} plugins ({} compatible)",
            self.registry.len(),
            self.browse_list.len(),
        ));
    }

    /// Toggle compat filter between platform-specific and show-all.
    pub fn toggle_compat_filter(&mut self) {
        self.compat_filter = match self.compat_filter {
            Some(_) => None, // show all
            None => {
                if cfg!(target_os = "windows") {
                    Some(Compat::PSMux)
                } else {
                    Some(Compat::Tmux)
                }
            }
        };
        self.refresh_browse();
        let label = match self.compat_filter {
            Some(Compat::PSMux) => "PSMux compatible",
            Some(Compat::Tmux) => "tmux compatible",
            None => "all plugins",
        };
        self.set_status(&format!(
            "Filter: {} ({} shown)",
            label,
            self.browse_list.len()
        ));
    }

    /// Look up a plugin in the loaded registry by repo path.
    pub fn get_registry_plugin(&self, repo: &str) -> Option<&RegistryPlugin> {
        registry::get_registry_plugin(&self.registry, repo)
    }

    pub fn set_status(&mut self, msg: &str) {
        self.status = StatusMessage {
            text: msg.to_string(),
            is_error: false,
        };
    }

    pub fn set_status_err(&mut self, msg: &str) {
        self.status = StatusMessage {
            text: msg.to_string(),
            is_error: true,
        };
    }

    /// Get the repo string for the currently selected item in the active tab.
    pub fn selected_repo(&self) -> Option<String> {
        match self.tab {
            Tab::Dashboard => None,
            Tab::Browse => self
                .browse_list
                .get(self.browse_selected)
                .map(|p| p.repo.to_string()),
            Tab::Installed => self
                .installed_list
                .get(self.installed_selected)
                .and_then(|p| {
                    // Return repo if available; otherwise derive from name
                    p.repo.clone().or_else(|| Some(p.name.clone()))
                }),
            Tab::Config => None,
        }
    }

    pub fn is_selected_installed(&self) -> bool {
        self.selected_repo()
            .map(|r| self.installed_repos.contains(&r))
            .unwrap_or(false)
    }

    /// Current list length for the active tab.
    pub fn current_list_len(&self) -> usize {
        match self.tab {
            Tab::Dashboard => DashboardItem::ALL.len(),
            Tab::Browse => self.browse_list.len(),
            Tab::Installed => self.installed_list.len(),
            Tab::Config => self.filtered_settings().len(),
        }
    }

    /// Current selected index (mutable reference).
    pub fn selected_mut(&mut self) -> &mut usize {
        match self.tab {
            Tab::Dashboard => &mut self.dashboard_selected,
            Tab::Browse => &mut self.browse_selected,
            Tab::Installed => &mut self.installed_selected,
            Tab::Config => &mut self.settings_selected,
        }
    }

    pub fn scroll_offset_mut(&mut self) -> &mut usize {
        match self.tab {
            Tab::Dashboard => &mut self.dashboard_selected, // no scrolling needed
            Tab::Browse => &mut self.browse_scroll_offset,
            Tab::Installed => &mut self.installed_scroll_offset,
            Tab::Config => &mut self.settings_scroll_offset,
        }
    }

    /// Move the selected index. Single-step (±1) wraps around;
    /// multi-step (page up/down) clamps at boundaries.
    pub fn move_selection(&mut self, delta: isize) {
        let len = self.current_list_len();
        if len == 0 {
            return;
        }
        let sel = self.selected_mut();
        if delta == 1 || delta == -1 {
            // Wrap around for single-step navigation
            let new = ((*sel as isize + delta) % len as isize + len as isize) % len as isize;
            *sel = new as usize;
        } else {
            // Clamp for multi-step (page up/down)
            let new = (*sel as isize + delta).max(0).min(len as isize - 1) as usize;
            *sel = new;
        }
        self.detail_scroll_offset = 0;
    }
}

// ── Unit Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tab_index_roundtrip() {
        for (i, tab) in Tab::ALL.iter().enumerate() {
            assert_eq!(tab.index(), i);
            assert_eq!(Tab::from_index(i), *tab);
        }
    }

    #[test]
    fn test_tab_from_index_out_of_bounds() {
        assert_eq!(Tab::from_index(99), Tab::Dashboard);
    }

    #[test]
    fn test_dashboard_items_count() {
        assert_eq!(DashboardItem::ALL.len(), 5);
    }

    #[test]
    fn test_move_selection_wraps_forward() {
        let mut app = App::new();
        app.tab = Tab::Dashboard; // 5 items
        app.dashboard_selected = 4; // last item
        app.move_selection(1);
        assert_eq!(app.dashboard_selected, 0); // wraps to first
    }

    #[test]
    fn test_move_selection_wraps_backward() {
        let mut app = App::new();
        app.tab = Tab::Dashboard;
        app.dashboard_selected = 0; // first item
        app.move_selection(-1);
        assert_eq!(app.dashboard_selected, 4); // wraps to last
    }

    #[test]
    fn test_move_selection_normal_forward() {
        let mut app = App::new();
        app.tab = Tab::Dashboard;
        app.dashboard_selected = 1;
        app.move_selection(1);
        assert_eq!(app.dashboard_selected, 2);
    }

    #[test]
    fn test_move_selection_normal_backward() {
        let mut app = App::new();
        app.tab = Tab::Dashboard;
        app.dashboard_selected = 2;
        app.move_selection(-1);
        assert_eq!(app.dashboard_selected, 1);
    }

    #[test]
    fn test_move_selection_page_clamps_top() {
        let mut app = App::new();
        app.tab = Tab::Dashboard;
        app.dashboard_selected = 1;
        app.move_selection(-10); // page up
        assert_eq!(app.dashboard_selected, 0); // clamps, doesn't wrap
    }

    #[test]
    fn test_move_selection_page_clamps_bottom() {
        let mut app = App::new();
        app.tab = Tab::Dashboard;
        app.dashboard_selected = 3;
        app.move_selection(10); // page down
        assert_eq!(app.dashboard_selected, 4); // clamps to last
    }

    #[test]
    fn test_move_selection_empty_list() {
        let mut app = App::new();
        app.tab = Tab::Installed; // no plugins installed
        app.installed_selected = 0;
        app.move_selection(1); // should not panic
        assert_eq!(app.installed_selected, 0);
    }

    #[test]
    fn test_initial_tab_is_dashboard() {
        let app = App::new();
        assert_eq!(app.tab, Tab::Dashboard);
    }

    #[test]
    fn test_status_message_default() {
        let app = App::new();
        assert!(!app.status.is_error);
    }

    #[test]
    fn test_set_status() {
        let mut app = App::new();
        app.set_status("hello");
        assert_eq!(app.status.text, "hello");
        assert!(!app.status.is_error);
    }

    #[test]
    fn test_set_status_err() {
        let mut app = App::new();
        app.set_status_err("fail");
        assert_eq!(app.status.text, "fail");
        assert!(app.status.is_error);
    }
}
