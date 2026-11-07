/// Command line interface, for managing plugins without the TUI.
///
/// ```text
/// tmuxpanel install <org/repo>[#branch]...   clone and add to the config
/// tmuxpanel remove  <org/repo>...            delete and remove from the config
/// tmuxpanel update  [org/repo...]            git pull (all plugins if none given)
/// tmuxpanel sync                             install every @plugin in the config
///                                            that is not on disk yet
/// tmuxpanel list                             plugins in the config and their state
/// ```
///
/// `--config <path>` picks the config file; otherwise the first one found is
/// used (the same one the TUI opens).
use std::path::PathBuf;

use crate::config::{self, ActivationStatus, TmuxConfig};
use crate::plugins;

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Install(Vec<String>),
    Remove(Vec<String>),
    Update(Vec<String>),
    Sync,
    List,
    Help,
    Version,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Invocation {
    pub command: Command,
    pub config: Option<PathBuf>,
}

pub const USAGE: &str = "\
tmuxpanel: plugin manager for psmux and tmux

Usage:
  tmuxpanel                              open the TUI
  tmuxpanel install <org/repo>[#branch]... install plugins and add them to the config
  tmuxpanel remove <org/repo>...         uninstall plugins and remove them from the config
  tmuxpanel update [org/repo...]         update plugins (all of them if none given)
  tmuxpanel sync                         install every @plugin line in the config
                                         that is not installed yet
  tmuxpanel list                         show plugins in the config and their state

Options:
  -c, --config <path>   config file to use (default: the first one found,
                        e.g. ~/.psmux.conf or ~/.tmux.conf)
  -h, --help            show this help
  -V, --version         show the version

Config only workflow: put lines like
  set -g @plugin 'psmux-plugins/psmux-sensible'
in your config, then run `tmuxpanel sync`.
";

/// Parse arguments (without the program name). `Ok(None)` means no command:
/// open the TUI.
pub fn parse_args(args: &[String]) -> Result<Option<Invocation>, String> {
    let mut config = None;
    let mut rest: Vec<String> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-c" | "--config" => {
                let p = it.next().ok_or_else(|| format!("{} needs a path", a))?;
                config = Some(PathBuf::from(p));
            }
            _ if a.starts_with("--config=") => {
                config = Some(PathBuf::from(&a["--config=".len()..]));
            }
            "-h" | "--help" | "help" => {
                return Ok(Some(Invocation { command: Command::Help, config }));
            }
            "-V" | "--version" | "version" => {
                return Ok(Some(Invocation { command: Command::Version, config }));
            }
            _ => rest.push(a.clone()),
        }
    }

    if rest.is_empty() {
        return Ok(None);
    }
    let cmd = rest.remove(0);
    for r in &rest {
        if r.starts_with('-') {
            return Err(format!("unknown option '{}'", r));
        }
    }
    let need_repos = |name: &str, v: &Vec<String>| -> Result<(), String> {
        if v.is_empty() {
            return Err(format!("'{}' needs at least one org/repo", name));
        }
        for r in v {
            let base = r.split('#').next().unwrap_or(r);
            if base.split('/').filter(|s| !s.is_empty()).count() != 2 {
                return Err(format!("'{}' is not in org/repo form", r));
            }
        }
        Ok(())
    };
    let command = match cmd.as_str() {
        "install" | "add" => {
            need_repos("install", &rest)?;
            Command::Install(rest)
        }
        "remove" | "uninstall" | "rm" => {
            need_repos("remove", &rest)?;
            Command::Remove(rest)
        }
        "update" | "upgrade" => Command::Update(rest),
        "sync" => Command::Sync,
        "list" | "ls" => Command::List,
        other => return Err(format!("unknown command '{}'", other)),
    };
    Ok(Some(Invocation { command, config }))
}

fn load_config(path: &Option<PathBuf>) -> Result<TmuxConfig, String> {
    match path {
        Some(p) => {
            if !p.is_file() {
                return Err(format!("config file not found: {}", p.display()));
            }
            let lower = p.to_string_lossy().to_lowercase();
            let ctype = if lower.contains("psmux") || lower.contains("pmux") { "psmux" } else { "tmux" };
            config::parse_config(p, ctype).map_err(|e| e.to_string())
        }
        None => config::find_configs().into_iter().next().ok_or_else(|| {
            "no psmux or tmux config found. Create ~/.psmux.conf or ~/.tmux.conf, or pass --config <path>.".to_string()
        }),
    }
}

