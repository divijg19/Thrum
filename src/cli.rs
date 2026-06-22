//! Command-line interface: argument parsing, configuration deserialization, file I/O.
//!
//! Entry points: [`parse_args`], [`print_help`]. Types: [`CliAction`], [`Config`].

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::app::{ProcSortField, Tab, TabOrientation};

/// Action to take after parsing command-line arguments.
pub enum CliAction {
    Help,
    Version,
    Error(String),
    Config(Config),
}

/// Configuration deserialized from a TOML file, with defaults for all fields.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Config {
    pub refresh_ms: u64,
    pub default_tab: Tab,
    pub hide_sidebar: bool,
    pub tab_orientation: TabOrientation,
    pub proc_sort_default: ProcSortField,
    pub proc_sort_asc_default: bool,
    pub history_window: usize,
    pub scroll_step: usize,
    #[serde(skip)]
    pub config_warning: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            refresh_ms: 1000,
            default_tab: Tab::Dash,
            hide_sidebar: false,
            tab_orientation: TabOrientation::Sidebar,
            proc_sort_default: ProcSortField::Cpu,
            proc_sort_asc_default: false,
            history_window: 60,
            scroll_step: 3,
            config_warning: None,
        }
    }
}

fn default_config_path() -> Option<PathBuf> {
    if let Ok(xdg) = env::var("XDG_CONFIG_HOME") {
        return Some(PathBuf::from(xdg).join("thrum/config.toml"));
    }
    let home = env::var("HOME").ok()?;
    Some(PathBuf::from(home).join(".config/thrum/config.toml"))
}

fn read_config_file(path: &Path) -> Result<Config, String> {
    let content = fs::read_to_string(path)
        .map_err(|e| format!("config file '{}' is unreadable: {e}", path.display()))?;
    toml::from_str(&content)
        .map_err(|e| format!("config file '{}' has invalid TOML: {e}", path.display()))
}

/// Prints usage information to stderr.
pub fn print_help() {
    eprintln!("Usage: thrum [OPTIONS]");
    eprintln!();
    eprintln!("  -c, --config <path>   Config file path (default: ~/.config/thrum/config.toml)");
    eprintln!("  -r, --refresh <ms>    Refresh interval (default: 1000)");
    eprintln!("  -t, --tab <name>      Default tab (dash|proc|net|files|time|temp|cores|disk|mem)");
    eprintln!("  -s, --no-sidebar      Start with sidebar hidden");
    eprintln!("  --tabs <mode>         Tab orientation: sidebar, horizontal, or horizontal_footer");
    eprintln!("  --scroll-step <n>     Mouse scroll step (default: 3)");
    eprintln!("  -V, --version         Show version");
    eprintln!("  --help                Show this help");
}

fn parse_flag_value<'a>(
    args: &'a [String],
    flag: &str,
    i: &mut usize,
) -> Result<&'a str, CliAction> {
    *i += 1;
    args.get(*i)
        .ok_or_else(|| CliAction::Error(format!("{flag} requires a value")))
        .map(String::as_str)
}

fn parse_positive_int(args: &[String], flag: &str, i: &mut usize) -> Result<u64, CliAction> {
    let val = parse_flag_value(args, flag, i)?;
    val.parse()
        .map_err(|_| CliAction::Error(format!("{flag} must be a positive integer")))
        .and_then(|n| {
            if n > 0 {
                Ok(n)
            } else {
                Err(CliAction::Error(format!(
                    "{flag} must be a positive integer"
                )))
            }
        })
}

fn parse_tab_name(name: &str) -> Result<Tab, CliAction> {
    match name.to_ascii_lowercase().as_str() {
        "dash" => Ok(Tab::Dash),
        "proc" => Ok(Tab::Proc),
        "net" => Ok(Tab::Net),
        "files" => Ok(Tab::Files),
        "time" => Ok(Tab::Time),
        "temp" => Ok(Tab::Temp),
        "cores" => Ok(Tab::Cores),
        "disk" => Ok(Tab::Disk),
        "mem" => Ok(Tab::Mem),
        _ => Err(CliAction::Error(format!("unknown tab '{name}'"))),
    }
}

