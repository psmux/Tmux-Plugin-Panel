/// Configuration parsing, modification, and settings.
///
/// Supports both tmux and psmux configuration files.
/// Manages the TPM-style `set -g @plugin 'org/repo'` syntax for plugin tracking.
/// Provides structured settings parsing for the Settings UI.
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use regex::Regex;



// ── Plugin entry ────────────────────────────────────────────────────────

/// A single plugin declaration found in a config file.
#[derive(Debug, Clone)]
pub struct PluginEntry {
    pub raw_line: String,
    pub line_number: usize, // 1-based
    pub repo: String,       // e.g. "tmux-plugins/tmux-sensible"
    pub branch: Option<String>,
    pub source: String,     // "tmux" or "psmux"
    pub enabled: bool,
}

impl PluginEntry {
    pub fn short_name(&self) -> &str {
        self.repo.split('/').last().unwrap_or(&self.repo)
    }

    pub fn github_url(&self) -> String {
        format!("https://github.com/{}", self.repo)
    }
}

// ── Config struct ───────────────────────────────────────────────────────

/// Represents a parsed tmux/psmux config file.
#[derive(Debug, Clone)]
pub struct TmuxConfig {
    pub path: PathBuf,
    pub config_type: String,   // "tmux" or "psmux"
    pub plugins: Vec<PluginEntry>,
    pub lines: Vec<String>,
    pub plugin_install_dir: PathBuf,
}

impl TmuxConfig {
    pub fn display_path(&self) -> String {
        let home = dirs::home_dir().unwrap_or_default();
        let p = self.path.display().to_string();
        let h = home.display().to_string();
        p.replace(&h, "~")
    }

    pub fn type_label(&self) -> &str {
        match self.config_type.as_str() {
            "psmux" => "PSMux",
            _ => "tmux",
        }
    }
}

// ── Settings types ──────────────────────────────────────────────────────

/// The data-type of a setting value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingType {
    Bool,
    Int,
    String,
    Choice, // one of several known values
}

/// A category to group settings in the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingCategory {
    General,
    Display,
    Mouse,
    StatusBar,
    KeyBindings,
    Plugins,
}

impl SettingCategory {
    pub const ALL: &'static [SettingCategory] = &[
        SettingCategory::General,
        SettingCategory::Display,
        SettingCategory::Mouse,
        SettingCategory::StatusBar,
        SettingCategory::KeyBindings,
        SettingCategory::Plugins,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            SettingCategory::General => "General",
            SettingCategory::Display => "Display",
            SettingCategory::Mouse => "Mouse",
            SettingCategory::StatusBar => "Status Bar",
            SettingCategory::KeyBindings => "Key Bindings",
            SettingCategory::Plugins => "Plugins",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            SettingCategory::General => "⚙",
            SettingCategory::Display => "🖥",
            SettingCategory::Mouse => "🖱",
            SettingCategory::StatusBar => "▄",
            SettingCategory::KeyBindings => "⌨",
            SettingCategory::Plugins => "🔌",
        }
    }
}

/// A single config setting — its definition and current value.
#[derive(Debug, Clone)]
pub struct ConfigSetting {
    pub key: String,       // e.g. "mouse"
    pub label: String,     // e.g. "Mouse Support"
    pub description: String,
    pub category: SettingCategory,
    pub stype: SettingType,
    pub value: String,     // current value from config (empty = not set)
    pub default: String,   // default value
    pub choices: Vec<String>, // for Choice type
    pub line_number: Option<usize>, // line in config where this is set (1-based)
}

impl ConfigSetting {
    pub fn is_bool_on(&self) -> bool {
        matches!(self.value.as_str(), "on" | "yes" | "true" | "1")
    }

    pub fn display_value(&self) -> &str {
        if self.value.is_empty() {
            &self.default
        } else {
            &self.value
        }
    }

    pub fn is_default(&self) -> bool {
        self.value.is_empty() || self.value == self.default
    }
}

