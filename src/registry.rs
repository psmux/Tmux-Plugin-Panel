/// Dynamic plugin registry — fetched from GitHub, cached locally, with embedded fallback.
///
/// Each plugin is tagged with platform compatibility (tmux / psmux / both).
/// On startup the registry is loaded from:
///   1. Remote URL (GitHub raw JSON)  →  cached locally
///   2. Local cache (~/.config/tppanel/registry_cache.json)
///   3. Embedded fallback compiled into the binary

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Default URL to fetch the curated registry JSON.
/// Point this at your own hosted copy to customise the plugin list.
const REGISTRY_URL: &str =
    "https://raw.githubusercontent.com/tppanel/registry/main/plugins.json";

/// Embedded fallback compiled from the repo-local registry.json.
const EMBEDDED_REGISTRY: &str = include_str!("../registry.json");

// ── Compat enum ─────────────────────────────────────────────────────────

/// Platform compatibility tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Compat {
    Tmux,
    #[serde(rename = "psmux")]
    PSMux,
}

impl Compat {
    pub fn label(&self) -> &'static str {
        match self {
            Compat::Tmux => "tmux",
            Compat::PSMux => "psmux",
        }
    }
}

// ── Category enum ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Essential,
    Theme,
    Session,
    Navigation,
    Statusbar,
    Clipboard,
    Utility,
}

impl Category {
    pub fn label(&self) -> &'static str {
        match self {
            Category::Essential => "Essential",
            Category::Theme => "Theme",
            Category::Session => "Session",
            Category::Navigation => "Navigation",
            Category::Statusbar => "Status Bar",
            Category::Clipboard => "Clipboard",
            Category::Utility => "Utility",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            Category::Essential => "⭐",
            Category::Theme => "🎨",
            Category::Session => "💾",
            Category::Navigation => "🧭",
            Category::Statusbar => "📊",
            Category::Clipboard => "📋",
            Category::Utility => "🔧",
        }
    }

    pub const ALL: &'static [Category] = &[
        Category::Essential,
        Category::Theme,
        Category::Session,
        Category::Navigation,
        Category::Statusbar,
        Category::Clipboard,
        Category::Utility,
    ];
}

// ── Plugin entry ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryPlugin {
    pub repo: String,
    pub name: String,
    pub description: String,
    pub category: Category,
    pub stars: u32,
    pub compat: Vec<Compat>,
}

impl RegistryPlugin {
    pub fn short_name(&self) -> &str {
        self.repo.split('/').last().unwrap_or(&self.repo)
    }

    /// Whether this plugin is compatible with the given platform.
    pub fn is_compatible(&self, filter: Compat) -> bool {
        self.compat.contains(&filter)
    }

    /// Short badge string, e.g. "[T+P]", "[T]", "[P]".
    pub fn compat_badge(&self) -> &'static str {
        let has_tmux = self.compat.contains(&Compat::Tmux);
        let has_psmux = self.compat.contains(&Compat::PSMux);
        match (has_tmux, has_psmux) {
            (true, true) => "[T+P]",
            (true, false) => "[T]",
            (false, true) => "[P]",
            _ => "[?]",
        }
    }
}

// ── Registry file wrapper ───────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
struct RegistryFile {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    updated: String,
    plugins: Vec<RegistryPlugin>,
}

// ── Cache paths ─────────────────────────────────────────────────────────

fn cache_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("tppanel")
}

fn cache_path() -> PathBuf {
    cache_dir().join("registry_cache.json")
}

// ── Loading functions ───────────────────────────────────────────────────

/// Load the embedded (compiled-in) registry — always succeeds.
pub fn load_embedded() -> Vec<RegistryPlugin> {
    serde_json::from_str::<RegistryFile>(EMBEDDED_REGISTRY)
        .map(|r| r.plugins)
        .unwrap_or_default()
}

/// Load from local cache file, if it exists and parses.
pub fn load_cache() -> Option<Vec<RegistryPlugin>> {
    let path = cache_path();
    let data = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str::<RegistryFile>(&data)
        .ok()
        .map(|r| r.plugins)
}

/// Save the plugin list to the local cache.
pub fn save_cache(plugins: &[RegistryPlugin]) {
    let reg = RegistryFile {
        version: 2,
        updated: String::new(),
        plugins: plugins.to_vec(),
    };
    if let Ok(data) = serde_json::to_string_pretty(&reg) {
        let dir = cache_dir();
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(cache_path(), data);
    }
}

/// Fetch the registry from the remote GitHub URL.
pub async fn fetch_remote() -> Result<Vec<RegistryPlugin>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .user_agent("tppanel/0.1")
        .build()?;
    let resp = client.get(REGISTRY_URL).send().await?;
    let body = resp.text().await?;
    let reg: RegistryFile = serde_json::from_str(&body)?;
    Ok(reg.plugins)
}

/// Load registry: try remote → cache → embedded fallback.
pub async fn load_registry() -> Vec<RegistryPlugin> {
    // 1. Try remote
    if let Ok(plugins) = fetch_remote().await {
        save_cache(&plugins);
        return plugins;
    }
    // 2. Try local cache
    if let Some(plugins) = load_cache() {
        return plugins;
    }
    // 3. Embedded fallback
    load_embedded()
}

// ── Search & Filter ─────────────────────────────────────────────────────

/// Search the registry by text query, optional category, and optional compat filter.
/// Returns cloned matching entries.
pub fn search_registry(
    registry: &[RegistryPlugin],
    query: &str,
    category: Option<Category>,
    compat_filter: Option<Compat>,
) -> Vec<RegistryPlugin> {
    let q = query.to_lowercase();
    registry
        .iter()
        .filter(|p| {
            // Category filter
            if let Some(cat) = category {
                if p.category != cat {
                    return false;
                }
            }
            // Compat filter — hide incompatible plugins
            if let Some(cf) = compat_filter {
                if !p.compat.contains(&cf) {
                    return false;
                }
            }
            // Text search
            if q.is_empty() {
                return true;
            }
            let searchable = format!(
                "{} {} {} {}",
                p.repo,
                p.name,
                p.description,
                p.category.label()
            )
            .to_lowercase();
            searchable.contains(&q)
        })
        .cloned()
        .collect()
}

/// Find a single plugin in the registry by repo path.
pub fn get_registry_plugin<'a>(
    registry: &'a [RegistryPlugin],
    repo: &str,
) -> Option<&'a RegistryPlugin> {
    registry.iter().find(|p| p.repo == repo)
}

// ── Cached embedded registry (for modules that need lookup without App) ──

use std::sync::OnceLock;
static EMBEDDED_CACHE: OnceLock<Vec<RegistryPlugin>> = OnceLock::new();

/// Get a reference to the embedded registry, loaded once and cached.
pub fn embedded_registry() -> &'static [RegistryPlugin] {
    EMBEDDED_CACHE.get_or_init(load_embedded)
}