fn parse_tab_orientation(name: &str) -> Result<TabOrientation, CliAction> {
    match name.to_ascii_lowercase().as_str() {
        "sidebar" => Ok(TabOrientation::Sidebar),
        "horizontal" => Ok(TabOrientation::Horizontal),
        "horizontal_footer" => Ok(TabOrientation::HorizontalFooter),
        _ => Err(CliAction::Error(
            "--tabs must be 'sidebar', 'horizontal', or 'horizontal_footer'".to_owned(),
        )),
    }
}

fn load_config(config_path: Option<&str>) -> Result<Config, CliAction> {
    if let Some(path) = config_path {
        let p = Path::new(path);
        if !p.exists() {
            return Err(CliAction::Error(format!("config file '{path}' not found")));
        }
        read_config_file(p).map_err(CliAction::Error)
    } else if let Some(p) = default_config_path() {
        match read_config_file(&p) {
            Ok(cfg) => Ok(cfg),
            Err(e) => Ok(Config {
                config_warning: Some(e),
                ..Config::default()
            }),
        }
    } else {
        Ok(Config::default())
    }
}

/// Parses command-line arguments and returns the corresponding [`CliAction`].
pub fn parse_args(args: &[String]) -> CliAction {
    match try_parse_args(args) {
        Ok(cfg) => CliAction::Config(cfg),
        Err(action) => action,
    }
}

fn find_config_path(args: &[String]) -> Result<Option<&str>, CliAction> {
    let pos = args.iter().position(|a| a == "--config" || a == "-c");
    match pos {
        None => Ok(None),
        Some(i) => {
            let val = args
                .get(i + 1)
                .ok_or_else(|| CliAction::Error("--config requires a value".to_owned()))?;
            Ok(Some(val.as_str()))
        }
    }
}

