/// Multiplexer detection module.
///
/// Thoroughly detects tmux and PSMux installations on all platforms.
/// Probes binary locations, parses versions, finds config files,
/// and reports installation health.
///
/// This is the single source of truth for "what multiplexers are on this system?"
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

// ── Public types ────────────────────────────────────────────────────────

/// Which multiplexer family a binary belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MuxKind {
    Tmux,
    PSMux,
}

impl MuxKind {
    pub fn label(&self) -> &'static str {
        match self {
            MuxKind::Tmux => "tmux",
            MuxKind::PSMux => "PSMux",
        }
    }
}

/// A detected multiplexer binary on the system.
#[derive(Debug, Clone)]
pub struct DetectedMux {
    pub kind: MuxKind,
    /// Display name — "PSMux" or "tmux"
    pub name: String,
    /// The exact binary name or path that was found
    pub binary: String,
    /// Full path to the binary (if resolved)
    pub binary_path: Option<PathBuf>,
    /// Parsed version string (e.g. "3.4" or "0.4.1")
    pub version: String,
    /// Raw output from `<binary> -V`
    pub raw_version_output: String,
}

/// A config file location with its type.
#[derive(Debug, Clone)]
pub struct ConfigLocation {
    pub path: PathBuf,
    pub kind: MuxKind,
    pub exists: bool,
    /// Reason this path is checked (e.g. "PSMux default", "XDG config")
    pub source: &'static str,
    /// Priority — lower number = checked first
    pub priority: u8,
}

/// Complete detection report for the system.
#[derive(Debug, Clone)]
pub struct DetectionReport {
    pub multiplexers: Vec<DetectedMux>,
    pub config_locations: Vec<ConfigLocation>,
    pub active_configs: Vec<ConfigLocation>,
    pub platform: Platform,
}

/// Platform info.
#[derive(Debug, Clone)]
pub struct Platform {
    pub os: &'static str,
    pub is_wsl: bool,
    pub home_dir: PathBuf,
    pub xdg_config: PathBuf,
}

impl DetectionReport {
    /// Does this system have any multiplexer installed?
    pub fn has_any_mux(&self) -> bool {
        !self.multiplexers.is_empty()
    }

    /// Does this system have PSMux?
    pub fn has_psmux(&self) -> bool {
        self.multiplexers.iter().any(|m| m.kind == MuxKind::PSMux)
    }

    /// Does this system have tmux (real, not PSMux alias)?
    pub fn has_tmux(&self) -> bool {
        self.multiplexers.iter().any(|m| m.kind == MuxKind::Tmux)
    }

    /// Get the primary (first detected) multiplexer.
    pub fn primary_mux(&self) -> Option<&DetectedMux> {
        self.multiplexers.first()
    }

    /// Get config locations that actually exist on disk.
    pub fn existing_configs(&self) -> Vec<&ConfigLocation> {
        self.config_locations.iter().filter(|c| c.exists).collect()
    }

    /// Best binary name for sourcing/reloading a config of a given kind.
    pub fn reload_binary(&self, kind: MuxKind) -> String {
        match kind {
            MuxKind::PSMux => {
                for name in &["psmux", "pmux"] {
                    if self.multiplexers.iter().any(|m| m.binary == *name) {
                        return name.to_string();
                    }
                }
                "psmux".to_string()
            }
            MuxKind::Tmux => {
                for name in &["tmux", "psmux", "pmux"] {
                    if self.multiplexers.iter().any(|m| m.binary == *name) {
                        return name.to_string();
                    }
                }
                "tmux".to_string()
            }
        }
    }
}

// ── Main detection entry point ──────────────────────────────────────────