/// Well-known tmux/psmux settings with descriptions.
fn known_settings() -> Vec<ConfigSetting> {
    vec![
        // ── General ─────────────────────────────────────
        ConfigSetting {
            key: "base-index".into(),
            label: "Window Base Index".into(),
            description: "Start numbering windows from this number (0 or 1)".into(),
            category: SettingCategory::General,
            stype: SettingType::Int,
            value: String::new(), default: "0".into(),
            choices: vec![], line_number: None,
        },
        ConfigSetting {
            key: "pane-base-index".into(),
            label: "Pane Base Index".into(),
            description: "Start numbering panes from this number".into(),
            category: SettingCategory::General,
            stype: SettingType::Int,
            value: String::new(), default: "0".into(),
            choices: vec![], line_number: None,
        },
        ConfigSetting {
            key: "escape-time".into(),
            label: "Escape Time (ms)".into(),
            description: "Delay after Escape key before sending to application. Lower = snappier.".into(),
            category: SettingCategory::General,
            stype: SettingType::Int,
            value: String::new(), default: "500".into(),
            choices: vec![], line_number: None,
        },
        ConfigSetting {
            key: "history-limit".into(),
            label: "Scroll-back Limit".into(),
            description: "Maximum lines of history kept per pane".into(),
            category: SettingCategory::General,
            stype: SettingType::Int,
            value: String::new(), default: "2000".into(),
            choices: vec![], line_number: None,
        },
        ConfigSetting {
            key: "default-shell".into(),
            label: "Default Shell".into(),
            description: "Shell to launch in new windows/panes".into(),
            category: SettingCategory::General,
            stype: SettingType::String,
            value: String::new(), default: "".into(),
            choices: vec![], line_number: None,
        },
        ConfigSetting {
            key: "default-terminal".into(),
            label: "Default Terminal".into(),
            description: "TERM value for new panes (e.g. screen-256color, tmux-256color)".into(),
            category: SettingCategory::General,
            stype: SettingType::Choice,
            value: String::new(), default: "screen".into(),
            choices: vec!["screen".into(), "screen-256color".into(), "tmux-256color".into(), "xterm-256color".into()],
            line_number: None,
        },
        ConfigSetting {
            key: "focus-events".into(),
            label: "Focus Events".into(),
            description: "Pass focus in/out events to applications running inside tmux".into(),
            category: SettingCategory::General,
            stype: SettingType::Bool,
            value: String::new(), default: "off".into(),
            choices: vec![], line_number: None,
        },
        ConfigSetting {
            key: "set-clipboard".into(),
            label: "Clipboard Integration".into(),
            description: "Use terminal clipboard (OSC 52) for copy/paste".into(),
            category: SettingCategory::General,
            stype: SettingType::Choice,
            value: String::new(), default: "external".into(),
            choices: vec!["on".into(), "external".into(), "off".into()],
            line_number: None,
        },

        // ── Display ─────────────────────────────────────
        ConfigSetting {
            key: "display-time".into(),
            label: "Message Display Time (ms)".into(),
            description: "How long status messages are shown".into(),
            category: SettingCategory::Display,
            stype: SettingType::Int,
            value: String::new(), default: "750".into(),
            choices: vec![], line_number: None,
        },
        ConfigSetting {
            key: "display-panes-time".into(),
            label: "Pane Number Display (ms)".into(),
            description: "How long pane numbers are shown (display-panes command)".into(),
            category: SettingCategory::Display,
            stype: SettingType::Int,
            value: String::new(), default: "1000".into(),
            choices: vec![], line_number: None,
        },
        ConfigSetting {
            key: "renumber-windows".into(),
            label: "Renumber Windows".into(),
            description: "Automatically renumber windows when one is closed".into(),
            category: SettingCategory::Display,
            stype: SettingType::Bool,
            value: String::new(), default: "off".into(),
            choices: vec![], line_number: None,
        },
        ConfigSetting {
            key: "allow-rename".into(),
            label: "Allow Window Rename".into(),
            description: "Allow programs to rename windows with escape sequences".into(),
            category: SettingCategory::Display,
            stype: SettingType::Bool,
            value: String::new(), default: "on".into(),
            choices: vec![], line_number: None,
        },
        ConfigSetting {
            key: "automatic-rename".into(),
            label: "Auto-Rename Windows".into(),
            description: "Automatically set window title based on running program".into(),
            category: SettingCategory::Display,
            stype: SettingType::Bool,
            value: String::new(), default: "on".into(),
            choices: vec![], line_number: None,
        },
        ConfigSetting {
            key: "aggressive-resize".into(),
            label: "Aggressive Resize".into(),
            description: "Resize windows based on the smallest client actually viewing it".into(),
            category: SettingCategory::Display,
            stype: SettingType::Bool,
            value: String::new(), default: "off".into(),
            choices: vec![], line_number: None,
        },
        ConfigSetting {
            key: "pane-border-style".into(),
            label: "Pane Border Style".into(),
            description: "Style for pane borders (e.g. fg=white)".into(),
            category: SettingCategory::Display,
            stype: SettingType::String,
            value: String::new(), default: "".into(),
            choices: vec![], line_number: None,
        },
        ConfigSetting {
            key: "pane-active-border-style".into(),
            label: "Active Pane Border".into(),
            description: "Style for the active pane's border (e.g. fg=green)".into(),
            category: SettingCategory::Display,
            stype: SettingType::String,
            value: String::new(), default: "".into(),
            choices: vec![], line_number: None,
        },

        // ── Mouse ───────────────────────────────────────
        ConfigSetting {
            key: "mouse".into(),
            label: "Mouse Support".into(),
            description: "Enable mouse for selecting panes, resizing, and scrolling".into(),
            category: SettingCategory::Mouse,
            stype: SettingType::Bool,
            value: String::new(), default: "off".into(),
            choices: vec![], line_number: None,
        },

        // ── Status Bar ──────────────────────────────────
        ConfigSetting {
            key: "status".into(),
            label: "Show Status Bar".into(),
            description: "Show or hide the status bar at the bottom".into(),
            category: SettingCategory::Display,
            stype: SettingType::Choice,
            value: String::new(), default: "on".into(),
            choices: vec!["on".into(), "off".into(), "2".into(), "3".into(), "4".into(), "5".into()],
            line_number: None,
        },
        ConfigSetting {
            key: "status-position".into(),
            label: "Status Bar Position".into(),
            description: "Place the status bar at the top or bottom of the terminal".into(),
            category: SettingCategory::StatusBar,
            stype: SettingType::Choice,
            value: String::new(), default: "bottom".into(),
            choices: vec!["top".into(), "bottom".into()],
            line_number: None,
        },
        ConfigSetting {
            key: "status-interval".into(),
            label: "Status Refresh (sec)".into(),
            description: "How often to refresh the status bar in seconds".into(),
            category: SettingCategory::StatusBar,
            stype: SettingType::Int,
            value: String::new(), default: "15".into(),
            choices: vec![], line_number: None,
        },
        ConfigSetting {
            key: "status-justify".into(),
            label: "Window List Alignment".into(),
            description: "Alignment of the window list in the status bar".into(),
            category: SettingCategory::StatusBar,
            stype: SettingType::Choice,
            value: String::new(), default: "left".into(),
            choices: vec!["left".into(), "centre".into(), "right".into()],
            line_number: None,
        },
        ConfigSetting {
            key: "status-style".into(),
            label: "Status Bar Style".into(),
            description: "Colors/style for the status bar (e.g. bg=blue,fg=white)".into(),
            category: SettingCategory::StatusBar,
            stype: SettingType::String,
            value: String::new(), default: "".into(),
            choices: vec![], line_number: None,
        },
        ConfigSetting {
            key: "status-left".into(),
            label: "Status Left Content".into(),
            description: "Content shown on the left side of the status bar".into(),
            category: SettingCategory::StatusBar,
            stype: SettingType::String,
            value: String::new(), default: "".into(),
            choices: vec![], line_number: None,
        },
        ConfigSetting {
            key: "status-right".into(),
            label: "Status Right Content".into(),
            description: "Content shown on the right side of the status bar".into(),
            category: SettingCategory::StatusBar,
            stype: SettingType::String,
            value: String::new(), default: "".into(),
            choices: vec![], line_number: None,
        },

        // ── Key Bindings ────────────────────────────────
        ConfigSetting {
            key: "prefix".into(),
            label: "Prefix Key".into(),
            description: "The key combination used as prefix for all tmux commands".into(),
            category: SettingCategory::KeyBindings,
            stype: SettingType::String,
            value: String::new(), default: "C-b".into(),
            choices: vec![], line_number: None,
        },
        ConfigSetting {
            key: "mode-keys".into(),
            label: "Copy Mode Keys".into(),
            description: "Key style for copy mode — vi or emacs".into(),
            category: SettingCategory::KeyBindings,
            stype: SettingType::Choice,
            value: String::new(), default: "emacs".into(),
            choices: vec!["vi".into(), "emacs".into()],
            line_number: None,
        },
        ConfigSetting {
            key: "status-keys".into(),
            label: "Command Prompt Keys".into(),
            description: "Key style for the command prompt — vi or emacs".into(),
            category: SettingCategory::KeyBindings,
            stype: SettingType::Choice,
            value: String::new(), default: "emacs".into(),
            choices: vec!["vi".into(), "emacs".into()],
            line_number: None,
        },
        ConfigSetting {
            key: "repeat-time".into(),
            label: "Repeat Time (ms)".into(),
            description: "Time window for repeatable key bindings after pressing prefix".into(),
            category: SettingCategory::KeyBindings,
            stype: SettingType::Int,
            value: String::new(), default: "500".into(),
            choices: vec![], line_number: None,
        },

        // ── Plugins ────────────────────────────────────
        ConfigSetting {
            key: "TMUX_PLUGIN_MANAGER_PATH".into(),
            label: "Plugin Install Directory".into(),
            description: "Where plugins are installed (TMUX_PLUGIN_MANAGER_PATH)".into(),
            category: SettingCategory::Plugins,
            stype: SettingType::String,
            value: String::new(), default: "~/.tmux/plugins".into(),
            choices: vec![], line_number: None,
        },
    ]
}

