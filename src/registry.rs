/// Plugin registry — bundled with the binary from the repo-local `registry.json`.
///
/// Each plugin is tagged with platform compatibility (tmux / psmux / both).
/// The registry is compiled into the binary via `include_str!` so there is
/// zero network overhead at startup.  Remote fetching may be added later.

use serde::{Deserialize, Serialize};

/// Embedded registry compiled from the repo-local registry.json.
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

// ── Loading functions ───────────────────────────────────────────────────

/// Load the embedded (compiled-in) registry — always succeeds.
pub fn load_embedded() -> Vec<RegistryPlugin> {
    serde_json::from_str::<RegistryFile>(EMBEDDED_REGISTRY)
        .map(|r| r.plugins)
        .unwrap_or_default()
}

/// Load the plugin registry (currently embedded-only; remote fetch may be added later).
pub fn load_registry() -> Vec<RegistryPlugin> {
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