/// Run the full detection sweep.
/// Probes all known binary names, resolves paths, finds configs.
pub fn detect_all() -> DetectionReport {
    let timing_file = std::fs::OpenOptions::new()
        .create(true).append(true).open("startup_timing.log").ok();
    let mut tlog = |msg: String| {
        if let Some(ref f) = timing_file {
            use std::io::Write;
            let _ = writeln!(&*f, "{}", msg);
        }
    };

    let t0 = std::time::Instant::now();
    let platform = detect_platform();
    tlog(format!("[DETECT] detect_platform       = {:?}", t0.elapsed()));

    let t1 = std::time::Instant::now();
    let multiplexers = detect_multiplexers(&platform);
    tlog(format!("[DETECT] detect_multiplexers   = {:?}", t1.elapsed()));

    let t2 = std::time::Instant::now();
    let config_locations = detect_config_locations(&platform);
    tlog(format!("[DETECT] detect_config_locs    = {:?}", t2.elapsed()));

    let active_configs = config_locations
        .iter()
        .filter(|c| c.exists)
        .cloned()
        .collect();

    DetectionReport {
        multiplexers,
        config_locations,
        active_configs,
        platform,
    }
}

// ── Platform detection ──────────────────────────────────────────────────

fn detect_platform() -> Platform {
    let os = if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "unknown"
    };

    // WSL detection: check for /proc/version containing "microsoft" or "WSL"
    let is_wsl = if cfg!(target_os = "linux") {
        std::fs::read_to_string("/proc/version")
            .map(|v| {
                let lower = v.to_lowercase();
                lower.contains("microsoft") || lower.contains("wsl")
            })
            .unwrap_or(false)
    } else {
        false
    };

    let home_dir = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let xdg_config = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home_dir.join(".config"));

    Platform {
        os,
        is_wsl,
        home_dir,
        xdg_config,
    }
}

// ── Binary detection ────────────────────────────────────────────────────

fn detect_multiplexers(platform: &Platform) -> Vec<DetectedMux> {
    let mut found: Vec<DetectedMux> = Vec::new();
    let mut seen_binaries = HashSet::new();

    // ── PSMux binaries ──────────────────────────────────────────
    // psmux is the primary binary name
    // pmux is an alias shipped by psmux
    for bin_name in &["psmux", "pmux"] {
        if seen_binaries.contains(*bin_name) {
            continue;
        }
        if let Some(mut mux) = probe_binary(bin_name) {
            mux.kind = MuxKind::PSMux;
            mux.name = "PSMux".to_string();
            // If we already found psmux and this is pmux (same thing), skip
            if found.iter().any(|m| m.kind == MuxKind::PSMux) {
                continue;
            }
            seen_binaries.insert(bin_name.to_string());
            found.push(mux);
        }
    }

    // ── tmux binary ─────────────────────────────────────────────
    if let Some(mux) = probe_binary("tmux") {
        // On Windows, PSMux ships a tmux.exe alias.
        // Detect it by checking if the version output contains "psmux".
        let is_psmux_alias = found.iter().any(|m| m.kind == MuxKind::PSMux)
            && mux
                .raw_version_output
                .to_lowercase()
                .contains("psmux");

        if is_psmux_alias {
            // Don't add — it's the same binary
        } else {
            seen_binaries.insert("tmux".to_string());
            found.push(mux);
        }
    }

    // ── Platform-specific extra locations ────────────────────────
    // On macOS, tmux might be in Homebrew paths not on PATH
    if platform.os == "macos" {
        for extra in &[
            "/opt/homebrew/bin/tmux",
            "/usr/local/bin/tmux",
        ] {
            if seen_binaries.contains("tmux") {
                break;
            }
            if Path::new(extra).exists() {
                if let Some(mux) = probe_binary(extra) {
                    seen_binaries.insert("tmux".to_string());
                    found.push(mux);
                }
            }
        }
    }

    // On Linux, check common package manager install locations
    if platform.os == "linux" && !seen_binaries.contains("tmux") {
        for extra in &[
            "/usr/bin/tmux",
            "/usr/local/bin/tmux",
            "/snap/bin/tmux",
        ] {
            if Path::new(extra).exists() {
                if let Some(mux) = probe_binary(extra) {
                    seen_binaries.insert("tmux".to_string());
                    found.push(mux);
                    break;
                }
            }
        }
    }

    // On Windows, check common install locations for psmux if not found on PATH
    if platform.os == "windows" && !found.iter().any(|m| m.kind == MuxKind::PSMux) {
        // Cargo install directory
        let cargo_bin = platform.home_dir.join(".cargo").join("bin");
        for name in &["psmux.exe", "pmux.exe"] {
            let full = cargo_bin.join(name);
            if full.exists() {
                if let Some(mut mux) = probe_binary(&full.display().to_string()) {
                    mux.kind = MuxKind::PSMux;
                    mux.name = "PSMux".to_string();
                    found.push(mux);
                    break;
                }
            }
        }

        // Scoop install directory
        let scoop_shims = platform.home_dir.join("scoop").join("shims");
        for name in &["psmux.exe", "pmux.exe"] {
            let full = scoop_shims.join(name);
            if full.exists() {
                if let Some(mut mux) = probe_binary(&full.display().to_string()) {
                    mux.kind = MuxKind::PSMux;
                    mux.name = "PSMux".to_string();
                    if !found.iter().any(|m| m.kind == MuxKind::PSMux) {
                        found.push(mux);
                    }
                    break;
                }
            }
        }
    }

    found
}

