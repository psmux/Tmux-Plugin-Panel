/// Plugin installation, removal, and update engine.
///
/// Handles git operations for installing, updating, and removing tmux plugins.
/// Also scans the plugins directory to find installed plugins.
use std::path::{Path, PathBuf};
use std::process::Command;
use std::fs;



use crate::config::{self, TmuxConfig};
use crate::registry;

/// Monorepo mapping: org prefix → actual GitHub repo containing all plugins
/// as subdirectories. When cloning `psmux-plugins/<name>` fails as an
/// individual repo, we fall back to cloning the full monorepo and extracting
/// just the `<name>` subdirectory.
const MONOREPO_MAP: &[(&str, &str)] = &[
    ("psmux-plugins", "marlocarlo/psmux-plugins"),
];

/// If `repo` is `<org>/<name>` and `<org>` is in MONOREPO_MAP, clone the
/// monorepo, move the `<name>` subdirectory to `target_dir`, and clean up.
fn clone_from_monorepo(repo: &str, target_dir: &Path) -> Option<OpResult> {
    let parts: Vec<&str> = repo.splitn(2, '/').collect();
    if parts.len() != 2 {
        return None;
    }
    let (org, name) = (parts[0], parts[1]);
    let monorepo_url = MONOREPO_MAP.iter()
        .find(|(prefix, _)| *prefix == org)
        .map(|(_, url)| *url);
    let monorepo_url = match monorepo_url {
        Some(u) => u,
        None => return None,
    };

    let tmp_dir = std::env::temp_dir().join(format!("tppanel-monorepo-{}", org));
    let _ = force_remove_dir(&tmp_dir);

    let clone_url = format!("https://github.com/{}.git", monorepo_url);
    let tmp_str = tmp_dir.display().to_string();
    let (ok, output) = run_git(&["clone", "--depth=1", &clone_url, &tmp_str], None);
    if !ok {
        let _ = force_remove_dir(&tmp_dir);
        return Some(OpResult {
            success: false,
            message: format!("Monorepo clone failed ({}): {}", monorepo_url, output),
        });
    }

    let sub_dir = tmp_dir.join(name);
    if !sub_dir.is_dir() {
        let _ = force_remove_dir(&tmp_dir);
        return Some(OpResult {
            success: false,
            message: format!("'{}' not found in monorepo {}", name, monorepo_url),
        });
    }

    // Move the subdirectory to the target location
    if let Err(e) = copy_dir_recursive(&sub_dir, target_dir) {
        let _ = force_remove_dir(&tmp_dir);
        let _ = force_remove_dir(target_dir);
        return Some(OpResult {
            success: false,
            message: format!("Failed to extract '{}' from monorepo: {}", name, e),
        });
    }

    let _ = force_remove_dir(&tmp_dir);
    Some(OpResult {
        success: true,
        message: format!("Installed '{}' from monorepo {}", name, monorepo_url),
    })
}

/// An installed plugin on disk.
#[derive(Debug, Clone)]
pub struct InstalledPlugin {
    pub name: String,
    pub path: PathBuf,
    pub repo: Option<String>,
    pub branch: Option<String>,
    pub current_commit: Option<String>,
    pub remote_url: Option<String>,
    pub in_config: bool,
}

impl InstalledPlugin {
    pub fn display_name(&self) -> String {
        if let Some(repo) = &self.repo {
            if let Some(rp) = registry::get_registry_plugin(registry::embedded_registry(), repo) {
                return rp.name.to_string();
            }
        }
        self.name.clone()
    }

    pub fn description(&self) -> String {
        if let Some(repo) = &self.repo {
            if let Some(rp) = registry::get_registry_plugin(registry::embedded_registry(), repo) {
                return rp.description.to_string();
            }
        }
        format!("Installed at {}", self.path.display())
    }
}

/// Result of a plugin operation.
#[derive(Debug, Clone)]
pub struct OpResult {
    pub success: bool,
    pub message: String,
}

// ── Robust directory removal (Windows read-only .git files) ─────────────

/// Check whether a directory contains at least one real file (not just empty dirs).
fn dir_has_content(path: &Path) -> bool {
    fs::read_dir(path)
        .map(|entries| {
            entries.flatten().any(|e| {
                let p = e.path();
                p.is_file() || (p.is_dir() && dir_has_content(&p))
            })
        })
        .unwrap_or(false)
}

