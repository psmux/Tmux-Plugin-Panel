/// Application state machine.
///
/// Holds the entire TUI state: active tab, selected items, search queries,
/// loaded data, and pending operations.
use crate::config::{ConfigSetting, SettingCategory, TmuxConfig};
use crate::detect::{DetectedMux, DetectionReport};
use crate::plugins::InstalledPlugin;
use crate::registry::{self, Category, Compat, RegistryPlugin};
use crate::themes::ThemeInfo;

/// Which tab is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Browse,
    Installed,
    Themes,
    Config,
}

impl Tab {
    pub const ALL: &'static [Tab] = &[
        Tab::Browse,
        Tab::Installed,
        Tab::Themes,
        Tab::Config,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            Tab::Browse => " Browse ",
            Tab::Installed => " Installed ",
            Tab::Themes => " Themes ",
            Tab::Config => " Config ",
        }
    }

    pub fn index(&self) -> usize {
        match self {
            Tab::Browse => 0,
            Tab::Installed => 1,
            Tab::Themes => 2,
            Tab::Config => 3,
        }
    }

    pub fn from_index(i: usize) -> Tab {
        match i {
            0 => Tab::Browse,
            1 => Tab::Installed,
            2 => Tab::Themes,
            3 => Tab::Config,
            _ => Tab::Browse,
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

/// Active confirmation dialog.
#[derive(Debug, Clone)]
pub struct ConfirmDialog {
    pub title: String,
    pub message: String,
    pub repo: String,
    pub confirm_selected: bool, // false = Cancel highlighted, true = Confirm
}

/// Status bar message with optional severity.
#[derive(Debug, Clone)]
pub struct StatusMessage {
    pub text: String,
    pub is_error: bool,
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

    // ── Themes tab ──────────────────────────────────
    pub themes_list: Vec<ThemeInfo>,
    pub themes_selected: usize,
    pub themes_scroll_offset: usize,


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
            tab: Tab::Browse,
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

            themes_list: Vec::new(),
            themes_selected: 0,
            themes_scroll_offset: 0,

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
            status: StatusMessage {
                text: "Ready — press ? for help".to_string(),
                is_error: false,
            },
            installed_repos: std::collections::HashSet::new(),
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

        self.refresh_installed();
        self.refresh_themes();
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
        self.refresh_themes();
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
        }
    }

    pub fn refresh_themes(&mut self) {
        if let Some(cfg) = &self.config {
            self.themes_list = crate::themes::get_theme_status(cfg, &self.registry);
        }
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

    /// Load registry from remote / cache / embedded.
    pub async fn load_registry(&mut self) {
        self.registry = registry::load_registry().await;
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
            Tab::Browse => self
                .browse_list
                .get(self.browse_selected)
                .map(|p| p.repo.to_string()),
            Tab::Installed => self
                .installed_list
                .get(self.installed_selected)
                .and_then(|p| p.repo.clone()),
            Tab::Themes => self
                .themes_list
                .get(self.themes_selected)
                .map(|t| t.repo().to_string()),
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
            Tab::Browse => self.browse_list.len(),
            Tab::Installed => self.installed_list.len(),
            Tab::Themes => self.themes_list.len(),
            Tab::Config => self.filtered_settings().len(),
        }
    }

    /// Current selected index (mutable reference).
    pub fn selected_mut(&mut self) -> &mut usize {
        match self.tab {
            Tab::Browse => &mut self.browse_selected,
            Tab::Installed => &mut self.installed_selected,
            Tab::Themes => &mut self.themes_selected,
            Tab::Config => &mut self.settings_selected,
        }
    }

    pub fn scroll_offset_mut(&mut self) -> &mut usize {
        match self.tab {
            Tab::Browse => &mut self.browse_scroll_offset,
            Tab::Installed => &mut self.installed_scroll_offset,
            Tab::Themes => &mut self.themes_scroll_offset,
            Tab::Config => &mut self.settings_scroll_offset,
        }
    }

    pub fn move_selection(&mut self, delta: isize) {
        let len = self.current_list_len();
        if len == 0 {
            return;
        }
        let sel = self.selected_mut();
        let new = (*sel as isize + delta).max(0).min(len as isize - 1) as usize;
        *sel = new;
        self.detail_scroll_offset = 0;
    }
}