fn probe_binary(name: &str) -> Option<DetectedMux> {
    let output = Command::new(name).arg("-V").output().ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let raw = if stdout.is_empty() { &stderr } else { &stdout };

    if raw.is_empty() && !output.status.success() {
        return None;
    }

    let version = parse_version_string(raw);

    // Resolve full path via `which` / `where`
    let binary_path = resolve_binary_path(name);

    // Determine kind from the output
    let lower = raw.to_lowercase();
    let kind = if lower.contains("psmux") || lower.contains("pmux") {
        MuxKind::PSMux
    } else {
        MuxKind::Tmux
    };

    Some(DetectedMux {
        kind,
        name: kind.label().to_string(),
        binary: name.to_string(),
        binary_path,
        version: if version.is_empty() {
            "unknown".to_string()
        } else {
            version
        },
        raw_version_output: raw.to_string(),
    })
}

fn parse_version_string(raw: &str) -> String {
    // Try to extract just the version number.
    // Common formats: "tmux 3.4", "psmux 0.4.1", "pmux 0.4.1"
    let re = regex::Regex::new(r"(\d+\.\d+(?:\.\d+)?)").ok();
    if let Some(re) = re {
        if let Some(m) = re.find(raw) {
            return m.as_str().to_string();
        }
    }
    raw.to_string()
}

fn resolve_binary_path(name: &str) -> Option<PathBuf> {
    let cmd = if cfg!(target_os = "windows") {
        Command::new("where").arg(name).output()
    } else {
        Command::new("which").arg(name).output()
    };
    cmd.ok().and_then(|o| {
        if o.status.success() {
            let path = String::from_utf8_lossy(&o.stdout)
                .lines()
                .next()?
                .trim()
                .to_string();
            if path.is_empty() {
                None
            } else {
                Some(PathBuf::from(path))
            }
        } else {
            None
        }
    })
}

// ── Config location detection ───────────────────────────────────────────

