/// Theme management for tmux.
///
/// Themes are just plugins, but we surface them with dedicated UI and
/// support switching between them.
use crate::config::TmuxConfig;
use crate::plugins::{self, OpResult};
use crate::registry::{self, Category, RegistryPlugin};

/// A theme with combined registry + install state.
#[derive(Debug, Clone)]
pub struct ThemeInfo {
    pub registry: RegistryPlugin,
    pub installed: bool,
    pub active: bool,
}

impl ThemeInfo {
    pub fn name(&self) -> &str {
        &self.registry.name
    }
    pub fn repo(&self) -> &str {
        &self.registry.repo
    }
    pub fn description(&self) -> &str {
        &self.registry.description
    }
    pub fn stars(&self) -> u32 {
        self.registry.stars
    }
}

/// Get all themes with their install status.
pub fn get_theme_status(config: &TmuxConfig, registry: &[RegistryPlugin]) -> Vec<ThemeInfo> {
    let themes = registry::search_registry(registry, "", Some(Category::Theme), None);
    let installed = plugins::scan_installed_plugins(config);
    let installed_repos: std::collections::HashSet<&str> = installed
        .iter()
        .filter_map(|p| p.repo.as_deref())
        .collect();
    let active_repos: std::collections::HashSet<&str> = config
        .plugins
        .iter()
        .filter(|p| p.enabled && installed_repos.contains(p.repo.as_str()))
        .map(|p| p.repo.as_str())
        .collect();

    let mut infos: Vec<ThemeInfo> = themes
        .into_iter()
        .map(|rp| {
            let is_installed = installed_repos.contains(rp.repo.as_str());
            let is_active = active_repos.contains(rp.repo.as_str());
            ThemeInfo {
                registry: rp,
                installed: is_installed,
                active: is_active,
            }
        })
        .collect();

    // Sort: active first, then installed, then by stars desc
    infos.sort_by(|a, b| {
        b.active
            .cmp(&a.active)
            .then(b.installed.cmp(&a.installed))
            .then(b.stars().cmp(&a.stars()))
    });

    infos
}

pub fn install_theme(repo: &str, config: &mut TmuxConfig) -> OpResult {
    plugins::install_plugin(repo, config, None)
}

pub fn remove_theme(repo: &str, config: &mut TmuxConfig) -> OpResult {
    plugins::remove_plugin(repo, config)
}