/// Parse settings from a TmuxConfig's lines, matching against known settings.
pub fn parse_settings(config: &TmuxConfig) -> Vec<ConfigSetting> {
    let mut settings = known_settings();

    // Patterns for `set -g key value` and `set-option -g key value`
    let set_re = Regex::new(
        r##"^\s*set(?:-option)?\s+(?:-g\s+)?(\S+)\s+['"]*([^'"#\n]+?)['"]*\s*(?:#.*)?$"##
    ).unwrap();
    let set_env_re = Regex::new(
        r#"set-environment\s+-g\s+TMUX_PLUGIN_MANAGER_PATH\s+['"]([^'"]+)['"]\s*$"#
    ).unwrap();
    let setw_re = Regex::new(
        r##"^\s*set(?:-window-option|w)\s+(?:-g\s+)?(\S+)\s+['"]*([^'"#\n]+?)['"]*\s*(?:#.*)?$"##
    ).unwrap();
    let prefix_re = Regex::new(
        r#"^\s*(?:set(?:-option)?)\s+(?:-g\s+)?prefix\s+(\S+)"#
    ).unwrap();

    for (idx, line) in config.lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // Plugin manager path
        if let Some(caps) = set_env_re.captures(trimmed) {
            if let Some(s) = settings.iter_mut().find(|s| s.key == "TMUX_PLUGIN_MANAGER_PATH") {
                s.value = caps.get(1).unwrap().as_str().trim().to_string();
                s.line_number = Some(idx + 1);
            }
            continue;
        }

        // Prefix key
        if let Some(caps) = prefix_re.captures(trimmed) {
            if let Some(s) = settings.iter_mut().find(|s| s.key == "prefix") {
                s.value = caps.get(1).unwrap().as_str().trim().to_string();
                s.line_number = Some(idx + 1);
            }
            continue;
        }

        // set -g / set-option -g
        if let Some(caps) = set_re.captures(trimmed) {
            let key = caps.get(1).unwrap().as_str();
            let val = caps.get(2).unwrap().as_str().trim();
            if let Some(s) = settings.iter_mut().find(|s| s.key == key) {
                s.value = val.to_string();
                s.line_number = Some(idx + 1);
            }
            continue;
        }

        // setw -g / set-window-option -g
        if let Some(caps) = setw_re.captures(trimmed) {
            let key = caps.get(1).unwrap().as_str();
            let val = caps.get(2).unwrap().as_str().trim();
            if let Some(s) = settings.iter_mut().find(|s| s.key == key) {
                s.value = val.to_string();
                s.line_number = Some(idx + 1);
            }
        }
    }

    settings
}

