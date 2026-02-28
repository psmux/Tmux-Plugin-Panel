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
/// `config_type` adjusts defaults to match the actual multiplexer.
fn known_settings_for(config_type: &str) -> Vec<ConfigSetting> {
    let is_psmux = config_type == "psmux";

    // PSMux vs tmux default differences (from psmux/src/types.rs vs tmux options-table.c):
    //   mouse:        PSMux=on,  tmux=off
    //   set-clipboard: PSMux=on, tmux=external
    //   plugin dir:   PSMux=~/.psmux/plugins, tmux=~/.tmux/plugins

    let mouse_default = if is_psmux { "on" } else { "off" };
    let clipboard_default = if is_psmux { "on" } else { "external" };
    let plugin_dir_default = if is_psmux { "~/.psmux/plugins" } else { "~/.tmux/plugins" };

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
            value: String::new(), default: clipboard_default.into(),
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
            value: String::new(), default: mouse_default.into(),
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
            value: String::new(), default: plugin_dir_default.into(),
            choices: vec![], line_number: None,
        },
    ]
}

/// Parse settings from a TmuxConfig's lines, matching against known settings.
pub fn parse_settings(config: &TmuxConfig) -> Vec<ConfigSetting> {
    let mut settings = known_settings_for(&config.config_type);

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

/// Reset a single setting to its default by removing it from the config file.
pub fn reset_setting(config: &mut TmuxConfig, key: &str) -> Result<()> {
    let set_re = Regex::new(
        &format!(r#"^\s*set(?:-option|-window-option|w)?\s+(?:-g\s+)?{}\s+"#, regex::escape(key))
    )?;

    // Special case: TMUX_PLUGIN_MANAGER_PATH uses set-environment
    if key == "TMUX_PLUGIN_MANAGER_PATH" {
        let env_re = Regex::new(r#"set-environment\s+-g\s+TMUX_PLUGIN_MANAGER_PATH"#)?;
        config.lines.retain(|l| !env_re.is_match(l));
        write_config(config)?;
        return Ok(());
    }

    // Special case: prefix
    if key == "prefix" {
        let prefix_re = Regex::new(r#"^\s*(?:set(?:-option)?)\s+(?:-g\s+)?prefix\s+"#)?;
        config.lines.retain(|l| !prefix_re.is_match(l));
        write_config(config)?;
        return Ok(());
    }

    config.lines.retain(|l| !set_re.is_match(l));
    write_config(config)?;
    Ok(())
}

/// Reset ALL settings to defaults by removing all `set -g` lines for known settings.
/// Preserves plugin lines, comments, and unknown settings.
pub fn reset_all_settings(config: &mut TmuxConfig) -> Result<usize> {
    let known = known_settings_for(&config.config_type);
    let mut removed = 0usize;

    for setting in &known {
        let key = &setting.key;

        if key == "TMUX_PLUGIN_MANAGER_PATH" {
            let env_re = Regex::new(r#"set-environment\s+-g\s+TMUX_PLUGIN_MANAGER_PATH"#)?;
            let before = config.lines.len();
            config.lines.retain(|l| !env_re.is_match(l));
            removed += before - config.lines.len();
            continue;
        }

        if key == "prefix" {
            let prefix_re = Regex::new(r#"^\s*(?:set(?:-option)?)\s+(?:-g\s+)?prefix\s+"#)?;
            let before = config.lines.len();
            config.lines.retain(|l| !prefix_re.is_match(l));
            removed += before - config.lines.len();
            continue;
        }

        let set_re = Regex::new(
            &format!(r#"^\s*set(?:-option|-window-option|w)?\s+(?:-g\s+)?{}\s+"#, regex::escape(key))
        )?;
        let before = config.lines.len();
        config.lines.retain(|l| !set_re.is_match(l));
        removed += before - config.lines.len();
    }

    write_config(config)?;
    Ok(removed)
}

/// Reset the entire config to a clean default for the given type.
/// Removes ALL content and recreates the default template,
/// preserving the file path.
///
/// ## tmux factory defaults
/// Based on the official tmux source (tmux/tmux on GitHub):
///   - prefix: C-b
///   - escape-time: 500ms
///   - base-index: 0
///   - mouse: off
///   - mode-keys: emacs
///   - status: on (bottom)
///   - history-limit: 2000
///   - default-terminal: screen
///
/// ## PSMux factory defaults
/// Based on the PSMux project (marlocarlo/psmux on GitHub):
///   - prefix: C-b (same as tmux)
///   - escape-time: 500ms
///   - base-index: 0
///   - mouse: off
///   - mode-keys: emacs
///   - status: on (bottom)
///   - history-limit: 2000
///
/// This function writes a CLEAN config that explicitly uses all tmux/psmux
/// defaults (no settings = defaults), with comments documenting each default.
pub fn reset_entire_config(config: &mut TmuxConfig) -> Result<()> {
    // PSMux defaults differ from tmux defaults — sourced directly from:
    //   PSMux: psmux/src/types.rs  AppState::new()
    //   tmux:  https://github.com/tmux/tmux  (options-table.c)
    let content = match config.config_type.as_str() {
        "psmux" => {
            // PSMux built-in defaults from AppState::new() in psmux/src/types.rs:
            //   mouse_enabled: true          (PSMux defaults mouse ON, unlike tmux)
            //   escape_time_ms: 500
            //   history_limit: 2000
            //   window_base_index: 0
            //   pane_base_index: 0
            //   status_visible: true
            //   status_position: "bottom"
            //   status_interval: 15
            //   status_style: "bg=green,fg=black"
            //   mode_keys: "emacs"
            //   focus_events: false
            //   set_clipboard: "on"          (PSMux defaults to "on", not "external")
            //   automatic_rename: true
            //   renumber_windows: false
            //   prefix_key: C-b
            //   repeat_time_ms: 500
            "\
# ─────────────────────────────────────────────────────────────
# PSMux Configuration — Factory Defaults
# ─────────────────────────────────────────────────────────────
# Reset to defaults by tppanel — Tmux Plugin Panel
# For more info: https://github.com/marlocarlo/psmux
#
# PSMux built-in defaults are applied automatically.
# Uncomment and change any line below to override a default.
# To restore a setting to default, comment it out or delete it.
# ─────────────────────────────────────────────────────────────

# ── General ──────────────────────────────────────────────────
# Prefix key (PSMux default: C-b)
# set -g prefix C-b

# Escape key delay in milliseconds (PSMux default: 500)
# set -g escape-time 500

# Repeat time for prefix keys in ms (PSMux default: 500)
# set -g repeat-time 500

# Scrollback history limit (PSMux default: 2000)
# set -g history-limit 2000

# Window/pane base index (PSMux default: 0)
# set -g base-index 0
# setw -g pane-base-index 0

# ── Mouse ────────────────────────────────────────────────────
# Mouse support (PSMux default: on — PSMux enables mouse by default)
# set -g mouse on

# ── Display ──────────────────────────────────────────────────
# Allow window auto-rename (PSMux default: on)
# setw -g automatic-rename on

# Renumber windows when one is closed (PSMux default: off)
# set -g renumber-windows off

# Message display time in ms (PSMux default: 750)
# set -g display-time 750

# Pane number display time in ms (PSMux default: 1000)
# set -g display-panes-time 1000

# ── Status Bar ───────────────────────────────────────────────
# Show status bar (PSMux default: on)
# set -g status on

# Status bar position (PSMux default: bottom)
# set -g status-position bottom

# Status refresh interval in seconds (PSMux default: 15)
# set -g status-interval 15

# Status bar style (PSMux default: bg=green,fg=black)
# set -g status-style 'bg=green,fg=black'

# Status bar justify (PSMux default: left)
# set -g status-justify left

# ── Key Bindings ─────────────────────────────────────────────
# Copy mode keys (PSMux default: emacs)
# setw -g mode-keys emacs

# Focus events (PSMux default: off)
# set -g focus-events off

# Clipboard integration (PSMux default: on)
# set -g set-clipboard on

"
        .to_string()
        }
        _ => "\
# ─────────────────────────────────────────────────────────────
# tmux Configuration — Factory Defaults
# ─────────────────────────────────────────────────────────────
# Reset to defaults by tppanel — Tmux Plugin Panel
# Official tmux source: https://github.com/tmux/tmux
#
# tmux built-in defaults are applied automatically.
# Uncomment and change any line below to override a default.
# To restore a setting to default, comment it out or delete it.
# ─────────────────────────────────────────────────────────────

# ── General ──────────────────────────────────────────────────
# Prefix key (tmux default: C-b)
# set -g prefix C-b

# Escape key delay in milliseconds (tmux default: 500)
# set -g escape-time 500

# Scrollback history limit (tmux default: 2000)
# set -g history-limit 2000

# Window/pane base index (tmux default: 0)
# set -g base-index 0
# setw -g pane-base-index 0

# ── Mouse ────────────────────────────────────────────────────
# Mouse support (tmux default: off)
# set -g mouse off

# ── Display ──────────────────────────────────────────────────
# Terminal type (tmux default: screen)
# set -g default-terminal screen

# Allow window auto-rename (tmux default: on)
# setw -g automatic-rename on

# Renumber windows when one is closed (tmux default: off)
# set -g renumber-windows off

# ── Status Bar ───────────────────────────────────────────────
# Show status bar (tmux default: on)
# set -g status on

# Status bar position (tmux default: bottom)
# set -g status-position bottom

# Status refresh interval in seconds (tmux default: 15)
# set -g status-interval 15

# ── Key Bindings ─────────────────────────────────────────────
# Copy mode keys (tmux default: emacs)
# setw -g mode-keys emacs

# Focus events (tmux default: off)
# set -g focus-events off

# Clipboard integration (tmux default: external)
# set -g set-clipboard external

"
        .to_string(),
    };

    // Backup the original config before overwriting
    let backup_path = config.path.with_extension("conf.bak");
    if config.path.exists() {
        let _ = fs::copy(&config.path, &backup_path);
    }

    // Remove all installed plugin directories so they don't linger on disk
    if config.plugin_install_dir.is_dir() {
        if let Ok(entries) = fs::read_dir(&config.plugin_install_dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    let _ = fs::remove_dir_all(&p);
                }
            }
        }
    }

    fs::write(&config.path, &content)
        .with_context(|| format!("Failed to write {}", config.path.display()))?;

    // Re-parse the freshly written config
    let fresh = parse_config(&config.path, &config.config_type)?;
    config.plugins = fresh.plugins;
    config.lines = fresh.lines;
    config.plugin_install_dir = fresh.plugin_install_dir;

    Ok(())
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

    let mut plugins = Vec::new();
    let mut install_dir: Option<PathBuf> = None;

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

        // Check for plugin declarations
        if let Some(caps) = plugin_re.captures(line) {
            let repo = caps.get(2).unwrap().as_str().to_string();
            let branch = caps.get(3).map(|m| m.as_str().to_string());
            let commented = comment_re.is_match(line);

            plugins.push(PluginEntry {
                raw_line: line.clone(),
                line_number: idx + 1,
                repo,
                branch,
                source: config_type.to_string(),
                enabled: !commented,
            });
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
        // Non-TPM (psmux or standalone): use managed section with
        // source-file or run-shell depending on what the plugin provides.
        let plugin_dir = config.plugin_install_dir.join(plugin_name);
        let plugin_conf = plugin_dir.join("plugin.conf");
        let entry_ps1 = plugin_dir.join(format!("{}.ps1", plugin_name));

        let activation_line = if config.config_type == "psmux" && plugin_conf.exists() {
            // PSMux themes/plugins with plugin.conf — most reliable:
            // source-file applies set-g directives during config load.
            format!("source-file '{}'", plugin_conf.display())
        } else if config.config_type == "psmux" && entry_ps1.exists() {
            // PSMux plugin with .ps1 entry — run it
            format!("run-shell '{}'", entry_ps1.display())
        } else {
            // Default (tmux): run-shell with .tmux entry script
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
            config.lines.insert(end_pos, activation_line);
            config.lines.insert(end_pos, new_line.clone());
        } else {
            // Create managed section at end of file
            config.lines.push(String::new());
            config.lines.push("# ── Plugins (managed by tppanel) ──────────────────────".to_string());
            config.lines.push(new_line.clone());
            config.lines.push(activation_line);
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
        // source-file line for this plugin (psmux themes use source-file 'plugin.conf')
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

/// Repair missing activation lines for @plugin entries.
///
/// Scans the config for `set -g @plugin 'org/name'` entries that have no
/// corresponding `source-file` or `run-shell` line referencing the plugin.
/// For each such entry, generates the correct activation line based on
/// what's on disk (plugin.conf → source-file, .ps1 → run-shell, .tmux → run-shell).
///
/// Only operates on non-TPM configs (psmux or configs without `run '~/.tmux/plugins/tpm/tpm'`).
/// Returns the number of lines repaired.
pub fn repair_missing_activation_lines(config: &mut TmuxConfig) -> usize {
    // Only repair non-TPM configs. TPM handles activation via its run line.
    if has_tpm_run_line(config) {
        return 0;
    }

    let mut repaired = 0;

    // For each plugin, check if there's an activation line
    let plugin_names: Vec<(String, String)> = config.plugins.iter()
        .map(|p| {
            let short = p.repo.split('/').last().unwrap_or(&p.repo).to_string();
            (p.repo.clone(), short)
        })
        .collect();

    for (repo, plugin_name) in &plugin_names {
        // Check if there's already an activation line for this plugin
        let has_activation = config.lines.iter().any(|l| {
            let lt = l.trim();
            (lt.starts_with("source-file") || lt.starts_with("run-shell") || lt.starts_with("run "))
                && lt.contains(plugin_name.as_str())
        });

        if has_activation {
            continue;
        }

        // No activation line — generate one based on what's on disk
        let plugin_dir = config.plugin_install_dir.join(plugin_name);
        if !plugin_dir.is_dir() {
            continue; // plugin not installed, nothing to repair
        }

        let plugin_conf = plugin_dir.join("plugin.conf");
        let entry_ps1 = plugin_dir.join(format!("{}.ps1", plugin_name));
        let entry_tmux = plugin_dir.join(format!("{}.tmux", plugin_name));

        let activation_line = if config.config_type == "psmux" && plugin_conf.exists() {
            format!("source-file '{}'", plugin_conf.display())
        } else if config.config_type == "psmux" && entry_ps1.exists() {
            format!("run-shell '{}'", entry_ps1.display())
        } else if entry_tmux.exists() {
            format!("run-shell '{}'", entry_tmux.display())
        } else if entry_ps1.exists() {
            format!("run-shell '{}'", entry_ps1.display())
        } else {
            continue; // no known entry point, skip
        };

        // Find the @plugin line for this repo and insert after it
        let plugin_line_idx = config.lines.iter().position(|l| {
            l.contains("@plugin") && l.contains(repo.as_str())
        });

        if let Some(idx) = plugin_line_idx {
            config.lines.insert(idx + 1, activation_line);
            repaired += 1;
        }
    }

    if repaired > 0 {
        let _ = write_config(config);
    }

    repaired
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

// ── Unit Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::atomic::{AtomicU64, Ordering};
    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn make_temp_config(content: &str, config_type: &str) -> TmuxConfig {
        let id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join("tppanel-tests");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join(format!("test-{}-{}-{}.conf", config_type, std::process::id(), id));
        fs::write(&path, content).unwrap();
        parse_config(&path, config_type).unwrap()
    }

    fn cleanup_temp(config: &TmuxConfig) {
        let _ = fs::remove_file(&config.path);
    }

    #[test]
    fn test_parse_empty_config() {
        let cfg = make_temp_config("", "tmux");
        assert_eq!(cfg.plugins.len(), 0);
        assert_eq!(cfg.config_type, "tmux");
        cleanup_temp(&cfg);
    }

    #[test]
    fn test_parse_plugin_lines() {
        let content = "\
set -g @plugin 'tmux-plugins/tpm'
set -g @plugin 'tmux-plugins/tmux-sensible'
# set -g @plugin 'tmux-plugins/tmux-yank'
";
        let cfg = make_temp_config(content, "tmux");
        assert_eq!(cfg.plugins.len(), 3);
        assert_eq!(cfg.plugins[0].repo, "tmux-plugins/tpm");
        assert!(cfg.plugins[0].enabled);
        assert_eq!(cfg.plugins[1].repo, "tmux-plugins/tmux-sensible");
        assert!(cfg.plugins[1].enabled);
        assert_eq!(cfg.plugins[2].repo, "tmux-plugins/tmux-yank");
        assert!(!cfg.plugins[2].enabled); // commented out
        cleanup_temp(&cfg);
    }

    #[test]
    fn test_parse_settings_values() {
        let content = "\
set -g mouse on
set -g base-index 1
set -g escape-time 0
set -g status-position top
";
        let cfg = make_temp_config(content, "tmux");
        let settings = parse_settings(&cfg);

        let mouse = settings.iter().find(|s| s.key == "mouse").unwrap();
        assert_eq!(mouse.value, "on");

        let base_idx = settings.iter().find(|s| s.key == "base-index").unwrap();
        assert_eq!(base_idx.value, "1");

        let escape = settings.iter().find(|s| s.key == "escape-time").unwrap();
        assert_eq!(escape.value, "0");

        let status_pos = settings.iter().find(|s| s.key == "status-position").unwrap();
        assert_eq!(status_pos.value, "top");

        cleanup_temp(&cfg);
    }

    #[test]
    fn test_parse_settings_defaults() {
        let cfg = make_temp_config("# empty config\n", "tmux");
        let settings = parse_settings(&cfg);

        let mouse = settings.iter().find(|s| s.key == "mouse").unwrap();
        assert_eq!(mouse.value, ""); // not set
        assert_eq!(mouse.default, "off");
        assert_eq!(mouse.display_value(), "off"); // shows default
        assert!(mouse.is_default());

        cleanup_temp(&cfg);
    }

    #[test]
    fn test_known_settings_have_defaults() {
        let settings = known_settings_for("tmux");
        assert!(!settings.is_empty());
        for s in &settings {
            assert!(!s.key.is_empty(), "Setting key must not be empty");
            assert!(!s.label.is_empty(), "Setting label must not be empty");
            assert!(!s.description.is_empty(), "Setting '{}' must have a description", s.key);
        }
    }

    #[test]
    fn test_setting_categories_cover_all() {
        let settings = known_settings_for("tmux");
        for cat in SettingCategory::ALL {
            let count = settings.iter().filter(|s| s.category == *cat).count();
            // Every category should have at least one setting
            assert!(count > 0, "Category {:?} has no settings", cat);
        }
    }

    #[test]
    fn test_psmux_defaults_differ_from_tmux() {
        let tmux = known_settings_for("tmux");
        let psmux = known_settings_for("psmux");

        let tmux_mouse = tmux.iter().find(|s| s.key == "mouse").unwrap();
        let psmux_mouse = psmux.iter().find(|s| s.key == "mouse").unwrap();
        assert_eq!(tmux_mouse.default, "off");
        assert_eq!(psmux_mouse.default, "on"); // PSMux defaults mouse ON

        let tmux_clip = tmux.iter().find(|s| s.key == "set-clipboard").unwrap();
        let psmux_clip = psmux.iter().find(|s| s.key == "set-clipboard").unwrap();
        assert_eq!(tmux_clip.default, "external");
        assert_eq!(psmux_clip.default, "on"); // PSMux defaults to "on"

        let tmux_dir = tmux.iter().find(|s| s.key == "TMUX_PLUGIN_MANAGER_PATH").unwrap();
        let psmux_dir = psmux.iter().find(|s| s.key == "TMUX_PLUGIN_MANAGER_PATH").unwrap();
        assert_eq!(tmux_dir.default, "~/.tmux/plugins");
        assert_eq!(psmux_dir.default, "~/.psmux/plugins");
    }

    #[test]
    fn test_psmux_settings_show_correct_defaults() {
        let cfg = make_temp_config("# empty psmux config\n", "psmux");
        let settings = parse_settings(&cfg);

        // With no explicit "set -g mouse" line, the display_value should show the PSMux default "on"
        let mouse = settings.iter().find(|s| s.key == "mouse").unwrap();
        assert_eq!(mouse.display_value(), "on");
        assert!(mouse.is_default());

        cleanup_temp(&cfg);
    }

    #[test]
    fn test_set_setting_new() {
        let mut cfg = make_temp_config("# test\n", "tmux");
        set_setting(&mut cfg, "mouse", "on").unwrap();

        // Re-read the file
        let content = fs::read_to_string(&cfg.path).unwrap();
        assert!(content.contains("set -g mouse on"));
        cleanup_temp(&cfg);
    }

    #[test]
    fn test_set_setting_update() {
        let mut cfg = make_temp_config("set -g mouse off\n", "tmux");
        set_setting(&mut cfg, "mouse", "on").unwrap();

        let content = fs::read_to_string(&cfg.path).unwrap();
        assert!(content.contains("set -g mouse on"));
        assert!(!content.contains("set -g mouse off"));
        cleanup_temp(&cfg);
    }

    #[test]
    fn test_reset_setting() {
        let mut cfg = make_temp_config("set -g mouse on\nset -g base-index 1\n", "tmux");
        reset_setting(&mut cfg, "mouse").unwrap();

        let content = fs::read_to_string(&cfg.path).unwrap();
        assert!(!content.contains("mouse"));
        assert!(content.contains("set -g base-index 1"));
        cleanup_temp(&cfg);
    }

    #[test]
    fn test_reset_all_settings() {
        let mut cfg = make_temp_config(
            "set -g mouse on\nset -g base-index 1\nset -g escape-time 0\n",
            "tmux",
        );
        let removed = reset_all_settings(&mut cfg).unwrap();
        assert!(removed >= 3);

        let content = fs::read_to_string(&cfg.path).unwrap();
        assert!(!content.contains("set -g mouse"));
        assert!(!content.contains("set -g base-index"));
        assert!(!content.contains("set -g escape-time"));
        cleanup_temp(&cfg);
    }

    #[test]
    fn test_reset_entire_config_tmux() {
        let mut cfg = make_temp_config(
            "set -g mouse on\nset -g @plugin 'foo/bar'\n",
            "tmux",
        );
        reset_entire_config(&mut cfg).unwrap();

        let content = fs::read_to_string(&cfg.path).unwrap();
        assert!(content.contains("Factory Defaults"));
        assert!(content.contains("tmux"));
        assert!(!content.contains("foo/bar")); // plugins removed
        assert!(cfg.plugins.is_empty());

        // Verify backup was created
        let bak = cfg.path.with_extension("conf.bak");
        assert!(bak.exists(), "Backup file should be created");
        let bak_content = fs::read_to_string(&bak).unwrap();
        assert!(bak_content.contains("foo/bar")); // original content preserved in backup
        let _ = fs::remove_file(&bak);

        // tmux-specific: mouse default should be "off"
        assert!(content.contains("Mouse support (tmux default: off)"));
        // tmux-specific: clipboard default should be "external"
        assert!(content.contains("set-clipboard external"));

        cleanup_temp(&cfg);
    }

    #[test]
    fn test_reset_entire_config_psmux() {
        let mut cfg = make_temp_config(
            "set -g mouse off\nsource-file ~/.psmux/plugins/psmux-sensible/plugin.conf\n",
            "psmux",
        );

        // Use a temp plugin dir so we don't touch the real one
        let temp_plugins = std::env::temp_dir()
            .join("tppanel-tests")
            .join(format!("plugins-{}-{}", std::process::id(), TEST_COUNTER.fetch_add(1, Ordering::SeqCst)));
        let _ = fs::create_dir_all(&temp_plugins);
        cfg.plugin_install_dir = temp_plugins.clone();

        // Create fake plugin directories to verify they get removed
        fs::create_dir_all(temp_plugins.join("psmux-sensible")).unwrap();
        fs::write(temp_plugins.join("psmux-sensible/plugin.conf"), "# fake").unwrap();
        fs::create_dir_all(temp_plugins.join("psmux-theme-catppuccin")).unwrap();
        assert!(temp_plugins.join("psmux-sensible").is_dir());
        assert!(temp_plugins.join("psmux-theme-catppuccin").is_dir());

        reset_entire_config(&mut cfg).unwrap();

        let content = fs::read_to_string(&cfg.path).unwrap();
        assert!(content.contains("PSMux Configuration"));
        assert!(content.contains("Factory Defaults"));
        assert!(content.contains("marlocarlo/psmux"));

        // PSMux-specific: mouse default should be "on" (not "off" like tmux!)
        assert!(content.contains("Mouse support (PSMux default: on"));
        // PSMux-specific: clipboard default should be "on"
        assert!(content.contains("set-clipboard on"));
        // source-file lines should be removed in reset
        assert!(!content.contains("source-file"));

        // Verify plugin directories were deleted from disk
        assert!(!temp_plugins.join("psmux-sensible").exists(), "Plugin dirs should be removed on reset");
        assert!(!temp_plugins.join("psmux-theme-catppuccin").exists(), "Plugin dirs should be removed on reset");

        // Verify backup preserves original
        let bak = cfg.path.with_extension("conf.bak");
        assert!(bak.exists());
        let bak_content = fs::read_to_string(&bak).unwrap();
        assert!(bak_content.contains("source-file"));
        let _ = fs::remove_file(&bak);
        let _ = fs::remove_dir_all(&temp_plugins);

        cleanup_temp(&cfg);
    }

    #[test]
    fn test_add_plugin_to_config() {
        let mut cfg = make_temp_config("# test\n", "tmux");
        let added = add_plugin_to_config(&mut cfg, "tmux-plugins/tmux-sensible", None).unwrap();
        assert!(added);
        assert_eq!(cfg.plugins.len(), 1);
        assert_eq!(cfg.plugins[0].repo, "tmux-plugins/tmux-sensible");

        let content = fs::read_to_string(&cfg.path).unwrap();
        assert!(content.contains("@plugin 'tmux-plugins/tmux-sensible'"));
        cleanup_temp(&cfg);
    }

    #[test]
    fn test_add_plugin_duplicate() {
        let mut cfg = make_temp_config(
            "set -g @plugin 'tmux-plugins/tmux-sensible'\n",
            "tmux",
        );
        let added = add_plugin_to_config(&mut cfg, "tmux-plugins/tmux-sensible", None).unwrap();
        assert!(!added); // already exists
        cleanup_temp(&cfg);
    }

    #[test]
    fn test_remove_plugin_from_config() {
        let mut cfg = make_temp_config(
            "set -g @plugin 'tmux-plugins/tpm'\nset -g @plugin 'tmux-plugins/tmux-sensible'\n",
            "tmux",
        );
        let removed = remove_plugin_from_config(&mut cfg, "tmux-plugins/tmux-sensible").unwrap();
        assert!(removed);
        assert_eq!(cfg.plugins.len(), 1);
        assert_eq!(cfg.plugins[0].repo, "tmux-plugins/tpm");
        cleanup_temp(&cfg);
    }

    #[test]
    fn test_config_setting_is_bool_on() {
        let s = ConfigSetting {
            key: "mouse".into(),
            label: "Mouse".into(),
            description: "".into(),
            category: SettingCategory::Mouse,
            stype: SettingType::Bool,
            value: "on".into(),
            default: "off".into(),
            choices: vec![],
            line_number: None,
        };
        assert!(s.is_bool_on());
    }

    #[test]
    fn test_config_setting_display_value_uses_default() {
        let s = ConfigSetting {
            key: "mouse".into(),
            label: "Mouse".into(),
            description: "".into(),
            category: SettingCategory::Mouse,
            stype: SettingType::Bool,
            value: "".into(),
            default: "off".into(),
            choices: vec![],
            line_number: None,
        };
        assert_eq!(s.display_value(), "off");
        assert!(s.is_default());
    }

    #[test]
    fn test_setting_category_labels() {
        for cat in SettingCategory::ALL {
            assert!(!cat.label().is_empty());
            assert!(!cat.icon().is_empty());
        }
    }

    #[test]
    fn test_plugin_entry_methods() {
        let pe = PluginEntry {
            raw_line: "set -g @plugin 'tmux-plugins/tmux-sensible'".into(),
            line_number: 1,
            repo: "tmux-plugins/tmux-sensible".into(),
            branch: None,
            source: "tmux".into(),
            enabled: true,
        };
        assert_eq!(pe.short_name(), "tmux-sensible");
        assert_eq!(pe.github_url(), "https://github.com/tmux-plugins/tmux-sensible");
    }
}
