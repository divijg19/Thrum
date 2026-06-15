use std::collections::VecDeque;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde::Deserialize;
use sysinfo::Signal;

macro_rules! fatal {
    ($($arg:tt)*) => {{
        eprintln!("error: {}", format_args!($($arg)*));
        std::process::exit(1);
    }};
}

pub const PAGE_SIZE: usize = 10;
pub const WINDOW: usize = 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcSortField {
    Name,
    Pid,
    Cpu,
    Memory,
    VirtualMemory,
    RunTime,
    Status,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KillState {
    Confirm,
    Dispatch(Signal),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tab {
    #[default]
    Dash,
    Proc,
    Net,
    Files,
    Time,
    Temp,
    Cores,
    Disk,
    Mem,
}

impl Tab {
    pub const ALL: [Self; 9] = [
        Self::Dash,
        Self::Proc,
        Self::Net,
        Self::Files,
        Self::Time,
        Self::Temp,
        Self::Cores,
        Self::Disk,
        Self::Mem,
    ];

    pub const fn index(self) -> usize {
        match self {
            Self::Dash => 0,
            Self::Proc => 1,
            Self::Net => 2,
            Self::Files => 3,
            Self::Time => 4,
            Self::Temp => 5,
            Self::Cores => 6,
            Self::Disk => 7,
            Self::Mem => 8,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Dash => "Dash",
            Self::Proc => "Proc",
            Self::Net => "Net",
            Self::Files => "Files",
            Self::Time => "Time",
            Self::Temp => "Temp",
            Self::Cores => "Cores",
            Self::Disk => "Disk",
            Self::Mem => "Mem",
        }
    }
}

#[expect(clippy::struct_excessive_bools)]
pub struct App {
    pub active_tab: Tab,
    pub sidebar_visible: bool,
    pub cpu_history: VecDeque<u64>,
    pub mem_history: VecDeque<u64>,
    pub net_rx_history: VecDeque<u64>,
    pub net_tx_history: VecDeque<u64>,
    pub disk_read_history: VecDeque<u64>,
    pub disk_write_history: VecDeque<u64>,
    pub temp_history: VecDeque<u64>,
    pub disk_usage_history: VecDeque<u64>,
    pub swap_history: VecDeque<u64>,
    pub proc_scroll: usize,
    pub proc_selection: usize,
    pub selected_pid: Option<u32>,
    pub selected_name: Option<String>,
    pub kill_state: Option<KillState>,
    pub proc_query: String,
    pub proc_search_focused: bool,
    pub net_query: String,
    pub net_search_focused: bool,
    pub files_query: String,
    pub files_search_focused: bool,
    pub proc_sort_field: ProcSortField,
    pub proc_sort_asc: bool,
    pub should_quit: bool,
    pub help_visible: bool,
    pub kill_feedback: Option<String>,
}

impl App {
    pub fn new() -> Self {
        Self {
            active_tab: Tab::Dash,
            sidebar_visible: true,
            cpu_history: VecDeque::with_capacity(WINDOW),
            mem_history: VecDeque::with_capacity(WINDOW),
            net_rx_history: VecDeque::with_capacity(WINDOW),
            net_tx_history: VecDeque::with_capacity(WINDOW),
            disk_read_history: VecDeque::with_capacity(WINDOW),
            disk_write_history: VecDeque::with_capacity(WINDOW),
            temp_history: VecDeque::with_capacity(WINDOW),
            disk_usage_history: VecDeque::with_capacity(WINDOW),
            swap_history: VecDeque::with_capacity(WINDOW),
            proc_scroll: 0,
            proc_selection: 0,
            selected_pid: None,
            selected_name: None,
            kill_state: None,
            proc_query: String::new(),
            proc_search_focused: false,
            net_query: String::new(),
            net_search_focused: false,
            files_query: String::new(),
            files_search_focused: false,
            proc_sort_field: ProcSortField::Cpu,
            proc_sort_asc: false,
            should_quit: false,
            help_visible: false,
            kill_feedback: None,
        }
    }

    pub const fn apply_config(&mut self, cfg: &Config) {
        self.active_tab = cfg.default_tab;
        self.sidebar_visible = !cfg.hide_sidebar;
    }

    const SORT_MAP: &[(char, ProcSortField, bool)] = &[
        ('n', ProcSortField::Name, true),
        ('p', ProcSortField::Pid, true),
        ('c', ProcSortField::Cpu, false),
        ('m', ProcSortField::Memory, false),
        ('v', ProcSortField::VirtualMemory, false),
        ('t', ProcSortField::RunTime, false),
        ('s', ProcSortField::Status, true),
    ];

    #[allow(clippy::too_many_lines)]
    pub fn handle_key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.should_quit = true;
            return;
        }
        if self.proc_search_focused
            && self.active_tab == Tab::Proc
            && handle_search_input(&mut self.proc_query, &mut self.proc_search_focused, key)
        {
            return;
        }

        if self.net_search_focused
            && self.active_tab == Tab::Net
            && handle_search_input(&mut self.net_query, &mut self.net_search_focused, key)
        {
            return;
        }

        if self.files_search_focused
            && self.active_tab == Tab::Files
            && handle_search_input(&mut self.files_query, &mut self.files_search_focused, key)
        {
            return;
        }

        if self.help_visible {
            self.help_visible = false;
            if key.code == KeyCode::Char('?') {
                return;
            }
        }

        if self.kill_state == Some(KillState::Confirm) {
            match key.code {
                KeyCode::Char('y' | 'Y') => {
                    self.kill_state = Some(KillState::Dispatch(Signal::Term));
                }
                KeyCode::Char('k') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.kill_state = Some(KillState::Dispatch(Signal::Kill));
                }
                _ => self.kill_state = None,
            }
            return;
        }

        if self.active_tab == Tab::Proc
            && key.modifiers.is_empty()
            && let KeyCode::Char(c) = key.code
        {
            if c == 'r' {
                self.proc_sort_asc = !self.proc_sort_asc;
                return;
            }
            for &(ch, field, asc) in Self::SORT_MAP {
                if ch == c {
                    self.proc_sort_field = field;
                    self.proc_sort_asc = asc;
                    return;
                }
            }
        }

        match key.code {
            KeyCode::Char('q') if key.modifiers.is_empty() => self.should_quit = true,
            KeyCode::Char('?') => self.help_visible = !self.help_visible,
            KeyCode::Tab => {
                let idx = self.active_tab.index();
                self.active_tab = Tab::ALL[(idx + 1) % Tab::ALL.len()];
            }
            KeyCode::BackTab => {
                let idx = self.active_tab.index();
                self.active_tab = Tab::ALL[(idx + Tab::ALL.len() - 1) % Tab::ALL.len()];
            }
            KeyCode::Char('1') => self.active_tab = Tab::Dash,
            KeyCode::Char('2') => self.active_tab = Tab::Proc,
            KeyCode::Char('3') => self.active_tab = Tab::Net,
            KeyCode::Char('4') => self.active_tab = Tab::Files,
            KeyCode::Char('5') => self.active_tab = Tab::Time,
            KeyCode::Char('6') => self.active_tab = Tab::Temp,
            KeyCode::Char('7') => self.active_tab = Tab::Cores,
            KeyCode::Char('8') => self.active_tab = Tab::Disk,
            KeyCode::Char('9') => self.active_tab = Tab::Mem,
            KeyCode::Char('/') if key.modifiers.is_empty() => match self.active_tab {
                Tab::Proc => self.proc_search_focused = true,
                Tab::Net => self.net_search_focused = true,
                Tab::Files => self.files_search_focused = true,
                _ => {}
            },
            KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.sidebar_visible = !self.sidebar_visible;
            }
            KeyCode::Up if self.active_tab == Tab::Proc => {
                self.proc_selection = self.proc_selection.saturating_sub(1);
            }
            KeyCode::Down if self.active_tab == Tab::Proc => {
                self.proc_selection = self.proc_selection.saturating_add(1);
            }
            KeyCode::PageUp if self.active_tab == Tab::Proc => {
                self.proc_selection = self.proc_selection.saturating_sub(PAGE_SIZE);
            }
            KeyCode::PageDown if self.active_tab == Tab::Proc => {
                self.proc_selection = self.proc_selection.saturating_add(PAGE_SIZE);
            }
            KeyCode::Delete if self.active_tab == Tab::Proc => {
                self.kill_state = Some(KillState::Confirm);
            }
            KeyCode::Char('k')
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    && self.active_tab == Tab::Proc =>
            {
                self.kill_state = Some(KillState::Dispatch(Signal::Kill));
            }
            _ => {}
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Config {
    pub refresh_ms: u64,
    pub default_tab: Tab,
    pub hide_sidebar: bool,
}

fn default_config_path() -> Option<PathBuf> {
    if let Ok(xdg) = env::var("XDG_CONFIG_HOME") {
        return Some(PathBuf::from(xdg).join("thrum/config.toml"));
    }
    let home = env::var("HOME").ok()?;
    Some(PathBuf::from(home).join(".config/thrum/config.toml"))
}

fn read_config_file(path: &Path) -> Option<Config> {
    let content = fs::read_to_string(path).ok()?;
    toml::from_str(&content)
        .inspect_err(|e| {
            eprintln!(
                "warning: config file '{}' has invalid TOML: {e}",
                path.display()
            );
        })
        .ok()
}

pub fn parse_args(args: &[String]) -> Config {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        eprintln!("Usage: thrum [OPTIONS]");
        eprintln!();
        eprintln!(
            "  -c, --config <path>   Config file path (default: ~/.config/thrum/config.toml)"
        );
        eprintln!("  -r, --refresh <ms>    Refresh interval (default: 1000)");
        eprintln!(
            "  -t, --tab <name>      Default tab (dash|proc|net|files|time|temp|cores|disk|mem)"
        );
        eprintln!("  -s, --no-sidebar      Start with sidebar hidden");
        eprintln!("  -V, --version         Show version");
        eprintln!("  --help                Show this help");
        std::process::exit(0);
    }

    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("thrum {}", env!("CARGO_PKG_VERSION"));
        std::process::exit(0);
    }

    let config_path = (0..args.len())
        .rfind(|&i| args[i] == "--config" || args[i] == "-c")
        .and_then(|i| args.get(i + 1))
        .cloned();

    let mut cfg = config_path.as_ref().map_or_else(
        || {
            default_config_path()
                .and_then(|p| read_config_file(&p))
                .unwrap_or_default()
        },
        |path| {
            let p = Path::new(path);
            if !p.exists() {
                fatal!("config file '{path}' not found");
            }
            read_config_file(p).unwrap_or_else(|| {
                fatal!("config file '{path}' has invalid TOML");
            })
        },
    );

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-r" | "--refresh" => {
                i += 1;
                let val = args.get(i).unwrap_or_else(|| {
                    fatal!("--refresh requires a value");
                });
                cfg.refresh_ms = val.parse().unwrap_or_else(|_| {
                    fatal!("--refresh must be a positive integer");
                });
                if cfg.refresh_ms == 0 {
                    fatal!("--refresh must be > 0");
                }
            }
            "-t" | "--tab" => {
                i += 1;
                let name = args.get(i).unwrap_or_else(|| {
                    fatal!("--tab requires a value");
                });
                cfg.default_tab = match name.to_lowercase().as_str() {
                    "dash" => Tab::Dash,
                    "proc" => Tab::Proc,
                    "net" => Tab::Net,
                    "files" => Tab::Files,
                    "time" => Tab::Time,
                    "temp" => Tab::Temp,
                    "cores" => Tab::Cores,
                    "disk" => Tab::Disk,
                    "mem" => Tab::Mem,
                    _ => {
                        fatal!("unknown tab '{name}'");
                    }
                };
            }
            "-s" | "--no-sidebar" => {
                cfg.hide_sidebar = true;
            }
            "--config" | "-c" => {
                args.get(i + 1).unwrap_or_else(|| {
                    fatal!("--config requires a value");
                });
                i += 1;
            }
            _ => {
                fatal!("unknown flag '{}'", args[i]);
            }
        }
        i += 1;
    }

    cfg
}