/// Write a setting to the config file.
/// Updates existing line or appends a new `set -g key value` line.
pub fn set_setting(config: &mut TmuxConfig, key: &str, value: &str) -> Result<()> {
    let set_re = Regex::new(
        &format!(r#"^\s*set(?:-option|-window-option|w)?\s+(?:-g\s+)?{}\s+"#, regex::escape(key))
    )?;

    // Special case: TMUX_PLUGIN_MANAGER_PATH uses set-environment
    if key == "TMUX_PLUGIN_MANAGER_PATH" {
        let env_re = Regex::new(r#"set-environment\s+-g\s+TMUX_PLUGIN_MANAGER_PATH"#)?;
        let new_line = format!("set-environment -g TMUX_PLUGIN_MANAGER_PATH '{}'", value);
        if let Some(pos) = config.lines.iter().position(|l| env_re.is_match(l)) {
            config.lines[pos] = new_line;
        } else {
            config.lines.push(new_line);
        }
        write_config(config)?;
        return Ok(());
    }

    // Special case: prefix uses set -g prefix
    let new_line = format!("set -g {} {}", key, value);

    if let Some(pos) = config.lines.iter().position(|l| set_re.is_match(l)) {
        config.lines[pos] = new_line;
    } else {
        // Find a good insertion point — after comments, before plugins
        let insert_at = find_settings_insert_point(config);
        config.lines.insert(insert_at, new_line);
    }

    write_config(config)?;
    Ok(())
}

fn find_settings_insert_point(config: &TmuxConfig) -> usize {
    // Insert before plugin section or before TPM run line, or at end
    for (i, line) in config.lines.iter().enumerate() {
        if line.contains("@plugin") || line.contains("# ── Plugins") || line.contains("run ") {
            return i;
        }
    }
    config.lines.len()
}

// ── Cross-platform config path detection ────────────────────────────────

/// Return ALL candidate config paths in priority order with their type.
/// Covers tmux and psmux paths on Windows, Linux, and macOS.
/// This mirrors detect.rs detect_config_locations — keep in sync.
fn candidate_paths() -> Vec<(PathBuf, &'static str)> {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let xdg = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home.join(".config"));

    let mut paths = Vec::new();

    // ── PSMux paths (from PSMux source config.rs — exact search order) ──
    // PSMux checks: ~/.psmux.conf → ~/.psmuxrc → ~/.tmux.conf → ~/.config/psmux/psmux.conf
    paths.push((home.join(".psmux.conf"), "psmux"));
    paths.push((home.join(".psmuxrc"), "psmux"));
    paths.push((xdg.join("psmux").join("psmux.conf"), "psmux"));

    // ── Windows-specific PSMux paths ────────────────────────────
    #[cfg(target_os = "windows")]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            let appdata = PathBuf::from(&appdata);
            paths.push((appdata.join("psmux").join("psmux.conf"), "psmux"));
            paths.push((appdata.join("psmux").join(".psmux.conf"), "psmux"));
        }
        if let Ok(localappdata) = std::env::var("LOCALAPPDATA") {
            let localappdata = PathBuf::from(&localappdata);
            paths.push((localappdata.join("psmux").join("psmux.conf"), "psmux"));
        }
    }

    // ── tmux paths ──────────────────────────────────────────────
    // XDG path (modern tmux >= 3.1)
    paths.push((xdg.join("tmux").join("tmux.conf"), "tmux"));
    // Classic home path
    paths.push((home.join(".tmux.conf"), "tmux"));

    // ── Windows-specific tmux paths ─────────────────────────────
    #[cfg(target_os = "windows")]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            let appdata = PathBuf::from(&appdata);
            paths.push((appdata.join("tmux").join("tmux.conf"), "tmux"));
        }

        // MSYS2 paths
        let msys2_roots = vec![
            PathBuf::from("C:\\msys64"),
            PathBuf::from("C:\\msys32"),
        ];
        if let Ok(msystem) = std::env::var("MSYSTEM_PREFIX") {
            paths.push((PathBuf::from(&msystem).join("etc").join("tmux.conf"), "tmux"));
        }
        for msys_root in &msys2_roots {
            if msys_root.exists() {
                paths.push((msys_root.join("etc").join("tmux.conf"), "tmux"));
                if let Some(username) = home.file_name() {
                    let msys_user_home = msys_root.join("home").join(username);
                    paths.push((msys_user_home.join(".tmux.conf"), "tmux"));
                    paths.push((msys_user_home.join(".config").join("tmux").join("tmux.conf"), "tmux"));
                }
            }
        }

        // Cygwin
        let cygwin_root = PathBuf::from("C:\\cygwin64");
        if cygwin_root.exists() {
            paths.push((cygwin_root.join("etc").join("tmux.conf"), "tmux"));
            if let Some(username) = home.file_name() {
                paths.push((cygwin_root.join("home").join(username).join(".tmux.conf"), "tmux"));
            }
        }

        // Git Bash
        if let Ok(programfiles) = std::env::var("ProgramFiles") {
            let git_root = PathBuf::from(&programfiles).join("Git");
            if git_root.exists() {
                paths.push((git_root.join("etc").join("tmux.conf"), "tmux"));
            }
        }
    }

    // ── macOS-specific paths ────────────────────────────────────
    #[cfg(target_os = "macos")]
    {
        paths.push((PathBuf::from("/opt/homebrew/etc/tmux.conf"), "tmux"));
        paths.push((PathBuf::from("/usr/local/etc/tmux.conf"), "tmux"));
        paths.push((PathBuf::from("/opt/local/etc/tmux.conf"), "tmux"));
    }

    // ── Linux system-wide paths ─────────────────────────────────
    #[cfg(target_os = "linux")]
    {
        paths.push((PathBuf::from("/etc/tmux.conf"), "tmux"));
        paths.push((PathBuf::from("/etc/tmux/tmux.conf"), "tmux"));
        paths.push((PathBuf::from("/snap/tmux/current/etc/tmux.conf"), "tmux"));
        paths.push((home.join(".nix-profile").join("etc").join("tmux.conf"), "tmux"));
        paths.push((home.join(".linuxbrew").join("etc").join("tmux.conf"), "tmux"));
        paths.push((PathBuf::from("/home/linuxbrew/.linuxbrew/etc/tmux.conf"), "tmux"));
    }

    paths
}