/// Remove a directory tree, clearing read-only flags first (needed on Windows
/// where git marks objects read-only).
fn force_remove_dir(path: &Path) -> std::io::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    // First pass: clear read-only attributes on all files
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                let _ = force_remove_dir(&p);
            } else {
                // Clear read-only flag
                if let Ok(md) = fs::metadata(&p) {
                    let mut perms = md.permissions();
                    if perms.readonly() {
                        perms.set_readonly(false);
                        let _ = fs::set_permissions(&p, perms);
                    }
                }
                let _ = fs::remove_file(&p);
            }
        }
    }
    fs::remove_dir_all(path)
}

// ── Git helpers ─────────────────────────────────────────────────────────

fn run_git(args: &[&str], cwd: Option<&Path>) -> (bool, String) {
    let mut cmd = Command::new("git");
    cmd.args(args);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    match cmd.output() {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            // Combine both streams so error details aren't lost
            let msg = match (stdout.is_empty(), stderr.is_empty()) {
                (true, true) => String::new(),
                (false, true) => stdout,
                (true, false) => stderr,
                (false, false) => format!("{} | {}", stdout, stderr),
            };
            // Strip noisy "Cloning into '...'" prefix to surface the real error
            let msg = msg
                .lines()
                .filter(|l| !l.starts_with("Cloning into"))
                .collect::<Vec<_>>()
                .join(" ");
            (output.status.success(), msg)
        }
        Err(e) => (false, format!("git error: {}", e)),
    }
}

fn get_remote_url(dir: &Path) -> Option<String> {
    let (ok, url) = run_git(&["config", "--get", "remote.origin.url"], Some(dir));
    if ok { Some(url) } else { None }
}

fn get_current_commit(dir: &Path) -> Option<String> {
    let (ok, hash) = run_git(&["rev-parse", "--short", "HEAD"], Some(dir));
    if ok { Some(hash) } else { None }
}

fn get_current_branch(dir: &Path) -> Option<String> {
    let (ok, branch) = run_git(&["rev-parse", "--abbrev-ref", "HEAD"], Some(dir));
    if ok { Some(branch) } else { None }
}

fn extract_repo_from_url(url: &str) -> Option<String> {
    if url.starts_with("git@") {
        // git@github.com:org/repo.git
        let parts: Vec<&str> = url.splitn(2, ':').collect();
        if parts.len() == 2 {
            let repo = parts[1].trim_end_matches('/').trim_end_matches(".git");
            return Some(repo.to_string());
        }
    }
    if url.contains("github.com") {
        let parts: Vec<&str> = url.splitn(2, "github.com/").collect();
        if parts.len() == 2 {
            let repo = parts[1].trim_end_matches('/').trim_end_matches(".git");
            return Some(repo.to_string());
        }
    }
    None
}

// ── Scanning installed plugins ──────────────────────────────────────────

pub fn scan_installed_plugins(config: &TmuxConfig) -> Vec<InstalledPlugin> {
    let install_dir = &config.plugin_install_dir;
    if !install_dir.is_dir() {
        return Vec::new();
    }

    let config_repos: std::collections::HashSet<&str> =
        config.plugins.iter().map(|p| p.repo.as_str()).collect();

    let mut plugins = Vec::new();

    let entries = match fs::read_dir(install_dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }

        // Skip monorepo parent directories (containers, not plugins themselves)
        let is_monorepo_parent = MONOREPO_MAP.iter().any(|(prefix, _)| *prefix == name);
        if is_monorepo_parent {
            continue;
        }

        // Skip empty or content-less directories
        if !dir_has_content(&path) {
            continue;
        }

        let remote_url = get_remote_url(&path);
        let repo = remote_url.as_deref().and_then(extract_repo_from_url);

        // If no git remote, derive repo from directory name using registry.
        // This handles monorepo-installed plugins that were copied without .git.
        let repo = repo.or_else(|| {
            let registry = crate::registry::embedded_registry();
            registry.iter()
                .find(|rp| rp.repo.split('/').last() == Some(name.as_str()))
                .map(|rp| rp.repo.clone())
        });

        // Defer expensive git calls — commit/branch are only needed on demand
        let commit = None;
        let branch = None;

        let in_config = repo
            .as_deref()
            .map(|r| config_repos.contains(r))
            .unwrap_or(false);

        // Also check if this plugin is referenced via source-file or run-shell
        // in the config (even without an @plugin line).
        let in_config = in_config || config.lines.iter().any(|l| {
            let lt = l.trim();
            (lt.starts_with("source-file") || lt.starts_with("run-shell") || lt.starts_with("run "))
                && lt.contains(&name)
        });

        plugins.push(InstalledPlugin {
            name,
            path,
            repo,
            branch,
            current_commit: commit,
            remote_url,
            in_config,
        });
    }

    plugins.sort_by(|a, b| a.name.cmp(&b.name));
    plugins
}