pub fn push_bounded<T>(deque: &mut VecDeque<T>, value: T, max: usize) {
    if deque.len() >= max {
        deque.pop_front();
    }
    deque.push_back(value);
}

fn handle_search_input(query: &mut String, focused: &mut bool, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Char(c) if key.modifiers.is_empty() => {
            query.push(c);
            true
        }
        KeyCode::Backspace => {
            query.pop();
            if query.is_empty() {
                *focused = false;
            }
            true
        }
        KeyCode::Esc => {
            query.clear();
            *focused = false;
            true
        }
        _ => {
            *focused = false;
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use sysinfo::Signal;

    #[test]
    fn tab_has_nine_variants() {
        assert_eq!(Tab::ALL.len(), 9);
    }

    #[test]
    fn tab_label_matches() {
        assert_eq!(Tab::Dash.label(), "Dash");
        assert_eq!(Tab::Proc.label(), "Proc");
        assert_eq!(Tab::Net.label(), "Net");
        assert_eq!(Tab::Files.label(), "Files");
        assert_eq!(Tab::Time.label(), "Time");
        assert_eq!(Tab::Temp.label(), "Temp");
        assert_eq!(Tab::Cores.label(), "Cores");
        assert_eq!(Tab::Disk.label(), "Disk");
        assert_eq!(Tab::Mem.label(), "Mem");
    }

    #[test]
    fn app_new_defaults() {
        let app = App::new();
        assert_eq!(app.active_tab, Tab::Dash);
        assert!(app.sidebar_visible);
        assert!(!app.should_quit);
        assert_eq!(app.proc_scroll, 0);
        assert_eq!(app.proc_selection, 0);
        assert!(app.selected_pid.is_none());
        assert!(app.selected_name.is_none());
        assert!(app.kill_state.is_none());
        assert!(app.proc_query.is_empty());
        assert!(!app.proc_search_focused);
        assert!(app.net_query.is_empty());
        assert!(!app.net_search_focused);
        assert!(app.files_query.is_empty());
        assert!(!app.files_search_focused);
        assert_eq!(app.mem_history.len(), 0);
        assert_eq!(app.net_rx_history.len(), 0);
        assert_eq!(app.net_tx_history.len(), 0);
        assert_eq!(app.disk_read_history.len(), 0);
        assert_eq!(app.disk_write_history.len(), 0);
        assert_eq!(app.temp_history.len(), 0);
        assert_eq!(app.disk_usage_history.len(), 0);
        assert_eq!(app.swap_history.len(), 0);
        assert_eq!(app.proc_sort_field, ProcSortField::Cpu);
        assert!(!app.proc_sort_asc);
        assert!(!app.help_visible);
        assert!(app.kill_feedback.is_none());
    }

    #[test]
    fn key_q_quits() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
        assert!(app.should_quit);
    }

    #[test]
    fn key_ctrl_c_quits() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(app.should_quit);
    }

    #[test]
    fn key_plain_c_does_not_quit() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));
        assert!(!app.should_quit);
    }

    #[test]
    fn key_tab_cycles_forward() {
        let mut app = App::new();
        assert_eq!(app.active_tab, Tab::Dash);
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.active_tab, Tab::Proc);
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.active_tab, Tab::Net);
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.active_tab, Tab::Files);
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.active_tab, Tab::Time);
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.active_tab, Tab::Temp);
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.active_tab, Tab::Cores);
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.active_tab, Tab::Disk);
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.active_tab, Tab::Mem);
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.active_tab, Tab::Dash);
    }

    #[test]
    fn key_backtab_cycles_backward() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('8'), KeyModifiers::NONE));
        assert_eq!(app.active_tab, Tab::Disk);
        app.handle_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE));
        assert_eq!(app.active_tab, Tab::Cores);
        app.handle_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE));
        assert_eq!(app.active_tab, Tab::Temp);
        app.handle_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE));
        assert_eq!(app.active_tab, Tab::Time);
        app.handle_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE));
        assert_eq!(app.active_tab, Tab::Files);
        app.handle_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE));
        assert_eq!(app.active_tab, Tab::Net);
        app.handle_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE));
        assert_eq!(app.active_tab, Tab::Proc);
        app.handle_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE));
        assert_eq!(app.active_tab, Tab::Dash);
        app.handle_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE));
        assert_eq!(app.active_tab, Tab::Mem);
        app.handle_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE));
        assert_eq!(app.active_tab, Tab::Disk);
    }

    #[test]
    fn key_numbers_jump_to_tab() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
        assert_eq!(app.active_tab, Tab::Proc);
        app.handle_key(KeyEvent::new(KeyCode::Char('4'), KeyModifiers::NONE));
        assert_eq!(app.active_tab, Tab::Files);
        app.handle_key(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE));
        assert_eq!(app.active_tab, Tab::Dash);
        app.handle_key(KeyEvent::new(KeyCode::Char('5'), KeyModifiers::NONE));
        assert_eq!(app.active_tab, Tab::Time);
        app.handle_key(KeyEvent::new(KeyCode::Char('3'), KeyModifiers::NONE));
        assert_eq!(app.active_tab, Tab::Net);
        app.handle_key(KeyEvent::new(KeyCode::Char('6'), KeyModifiers::NONE));
        assert_eq!(app.active_tab, Tab::Temp);
        app.handle_key(KeyEvent::new(KeyCode::Char('7'), KeyModifiers::NONE));
        assert_eq!(app.active_tab, Tab::Cores);
        app.handle_key(KeyEvent::new(KeyCode::Char('8'), KeyModifiers::NONE));
        assert_eq!(app.active_tab, Tab::Disk);
        app.handle_key(KeyEvent::new(KeyCode::Char('9'), KeyModifiers::NONE));
        assert_eq!(app.active_tab, Tab::Mem);
    }

    #[test]
    fn key_ctrl_s_toggles_sidebar() {
        let mut app = App::new();
        assert!(app.sidebar_visible);
        app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
        assert!(!app.sidebar_visible);
        app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
        assert!(app.sidebar_visible);
    }

    #[test]
    fn key_plain_s_does_not_toggle() {
        let mut app = App::new();
        assert!(app.sidebar_visible);
        app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        assert!(app.sidebar_visible);
    }

    #[test]
    fn selection_moves_down() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
        assert_eq!(app.proc_selection, 0);
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.proc_selection, 1);
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.proc_selection, 2);
    }

    #[test]
    fn selection_moves_up() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.proc_selection, 2);
        app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(app.proc_selection, 1);
        app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(app.proc_selection, 0);
        app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(app.proc_selection, 0);
    }

    #[test]
    fn selection_page_down() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
        assert_eq!(app.proc_selection, 0);
        app.handle_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE));
        assert_eq!(app.proc_selection, PAGE_SIZE);
        app.handle_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE));
        assert_eq!(app.proc_selection, PAGE_SIZE * 2);
    }

    #[test]
    fn selection_page_up() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE));
        assert_eq!(app.proc_selection, PAGE_SIZE);
        app.handle_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE));
        assert_eq!(app.proc_selection, 0);
        app.handle_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE));
        assert_eq!(app.proc_selection, 0);
    }

    #[test]
    fn selection_noop_on_non_proc() {
        let mut app = App::new();
        assert_eq!(app.active_tab, Tab::Dash);
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.proc_selection, 0);
        app.handle_key(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE));
        assert!(app.kill_state.is_none());
    }

    #[test]
    fn delete_sets_kill_pending_on_proc() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
        assert!(app.kill_state.is_none());
        app.handle_key(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE));
        assert_eq!(app.kill_state, Some(KillState::Confirm));
    }

    #[test]
    fn ctrl_k_sets_immediate_kill() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL));
        assert_eq!(app.kill_state, Some(KillState::Dispatch(Signal::Kill)));
    }

    #[test]
    fn kill_pending_dismissed_by_any_key() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE));
        assert_eq!(app.kill_state, Some(KillState::Confirm));
        app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
        assert!(app.kill_state.is_none());
    }

    #[test]
    fn kill_pending_confirmed_by_y() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE));
        assert_eq!(app.kill_state, Some(KillState::Confirm));
        app.handle_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE));
        assert_eq!(app.kill_state, Some(KillState::Dispatch(Signal::Term)));
    }

    #[test]
    fn kill_pending_sends_sigkill_on_ctrl_k() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE));
        assert_eq!(app.kill_state, Some(KillState::Confirm));
        app.handle_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL));
        assert_eq!(app.kill_state, Some(KillState::Dispatch(Signal::Kill)));
    }

    #[test]
    fn kill_pending_confirmed_by_capital_y() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('Y'), KeyModifiers::NONE));
        assert_eq!(app.kill_state, Some(KillState::Dispatch(Signal::Term)));
    }

    #[test]
    fn delete_exits_search_and_kills() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        assert!(app.proc_search_focused);
        app.handle_key(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE));
        assert!(!app.proc_search_focused);
        assert_eq!(app.kill_state, Some(KillState::Confirm));
    }

    #[test]
    fn key_slash_enters_search_only_on_proc() {
        let mut app = App::new();
        assert_eq!(app.active_tab, Tab::Dash);
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        assert!(!app.proc_search_focused);

        app.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
        assert_eq!(app.active_tab, Tab::Proc);
        assert!(!app.proc_search_focused);

        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        assert!(app.proc_search_focused);
    }

    #[test]
    fn search_typing_appends_query() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        assert!(app.proc_search_focused);
        assert_eq!(app.active_tab, Tab::Proc);

        app.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));
        assert_eq!(app.proc_query, "fir");
        assert_eq!(app.active_tab, Tab::Proc);
    }

    #[test]
    fn search_backspace_pops() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
        assert_eq!(app.proc_query, "ab");

        app.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(app.proc_query, "a");
        assert!(app.proc_search_focused);
    }

    #[test]
    fn search_backspace_empty_exits() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert!(app.proc_query.is_empty());
        assert!(!app.proc_search_focused);
    }

    #[test]
    fn search_esc_clears_and_exits() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        assert_eq!(app.proc_query, "x");

        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.proc_query.is_empty());
        assert!(!app.proc_search_focused);
    }

    #[test]
    fn search_enter_exits_keeps_query() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE));

        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(!app.proc_search_focused);
        assert_eq!(app.proc_query, "f");
    }

    #[test]
    fn search_tab_switches_tab() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        assert!(app.proc_search_focused);

        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert!(!app.proc_search_focused);
        assert_eq!(app.active_tab, Tab::Net);
    }

    #[test]
    fn key_slash_enters_search_on_net() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('3'), KeyModifiers::NONE));
        assert!(!app.net_search_focused);
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        assert!(app.net_search_focused);
    }

    #[test]
    fn key_slash_enters_search_on_files() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('4'), KeyModifiers::NONE));
        assert!(!app.files_search_focused);
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        assert!(app.files_search_focused);
    }

    #[test]
    fn key_slash_noop_on_dash() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        assert!(!app.proc_search_focused);
        assert!(!app.net_search_focused);
        assert!(!app.files_search_focused);
    }

    #[test]
    fn net_search_esc_clears() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('3'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE));
        assert_eq!(app.net_query, "e");
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.net_query.is_empty());
        assert!(!app.net_search_focused);
    }

    #[test]
    fn files_search_esc_clears() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('4'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE));
        assert_eq!(app.files_query, "m");
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.files_query.is_empty());
        assert!(!app.files_search_focused);
    }

    #[test]
    fn sort_key_s_sorts_by_status() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        assert_eq!(app.proc_sort_field, ProcSortField::Status);
        assert!(app.proc_sort_asc);
    }

    #[test]
    fn sort_keys_change_sort_field() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
        assert_eq!(app.active_tab, Tab::Proc);
        assert_eq!(app.proc_sort_field, ProcSortField::Cpu);
        assert!(!app.proc_sort_asc);

        app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
        assert_eq!(app.proc_sort_field, ProcSortField::Name);
        assert!(app.proc_sort_asc);

        app.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));
        assert!(!app.proc_sort_asc);

        app.handle_key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE));
        assert_eq!(app.proc_sort_field, ProcSortField::Memory);
        assert!(!app.proc_sort_asc);
    }

    #[test]
    fn key_unknown_does_nothing() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        assert!(!app.should_quit);
        assert_eq!(app.active_tab, Tab::Dash);
        assert!(app.sidebar_visible);
    }

    #[test]
    fn help_toggle_opens_and_closes() {
        let mut app = App::new();
        assert!(!app.help_visible);
        app.handle_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));
        assert!(app.help_visible);
        app.handle_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));
        assert!(!app.help_visible);
    }

    #[test]
    fn help_closes_on_any_key() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));
        assert!(app.help_visible);
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert!(!app.help_visible);
        assert_eq!(app.active_tab, Tab::Proc);
    }

    #[test]
    fn apply_config_sets_active_tab() {
        let mut app = App::new();
        assert_eq!(app.active_tab, Tab::Dash);
        let cfg = Config {
            refresh_ms: 500,
            default_tab: Tab::Proc,
            hide_sidebar: false,
        };
        app.apply_config(&cfg);
        assert_eq!(app.active_tab, Tab::Proc);
        assert!(app.sidebar_visible);
    }

    #[test]
    fn apply_config_hides_sidebar() {
        let mut app = App::new();
        assert!(app.sidebar_visible);
        let cfg = Config {
            refresh_ms: 1000,
            default_tab: Tab::Dash,
            hide_sidebar: true,
        };
        app.apply_config(&cfg);
        assert!(!app.sidebar_visible);
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
        assert_eq!(cfg.refresh_ms, 0);
        assert_eq!(cfg.default_tab, Tab::Dash);
        assert!(!cfg.hide_sidebar);
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
        assert_eq!(cfg.refresh_ms, 0);
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
    fn config_cli_overrides() {
        let toml_str = "refresh_ms = 500\ndefault_tab = \"dash\"\nhide_sidebar = true\n";
        let mut cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.refresh_ms, 500);
        cfg.refresh_ms = 200;
        cfg.hide_sidebar = false;
        assert_eq!(cfg.refresh_ms, 200);
        assert_eq!(cfg.default_tab, Tab::Dash);
        assert!(!cfg.hide_sidebar);
    }

    #[test]
    fn tab_index_matches_all() {
        for (i, tab) in Tab::ALL.iter().enumerate() {
            assert_eq!(tab.index(), i, "{tab:?} index should be {i}");
        }
    }

    #[test]
    fn parse_args_short_flags() {
        let cfg = parse_args(&[
            "-r".into(),
            "500".into(),
            "-t".into(),
            "net".into(),
            "-s".into(),
        ]);
        assert_eq!(cfg.refresh_ms, 500);
        assert_eq!(cfg.default_tab, Tab::Net);
        assert!(cfg.hide_sidebar);
    }

    #[test]
    fn parse_args_long_flags() {
        let cfg = parse_args(&[
            "--refresh".into(),
            "300".into(),
            "--tab".into(),
            "files".into(),
            "--no-sidebar".into(),
        ]);
        assert_eq!(cfg.refresh_ms, 300);
        assert_eq!(cfg.default_tab, Tab::Files);
        assert!(cfg.hide_sidebar);
    }

    #[test]
    fn parse_args_partial() {
        let cfg = parse_args(&["-r".into(), "200".into()]);
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
            let cfg = parse_args(&["-t".into(), name.into()]);
            assert_eq!(cfg.default_tab, tab, "tab '{name}'");
        }
    }

    #[test]
    fn parse_args_order() {
        let cfg1 = parse_args(&[
            "-r".into(),
            "500".into(),
            "-t".into(),
            "mem".into(),
            "-s".into(),
        ]);
        assert_eq!(cfg1.refresh_ms, 500);
        assert_eq!(cfg1.default_tab, Tab::Mem);
        assert!(cfg1.hide_sidebar);
        let cfg2 = parse_args(&[
            "-s".into(),
            "-r".into(),
            "500".into(),
            "-t".into(),
            "mem".into(),
        ]);
        assert_eq!(cfg2, cfg1);
        let cfg3 = parse_args(&[
            "-t".into(),
            "mem".into(),
            "-s".into(),
            "-r".into(),
            "500".into(),
        ]);
        assert_eq!(cfg3, cfg1);
    }

    #[test]
    fn parse_args_long_refresh() {
        let cfg = parse_args(&["--refresh".into(), "500".into()]);
        assert_eq!(cfg.refresh_ms, 500);
    }

    #[test]
    fn parse_args_long_tab() {
        let cfg = parse_args(&["--tab".into(), "disk".into()]);
        assert_eq!(cfg.default_tab, Tab::Disk);
    }

    #[test]
    fn sort_keys_all_fields() {
        use crossterm::event::KeyCode;

        let keys: [(KeyCode, ProcSortField, bool); 7] = [
            (KeyCode::Char('n'), ProcSortField::Name, true),
            (KeyCode::Char('p'), ProcSortField::Pid, true),
            (KeyCode::Char('c'), ProcSortField::Cpu, false),
            (KeyCode::Char('m'), ProcSortField::Memory, false),
            (KeyCode::Char('v'), ProcSortField::VirtualMemory, false),
            (KeyCode::Char('t'), ProcSortField::RunTime, false),
            (KeyCode::Char('s'), ProcSortField::Status, true),
        ];
        for (key, expected_field, expected_asc) in &keys {
            let mut app = App::new();
            app.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
            app.handle_key(KeyEvent::new(*key, KeyModifiers::NONE));
            assert_eq!(
                app.proc_sort_field, *expected_field,
                "key '{:?}' should set sort field to {expected_field:?}",
                key
            );
            assert_eq!(
                app.proc_sort_asc, *expected_asc,
                "key '{:?}' should set sort_asc to {expected_asc}",
                key
            );
        }
    }

    #[test]
    fn push_bounded_respects_capacity() {
        let mut d: VecDeque<u32> = VecDeque::with_capacity(3);
        push_bounded(&mut d, 1, 3);
        push_bounded(&mut d, 2, 3);
        push_bounded(&mut d, 3, 3);
        assert_eq!(d.len(), 3);
        push_bounded(&mut d, 4, 3);
        assert_eq!(d.len(), 3);
        assert_eq!(*d.front().unwrap(), 2);
        assert_eq!(*d.back().unwrap(), 4);
    }

    #[test]
    fn push_bounded_under_max_appends() {
        let mut d: VecDeque<i32> = VecDeque::new();
        push_bounded(&mut d, 42, 10);
        assert_eq!(d.len(), 1);
        assert_eq!(*d.front().unwrap(), 42);
    }

    #[test]
    fn push_bounded_fifo_order() {
        let mut d: VecDeque<&str> = VecDeque::with_capacity(2);
        push_bounded(&mut d, "a", 2);
        push_bounded(&mut d, "b", 2);
        push_bounded(&mut d, "c", 2);
        let v: Vec<&&str> = d.iter().collect();
        assert_eq!(v, vec![&"b", &"c"]);
    }
}
