/// Plugin registry — bundled with the binary from the repo-local `registry.json`.
///
/// Each plugin is tagged with platform compatibility (tmux / psmux / both).
/// The registry is compiled into the binary via `include_str!` so there is
/// zero network overhead at startup.
///
/// External registries can be loaded from local JSON files or remote URLs
/// and merged with the embedded registry.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

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

// ── External Registry Sources ───────────────────────────────────────────

/// Type of registry source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceType {
    /// Built into the binary (cannot be removed).
    Embedded,
    /// A local JSON file on disk.
    Local,
    /// A remote URL (fetched via HTTP).
    Remote,
}

/// A registry source — defines where to load plugin entries from.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrySource {
    /// Display name for this source.
    pub name: String,
    /// URL (for Remote) or file path (for Local). Unused for Embedded.
    #[serde(default)]
    pub url: String,
    /// Source type.
    pub source_type: SourceType,
    /// Whether this source is enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

/// Wrapper for the sources config file.
#[derive(Debug, Serialize, Deserialize)]
pub struct RegistrySourcesFile {
    pub sources: Vec<RegistrySource>,
}

impl RegistrySource {
    /// Create the default embedded source entry.
    pub fn embedded() -> Self {
        RegistrySource {
            name: "Built-in Registry".to_string(),
            url: String::new(),
            source_type: SourceType::Embedded,
            enabled: true,
        }
    }
}

/// Default sources file path: `~/.config/tppanel/registry_sources.json`
pub fn sources_config_path() -> PathBuf {
    let config_dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("tppanel");
    config_dir.join("registry_sources.json")
}

/// Load the registry sources from the config file.
/// Returns a default list with just the embedded source if the file doesn't exist.
pub fn load_sources() -> Vec<RegistrySource> {
    let path = sources_config_path();
    if !path.exists() {
        return vec![RegistrySource::embedded()];
    }
    match std::fs::read_to_string(&path) {
        Ok(content) => match serde_json::from_str::<RegistrySourcesFile>(&content) {
            Ok(file) => {
                let mut sources = file.sources;
                // Ensure embedded source is always present
                if !sources.iter().any(|s| s.source_type == SourceType::Embedded) {
                    sources.insert(0, RegistrySource::embedded());
                }
                sources
            }
            Err(_) => vec![RegistrySource::embedded()],
        },
        Err(_) => vec![RegistrySource::embedded()],
    }
}

/// Save the registry sources to the config file.
pub fn save_sources(sources: &[RegistrySource]) -> anyhow::Result<()> {
    let path = sources_config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = RegistrySourcesFile {
        sources: sources.to_vec(),
    };
    let json = serde_json::to_string_pretty(&file)?;
    std::fs::write(&path, json)?;
    Ok(())
}

/// Load plugins from a local JSON file (same schema as registry.json).
pub fn load_from_file(path: &Path) -> anyhow::Result<Vec<RegistryPlugin>> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("Failed to read {}: {}", path.display(), e))?;
    parse_registry_json(&content)
}

/// Parse registry JSON content into plugin entries.
pub fn parse_registry_json(content: &str) -> anyhow::Result<Vec<RegistryPlugin>> {
    let file: RegistryFile = serde_json::from_str(content)
        .map_err(|e| anyhow::anyhow!("Invalid registry JSON: {}", e))?;
    Ok(file.plugins)
}

/// Load all registries from all enabled sources and merge them.
/// Returns the combined list with duplicates removed (by repo path).
pub fn load_all_sources() -> Vec<RegistryPlugin> {
    let sources = load_sources();
    let mut all_plugins: Vec<RegistryPlugin> = Vec::new();
    let mut seen_repos: std::collections::HashSet<String> = std::collections::HashSet::new();

    for source in &sources {
        if !source.enabled {
            continue;
        }
        let plugins = match source.source_type {
            SourceType::Embedded => load_embedded(),
            SourceType::Local => {
                load_from_file(Path::new(&source.url)).unwrap_or_default()
            }
            SourceType::Remote => {
                // Remote loading is async — skip in synchronous context.
                // Use load_from_url() separately for async loading.
                Vec::new()
            }
        };
        for plugin in plugins {
            if seen_repos.insert(plugin.repo.clone()) {
                all_plugins.push(plugin);
            }
        }
    }

    all_plugins
}