// ── Install ─────────────────────────────────────────────────────────────

pub fn install_plugin(
    repo: &str,
    config: &mut TmuxConfig,
    branch: Option<&str>,
) -> OpResult {
    let install_dir = &config.plugin_install_dir;
    if let Err(e) = fs::create_dir_all(install_dir) {
        return OpResult {
            success: false,
            message: format!("Cannot create plugin dir: {}", e),
        };
    }

    let plugin_name = repo.split('/').last().unwrap_or(repo);
    let target_dir = install_dir.join(plugin_name);

    if target_dir.exists() {
        // If it has a .git dir OR contains real content (e.g. plugin.conf,
        // scripts), treat it as a valid prior install.  PSMux themes that
        // were installed from a monorepo or by PPM won't have .git but are
        // perfectly valid.
        let has_git = target_dir.join(".git").exists();
        let has_content = !has_git && dir_has_content(&target_dir);
        if has_git || has_content {
            // Already present — just make sure it's registered in the config
            let _ = config::add_plugin_to_config(config, repo, branch);
            return OpResult {
                success: true,
                message: format!("'{}' already installed", plugin_name),
            };
        }
        // Truly empty / stale partial dir — safe to clean up
        let _ = force_remove_dir(&target_dir);
        if target_dir.exists() {
            return OpResult {
                success: false,
                message: format!("Cannot remove stale dir for '{}'", plugin_name),
            };
        }
    }

    // ── Try local monorepo first (fast, no network) ─────────────────
    // For "psmux-plugins/psmux-theme-X", check if ~/.psmux/plugins/psmux-plugins/
    // has the subdirectory and copy from there.
    {
        let parts: Vec<&str> = repo.splitn(2, '/').collect();
        if parts.len() == 2 {
            let (org, name) = (parts[0], parts[1]);
            let monorepo_local = install_dir.join(org);
            if monorepo_local.is_dir() {
                if !monorepo_local.join(name).is_dir() && monorepo_local.join(".git").exists() {
                    // Subdirectory missing — try git pull to get latest
                    let _ = run_git(&["pull", "--ff-only"], Some(&monorepo_local));
                }
                let sub = monorepo_local.join(name);
                if sub.is_dir() {
                    match copy_dir_recursive(&sub, &target_dir) {
                        Ok(()) => {
                            let _ = config::add_plugin_to_config(config, repo, branch);
                            return OpResult {
                                success: true,
                                message: format!("Installed '{}' from local monorepo", plugin_name),
                            };
                        }
                        Err(_) => {
                            // Copy failed — clean up and try clone
                            let _ = force_remove_dir(&target_dir);
                        }
                    }
                }
            }
        }
    }

    let clone_url = format!("https://github.com/{}.git", repo);
    let target_str = target_dir.display().to_string();

    let mut args = vec!["clone"];
    if let Some(b) = branch {
        args.push("-b");
        args.push(b);
    }
    args.extend_from_slice(&["--depth=1", &clone_url, &target_str]);

    let (ok, output) = run_git(&args, None);
    if !ok {
        // Clean up partial clone (force-remove for Windows read-only .git files)
        let _ = force_remove_dir(&target_dir);

        // Fallback: try monorepo clone for orgs like psmux-plugins
        if let Some(mono_result) = clone_from_monorepo(repo, &target_dir) {
            if !mono_result.success {
                return mono_result;
            }
            // Monorepo extraction succeeded — fall through to add to config
        } else {
            return OpResult {
                success: false,
                message: format!("Clone failed: {}", output),
            };
        }
    }

    // Add to config
    let _ = config::add_plugin_to_config(config, repo, branch);

    OpResult {
        success: true,
        message: format!("Installed '{}' successfully", plugin_name),
    }
}

// ── Remove ──────────────────────────────────────────────────────────────