/// Auto-scan for all tmux/psmux config files that exist on this system.
pub fn find_configs() -> Vec<TmuxConfig> {
    let mut found = Vec::new();
    let mut seen_paths = std::collections::HashSet::new();

    for (path, ctype) in candidate_paths() {
        if !path.is_file() {
            continue;
        }
        // Canonicalize to avoid duplicates (e.g. symlinks, same file via different paths)
        let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
        if !seen_paths.insert(canonical) {
            continue;
        }
        match parse_config(&path, ctype) {
            Ok(cfg) => found.push(cfg),
            Err(_) => {
                found.push(TmuxConfig {
                    path: path.clone(),
                    config_type: ctype.to_string(),
                    plugins: Vec::new(),
                    lines: Vec::new(),
                    plugin_install_dir: default_install_dir(&path, ctype),
                });
            }
        }
    }
    found
}

// ── Plugin install directory ────────────────────────────────────────────

fn default_install_dir(config_path: &Path, config_type: &str) -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let xdg = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home.join(".config"));

    match config_type {
        "psmux" => {
            if config_path.starts_with(xdg.join("psmux")) {
                xdg.join("psmux").join("plugins")
            } else {
                home.join(".psmux").join("plugins")
            }
        }
        _ => {
            if config_path.starts_with(xdg.join("tmux")) {
                xdg.join("tmux").join("plugins")
            } else {
                home.join(".tmux").join("plugins")
            }
        }
    }
}

