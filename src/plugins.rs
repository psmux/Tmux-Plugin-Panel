/// Plugin installation, removal, and update engine.
///
/// Handles git operations for installing, updating, and removing tmux plugins.
/// Also scans the plugins directory to find installed plugins.
use std::path::{Path, PathBuf};
use std::process::Command;
use std::fs;



use crate::config::{self, TmuxConfig};
use crate::registry;

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
            let msg = if stdout.is_empty() { stderr } else { stdout };
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

/// Read the `.tppanel` marker file to get repo info for non-git plugin dirs
/// (e.g., monorepo plugins extracted from a sparse checkout).
fn read_tppanel_marker(dir: &Path) -> Option<String> {
    let marker = dir.join(".tppanel");
    let content = fs::read_to_string(&marker).ok()?;
    for line in content.lines() {
        if let Some(repo) = line.strip_prefix("repo=") {
            let repo = repo.trim();
            if !repo.is_empty() {
                return Some(repo.to_string());
            }
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

        let remote_url = get_remote_url(&path);
        let repo = remote_url.as_deref().and_then(extract_repo_from_url)
            .or_else(|| read_tppanel_marker(&path));  // fallback to .tppanel marker
        let commit = get_current_commit(&path);
        let branch = get_current_branch(&path);

        let in_config = repo
            .as_deref()
            .map(|r| config_repos.contains(r))
            .unwrap_or(false);

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

/// Check if a repo path is a monorepo plugin (3+ path segments: org/repo/subdir).
fn is_monorepo(repo: &str) -> bool {
    repo.split('/').count() >= 3
}

/// Extract the GitHub repo path (first 2 segments) from a monorepo path.
fn monorepo_base(repo: &str) -> String {
    repo.split('/').take(2).collect::<Vec<_>>().join("/")
}

/// Extract the subdirectory (3rd+ segments) from a monorepo path.
fn monorepo_subdir(repo: &str) -> String {
    repo.split('/').skip(2).collect::<Vec<_>>().join("/")
}

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
        return OpResult {
            success: false,
            message: format!("'{}' already exists", plugin_name),
        };
    }

    if is_monorepo(repo) {
        // Monorepo plugin: clone base repo to temp dir, then move the subdirectory
        let base_repo = monorepo_base(repo);
        let subdir = monorepo_subdir(repo);
        let clone_url = format!("https://github.com/{}.git", base_repo);

        // Use a temp directory inside the install dir
        let temp_dir = install_dir.join(".tppanel-temp-clone");
        let _ = fs::remove_dir_all(&temp_dir); // clean up any previous attempt
        let temp_str = temp_dir.display().to_string();

        let mut args = vec!["clone", "--depth=1", "--filter=blob:none", "--sparse"];
        if let Some(b) = branch {
            args.push("-b");
            args.push(b);
        }
        args.extend_from_slice(&[&clone_url, &temp_str]);

        let (ok, output) = run_git(&args, None);
        if !ok {
            let _ = fs::remove_dir_all(&temp_dir);
            return OpResult {
                success: false,
                message: format!("Clone failed: {}", output),
            };
        }

        // Set sparse-checkout to only fetch the subdirectory we need
        let (ok, _) = run_git(&["sparse-checkout", "set", &subdir], Some(&temp_dir));
        if !ok {
            // Fallback: try without sparse-checkout (full clone already has it)
        }

        // Move the subdirectory to the target
        let source_dir = temp_dir.join(&subdir);
        if source_dir.is_dir() {
            if let Err(e) = copy_dir_recursive(&source_dir, &target_dir) {
                let _ = fs::remove_dir_all(&temp_dir);
                let _ = fs::remove_dir_all(&target_dir);
                return OpResult {
                    success: false,
                    message: format!("Failed to extract plugin: {}", e),
                };
            }
        } else {
            // sparse-checkout might not have worked, try full checkout
            let _ = run_git(&["checkout"], Some(&temp_dir));
            let source_dir = temp_dir.join(&subdir);
            if source_dir.is_dir() {
                if let Err(e) = copy_dir_recursive(&source_dir, &target_dir) {
                    let _ = fs::remove_dir_all(&temp_dir);
                    let _ = fs::remove_dir_all(&target_dir);
                    return OpResult {
                        success: false,
                        message: format!("Failed to extract plugin: {}", e),
                    };
                }
            } else {
                let _ = fs::remove_dir_all(&temp_dir);
                return OpResult {
                    success: false,
                    message: format!("Subdirectory '{}' not found in repo", subdir),
                };
            }
        }

        // Clean up temp clone
        let _ = fs::remove_dir_all(&temp_dir);
    } else {
        // Standard single-repo plugin: direct git clone
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
            // Clean up partial clone
            let _ = fs::remove_dir_all(&target_dir);
            return OpResult {
                success: false,
                message: format!("Clone failed: {}", output),
            };
        }
    }

    // Add to config
    let _ = config::add_plugin_to_config(config, repo, branch);

    // Write marker file for plugin identification (especially for monorepo plugins)
    let marker = target_dir.join(".tppanel");
    let _ = fs::write(&marker, format!("repo={}\n", repo));

    OpResult {
        success: true,
        message: format!("Installed '{}' successfully", plugin_name),
    }
}