fn try_parse_args(args: &[String]) -> Result<Config, CliAction> {
    for arg in args {
        match arg.as_str() {
            "--help" | "-h" => return Err(CliAction::Help),
            "--version" | "-V" => return Err(CliAction::Version),
            _ => {}
        }
    }

    let mut cfg = load_config(find_config_path(args)?)?;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-r" | "--refresh" => {
                cfg.refresh_ms = parse_positive_int(args, "--refresh", &mut i)?;
            }
            "-t" | "--tab" => {
                let name = parse_flag_value(args, "--tab", &mut i)?;
                cfg.default_tab = parse_tab_name(name)?;
            }
            "-s" | "--no-sidebar" => cfg.hide_sidebar = true,
            "--tabs" => {
                let val = parse_flag_value(args, "--tabs", &mut i)?;
                cfg.tab_orientation = parse_tab_orientation(val)?;
            }
            "--scroll-step" => {
                cfg.scroll_step = parse_positive_int(args, "--scroll-step", &mut i)? as usize;
            }
            "-c" | "--config" => {
                i += 1;
            }
            _ => return Err(CliAction::Error(format!("unknown flag '{}'", args[i]))),
        }
        i += 1;
    }

    if cfg.refresh_ms == 0 {
        cfg.refresh_ms = 1000;
    }
    Ok(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::TabOrientation;

    fn parse_config(args: &[&str]) -> Config {
        let input: Vec<String> = args.iter().map(ToString::to_string).collect();
        match parse_args(&input) {
            CliAction::Config(cfg) => cfg,
            _ => panic!("expected Config, got {input:?}"),
        }
    }

    #[test]
    fn config_deserialize_full() {
        let toml_str = "refresh_ms = 500\ndefault_tab = \"proc\"\nhide_sidebar = true\n";
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.refresh_ms, 500);
        assert_eq!(cfg.default_tab, Tab::Proc);
        assert!(cfg.hide_sidebar);
    }

    #[test]
    fn config_deserialize_partial() {
        let toml_str = "refresh_ms = 200\n";
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.refresh_ms, 200);
        assert_eq!(cfg.default_tab, Tab::Dash);
        assert!(!cfg.hide_sidebar);
    }

    #[test]
    fn config_deserialize_empty() {
        let toml_str = "";
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.refresh_ms, 1000);
        assert_eq!(cfg.default_tab, Tab::Dash);
        assert!(!cfg.hide_sidebar);
        assert_eq!(cfg.tab_orientation, TabOrientation::Sidebar);
    }

    #[test]
    fn config_deserialize_all_tabs() {
        for (name, tab) in [
            ("dash", Tab::Dash),
            ("proc", Tab::Proc),
            ("net", Tab::Net),
            ("files", Tab::Files),
            ("time", Tab::Time),
            ("temp", Tab::Temp),
            ("cores", Tab::Cores),
            ("disk", Tab::Disk),
            ("mem", Tab::Mem),
        ] {
            let toml_str = format!("default_tab = \"{name}\"");
            let cfg: Config = toml::from_str(&toml_str).unwrap();
            assert_eq!(cfg.default_tab, tab, "tab name '{name}'");
        }
    }

    #[test]
    fn config_deserialize_unknown_field_ignored() {
        let toml_str = "nonexistent = true\n";
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.refresh_ms, 1000);
        assert_eq!(cfg.tab_orientation, TabOrientation::Sidebar);
    }

    #[test]
    fn config_deserialize_wrong_type() {
        let toml_str = "refresh_ms = \"not_a_number\"\n";
        let cfg: Result<Config, _> = toml::from_str(toml_str);
        assert!(cfg.is_err());
    }

    #[test]
    fn config_path_format() {
        let path = default_config_path();
        assert!(path.is_some());
        let path = path.unwrap();
        assert!(path.ends_with("thrum/config.toml"));
    }

    #[test]
    fn parse_args_short_flags() {
        let cfg = parse_config(&["-r", "500", "-t", "net", "-s"]);
        assert_eq!(cfg.refresh_ms, 500);
        assert_eq!(cfg.default_tab, Tab::Net);
        assert!(cfg.hide_sidebar);
    }

    #[test]
    fn parse_args_long_flags() {
        let cfg = parse_config(&["--refresh", "300", "--tab", "files", "--no-sidebar"]);
        assert_eq!(cfg.refresh_ms, 300);
        assert_eq!(cfg.default_tab, Tab::Files);
        assert!(cfg.hide_sidebar);
    }

    #[test]
    fn parse_args_partial() {
        let cfg = parse_config(&["-r", "200"]);
        assert_eq!(cfg.refresh_ms, 200);
        assert_eq!(cfg.default_tab, Tab::Dash);
        assert!(!cfg.hide_sidebar);
    }

    #[test]
    fn parse_args_tab_names() {
        for (name, tab) in [
            ("dash", Tab::Dash),
            ("proc", Tab::Proc),
            ("net", Tab::Net),
            ("files", Tab::Files),
            ("time", Tab::Time),
            ("temp", Tab::Temp),
            ("cores", Tab::Cores),
            ("disk", Tab::Disk),
            ("mem", Tab::Mem),
        ] {
            let cfg = parse_config(&["-t", name]);
            assert_eq!(cfg.default_tab, tab, "tab '{name}'");
        }
    }

    #[test]
    fn parse_args_order() {
        let cfg1 = parse_config(&["-r", "500", "-t", "mem", "-s"]);
        assert_eq!(cfg1.refresh_ms, 500);
        assert_eq!(cfg1.default_tab, Tab::Mem);
        assert!(cfg1.hide_sidebar);
        let cfg2 = parse_config(&["-s", "-r", "500", "-t", "mem"]);
        assert_eq!(cfg2, cfg1);
        let cfg3 = parse_config(&["-t", "mem", "-s", "-r", "500"]);
        assert_eq!(cfg3, cfg1);
    }

    #[test]
    fn parse_args_help_returns_help_action() {
        let result = parse_args(&["--help".into()]);
        assert!(matches!(result, CliAction::Help));
        let result = parse_args(&["-h".into()]);
        assert!(matches!(result, CliAction::Help));
    }

    #[test]
    fn parse_args_version_returns_version_action() {
        let result = parse_args(&["--version".into()]);
        assert!(matches!(result, CliAction::Version));
        let result = parse_args(&["-V".into()]);
        assert!(matches!(result, CliAction::Version));
    }

    #[test]
    fn parse_args_config_missing_value() {
        let result = parse_args(&["--config".into()]);
        assert!(matches!(result, CliAction::Error(_)));
    }

    #[test]
    fn parse_args_tabs_missing_value() {
        let result = parse_args(&["--tabs".into()]);
        assert!(matches!(result, CliAction::Error(_)));
    }

    #[test]
    fn parse_args_unknown_flag() {
        let result = parse_args(&["--bogus".into()]);
        assert!(matches!(result, CliAction::Error(_)));
    }

    #[test]
    fn parse_args_tabs_flag() {
        let cfg = parse_config(&["--tabs", "horizontal"]);
        assert_eq!(cfg.tab_orientation, TabOrientation::Horizontal);
        let cfg = parse_config(&["--tabs", "sidebar"]);
        assert_eq!(cfg.tab_orientation, TabOrientation::Sidebar);
    }

    #[test]
    fn parse_args_tabs_horizontal_footer() {
        let cfg = parse_config(&["--tabs", "horizontal_footer"]);
        assert_eq!(cfg.tab_orientation, TabOrientation::HorizontalFooter);
    }

    #[test]
    fn config_tab_orientation_deserialize() {
        let toml_str = "tab_orientation = \"horizontal\"\n";
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.tab_orientation, TabOrientation::Horizontal);
        let toml_str = "tab_orientation = \"sidebar\"\n";
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.tab_orientation, TabOrientation::Sidebar);
        let toml_str = "tab_orientation = \"horizontal_footer\"\n";
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.tab_orientation, TabOrientation::HorizontalFooter);
    }

    #[test]
    fn config_deserialize_proc_sort() {
        let toml_str = "proc_sort_default = \"name\"\nproc_sort_asc_default = true\n";
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.proc_sort_default, ProcSortField::Name);
        assert!(cfg.proc_sort_asc_default);
    }

    #[test]
    fn config_deserialize_history_window() {
        let toml_str = "history_window = 120\n";
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.history_window, 120);
    }

    #[test]
    fn config_deserialize_proc_sort_all_fields() {
        let cases: [(&str, ProcSortField); 7] = [
            ("name", ProcSortField::Name),
            ("pid", ProcSortField::Pid),
            ("cpu", ProcSortField::Cpu),
            ("memory", ProcSortField::Memory),
            ("virtual_memory", ProcSortField::VirtualMemory),
            ("run_time", ProcSortField::RunTime),
            ("status", ProcSortField::Status),
        ];
        for (name, expected) in &cases {
            let toml_str = format!("proc_sort_default = \"{name}\"");
            let cfg: Config = toml::from_str(&toml_str).unwrap();
            assert_eq!(cfg.proc_sort_default, *expected, "sort field '{name}'");
        }
    }

    #[test]
    fn read_config_file_invalid_toml() {
        let dir = std::env::temp_dir();
        let path = dir.join("thrum_test_bad_config.toml");
        let _ = std::fs::remove_file(&path);
        std::fs::write(&path, "invalid toml {{{").unwrap();
        let err = read_config_file(&path).unwrap_err();
        assert!(err.contains("invalid TOML"), "error: {err}");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn scroll_step_config_deserialize() {
        let toml_str = "scroll_step = 7\n";
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.scroll_step, 7);

        let toml_str = "";
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.scroll_step, 3, "default scroll_step");
    }

    #[test]
    fn scroll_step_cli_flag() {
        let cfg = parse_config(&["--scroll-step", "5"]);
        assert_eq!(cfg.scroll_step, 5);

        let cfg = parse_config(&[]);
        assert_eq!(cfg.scroll_step, 3, "default from Config::default()");
    }
}