// ── Parsing ─────────────────────────────────────────────────────────────

/// Parse a config file and extract plugin entries.
pub fn parse_config(path: &Path, config_type: &str) -> Result<TmuxConfig> {
    let content =
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;
    let lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();

    let plugin_re = Regex::new(
        r#"^\s*(?:#\s*)?set\s+(?:-g\s+)?@plugin\s+['"]((?:(?:git@(?:github|bitbucket)\.com:|https?://github\.com/)?)([A-Za-z0-9._-]+/[A-Za-z0-9._-]+?)(?:\.git)?(?:#([A-Za-z0-9._/-]+))?)['"]\s*$"#,
    )?;
    let comment_re = Regex::new(r"^\s*#")?;
    let plugin_dir_re = Regex::new(
        r#"set-environment\s+-g\s+TMUX_PLUGIN_MANAGER_PATH\s+['"]([^'"]+)['"]\s*$"#,
    )?;
    // tppanel managed plugin dir marker
    let tppanel_dir_re = Regex::new(
        r#"#\s*tppanel:plugin-dir\s+(.+)\s*$"#,
    )?;
    // PSMux-style plugin loading: source-file '~/.psmux/plugins/<name>/plugin.conf'
    // Also matches: source-file ~/.psmux/plugins/<name>/plugin.conf (without quotes)
    let source_file_plugin_re = Regex::new(
        r##"^\s*(?:#\s*)?source-file\s+['"]?([^'"#\n]+?/plugins/([A-Za-z0-9._-]+)/plugin\.conf)['"]?\s*(?:#.*)?$"##,
    )?;

    let mut plugins = Vec::new();
    let mut install_dir: Option<PathBuf> = None;
    let mut seen_repos = std::collections::HashSet::new();

    for (idx, line) in lines.iter().enumerate() {
        // Check for plugin install dir override (TPM style)
        if let Some(caps) = plugin_dir_re.captures(line) {
            let dir_str = caps.get(1).unwrap().as_str();
            install_dir = Some(expand_home(dir_str));
        }

        // Check for tppanel managed dir
        if let Some(caps) = tppanel_dir_re.captures(line) {
            let dir_str = caps.get(1).unwrap().as_str().trim();
            install_dir = Some(expand_home(dir_str));
        }

        // Check for plugin declarations (@plugin syntax — TPM/tppanel)
        if let Some(caps) = plugin_re.captures(line) {
            let repo = caps.get(2).unwrap().as_str().to_string();
            let branch = caps.get(3).map(|m| m.as_str().to_string());
            let commented = comment_re.is_match(line);
            seen_repos.insert(repo.clone());

            plugins.push(PluginEntry {
                raw_line: line.clone(),
                line_number: idx + 1,
                repo,
                branch,
                source: config_type.to_string(),
                enabled: !commented,
            });
        }

        // Check for PSMux source-file plugin loading
        // source-file ~/.psmux/plugins/psmux-sensible/plugin.conf
        if let Some(caps) = source_file_plugin_re.captures(line) {
            let plugin_name = caps.get(2).unwrap().as_str().to_string();
            let commented = comment_re.is_match(line);

            // Build a repo path — check if it might be a known psmux plugin
            let repo = format!("marlocarlo/psmux-plugins/{}", plugin_name);

            // Don't double-count if also declared via @plugin
            if !seen_repos.contains(&repo) && !seen_repos.contains(&plugin_name) {
                seen_repos.insert(repo.clone());
                plugins.push(PluginEntry {
                    raw_line: line.clone(),
                    line_number: idx + 1,
                    repo,
                    branch: None,
                    source: config_type.to_string(),
                    enabled: !commented,
                });
            }
        }
    }

    let final_dir = install_dir.unwrap_or_else(|| default_install_dir(path, config_type));

    Ok(TmuxConfig {
        path: path.to_path_buf(),
        config_type: config_type.to_string(),
        plugins,
        lines,
        plugin_install_dir: final_dir,
    })
}