/// Recursively copy a directory and its contents.
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

// ── Remove ──────────────────────────────────────────────────────────────

pub fn remove_plugin(repo: &str, config: &mut TmuxConfig) -> OpResult {
    let plugin_name = repo.split('/').last().unwrap_or(repo);
    let target_dir = config.plugin_install_dir.join(plugin_name);

    if target_dir.exists() {
        if let Err(e) = fs::remove_dir_all(&target_dir) {
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

    // Monorepo plugins (no .git dir) need to be re-fetched entirely
    let has_git = plugin.path.join(".git").exists();
    if !has_git {
        if let Some(repo) = &plugin.repo {
            if is_monorepo(repo) {
                // Re-install: remove old dir, clone fresh from monorepo
                let install_dir = plugin.path.parent().unwrap_or(Path::new("."));
                let subdir = monorepo_subdir(repo);
                let base_repo = monorepo_base(repo);
                let clone_url = format!("https://github.com/{}.git", base_repo);

                let temp_dir = install_dir.join(".tppanel-temp-clone");
                let _ = fs::remove_dir_all(&temp_dir);
                let temp_str = temp_dir.display().to_string();

                let (ok, output) = run_git(
                    &["clone", "--depth=1", "--filter=blob:none", "--sparse", &clone_url, &temp_str],
                    None,
                );
                if !ok {
                    let _ = fs::remove_dir_all(&temp_dir);
                    return OpResult {
                        success: false,
                        message: format!("Update failed (clone): {}", output),
                    };
                }
                let _ = run_git(&["sparse-checkout", "set", &subdir], Some(&temp_dir));

                let source_dir = temp_dir.join(&subdir);
                if source_dir.is_dir() {
                    // Replace the old plugin dir
                    let _ = fs::remove_dir_all(&plugin.path);
                    if let Err(e) = copy_dir_recursive(&source_dir, &plugin.path) {
                        let _ = fs::remove_dir_all(&temp_dir);
                        return OpResult {
                            success: false,
                            message: format!("Update failed (copy): {}", e),
                        };
                    }
                    // Re-write marker
                    let marker = plugin.path.join(".tppanel");
                    let _ = fs::write(&marker, format!("repo={}\n", repo));
                }
                let _ = fs::remove_dir_all(&temp_dir);

                return OpResult {
                    success: true,
                    message: format!("Updated '{}' (re-fetched from monorepo)", plugin.name),
                };
            }
        }
        return OpResult {
            success: false,
            message: format!("Cannot update '{}': no git info and not a known monorepo plugin", plugin.name),
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