/// Run a parsed command. Returns the process exit code.
pub fn run(inv: Invocation) -> i32 {
    match inv.command {
        Command::Help => {
            print!("{}", USAGE);
            return 0;
        }
        Command::Version => {
            println!("tmuxpanel {}", env!("CARGO_PKG_VERSION"));
            return 0;
        }
        _ => {}
    }

    let mut cfg = match load_config(&inv.config) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {}", e);
            return 1;
        }
    };
    println!("config: {}", cfg.path.display());

    let mut failures = 0;
    let mut report = |ok: bool, msg: &str| {
        if ok {
            println!("  ok    {}", msg);
        } else {
            eprintln!("  fail  {}", msg);
            failures += 1;
        }
    };

    match inv.command {
        Command::Install(repos) => {
            for spec in repos {
                let (repo, branch) = split_spec(&spec);
                let r = plugins::install_plugin(repo, &mut cfg, branch);
                report(r.success, &r.message);
            }
        }
        Command::Remove(repos) => {
            for spec in repos {
                let (repo, _) = split_spec(&spec);
                if !cfg.plugins.iter().any(|p| p.repo == repo) {
                    report(false, &format!("'{}' is not in {}", repo, cfg.path.display()));
                    continue;
                }
                let r = plugins::remove_plugin(repo, &mut cfg);
                report(r.success, &r.message);
            }
        }
        Command::Update(repos) => {
            let installed = plugins::scan_installed_plugins(&cfg);
            let wanted: Vec<_> = installed
                .iter()
                .filter(|p| {
                    repos.is_empty()
                        || repos.iter().any(|r| {
                            let (r, _) = split_spec(r);
                            p.repo.as_deref() == Some(r) || r.split('/').last() == Some(p.name.as_str())
                        })
                })
                .collect();
            if wanted.is_empty() {
                report(false, "no matching installed plugins");
            }
            for p in wanted {
                let r = plugins::update_plugin(p);
                report(r.success, &r.message);
            }
        }
        Command::Sync => {
            let declared: Vec<(String, Option<String>)> = cfg
                .plugins
                .iter()
                .filter(|p| p.enabled)
                .map(|p| (p.repo.clone(), p.branch.clone()))
                .collect();
            if declared.is_empty() {
                println!("  no @plugin lines in the config");
            }
            for (repo, branch) in declared {
                let name = repo.split('/').last().unwrap_or(&repo).to_string();
                if repo == "tmux-plugins/tpm" {
                    continue;
                }
                if cfg.plugin_install_dir.join(&name).is_dir() {
                    println!("  have  {}", repo);
                    continue;
                }
                let r = plugins::install_plugin(&repo, &mut cfg, branch.as_deref());
                report(r.success, &r.message);
            }
            let fixed = config::repair_missing_activation_lines(&mut cfg);
            if fixed > 0 {
                println!("  added {} missing activation line(s)", fixed);
            }
        }
        Command::List => {
            if cfg.plugins.is_empty() {
                println!("  no @plugin lines in the config");
            }
            for p in &cfg.plugins {
                let state = if !p.enabled {
                    "commented out"
                } else {
                    match config::plugin_activation_status(&cfg, &p.repo) {
                        ActivationStatus::Ok => "installed",
                        ActivationStatus::NotInstalled => "not installed (run: tmuxpanel sync)",
                        ActivationStatus::Missing => "installed, activation line missing (run: tmuxpanel sync)",
                        ActivationStatus::NoEntryScript => "installed, no entry script found",
                    }
                };
                println!("  {:<45} {}", p.repo, state);
            }
        }
        Command::Help | Command::Version => unreachable!(),
    }

    if failures > 0 { 1 } else { 0 }
}

fn split_spec(spec: &str) -> (&str, Option<&str>) {
    match spec.split_once('#') {
        Some((r, b)) if !b.is_empty() => (r, Some(b)),
        Some((r, _)) => (r, None),
        None => (spec, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn test_no_args_opens_tui() {
        assert_eq!(parse_args(&[]).unwrap(), None);
    }

    #[test]
    fn test_parse_install_with_config() {
        let inv = parse_args(&a(&["install", "catppuccin/tmux#v2.1.0", "tmux-plugins/tmux-yank", "--config", "x.conf"]))
            .unwrap()
            .unwrap();
        assert_eq!(inv.command, Command::Install(a(&["catppuccin/tmux#v2.1.0", "tmux-plugins/tmux-yank"])));
        assert_eq!(inv.config, Some(PathBuf::from("x.conf")));
    }

    #[test]
    fn test_parse_other_commands() {
        assert_eq!(parse_args(&a(&["sync"])).unwrap().unwrap().command, Command::Sync);
        assert_eq!(parse_args(&a(&["ls"])).unwrap().unwrap().command, Command::List);
        assert_eq!(parse_args(&a(&["update"])).unwrap().unwrap().command, Command::Update(vec![]));
        assert_eq!(parse_args(&a(&["rm", "a/b"])).unwrap().unwrap().command, Command::Remove(a(&["a/b"])));
        assert_eq!(parse_args(&a(&["--help"])).unwrap().unwrap().command, Command::Help);
        assert_eq!(parse_args(&a(&["-V"])).unwrap().unwrap().command, Command::Version);
    }

    #[test]
    fn test_parse_errors() {
        assert!(parse_args(&a(&["install"])).is_err());
        assert!(parse_args(&a(&["install", "notarepo"])).is_err());
        assert!(parse_args(&a(&["frobnicate"])).is_err());
        assert!(parse_args(&a(&["--config"])).is_err());
        assert!(parse_args(&a(&["sync", "--bogus"])).is_err());
    }

    #[test]
    fn test_split_spec() {
        assert_eq!(split_spec("a/b#main"), ("a/b", Some("main")));
        assert_eq!(split_spec("a/b"), ("a/b", None));
    }
}