fn expand_home(p: &str) -> PathBuf {
    let home = dirs::home_dir().unwrap_or_default();
    let expanded = p.replace('~', &home.display().to_string());
    let expanded = expanded.replace("$HOME", &home.display().to_string());
    PathBuf::from(expanded)
}

// ── Config modification ─────────────────────────────────────────────────

/// Add a plugin line to the config file.
///
/// For configs with a TPM `run` line, inserts before it.
/// For psmux or configs without TPM, uses a managed section with run-shell lines.
pub fn add_plugin_to_config(
    config: &mut TmuxConfig,
    repo: &str,
    branch: Option<&str>,
) -> Result<bool> {
    if config.plugins.iter().any(|p| p.repo == repo) {
        return Ok(false);
    }

    let plugin_str = match branch {
        Some(b) => format!("{}#{}", repo, b),
        None => repo.to_string(),
    };
    let new_line = format!("set -g @plugin '{}'", plugin_str);
    let plugin_name = repo.split('/').last().unwrap_or(repo);

    if has_tpm_run_line(config) {
        // TPM-style: insert @plugin line before the `run` line
        let insert_at = find_insert_point(config);
        config.lines.insert(insert_at, new_line.clone());
    } else {
        // Non-TPM (psmux or standalone): use managed section
        // PSMux plugins use `source-file plugin.conf`, tmux uses `run-shell plugin.tmux`
        let load_line = if config.config_type == "psmux" {
            format!(
                "source-file '{}/{}/plugin.conf'",
                config.plugin_install_dir.display(),
                plugin_name,
            )
        } else {
            format!(
                "run-shell '{}/{}/{}.tmux'",
                config.plugin_install_dir.display(),
                plugin_name,
                plugin_name,
            )
        };

        if has_managed_section(config) {
            // Insert before the "End plugins" marker
            let end_pos = config.lines.iter()
                .position(|l| l.contains("# ── End plugins"))
                .unwrap_or(config.lines.len());
            config.lines.insert(end_pos, load_line);
            config.lines.insert(end_pos, new_line.clone());
        } else {
            // Create managed section at end of file
            config.lines.push(String::new());
            config.lines.push("# ── Plugins (managed by tppanel) ──────────────────────".to_string());
            config.lines.push(new_line.clone());
            config.lines.push(load_line);
            config.lines.push("# ── End plugins ──────────────────────────────────────".to_string());
        }
    }

    write_config(config)?;

    config.plugins.push(PluginEntry {
        raw_line: new_line,
        line_number: 0, // will be recalculated on next parse
        repo: repo.to_string(),
        branch: branch.map(|s| s.to_string()),
        source: config.config_type.clone(),
        enabled: true,
    });

    Ok(true)
}