/// Validate a registry JSON string. Returns a list of issues found (empty = valid).
pub fn validate_registry(content: &str) -> Vec<String> {
    let mut issues = Vec::new();

    let file: RegistryFile = match serde_json::from_str(content) {
        Ok(f) => f,
        Err(e) => {
            issues.push(format!("JSON parse error: {}", e));
            return issues;
        }
    };

    if file.version == 0 {
        issues.push("Missing or zero 'version' field".to_string());
    }

    if file.plugins.is_empty() {
        issues.push("No plugins in registry".to_string());
        return issues;
    }

    for (i, plugin) in file.plugins.iter().enumerate() {
        if plugin.repo.is_empty() {
            issues.push(format!("Plugin [{}]: missing 'repo' field", i));
        } else if !plugin.repo.contains('/') {
            issues.push(format!(
                "Plugin [{}]: repo '{}' should be in 'owner/name' format",
                i, plugin.repo
            ));
        }
        if plugin.name.is_empty() {
            issues.push(format!("Plugin [{}] ({}): missing 'name' field", i, plugin.repo));
        }
        if plugin.compat.is_empty() {
            issues.push(format!(
                "Plugin [{}] ({}): missing 'compat' — must be [\"tmux\"], [\"psmux\"], or both",
                i, plugin.repo
            ));
        }
    }

    issues
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

// ── Unit Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_embedded_not_empty() {
        let plugins = load_embedded();
        assert!(!plugins.is_empty(), "Embedded registry should have plugins");
    }

    #[test]
    fn test_embedded_plugins_have_repos() {
        let plugins = load_embedded();
        for p in &plugins {
            assert!(!p.repo.is_empty(), "Plugin must have a repo");
            assert!(p.repo.contains('/'), "Repo '{}' should be owner/name format", p.repo);
        }
    }

    #[test]
    fn test_embedded_plugins_have_compat() {
        let plugins = load_embedded();
        for p in &plugins {
            assert!(!p.compat.is_empty(), "Plugin '{}' must have compat tags", p.repo);
        }
    }

    #[test]
    fn test_compat_label() {
        assert_eq!(Compat::Tmux.label(), "tmux");
        assert_eq!(Compat::PSMux.label(), "psmux");
    }

    #[test]
    fn test_category_label_and_icon() {
        for cat in Category::ALL {
            assert!(!cat.label().is_empty());
            assert!(!cat.icon().is_empty());
        }
    }

    #[test]
    fn test_category_all_count() {
        assert_eq!(Category::ALL.len(), 7);
    }

    #[test]
    fn test_compat_badge() {
        let p = RegistryPlugin {
            repo: "test/both".into(),
            name: "Test".into(),
            description: "desc".into(),
            category: Category::Utility,
            stars: 0,
            compat: vec![Compat::Tmux, Compat::PSMux],
        };
        assert_eq!(p.compat_badge(), "[T+P]");

        let p_tmux = RegistryPlugin {
            compat: vec![Compat::Tmux],
            ..p.clone()
        };
        assert_eq!(p_tmux.compat_badge(), "[T]");

        let p_psmux = RegistryPlugin {
            compat: vec![Compat::PSMux],
            ..p.clone()
        };
        assert_eq!(p_psmux.compat_badge(), "[P]");
    }

    #[test]
    fn test_short_name() {
        let p = RegistryPlugin {
            repo: "tmux-plugins/tmux-sensible".into(),
            name: "Sensible".into(),
            description: "".into(),
            category: Category::Essential,
            stars: 100,
            compat: vec![Compat::Tmux],
        };
        assert_eq!(p.short_name(), "tmux-sensible");
    }

    #[test]
    fn test_is_compatible() {
        let p = RegistryPlugin {
            repo: "test/plugin".into(),
            name: "Test".into(),
            description: "".into(),
            category: Category::Utility,
            stars: 0,
            compat: vec![Compat::Tmux],
        };
        assert!(p.is_compatible(Compat::Tmux));
        assert!(!p.is_compatible(Compat::PSMux));
    }

    #[test]
    fn test_search_empty_query() {
        let plugins = load_embedded();
        let results = search_registry(&plugins, "", None, None);
        assert_eq!(results.len(), plugins.len());
    }

    #[test]
    fn test_search_by_text() {
        let plugins = load_embedded();
        let results = search_registry(&plugins, "sensible", None, None);
        assert!(!results.is_empty());
        assert!(results.iter().any(|p| p.name.to_lowercase().contains("sensible")
            || p.repo.to_lowercase().contains("sensible")));
    }

    #[test]
    fn test_search_by_category() {
        let plugins = load_embedded();
        let results = search_registry(&plugins, "", Some(Category::Theme), None);
        assert!(!results.is_empty());
        for p in &results {
            assert_eq!(p.category, Category::Theme);
        }
    }

    #[test]
    fn test_search_by_compat() {
        let plugins = load_embedded();
        let tmux_only = search_registry(&plugins, "", None, Some(Compat::Tmux));
        let psmux_only = search_registry(&plugins, "", None, Some(Compat::PSMux));

        for p in &tmux_only {
            assert!(p.compat.contains(&Compat::Tmux));
        }
        for p in &psmux_only {
            assert!(p.compat.contains(&Compat::PSMux));
        }
    }

    #[test]
    fn test_search_combined_filters() {
        let plugins = load_embedded();
        let results = search_registry(&plugins, "", Some(Category::Theme), Some(Compat::PSMux));
        for p in &results {
            assert_eq!(p.category, Category::Theme);
            assert!(p.compat.contains(&Compat::PSMux));
        }
    }

    #[test]
    fn test_get_registry_plugin_found() {
        let plugins = load_embedded();
        if let Some(first) = plugins.first() {
            let found = get_registry_plugin(&plugins, &first.repo);
            assert!(found.is_some());
            assert_eq!(found.unwrap().repo, first.repo);
        }
    }

    #[test]
    fn test_get_registry_plugin_not_found() {
        let plugins = load_embedded();
        let found = get_registry_plugin(&plugins, "nonexistent/repo");
        assert!(found.is_none());
    }

    #[test]
    fn test_parse_registry_json_valid() {
        let json = r#"{
            "version": 1,
            "updated": "2025-01-01",
            "plugins": [
                {
                    "repo": "test/plugin",
                    "name": "Test Plugin",
                    "description": "A test plugin",
                    "category": "utility",
                    "stars": 42,
                    "compat": ["tmux"]
                }
            ]
        }"#;
        let result = parse_registry_json(json);
        assert!(result.is_ok());
        let plugins = result.unwrap();
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].repo, "test/plugin");
        assert_eq!(plugins[0].stars, 42);
    }

    #[test]
    fn test_parse_registry_json_invalid() {
        let result = parse_registry_json("not json");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_registry_valid() {
        let json = r#"{
            "version": 2,
            "updated": "2025-01-01",
            "plugins": [
                {
                    "repo": "test/plugin",
                    "name": "Test",
                    "description": "desc",
                    "category": "utility",
                    "stars": 0,
                    "compat": ["tmux"]
                }
            ]
        }"#;
        let issues = validate_registry(json);
        assert!(issues.is_empty(), "Expected no issues but got: {:?}", issues);
    }

    #[test]
    fn test_validate_registry_missing_version() {
        let json = r#"{
            "plugins": [
                {
                    "repo": "test/plugin",
                    "name": "Test",
                    "description": "desc",
                    "category": "utility",
                    "stars": 0,
                    "compat": ["tmux"]
                }
            ]
        }"#;
        let issues = validate_registry(json);
        assert!(issues.iter().any(|i| i.contains("version")));
    }

    #[test]
    fn test_validate_registry_bad_repo() {
        let json = r#"{
            "version": 1,
            "plugins": [
                {
                    "repo": "noslash",
                    "name": "Test",
                    "description": "desc",
                    "category": "utility",
                    "stars": 0,
                    "compat": ["tmux"]
                }
            ]
        }"#;
        let issues = validate_registry(json);
        assert!(issues.iter().any(|i| i.contains("owner/name")));
    }

    #[test]
    fn test_validate_registry_invalid_json() {
        let issues = validate_registry("nope");
        assert!(!issues.is_empty());
        assert!(issues[0].contains("parse error"));
    }

    #[test]
    fn test_registry_source_embedded() {
        let src = RegistrySource::embedded();
        assert_eq!(src.source_type, SourceType::Embedded);
        assert!(src.enabled);
        assert!(src.url.is_empty());
    }

    #[test]
    fn test_load_sources_default() {
        // If no config file exists (or exists but we haven't created one),
        // load_sources should return at least the embedded source.
        let sources = load_sources();
        assert!(!sources.is_empty());
        assert!(sources.iter().any(|s| s.source_type == SourceType::Embedded));
    }

    #[test]
    fn test_load_from_file_nonexistent() {
        let result = load_from_file(Path::new("/nonexistent/path/registry.json"));
        assert!(result.is_err());
    }

    #[test]
    fn test_load_from_file_valid() {
        let dir = std::env::temp_dir().join("tppanel-registry-test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test_registry.json");
        let content = r#"{
            "version": 1,
            "updated": "2025-01-01",
            "plugins": [
                {
                    "repo": "custom/plugin",
                    "name": "Custom Plugin",
                    "description": "From custom registry",
                    "category": "utility",
                    "stars": 5,
                    "compat": ["tmux", "psmux"]
                }
            ]
        }"#;
        std::fs::write(&path, content).unwrap();

        let result = load_from_file(&path);
        assert!(result.is_ok());
        let plugins = result.unwrap();
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].repo, "custom/plugin");
        assert_eq!(plugins[0].compat.len(), 2);

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn test_embedded_registry_cached() {
        let r1 = embedded_registry();
        let r2 = embedded_registry();
        // Same pointer — cached via OnceLock
        assert!(std::ptr::eq(r1, r2));
    }
}