pub fn remove_plugin(repo: &str, config: &mut TmuxConfig) -> OpResult {
    let plugin_name = repo.split('/').last().unwrap_or(repo);
    let target_dir = config.plugin_install_dir.join(plugin_name);

    if target_dir.exists() {
        if let Err(e) = force_remove_dir(&target_dir) {
            return OpResult {
                success: false,
                message: format!("Failed to delete: {}", e),
            };
        }
    }

    let _ = config::remove_plugin_from_config(config, repo);

    OpResult {
        success: true,
        message: format!("Removed '{}' successfully", plugin_name),
    }
}

// ── Update ──────────────────────────────────────────────────────────────

pub fn update_plugin(plugin: &InstalledPlugin) -> OpResult {
    if !plugin.path.exists() {
        return OpResult {
            success: false,
            message: format!("Plugin dir not found: {}", plugin.path.display()),
        };
    }

    let (ok, output) = run_git(&["fetch", "--depth=1"], Some(&plugin.path));
    if !ok {
        return OpResult {
            success: false,
            message: format!("Fetch failed: {}", output),
        };
    }

    let branch = plugin.branch.as_deref().unwrap_or("HEAD");
    let target = if branch == "HEAD" {
        "origin/HEAD".to_string()
    } else {
        format!("origin/{}", branch)
    };

    let (ok, _output) = run_git(&["reset", "--hard", &target], Some(&plugin.path));
    if !ok {
        // Fallback: pull
        let (ok2, output2) = run_git(&["pull", "--ff-only"], Some(&plugin.path));
        if !ok2 {
            return OpResult {
                success: false,
                message: format!("Update failed: {}", output2),
            };
        }
    }

    OpResult {
        success: true,
        message: format!("Updated '{}' successfully", plugin.name),
    }
}

pub fn update_all_plugins(config: &TmuxConfig) -> Vec<OpResult> {
    scan_installed_plugins(config)
        .iter()
        .map(|p| update_plugin(p))
        .collect()
}

// ── Reload / Source ─────────────────────────────────────────────────────

/// Reload configuration using the appropriate multiplexer binary.
/// Detects tmux vs psmux and sources the correct config file.
pub fn reload_config(config: &TmuxConfig, detected: &[crate::detect::DetectedMux]) -> OpResult {
    let kind = match config.config_type.as_str() {
        "psmux" => crate::detect::MuxKind::PSMux,
        _ => crate::detect::MuxKind::Tmux,
    };
    // Build a temporary report to use reload_binary
    let binary = {
        let mut best = match kind {
            crate::detect::MuxKind::PSMux => "psmux".to_string(),
            crate::detect::MuxKind::Tmux => "tmux".to_string(),
        };
        for name in &["psmux", "pmux", "tmux"] {
            if detected.iter().any(|d| d.binary == *name) {
                best = name.to_string();
                break;
            }
        }
        best
    };
    let conf_path = config.path.display().to_string();

    match Command::new(&binary)
        .args(["source-file", &conf_path])
        .output()
    {
        Ok(output) if output.status.success() => OpResult {
            success: true,
            message: format!("{} config reloaded ({})", config.type_label(), config.display_path()),
        },
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // If source-file fails because no server is running, that's ok
            if stderr.contains("no server running") || stderr.contains("no current client") {
                OpResult {
                    success: true,
                    message: format!(
                        "{} config saved (no {} session running)",
                        config.type_label(),
                        binary
                    ),
                }
            } else {
                OpResult {
                    success: false,
                    message: format!("Reload failed: {}", stderr.trim()),
                }
            }
        }
        Err(_) => OpResult {
            success: true,
            message: format!(
                "{} config saved ({} binary not found, will apply on next session start)",
                config.type_label(),
                binary,
            ),
        },
    }
}