fn detect_config_locations(platform: &Platform) -> Vec<ConfigLocation> {
    let home = &platform.home_dir;
    let xdg = &platform.xdg_config;

    let mut locs: Vec<ConfigLocation> = Vec::new();
    let mut priority: u8 = 0;

    // Helper macro to keep it concise
    macro_rules! add_loc {
        ($path:expr, $kind:expr, $source:expr) => {
            let p: PathBuf = $path;
            let exists = p.is_file();
            locs.push(ConfigLocation {
                path: p,
                kind: $kind,
                exists,
                source: $source,
                priority,
            });
            priority += 1;
        };
    }

    // ── PSMux config paths ──────────────────────────────────────
    // From PSMux source (config.rs load_config): searched in this exact order:
    //   1. $USERPROFILE/.psmux.conf  (or $HOME/.psmux.conf)
    //   2. $USERPROFILE/.psmuxrc     (or $HOME/.psmuxrc)
    //   3. $USERPROFILE/.tmux.conf   (or $HOME/.tmux.conf) — PSMux reads tmux configs!
    //   4. $USERPROFILE/.config/psmux/psmux.conf (XDG-style)
    add_loc!(home.join(".psmux.conf"), MuxKind::PSMux, "PSMux default (~/.psmux.conf)");
    add_loc!(home.join(".psmuxrc"), MuxKind::PSMux, "PSMux alt (~/.psmuxrc)");
    add_loc!(xdg.join("psmux").join("psmux.conf"), MuxKind::PSMux, "XDG PSMux config");

    // ── Windows-specific PSMux paths ────────────────────────────
    #[cfg(target_os = "windows")]
    {
        // %APPDATA%\psmux\psmux.conf — common Windows app config pattern
        if let Ok(appdata) = std::env::var("APPDATA") {
            let appdata = PathBuf::from(&appdata);
            add_loc!(appdata.join("psmux").join("psmux.conf"), MuxKind::PSMux,
                "%APPDATA%\\psmux\\psmux.conf");
            add_loc!(appdata.join("psmux").join(".psmux.conf"), MuxKind::PSMux,
                "%APPDATA%\\psmux\\.psmux.conf");
        }

        // %LOCALAPPDATA%\psmux\psmux.conf
        if let Ok(localappdata) = std::env::var("LOCALAPPDATA") {
            let localappdata = PathBuf::from(&localappdata);
            add_loc!(localappdata.join("psmux").join("psmux.conf"), MuxKind::PSMux,
                "%LOCALAPPDATA%\\psmux\\psmux.conf");
        }
    }

    // ── tmux config paths (all platforms) ───────────────────────
    // XDG path (modern tmux >= 3.1) — checked first as it's the modern standard
    add_loc!(xdg.join("tmux").join("tmux.conf"), MuxKind::Tmux, "XDG tmux config (modern)");
    // Classic home path — most common
    add_loc!(home.join(".tmux.conf"), MuxKind::Tmux, "Classic tmux config");

    // ── Windows-specific tmux paths ─────────────────────────────
    #[cfg(target_os = "windows")]
    {
        // Windows native tmux (e.g. via MSYS2 running natively)
        if let Ok(appdata) = std::env::var("APPDATA") {
            let appdata = PathBuf::from(&appdata);
            add_loc!(appdata.join("tmux").join("tmux.conf"), MuxKind::Tmux,
                "%APPDATA%\\tmux\\tmux.conf");
        }

        // MSYS2 paths — tmux is commonly installed via MSYS2 on Windows
        // MSYS2 uses /home/<user>/ which maps to C:\msys64\home\<user>\
        let msys2_roots = vec![
            PathBuf::from("C:\\msys64"),
            PathBuf::from("C:\\msys32"),
        ];
        // Also check MSYSTEM_PREFIX env
        if let Ok(msystem) = std::env::var("MSYSTEM_PREFIX") {
            let msys_home = PathBuf::from(&msystem);
            add_loc!(msys_home.join("etc").join("tmux.conf"), MuxKind::Tmux,
                "MSYS2 system ($MSYSTEM_PREFIX/etc)");
        }
        for msys_root in &msys2_roots {
            if msys_root.exists() {
                // MSYS2 system config
                add_loc!(msys_root.join("etc").join("tmux.conf"), MuxKind::Tmux,
                    "MSYS2 system (/etc/tmux.conf)");
                // MSYS2 user config — username from USERPROFILE
                if let Some(username) = home.file_name() {
                    let msys_user_home = msys_root.join("home").join(username);
                    add_loc!(msys_user_home.join(".tmux.conf"), MuxKind::Tmux,
                        "MSYS2 user (~/.tmux.conf)");
                    add_loc!(msys_user_home.join(".config").join("tmux").join("tmux.conf"),
                        MuxKind::Tmux, "MSYS2 user XDG");
                }
            }
        }

        // Cygwin paths
        let cygwin_root = PathBuf::from("C:\\cygwin64");
        if cygwin_root.exists() {
            add_loc!(cygwin_root.join("etc").join("tmux.conf"), MuxKind::Tmux,
                "Cygwin system (/etc/tmux.conf)");
            if let Some(username) = home.file_name() {
                let cyg_user_home = cygwin_root.join("home").join(username);
                add_loc!(cyg_user_home.join(".tmux.conf"), MuxKind::Tmux,
                    "Cygwin user (~/.tmux.conf)");
            }
        }

        // Git Bash paths (Git for Windows includes a minimal MSYS2)
        if let Ok(programfiles) = std::env::var("ProgramFiles") {
            let git_root = PathBuf::from(&programfiles).join("Git");
            if git_root.exists() {
                add_loc!(git_root.join("etc").join("tmux.conf"), MuxKind::Tmux,
                    "Git Bash system (/etc/tmux.conf)");
            }
        }

        // WSL interop — if running natively on Windows, check common WSL distro paths
        // These are accessible via \\wsl$\<distro>\ paths
        let wsl_paths = vec![
            PathBuf::from(r"\\wsl$\Ubuntu\home"),
            PathBuf::from(r"\\wsl$\Debian\home"),
            PathBuf::from(r"\\wsl.localhost\Ubuntu\home"),
            PathBuf::from(r"\\wsl.localhost\Debian\home"),
        ];
        if let Some(username) = home.file_name() {
            for wsl_home_parent in &wsl_paths {
                let wsl_user_home = wsl_home_parent.join(username);
                if wsl_user_home.exists() {
                    add_loc!(wsl_user_home.join(".tmux.conf"), MuxKind::Tmux,
                        "WSL tmux config");
                    break;
                }
            }
        }
    }

    // ── macOS-specific ──────────────────────────────────────────
    #[cfg(target_os = "macos")]
    {
        add_loc!(
            PathBuf::from("/opt/homebrew/etc/tmux.conf"),
            MuxKind::Tmux,
            "Homebrew ARM (macOS)"
        );
        add_loc!(
            PathBuf::from("/usr/local/etc/tmux.conf"),
            MuxKind::Tmux,
            "Homebrew Intel (macOS)"
        );
        // MacPorts
        add_loc!(
            PathBuf::from("/opt/local/etc/tmux.conf"),
            MuxKind::Tmux,
            "MacPorts (macOS)"
        );
    }

    // ── Linux ───────────────────────────────────────────────────
    #[cfg(target_os = "linux")]
    {
        // System-wide config
        add_loc!(
            PathBuf::from("/etc/tmux.conf"),
            MuxKind::Tmux,
            "System-wide (/etc)"
        );
        // Some distros use /etc/tmux/ directory
        add_loc!(
            PathBuf::from("/etc/tmux/tmux.conf"),
            MuxKind::Tmux,
            "System-wide (/etc/tmux/)"
        );
        // Snap install path
        add_loc!(
            PathBuf::from("/snap/tmux/current/etc/tmux.conf"),
            MuxKind::Tmux,
            "Snap tmux config"
        );
        // Nix profile
        let nix_profile = home.join(".nix-profile").join("etc").join("tmux.conf");
        add_loc!(nix_profile, MuxKind::Tmux, "Nix profile");
        // Homebrew on Linux (Linuxbrew)
        add_loc!(
            home.join(".linuxbrew").join("etc").join("tmux.conf"),
            MuxKind::Tmux,
            "Linuxbrew"
        );
        add_loc!(
            PathBuf::from("/home/linuxbrew/.linuxbrew/etc/tmux.conf"),
            MuxKind::Tmux,
            "Linuxbrew (system)"
        );
    }

    locs
}

// ── Install directory defaults ──────────────────────────────────────────

/// Get the default plugin install directory for a given config.
pub fn default_install_dir(config_path: &Path, kind: MuxKind) -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let xdg = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home.join(".config"));

    match kind {
        MuxKind::PSMux => {
            if config_path.starts_with(xdg.join("psmux")) {
                xdg.join("psmux").join("plugins")
            } else {
                home.join(".psmux").join("plugins")
            }
        }
        MuxKind::Tmux => {
            if config_path.starts_with(xdg.join("tmux")) {
                xdg.join("tmux").join("plugins")
            } else {
                home.join(".tmux").join("plugins")
            }
        }
    }
}
