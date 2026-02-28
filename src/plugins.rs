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
        let repo = remote_url.as_deref().and_then(extract_repo_from_url);
        // Defer expensive git calls — commit/branch are only needed on demand
        let commit = None;
        let branch = None;

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
        let _ = fs::remove_dir_all(&preview_dir);
    }
    let _ = fs::create_dir_all(&preview_dir);

    let plugins_dir = preview_dir.join("plugins");
    let _ = fs::create_dir_all(&plugins_dir);

    let target_dir = plugins_dir.join(plugin_name);

    // ── Try to reuse the already-installed copy first ──────────────
    let already_installed_dir = config.plugin_install_dir.join(plugin_name);
    let have_local_copy = if already_installed_dir.is_dir() {
        // Copy the installed plugin to our preview dir
        match copy_dir_recursive(&already_installed_dir, &target_dir) {
            Ok(()) => true,
            Err(_) => false,
        }
    } else {
        false
    };

    // ── If no local copy, clone from GitHub ───────────────────────
    if !have_local_copy {
        let clone_url = format!("https://github.com/{}.git", repo);
        let target_str = target_dir.display().to_string();

        let (ok, output) = run_git(&["clone", "--depth=1", &clone_url, &target_str], None);
        if !ok {
            // Try SSH URL as fallback
            let ssh_url = format!("git@github.com:{}.git", repo);
            let (ok2, output2) = run_git(&["clone", "--depth=1", &ssh_url, &target_str], None);
            if !ok2 {
                let _ = fs::remove_dir_all(&preview_dir);
                return OpResult {
                    success: false,
                    message: format!(
                        "Preview clone failed (plugin not installed locally either).\n\
                         HTTPS: {}\nSSH: {}\n\
                         Tip: Install the plugin first (Enter), then preview (p).",
                        output, output2
                    ),
                };
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

    // Add plugin declaration
    conf_lines.push(format!("set -g @plugin '{}'", repo));

    // Set a fallback PREVIEW indicator (themes will override status-right with
    // their own styling — this only shows if the theme doesn't touch it)
    conf_lines.push(String::new());
    conf_lines.push(format!(
        "set -g status-right '#[fg=yellow,bold] PREVIEW: {} #[default]'",
        plugin_name
    ));
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
    if plugin_conf.exists() {
        // Static conf — source it directly (most reliable for psmux themes).
        // This contains all the set-g directives and works during config load
        // (unlike .ps1 scripts which call `psmux set -g` via the control port
        // that isn't ready during config parsing).
        conf_lines.push(format!("source-file '{}'", plugin_conf.display()));
    } else {
        // No plugin.conf — look for entry scripts (.tmux or .ps1)
        let entry_tmux = target_dir.join(format!("{}.tmux", plugin_name));
        let entry_ps1 = target_dir.join(format!("{}.ps1", plugin_name));
        if entry_tmux.exists() {
            conf_lines.push(format!("run-shell '{}'", entry_tmux.display()));
        } else if entry_ps1.exists() {
            // psmux PowerShell plugin entry point
            conf_lines.push(format!("run-shell '{}'", entry_ps1.display()));
        } else {
            // Search for any .tmux or .ps1 in plugin root and subdirs
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

    let conf_content = conf_lines.join("\n") + "\n";
    if let Err(e) = fs::write(&conf_path, &conf_content) {
        let _ = fs::remove_dir_all(&preview_dir);
        return OpResult {
            success: false,
            message: format!("Failed to write preview config: {}", e),
        };
    }

    let conf_path_str = conf_path.display().to_string();

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
                let _ = fs::remove_dir_all(&preview_dir);
                return OpResult {
                    success: false,
                    message: format!(
                        "Preview session failed (exit {}). Binary: '{}'. Config: '{}'",
                        status, binary, conf_path_str
                    ),
                };
            }
            Err(e) => {
                let _ = fs::remove_dir_all(&preview_dir);
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
    let _ = fs::remove_dir_all(&preview_dir);

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