/// Launch a temporary preview session of tmux/psmux with only the specified
/// plugin or theme installed. Creates a temp config, clones the plugin to a
/// temp dir, and starts a new mux session. Returns when the session ends.
pub fn preview_plugin(
    repo: &str,
    config: &TmuxConfig,
    detected: &[crate::detect::DetectedMux],
) -> OpResult {
    use std::env;

    let kind = match config.config_type.as_str() {
        "psmux" => crate::detect::MuxKind::PSMux,
        _ => crate::detect::MuxKind::Tmux,
    };

    // Determine the binary to use
    let binary = {
        let mut best = match kind {
            crate::detect::MuxKind::PSMux => "psmux".to_string(),
            crate::detect::MuxKind::Tmux => "tmux".to_string(),
        };
        for name in &["psmux", "pmux", "tmux"] {
            if detected.iter().any(|d| d.binary == *name) {
                best = name.to_string();
                break;
            }
        }
        best
    };

    let plugin_name = repo.split('/').last().unwrap_or(repo);

    // Create temp directory for preview
    let tmp_base = env::temp_dir().join("tppanel-preview");
    let _ = fs::create_dir_all(&tmp_base);
    let preview_dir = tmp_base.join(format!("preview-{}", plugin_name));

    // Clean up any previous preview for this plugin
    if preview_dir.exists() {
        let _ = force_remove_dir(&preview_dir);
    }
    let _ = fs::create_dir_all(&preview_dir);

    let plugins_dir = preview_dir.join("plugins");
    let _ = fs::create_dir_all(&plugins_dir);

    let target_dir = plugins_dir.join(plugin_name);

    // ── Try to reuse the already-installed copy first ──────────────
    let already_installed_dir = config.plugin_install_dir.join(plugin_name);
    let mut have_local_copy = false;
    let mut copy_err: Option<String> = None;

    if already_installed_dir.is_dir() {
        // Copy the installed plugin to our preview dir
        match copy_dir_recursive(&already_installed_dir, &target_dir) {
            Ok(()) => { have_local_copy = true; }
            Err(e) => { copy_err = Some(format!("{}", e)); }
        }
    }

    // Fallback: if the plugin is from a monorepo and the full monorepo is
    // cloned locally, extract the subdirectory from there.
    if !have_local_copy {
        let parts: Vec<&str> = repo.splitn(2, '/').collect();
        if parts.len() == 2 {
            let (org, name) = (parts[0], parts[1]);
            // Check if the monorepo directory itself is installed
            let monorepo_local = config.plugin_install_dir.join(org);
            if monorepo_local.is_dir() {
                let sub = monorepo_local.join(name);
                if !sub.is_dir() {
                    // Subdir missing — try `git pull` to update the local clone
                    if monorepo_local.join(".git").exists() {
                        let _ = run_git(&["pull", "--ff-only"], Some(&monorepo_local));
                    }
                }
                // Re-check after possible pull
                let sub = monorepo_local.join(name);
                if sub.is_dir() {
                    // Clean partial target from failed copy above
                    if target_dir.exists() { let _ = force_remove_dir(&target_dir); }
                    match copy_dir_recursive(&sub, &target_dir) {
                        Ok(()) => { have_local_copy = true; copy_err = None; }
                        Err(e) => { copy_err = Some(format!("monorepo local: {}", e)); }
                    }
                }
            }
        }
    }

    // ── If no local copy, clone from GitHub ───────────────────────
    if !have_local_copy {
        // Clean up any partial target before cloning
        if target_dir.exists() { let _ = force_remove_dir(&target_dir); }

        let clone_url = format!("https://github.com/{}.git", repo);
        let target_str = target_dir.display().to_string();

        let (ok, output) = run_git(&["clone", "--depth=1", &clone_url, &target_str], None);
        if !ok {
            // Clean up partial clone
            if target_dir.exists() { let _ = force_remove_dir(&target_dir); }

            // Try monorepo fallback for psmux-plugins/* etc.
            let mono_result = clone_from_monorepo(repo, &target_dir);
            let mono_ok = mono_result.as_ref().map(|r| r.success).unwrap_or(false);
            let mono_err = mono_result.as_ref()
                .filter(|r| !r.success)
                .map(|r| r.message.clone());

            if !mono_ok {
                // Clean up any partial monorepo extraction
                if target_dir.exists() { let _ = force_remove_dir(&target_dir); }

                // Try SSH URL as last resort
                let ssh_url = format!("git@github.com:{}.git", repo);
                let (ok2, output2) = run_git(&["clone", "--depth=1", &ssh_url, &target_str], None);
                if !ok2 {
                    let _ = force_remove_dir(&preview_dir);

                    // Build detailed error showing which step failed
                    let mut detail = format!("HTTPS: {}", output);
                    if let Some(me) = mono_err {
                        detail.push_str(&format!(" | Monorepo: {}", me));
                    }
                    if let Some(ref ce) = copy_err {
                        detail.push_str(&format!(" | Copy: {}", ce));
                    }
                    detail.push_str(&format!(" | SSH: {}", output2));

                    return OpResult {
                        success: false,
                        message: format!(
                            "Preview failed for '{}'. {}",
                            plugin_name, detail
                        ),
                    };
                }
            }
        }
    }

    // Build a minimal temp config
    let conf_path = preview_dir.join(format!("{}.conf", config.config_type));
    let mut conf_lines = Vec::new();
    conf_lines.push(format!("# tppanel preview — {} (temporary)", plugin_name));
    conf_lines.push(String::new());
    conf_lines.push("set -g mouse on".to_string());
    conf_lines.push("set -g base-index 1".to_string());
    conf_lines.push(String::new());

    // ── Source plugin theme/settings ─────────────────────────────────
    //
    // psmux plugins use:  plugin.conf  (static set-g directives)
    //                     <name>.ps1   (PowerShell script that calls psmux set -g ...)
    // tmux  plugins use:  <name>.tmux  (shell script entry point)
    //
    // For previews the most reliable approach is:
    //   1) plugin.conf — embed its set-g lines directly (works during config load)
    //   2) .ps1 script — use run-shell to execute it (psmux)
    //   3) .tmux script — use run-shell to execute it (tmux)
    //   4) search subdirs for .tmux/.ps1 scripts

    let plugin_conf = target_dir.join("plugin.conf");
    let mut has_theme_source = false;
    if plugin_conf.exists() {
        // Static conf — source it directly (most reliable for psmux themes).
        // This contains all the set-g directives and works during config load
        // (unlike .ps1 scripts which call `psmux set -g` via the control port
        // that isn't ready during config parsing).
        conf_lines.push(format!("source-file '{}'", plugin_conf.display()));
        has_theme_source = true;
    } else {
        // No plugin.conf — look for entry scripts (.tmux or .ps1)
        let entry_tmux = target_dir.join(format!("{}.tmux", plugin_name));
        let entry_ps1 = target_dir.join(format!("{}.ps1", plugin_name));

        // For psmux on Windows, .tmux files (bash scripts) can't be executed
        // via run-shell (pwsh can't run bash). Instead, parse the .tmux file
        // for `tmux source` commands and emit `source-file` directives, or
        // directly find and source .conf files from the plugin directory.
        if kind == crate::detect::MuxKind::PSMux {
            // Strategy for psmux: prefer sourcing .conf files directly, or
            // parse .tmux scripts for source commands.
            let mut sourced = false;

            // If there's a .tmux entry script, parse it for source commands
            if entry_tmux.exists() {
                if let Ok(script_content) = fs::read_to_string(&entry_tmux) {
                    for script_line in script_content.lines() {
                        let sl = script_line.trim();
                        // Look for: tmux source "path" or tmux source-file "path"
                        if sl.starts_with("tmux source-file ") || sl.starts_with("tmux source ") {
                            let path_part = sl.splitn(3, ' ').nth(2).unwrap_or("").trim();
                            // Expand $PLUGIN_DIR and ${PLUGIN_DIR}
                            let expanded = path_part
                                .trim_matches('"').trim_matches('\'')
                                .replace("${PLUGIN_DIR}", &target_dir.display().to_string())
                                .replace("$PLUGIN_DIR", &target_dir.display().to_string());
                            let conf_p = std::path::Path::new(&expanded);
                            if conf_p.is_file() {
                                conf_lines.push(format!("source-file '{}'", expanded));
                                sourced = true;
                            }
                        }
                    }
                }
            }

            // Fallback: find and source any .conf files in the plugin directory
            if !sourced {
                let mut conf_files: Vec<String> = Vec::new();
                if let Ok(entries) = fs::read_dir(&target_dir) {
                    for entry in entries.flatten() {
                        let p = entry.path();
                        if p.is_file() {
                            let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
                            if ext == "conf" {
                                conf_files.push(p.display().to_string());
                            }
                        }
                    }
                }
                // Sort so options files come before main files
                conf_files.sort();
                for cf in &conf_files {
                    conf_lines.push(format!("source-file '{}'", cf));
                    sourced = true;
                }
            }

            // Last resort: try .ps1 entry script
            if !sourced && entry_ps1.exists() {
                conf_lines.push(format!("run-shell '{}'", entry_ps1.display()));
            } else if !sourced {
                // Search subdirectories for .ps1 or .conf files
                for subdir in &["scripts", "plugin", "src"] {
                    let sub = target_dir.join(subdir);
                    if sub.is_dir() {
                        if let Ok(entries) = fs::read_dir(&sub) {
                            for entry in entries.flatten() {
                                let p = entry.path();
                                if p.is_file() {
                                    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
                                    if ext == "ps1" {
                                        conf_lines.push(format!("run-shell '{}'", p.display()));
                                        sourced = true;
                                        break;
                                    } else if ext == "conf" {
                                        conf_lines.push(format!("source-file '{}'", p.display()));
                                        sourced = true;
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    if sourced { break; }
                }
            }
        } else {
            // tmux mode — use run-shell as before (bash scripts work on Linux/macOS)
            if entry_tmux.exists() {
                conf_lines.push(format!("run-shell '{}'", entry_tmux.display()));
            } else if entry_ps1.exists() {
                conf_lines.push(format!("run-shell '{}'", entry_ps1.display()));
            } else {
                let mut found_entry = false;
                if let Ok(entries) = fs::read_dir(&target_dir) {
                    for entry in entries.flatten() {
                        let p = entry.path();
                        if p.is_file() {
                            let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
                            if ext == "tmux" {
                                conf_lines.push(format!("run-shell '{}'", p.display()));
                                found_entry = true;
                                break;
                            } else if ext == "ps1" {
                                conf_lines.push(format!("run-shell '{}'", p.display()));
                                found_entry = true;
                                break;
                            }
                        }
                    }
                }
                if !found_entry {
                    for subdir in &["scripts", "plugin", "src"] {
                        let sub = target_dir.join(subdir);
                        if sub.is_dir() {
                            if let Ok(entries) = fs::read_dir(&sub) {
                                for entry in entries.flatten() {
                                    let p = entry.path();
                                    if p.is_file() {
                                        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
                                        if ext == "tmux" || ext == "ps1" {
                                            conf_lines.push(format!("run-shell '{}'", p.display()));
                                            found_entry = true;
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                        if found_entry { break; }
                    }
                }
            }
        }
    }

    // If no theme/plugin.conf was sourced, add a fallback PREVIEW indicator
    if !has_theme_source {
        conf_lines.push(String::new());
        conf_lines.push(format!(
            "set -g status-right '#[fg=yellow,bold] PREVIEW: {} #[default]'",
            plugin_name
        ));
    }

    let conf_content = conf_lines.join("\n") + "\n";
    if let Err(e) = fs::write(&conf_path, &conf_content) {
        let _ = force_remove_dir(&preview_dir);
        return OpResult {
            success: false,
            message: format!("Failed to write preview config: {}", e),
        };
    }

    let conf_path_str = conf_path.display().to_string();

    // Kill any stale preview server from a previous run so we don't get
    // "session 'preview' already exists".
    let _ = Command::new(&binary)
        .args(["-L", "tppanel-preview", "kill-server"])
        .output();

    // Launch the multiplexer with the temp config in a fully ISOLATED server.
    //
    // Key points:
    //   -f <config>    MUST come BEFORE the subcommand (it's a server flag)
    //   -L <socket>    creates a separate server so the preview doesn't reuse
    //                  the user's running server (which already loaded their theme)
    //
    // Syntax: <binary> -f <config> -L tppanel-preview new-session -s preview
    let result = Command::new(&binary)
        .args([
            "-f", &conf_path_str,
            "-L", "tppanel-preview",
            "new-session", "-s", "preview",
        ])
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status();

    let launch_ok = match &result {
        Ok(status) => status.success(),
        Err(_) => false,
    };

    if !launch_ok {
        // Fallback for muxes that may not support -f:
        // Start an isolated server, source the config, then attach.
        let _ = Command::new(&binary)
            .args(["-L", "tppanel-preview", "start-server"])
            .output();
        let _ = Command::new(&binary)
            .args(["-L", "tppanel-preview", "source-file", &conf_path_str])
            .output();
        let r2 = Command::new(&binary)
            .args(["-L", "tppanel-preview", "new-session", "-s", "preview"])
            .stdin(std::process::Stdio::inherit())
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .status();

        match r2 {
            Ok(status) if status.success() => {}
            Ok(status) => {
                // Kill the isolated server before bailing
                let _ = Command::new(&binary)
                    .args(["-L", "tppanel-preview", "kill-server"])
                    .output();
                let _ = force_remove_dir(&preview_dir);
                return OpResult {
                    success: false,
                    message: format!(
                        "Preview session failed (exit {}). Binary: '{}'. Config: '{}'",
                        status, binary, conf_path_str
                    ),
                };
            }
            Err(e) => {
                let _ = force_remove_dir(&preview_dir);
                return OpResult {
                    success: false,
                    message: format!(
                        "Could not launch '{}': {}. Is {} installed and in PATH?",
                        binary, e, binary
                    ),
                };
            }
        }
    }

    // Kill the isolated preview server (it may linger after detach)
    let _ = Command::new(&binary)
        .args(["-L", "tppanel-preview", "kill-server"])
        .output();

    // Clean up temp files after session ends
    let _ = force_remove_dir(&preview_dir);

    OpResult {
        success: true,
        message: format!("Preview of '{}' finished", plugin_name),
    }
}

/// Recursively copy a directory.
fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

pub fn find_orphaned_plugins(config: &TmuxConfig) -> Vec<InstalledPlugin> {
    scan_installed_plugins(config)
        .into_iter()
        .filter(|p| !p.in_config && p.name != "tpm")
        .collect()
}

pub fn clean_orphaned_plugins(config: &mut TmuxConfig) -> Vec<OpResult> {
    let orphans = find_orphaned_plugins(config);
    orphans
        .iter()
        .map(|p| {
            let repo = p.repo.as_deref().unwrap_or(&p.name);
            remove_plugin(repo, config)
        })
        .collect()
}

/// Activate a theme plugin: install it if needed, add to config, remove other
/// theme plugins from config, and reload.
pub fn activate_theme(
    repo: &str,
    config: &mut TmuxConfig,
    detected: &[crate::detect::DetectedMux],
) -> OpResult {
    let registry = crate::registry::embedded_registry();

    // ── Compat guard: block activating themes incompatible with the config type ──
    let required_compat = if config.config_type == "psmux" {
        crate::registry::Compat::PSMux
    } else {
        crate::registry::Compat::Tmux
    };
    if let Some(rp) = crate::registry::get_registry_plugin(registry, repo) {
        if !rp.is_compatible(required_compat) {
            let plugin_name = repo.split('/').last().unwrap_or(repo);
            return OpResult {
                success: false,
                message: format!(
                    "'{}' is {} only and not compatible with {}. Install a {} theme instead.",
                    plugin_name,
                    if config.config_type == "psmux" { "tmux" } else { "psmux" },
                    config.type_label(),
                    config.type_label(),
                ),
            };
        }
    }

    // Remove other theme plugins from the config AND their source-file lines
    // (but don't uninstall their files from disk — user can switch back)
    let theme_repos: Vec<String> = registry.iter()
        .filter(|rp| rp.category == crate::registry::Category::Theme && rp.repo != repo)
        .map(|rp| rp.repo.clone())
        .collect();
    for tr in &theme_repos {
        let _ = config::remove_plugin_from_config(config, tr);
    }
    // Also remove any stray source-file lines left from old themes
    for tr in &theme_repos {
        let old_name = tr.split('/').last().unwrap_or(tr);
        let old_dir = config.plugin_install_dir.join(old_name);
        let old_conf_display = old_dir.join("plugin.conf").display().to_string();
        config.lines.retain(|l| {
            let lt = l.trim();
            !(lt.contains("source-file") && lt.contains(&old_conf_display))
        });
    }

    // Install the theme if not already on disk.
    // add_plugin_to_config (called inside install_plugin) now generates the
    // correct `source-file 'plugin.conf'` for psmux themes automatically.
    let plugin_name = repo.split('/').last().unwrap_or(repo);
    let target_dir = config.plugin_install_dir.join(plugin_name);
    if !target_dir.exists() || !dir_has_content(&target_dir) {
        let result = install_plugin(repo, config, None);
        if !result.success {
            return result;
        }
    } else {
        // Already on disk — remove any existing entry for this plugin
        // (to ensure a clean re-add with the correct source-file line)
        let _ = config::remove_plugin_from_config(config, repo);
        // Re-add with correct activation line (source-file for psmux themes)
        let _ = config::add_plugin_to_config(config, repo, None);
    }

    // Write config to ensure all changes are persisted
    let content = config.lines.join("\n") + "\n";
    let _ = std::fs::write(&config.path, &content);

    // Reload config in the running multiplexer (if any)
    let reload = reload_config(config, detected);

    OpResult {
        success: true,
        message: format!(
            "Theme '{}' activated. {}",
            plugin_name,
            if reload.success { reload.message } else { "Restart mux to apply.".to_string() }
        ),
    }
}