/// Remove a plugin line from the config file.
/// Also removes associated run-shell lines.
pub fn remove_plugin_from_config(config: &mut TmuxConfig, repo: &str) -> Result<bool> {
    let plugin_re = Regex::new(
        r#"^\s*(?:#\s*)?set\s+(?:-g\s+)?@plugin\s+['"]((?:(?:git@(?:github|bitbucket)\.com:|https?://github\.com/)?)([A-Za-z0-9._-]+/[A-Za-z0-9._-]+?)(?:\.git)?(?:#([A-Za-z0-9._/-]+))?)['"]\s*$"#,
    )?;

    let plugin_name = repo.split('/').last().unwrap_or(repo);

    // Collect indices of lines to remove
    let mut indices_to_remove = Vec::new();
    for (i, line) in config.lines.iter().enumerate() {
        // @plugin line for this repo
        if let Some(caps) = plugin_re.captures(line) {
            let line_repo = caps.get(2).unwrap().as_str();
            if line_repo == repo {
                indices_to_remove.push(i);
            }
        }
        // run-shell line for this plugin
        if line.contains("run-shell") && line.contains(plugin_name) {
            indices_to_remove.push(i);
        }
        // source-file line for this plugin (psmux style)
        if line.contains("source-file") && line.contains(plugin_name) {
            indices_to_remove.push(i);
        }
    }

    if indices_to_remove.is_empty() {
        return Ok(false);
    }

    // Remove in reverse order to preserve indices
    indices_to_remove.sort();
    indices_to_remove.dedup();
    for i in indices_to_remove.into_iter().rev() {
        config.lines.remove(i);
    }

    // Clean up empty managed section if no plugins remain
    let remaining = config.plugins.iter().filter(|p| p.repo != repo).count();
    if remaining == 0 {
        config.lines.retain(|l| {
            !l.contains("# ── Plugins (managed by tppanel)") &&
            !l.contains("# ── End plugins")
        });
    }

    write_config(config)?;
    config.plugins.retain(|p| p.repo != repo);

    Ok(true)
}

// ── Config creation ─────────────────────────────────────────────────────

/// Create a new config file for the given multiplexer type.
pub fn create_default_config(config_type: &str) -> Result<TmuxConfig> {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));

    let (path, content) = match config_type {
        "psmux" => {
            let p = home.join(".psmux.conf");
            let c = "# PSMux configuration\n\
                     # Managed by tppanel — Tmux Plugin Panel\n\
                     #\n\
                     # For more info: https://github.com/marlocarlo/psmux\n\
                     \n\
                     # Enable mouse\n\
                     set -g mouse on\n\
                     \n\
                     # Window numbering\n\
                     set -g base-index 1\n\
                     \n".to_string();
            (p, c)
        }
        _ => {
            let p = home.join(".tmux.conf");
            let c = "# tmux configuration\n\
                     # Managed by tppanel — Tmux Plugin Panel\n\
                     #\n\
                     \n\
                     # Enable mouse\n\
                     set -g mouse on\n\
                     \n\
                     # Window numbering\n\
                     set -g base-index 1\n\
                     \n".to_string();
            (p, c)
        }
    };

    if path.exists() {
        anyhow::bail!("Config file already exists: {}", path.display());
    }

    fs::write(&path, &content)
        .with_context(|| format!("Failed to create {}", path.display()))?;

    parse_config(&path, config_type)
}

// ── Helpers ─────────────────────────────────────────────────────────────

fn write_config(config: &TmuxConfig) -> Result<()> {
    let content = config.lines.join("\n") + "\n";
    fs::write(&config.path, &content)
        .with_context(|| format!("Failed to write {}", config.path.display()))?;
    Ok(())
}

fn has_tpm_run_line(config: &TmuxConfig) -> bool {
    let re = Regex::new(r#"run\s+['"].*tpm/tpm['"]\s*$"#).unwrap();
    config.lines.iter().any(|l| re.is_match(l))
}

fn has_managed_section(config: &TmuxConfig) -> bool {
    config.lines.iter().any(|l| l.contains("# ── Plugins (managed by tppanel)"))
}

fn find_insert_point(config: &TmuxConfig) -> usize {
    // Before TPM run line
    let re = Regex::new(r#"run\s+['"].*tpm/tpm['"]\s*$"#).unwrap();
    if let Some(pos) = config.lines.iter().position(|l| re.is_match(l)) {
        return pos;
    }
    // Before "End plugins" marker
    if let Some(pos) = config.lines.iter().position(|l| l.contains("# ── End plugins")) {
        return pos;
    }
    // End of file
    config.lines.len()
}
