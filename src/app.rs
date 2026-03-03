use std::collections::VecDeque;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEventKind};
use serde::Deserialize;
use sysinfo::Signal;

use crate::samplers::Samples;

pub const PAGE_SIZE: usize = 10;
pub const WINDOW: usize = 60;
pub const MAX_QUERY_LEN: usize = 256;
pub const MAX_CLICK_OFFSET: usize = 1000;

pub const KILL_SIGNAL_MAP: &[(char, &str, Signal)] = &[
    ('1', "SIGTERM", Signal::Term),
    ('2', "SIGKILL", Signal::Kill),
    ('3', "SIGINT", Signal::Interrupt),
    ('4', "SIGHUP", Signal::Hangup),
    ('5', "SIGSTOP", Signal::Stop),
    ('6', "SIGCONT", Signal::Continue),
];

pub enum CliAction {
    Help,
    Version,
    Error(String),
    Config(Config),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcSortField {
    Name,
    Pid,
    Cpu,
    Memory,
    VirtualMemory,
    RunTime,
    Status,
}

impl std::fmt::Display for ProcSortField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Name => write!(f, "Name"),
            Self::Pid => write!(f, "PID"),
            Self::Cpu => write!(f, "CPU"),
            Self::Memory => write!(f, "Memory"),
            Self::VirtualMemory => write!(f, "Virt Mem"),
            Self::RunTime => write!(f, "Run Time"),
            Self::Status => write!(f, "Status"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KillState {
    Confirm,
    Dispatch(Signal),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TabOrientation {
    #[default]
    Sidebar,
    Horizontal,
    HorizontalFooter,
}

impl TabOrientation {
    pub const fn is_horizontal(self) -> bool {
        matches!(self, Self::Horizontal | Self::HorizontalFooter)
    }
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

    pub const fn is_proc(self) -> bool {
        matches!(self, Self::Proc)
    }

    pub const fn has_searchable_state(self) -> bool {
        matches!(self, Self::Proc | Self::Net | Self::Files)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TabState {
    pub query: String,
    pub focused: bool,
    pub scroll: usize,
    pub selection: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectionState {
    pub pid: u32,
    pub name: String,
}

#[expect(clippy::struct_excessive_bools)]
pub struct App {
    pub active_tab: Tab,
    pub sidebar_visible: bool,
    pub tab_orientation: TabOrientation,
    pub tab_bar_visible: bool,
    pub cpu_history: VecDeque<u64>,
    pub mem_history: VecDeque<u64>,
    pub net_rx_history: VecDeque<u64>,
    pub net_tx_history: VecDeque<u64>,
    pub disk_read_history: VecDeque<u64>,
    pub disk_write_history: VecDeque<u64>,
    pub temp_history: VecDeque<u64>,
    pub disk_usage_history: VecDeque<u64>,
    pub swap_history: VecDeque<u64>,
    pub proc_state: TabState,
    pub net_state: TabState,
    pub files_state: TabState,
    pub selected: Option<SelectionState>,
    pub kill_state: Option<KillState>,
    pub scroll_step: usize,
    pub proc_sort_field: ProcSortField,
    pub proc_sort_asc: bool,
    pub should_quit: bool,
    pub help_visible: bool,
    pub kill_feedback: Option<String>,
    pub paused: bool,
    pub history_window: usize,
    pub term_width: u16,
    pub term_height: u16,
    pub error_msg: Option<String>,
}

impl App {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn apply_config(&mut self, cfg: &Config) {
        self.active_tab = cfg.default_tab;
        self.sidebar_visible = !cfg.hide_sidebar;
        self.tab_orientation = cfg.tab_orientation;
        self.proc_sort_field = cfg.proc_sort_default;
        self.proc_sort_asc = cfg.proc_sort_asc_default;
        self.history_window = cfg.history_window.clamp(1, 3600);
        self.scroll_step = cfg.scroll_step.clamp(1, 100);
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

    pub fn handle_key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.should_quit = true;
            return;
        }

        if self.handle_search_mode(key) {
            return;
        }

        if self.help_visible {
            self.help_visible = false;
            if key.code == KeyCode::Char('?') {
                return;
            }
        }

        if self.handle_kill_confirm_mode(key) {
            return;
        }

        if key.code == KeyCode::Char(' ') && key.modifiers.is_empty() {
            self.paused = !self.paused;
            return;
        }

        if self.handle_proc_sort_key(key) {
            return;
        }

        self.dispatch_key(key);
    }

    fn dispatch_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') if key.modifiers.is_empty() => self.should_quit = true,
            KeyCode::Char('?') if key.modifiers.is_empty() => {
                self.help_visible = !self.help_visible;
            }
            KeyCode::Tab => self.cycle_tab(true),
            KeyCode::BackTab => self.cycle_tab(false),
            KeyCode::Char(c) if key.modifiers.is_empty() && c.is_ascii_digit() && c != '0' => {
                let idx = (c as u8 - b'1') as usize;
                if idx < Tab::ALL.len() {
                    self.active_tab = Tab::ALL[idx];
                }
            }
            KeyCode::Char('/') if key.modifiers.is_empty() => {
                if let Some(state) = self.tab_state_mut() {
                    state.focused = true;
                }
            }
            KeyCode::Char('t') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.cycle_orientation();
            }
            KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.toggle_sidebar_or_bar();
            }
            KeyCode::Up | KeyCode::Down | KeyCode::PageUp | KeyCode::PageDown => {
                self.handle_nav_key(key.code);
            }
            KeyCode::Right if self.tab_orientation.is_horizontal() => {
                self.cycle_tab(true);
            }
            KeyCode::Left if self.tab_orientation.is_horizontal() => {
                self.cycle_tab(false);
            }
            KeyCode::Delete if self.active_tab.is_proc() && self.selected.is_some() => {
                self.kill_state = Some(KillState::Confirm);
            }
            KeyCode::Char('k')
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    && self.active_tab.is_proc()
                    && self.selected.is_some() =>
            {
                self.kill_state = Some(KillState::Dispatch(Signal::Kill));
            }
            _ => {}
        }
    }

    const fn cycle_orientation(&mut self) {
        self.tab_orientation = match self.tab_orientation {
            TabOrientation::Sidebar => TabOrientation::Horizontal,
            TabOrientation::Horizontal => TabOrientation::HorizontalFooter,
            TabOrientation::HorizontalFooter => TabOrientation::Sidebar,
        };
    }

    const fn toggle_sidebar_or_bar(&mut self) {
        if self.tab_orientation.is_horizontal() {
            self.tab_bar_visible = !self.tab_bar_visible;
        } else {
            self.sidebar_visible = !self.sidebar_visible;
        }
    }

    fn handle_nav_key(&mut self, code: KeyCode) {
        if let Some(state) = self.tab_state_mut() {
            Self::move_selection(code, &mut state.selection);
            self.clear_selection_and_kill();
        }
    }

    fn handle_search_mode(&mut self, key: KeyEvent) -> bool {
        let is_ctrl_k =
            key.code == KeyCode::Char('k') && key.modifiers.contains(KeyModifiers::CONTROL);
        let Some(state) = self.tab_state_mut() else {
            return false;
        };
        if state.focused {
            let consumed = handle_search_input(&mut state.query, &mut state.focused, key);
            if consumed && !is_ctrl_k {
                return true;
            }
        }
        false
    }

    fn handle_kill_confirm_mode(&mut self, key: KeyEvent) -> bool {
        if self.kill_state != Some(KillState::Confirm) {
            return false;
        }
        self.kill_state = match key.code {
            KeyCode::Char(c) => KILL_SIGNAL_MAP
                .iter()
                .find(|&&(k, _, _)| k == c)
                .map(|&(_, _, s)| KillState::Dispatch(s)),
            _ => None,
        };
        true
    }

    fn handle_proc_sort_key(&mut self, key: KeyEvent) -> bool {
        if !self.active_tab.is_proc() || !key.modifiers.is_empty() {
            return false;
        }
        if let KeyCode::Char(c) = key.code {
            if c == 'r' {
                self.proc_sort_asc = !self.proc_sort_asc;
                return true;
            }
            for &(ch, field, asc) in Self::SORT_MAP {
                if ch == c {
                    self.proc_sort_field = field;
                    self.proc_sort_asc = asc;
                    return true;
                }
            }
        }
        false
    }

    fn apply_scroll(&mut self, up: bool) {
        let step = self.scroll_step;
        if let Some(state) = self.tab_state_mut() {
            if up {
                state.selection = state.selection.saturating_sub(step);
            } else {
                state.selection = state.selection.saturating_add(step);
            }
            self.clear_selection_and_kill();
        }
    }

    pub fn handle_mouse(&mut self, col: u16, row: u16, kind: MouseEventKind) {
        match kind {
            MouseEventKind::Down(MouseButton::Left) => self.handle_click(col, row),
            MouseEventKind::ScrollUp => {
                if self.is_on_tab_bar(col, row) {
                    return self.cycle_tab(false);
                }
                self.apply_scroll(true);
            }
            MouseEventKind::ScrollDown => {
                if self.is_on_tab_bar(col, row) {
                    return self.cycle_tab(true);
                }
                self.apply_scroll(false);
            }
            _ => {}
        }
    }

    fn handle_click(&mut self, col: u16, row: u16) {
        if self.handle_sidebar_click(col, row) {
            return;
        }
        if self.handle_tab_bar_click(col, row) {
            return;
        }
        self.handle_searchable_data_click(row);
    }

    fn handle_sidebar_click(&mut self, col: u16, row: u16) -> bool {
        if self.tab_orientation == TabOrientation::Sidebar
            && self.sidebar_visible
            && (1..=9).contains(&col)
            && row >= 1
        {
            let idx = (row - 1) as usize;
            if idx < Tab::ALL.len() {
                self.active_tab = Tab::ALL[idx];
                self.kill_state = None;
                return true;
            }
        }
        false
    }

    fn handle_tab_bar_click(&mut self, col: u16, row: u16) -> bool {
        if self.tab_bar_visible
            && match self.tab_orientation {
                TabOrientation::Horizontal => row == 1,
                TabOrientation::HorizontalFooter => row == self.term_height.saturating_sub(3),
                TabOrientation::Sidebar => false,
            }
            && let Some(idx) = self.tab_from_horizontal_click(col)
        {
            self.active_tab = Tab::ALL[idx];
            self.kill_state = None;
            return true;
        }
        false
    }

    fn handle_searchable_data_click(&mut self, row: u16) {
        if self.active_tab.has_searchable_state() {
            let tab_y: u16 = match self.tab_orientation {
                TabOrientation::Horizontal if self.tab_bar_visible => 2,
                _ => 1,
            };
            let state = self.tab_state_mut().expect("checked Proc|Net|Files");
            let search_h: u16 = if !state.query.is_empty() || state.focused {
                3
            } else {
                0
            };
            let data_start = tab_y + search_h + 2;
            if row >= data_start {
                state.focused = false;
                let offset = (row - data_start) as usize;
                state.selection = state.scroll.saturating_add(offset.min(MAX_CLICK_OFFSET));
                self.clear_selection_and_kill();
            }
        }
    }

    fn is_on_tab_bar(&self, col: u16, row: u16) -> bool {
        match self.tab_orientation {
            TabOrientation::Sidebar if self.sidebar_visible => {
                (1..=9).contains(&col) && row >= 1 && ((row - 1) as usize) < Tab::ALL.len()
            }
            TabOrientation::Horizontal if self.tab_bar_visible => row == 1,
            TabOrientation::HorizontalFooter if self.tab_bar_visible => {
                row == self.term_height.saturating_sub(3)
            }
            _ => false,
        }
    }

    fn clear_selection_and_kill(&mut self) {
        if self.active_tab.is_proc() {
            self.selected = None;
            self.kill_state = None;
        }
    }

    const fn move_selection(code: KeyCode, selection: &mut usize) -> bool {
        match code {
            KeyCode::Up => *selection = selection.saturating_sub(1),
            KeyCode::Down => *selection = selection.saturating_add(1),
            KeyCode::PageUp => *selection = selection.saturating_sub(PAGE_SIZE),
            KeyCode::PageDown => *selection = selection.saturating_add(PAGE_SIZE),
            _ => return false,
        }
        true
    }

    const fn cycle_tab(&mut self, forward: bool) {
        let idx = self.active_tab.index();
        let n = Tab::ALL.len();
        self.active_tab = Tab::ALL[(idx + if forward { 1 } else { n - 1 }) % n];
    }

    pub fn refresh_term_size(&mut self) -> bool {
        if let Ok((w, h)) = crossterm::terminal::size() {
            self.term_width = w;
            self.term_height = h;
            true
        } else {
            false
        }
    }

    pub fn selected_pid(&self) -> u32 {
        self.selected.as_ref().map_or(0, |s| s.pid)
    }

    pub fn selected_name(&self) -> &str {
        self.selected.as_ref().map_or("?", |s| s.name.as_str())
    }

    pub fn push_history(&mut self, samples: &Samples) {
        let w = self.history_window;
        push_bounded(&mut self.cpu_history, samples.cpu_usage as u64, w);
        push_bounded(
            &mut self.mem_history,
            pct(samples.mem_used, samples.mem_total) as u64,
            w,
        );
        push_bounded(&mut self.net_rx_history, samples.net_rx_rate, w);
        push_bounded(&mut self.net_tx_history, samples.net_tx_rate, w);
        push_bounded(&mut self.disk_read_history, samples.disk_read_rate, w);
        push_bounded(&mut self.disk_write_history, samples.disk_write_rate, w);
        push_bounded(
            &mut self.temp_history,
            (samples.avg_temperature() * 10.0) as u64,
            w,
        );
        push_bounded(
            &mut self.disk_usage_history,
            (samples.avg_disk_usage() * 10.0) as u64,
            w,
        );
        push_bounded(
            &mut self.swap_history,
            pct(samples.swap_used, samples.swap_total) as u64,
            w,
        );
    }

    pub const fn tab_state(&self) -> Option<&TabState> {
        match self.active_tab {
            Tab::Proc => Some(&self.proc_state),
            Tab::Net => Some(&self.net_state),
            Tab::Files => Some(&self.files_state),
            _ => None,
        }
    }

    pub const fn tab_state_mut(&mut self) -> Option<&mut TabState> {
        match self.active_tab {
            Tab::Proc => Some(&mut self.proc_state),
            Tab::Net => Some(&mut self.net_state),
            Tab::Files => Some(&mut self.files_state),
            _ => None,
        }
    }

    fn tab_from_horizontal_click(&self, col: u16) -> Option<usize> {
        let tab_width = (self.term_width.saturating_sub(2) / (Tab::ALL.len() as u16)).max(1);
        let idx = col.saturating_sub(1) / tab_width;
        if (idx as usize) < Tab::ALL.len() {
            Some(idx as usize)
        } else {
            None
        }
    }
}

impl Default for App {
    fn default() -> Self {
        fn history() -> VecDeque<u64> {
            VecDeque::with_capacity(WINDOW)
        }
        Self {
            active_tab: Tab::Dash,
            sidebar_visible: true,
            tab_orientation: TabOrientation::Sidebar,
            tab_bar_visible: true,
            cpu_history: history(),
            mem_history: history(),
            net_rx_history: history(),
            net_tx_history: history(),
            disk_read_history: history(),
            disk_write_history: history(),
            temp_history: history(),
            disk_usage_history: history(),
            swap_history: history(),
            proc_state: TabState::default(),
            net_state: TabState::default(),
            files_state: TabState::default(),
            selected: None,
            kill_state: None,
            scroll_step: 3,
            proc_sort_field: ProcSortField::Cpu,
            proc_sort_asc: false,
            should_quit: false,
            help_visible: false,
            kill_feedback: None,
            paused: false,
            history_window: WINDOW,
            term_width: 80,
            term_height: 24,
            error_msg: None,
        }
    }
}

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
            history_window: WINDOW,
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

pub fn parse_args(args: &[String]) -> CliAction {
    for arg in args {
        match arg.as_str() {
            "--help" | "-h" => return CliAction::Help,
            "--version" | "-V" => return CliAction::Version,
            _ => {}
        }
    }

    let config_path = (0..args.len())
        .position(|i| matches!(args[i].as_str(), "--config" | "-c"))
        .and_then(|i| {
            if i + 1 < args.len() {
                Some(args[i + 1].as_str())
            } else {
                None
            }
        });

    let mut cfg = match load_config(config_path) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-r" | "--refresh" => {
                cfg.refresh_ms = match parse_positive_int(args, "--refresh", &mut i) {
                    Ok(n) => n,
                    Err(e) => return e,
                };
            }
            "-t" | "--tab" => {
                let name = match parse_flag_value(args, "--tab", &mut i) {
                    Ok(n) => n,
                    Err(e) => return e,
                };
                cfg.default_tab = match parse_tab_name(name) {
                    Ok(t) => t,
                    Err(e) => return e,
                };
            }
            "-s" | "--no-sidebar" => cfg.hide_sidebar = true,
            "--tabs" => {
                let val = match parse_flag_value(args, "--tabs", &mut i) {
                    Ok(v) => v,
                    Err(e) => return e,
                };
                cfg.tab_orientation = match parse_tab_orientation(val) {
                    Ok(o) => o,
                    Err(e) => return e,
                };
            }
            "--scroll-step" => {
                cfg.scroll_step = match parse_positive_int(args, "--scroll-step", &mut i) {
                    Ok(n) => n as usize,
                    Err(e) => return e,
                };
            }
            "-c" | "--config" => {
                if i + 1 >= args.len() {
                    return CliAction::Error("--config requires a value".to_owned());
                }
                i += 1;
            }
            _ => return CliAction::Error(format!("unknown flag '{}'", args[i])),
        }
        i += 1;
    }

    if cfg.refresh_ms == 0 {
        cfg.refresh_ms = 1000;
    }
    CliAction::Config(cfg)
}

pub const fn pct(part: u64, total: u64) -> f64 {
    if total > 0 {
        part as f64 / total as f64 * 100.0
    } else {
        0.0
    }
}

fn push_bounded<T>(deque: &mut VecDeque<T>, value: T, max: usize) {
    if deque.len() >= max {
        deque.pop_front();
    }
    deque.push_back(value);
}

fn handle_search_input(query: &mut String, focused: &mut bool, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Char(c) if key.modifiers.is_empty() => {
            if query.len() < MAX_QUERY_LEN {
                query.push(c);
            }
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
        KeyCode::Up | KeyCode::Down | KeyCode::PageUp | KeyCode::PageDown => {
            *focused = false;
            false
        }
        KeyCode::F(_) | KeyCode::Insert | KeyCode::Home | KeyCode::End | KeyCode::Null => true,
        _ => {
            *focused = false;
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(app.proc_state.scroll, 0);
        assert_eq!(app.proc_state.selection, 0);
        assert!(app.selected.is_none());
        assert!(app.kill_state.is_none());
        assert!(app.proc_state.query.is_empty());
        assert!(!app.proc_state.focused);
        assert!(app.net_state.query.is_empty());
        assert!(!app.net_state.focused);
        assert!(app.files_state.query.is_empty());
        assert!(!app.files_state.focused);
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
        assert_eq!(app.term_width, 80);
        assert_eq!(app.term_height, 24);
        assert!(app.error_msg.is_none());
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
        for (i, tab) in Tab::ALL.iter().enumerate() {
            let key_char = (b'1' + i as u8) as char;
            let mut app = App::new();
            app.handle_key(KeyEvent::new(KeyCode::Char(key_char), KeyModifiers::NONE));
            assert_eq!(
                app.active_tab, *tab,
                "key '{key_char}' should jump to {tab:?}"
            );
        }
    }

    #[test]
    fn key_ctrl_1_does_not_jump_tab() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::CONTROL));
        assert_eq!(app.active_tab, Tab::Dash, "Ctrl+1 should not change tab");
    }

    #[test]
    fn key_alt_question_does_not_toggle_help() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::ALT));
        assert!(!app.help_visible, "Alt+? should not toggle help");
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
        assert_eq!(app.proc_state.selection, 0);
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.proc_state.selection, 1);
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.proc_state.selection, 2);
    }

    #[test]
    fn selected_pid_cleared_on_down() {
        let mut app = App::new();
        app.selected = Some(SelectionState {
            pid: 42,
            name: "bash".to_owned(),
        });
        app.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert!(app.selected.is_none());
    }

    #[test]
    fn selected_pid_cleared_on_up() {
        let mut app = App::new();
        app.selected = Some(SelectionState {
            pid: 42,
            name: "bash".to_owned(),
        });
        app.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert!(app.selected.is_none());
    }

    #[test]
    fn selection_moves_up() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.proc_state.selection, 2);
        app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(app.proc_state.selection, 1);
        app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(app.proc_state.selection, 0);
        app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(app.proc_state.selection, 0);
    }

    #[test]
    fn selection_page_down() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
        assert_eq!(app.proc_state.selection, 0);
        app.handle_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE));
        assert_eq!(app.proc_state.selection, PAGE_SIZE);
        app.handle_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE));
        assert_eq!(app.proc_state.selection, PAGE_SIZE * 2);
    }

    #[test]
    fn selected_pid_cleared_on_page_down() {
        let mut app = App::new();
        app.selected = Some(SelectionState {
            pid: 42,
            name: "bash".to_owned(),
        });
        app.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE));
        assert!(app.selected.is_none());
    }

    #[test]
    fn selection_page_up() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE));
        assert_eq!(app.proc_state.selection, PAGE_SIZE);
        app.handle_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE));
        assert_eq!(app.proc_state.selection, 0);
        app.handle_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE));
        assert_eq!(app.proc_state.selection, 0);
    }

    #[test]
    fn selected_pid_cleared_on_page_up() {
        let mut app = App::new();
        app.selected = Some(SelectionState {
            pid: 42,
            name: "bash".to_owned(),
        });
        app.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE));
        assert!(app.selected.is_none());
    }

    #[test]
    fn selection_noop_on_non_proc() {
        let mut app = App::new();
        assert_eq!(app.active_tab, Tab::Dash);
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.proc_state.selection, 0);
        app.handle_key(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE));
        assert!(app.kill_state.is_none());
    }

    // --- Net/Files keyboard navigation ---

    #[test]
    fn net_files_keyboard_navigation() {
        let mut app = App::new();

        app.active_tab = Tab::Net;
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.net_state.selection, 1);
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.net_state.selection, 2);
        app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(app.net_state.selection, 1);
        app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(app.net_state.selection, 0);
        app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(app.net_state.selection, 0);

        app.handle_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE));
        assert_eq!(app.net_state.selection, PAGE_SIZE);
        app.handle_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE));
        assert_eq!(app.net_state.selection, 0);

        app.active_tab = Tab::Files;
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.files_state.selection, 1);
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.files_state.selection, 2);
        app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(app.files_state.selection, 1);
        app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(app.files_state.selection, 0);
        app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(app.files_state.selection, 0);

        app.handle_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE));
        assert_eq!(app.files_state.selection, PAGE_SIZE);
        app.handle_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE));
        assert_eq!(app.files_state.selection, 0);
    }

    #[test]
    fn net_files_keyboard_noop_on_non_scrollable_tabs() {
        let mut app = App::new();
        app.net_state.selection = 3;
        app.files_state.selection = 5;
        app.active_tab = Tab::Dash;
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.net_state.selection, 3, "Net unchanged on Dash");
        assert_eq!(app.files_state.selection, 5, "Files unchanged on Dash");
        app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(app.net_state.selection, 3, "Net unchanged on Dash");
        assert_eq!(app.files_state.selection, 5, "Files unchanged on Dash");
    }

    #[test]
    fn delete_sets_kill_pending_on_proc() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
        app.selected = Some(SelectionState {
            pid: 42,
            name: String::new(),
        });
        assert!(app.kill_state.is_none());
        app.handle_key(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE));
        assert_eq!(app.kill_state, Some(KillState::Confirm));
    }

    #[test]
    fn ctrl_k_sets_immediate_kill() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
        app.selected = Some(SelectionState {
            pid: 42,
            name: String::new(),
        });
        app.handle_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL));
        assert_eq!(app.kill_state, Some(KillState::Dispatch(Signal::Kill)));
    }

    #[test]
    fn kill_pending_dismissed_by_any_key() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
        app.selected = Some(SelectionState {
            pid: 42,
            name: String::new(),
        });
        app.handle_key(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE));
        assert_eq!(app.kill_state, Some(KillState::Confirm));
        app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
        assert!(app.kill_state.is_none());
    }

    #[test]
    fn kill_pending_confirmed_by_one() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
        app.selected = Some(SelectionState {
            pid: 42,
            name: String::new(),
        });
        app.handle_key(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE));
        assert_eq!(app.kill_state, Some(KillState::Confirm));
        app.handle_key(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE));
        assert_eq!(app.kill_state, Some(KillState::Dispatch(Signal::Term)));
    }

    #[test]
    fn kill_pending_sends_all_signals() {
        for &(key, _, expected) in KILL_SIGNAL_MAP {
            let mut app = App::new();
            app.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
            app.selected = Some(SelectionState {
                pid: 42,
                name: String::new(),
            });
            app.handle_key(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE));
            app.handle_key(KeyEvent::new(KeyCode::Char(key), KeyModifiers::NONE));
            assert_eq!(
                app.kill_state,
                Some(KillState::Dispatch(expected)),
                "key '{key}' should dispatch {expected:?}"
            );
        }
    }

    // Restored in v0.4.2 — dropped during PR #132 merge to main (58c0702)
    #[test]
    fn ctrl_k_exits_search_and_kills_in_one_press() {
        let mut app = App::new();
        app.active_tab = Tab::Proc;
        app.proc_state.focused = true;
        app.selected = Some(SelectionState {
            pid: 42,
            name: String::new(),
        });
        app.handle_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL));
        assert!(!app.proc_state.focused, "Ctrl+K should exit search");
        assert_eq!(app.kill_state, Some(KillState::Dispatch(Signal::Kill)));
        assert_eq!(app.selected.as_ref().map(|s| s.pid), Some(42));
    }

    #[test]
    fn delete_exits_search_then_kill_on_second_press() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
        app.selected = Some(SelectionState {
            pid: 42,
            name: String::new(),
        });
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        assert!(app.proc_state.focused);
        // First Delete: exits search but does not trigger kill (key consumed)
        app.handle_key(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE));
        assert!(!app.proc_state.focused);
        assert!(app.kill_state.is_none(), "key consumed by search exit");
        // Second Delete: now triggers kill confirm
        app.handle_key(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE));
        assert_eq!(app.kill_state, Some(KillState::Confirm));
    }

    #[test]
    fn key_slash_enters_search_only_on_proc() {
        let mut app = App::new();
        assert_eq!(app.active_tab, Tab::Dash);
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        assert!(!app.proc_state.focused);

        app.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
        assert_eq!(app.active_tab, Tab::Proc);
        assert!(!app.proc_state.focused);

        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        assert!(app.proc_state.focused);
    }

    #[test]
    fn search_typing_appends_query() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        assert!(app.proc_state.focused);
        assert_eq!(app.active_tab, Tab::Proc);

        app.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));
        assert_eq!(app.proc_state.query, "fir");
        assert_eq!(app.active_tab, Tab::Proc);
    }

    #[test]
    fn search_backspace_pops() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
        assert_eq!(app.proc_state.query, "ab");

        app.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(app.proc_state.query, "a");
        assert!(app.proc_state.focused);
    }

    #[test]
    fn search_backspace_empty_exits() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert!(app.proc_state.query.is_empty());
        assert!(!app.proc_state.focused);
    }

    #[test]
    fn search_esc_clears_and_exits() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        assert_eq!(app.proc_state.query, "x");

        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.proc_state.query.is_empty());
        assert!(!app.proc_state.focused);
    }

    #[test]
    fn search_enter_exits_keeps_query() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE));

        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(!app.proc_state.focused);
        assert_eq!(app.proc_state.query, "f");
    }

    #[test]
    fn search_arrows_exit_and_move_selection() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        assert!(app.proc_state.focused);
        assert_eq!(app.proc_state.selection, 0);
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert!(!app.proc_state.focused, "Down exits search");
        assert_eq!(
            app.proc_state.selection, 1,
            "Down moves selection after exiting search"
        );
    }

    #[test]
    fn search_tab_exits_then_tab_cycles_on_second_press() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        assert!(app.proc_state.focused);
        // First Tab: exits search (key consumed), does not cycle tab
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert!(!app.proc_state.focused);
        assert_eq!(app.active_tab, Tab::Proc, "Tab key consumed by search exit");
        // Second Tab: now cycles to next tab
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.active_tab, Tab::Net);
    }

    #[test]
    fn search_f_key_does_not_exit() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        app.proc_state.query = "test".to_owned();
        assert!(app.proc_state.focused);
        app.handle_key(KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE));
        assert!(app.proc_state.focused, "F1 should not exit search");
        assert_eq!(app.proc_state.query, "test", "F1 should not clear query");
    }

    #[test]
    fn key_slash_enters_search_on_net_and_files() {
        for tab_key in ['3', '4'] {
            let mut app = App::new();
            app.handle_key(KeyEvent::new(KeyCode::Char(tab_key), KeyModifiers::NONE));
            assert!(!app.net_state.focused);
            assert!(!app.files_state.focused);
            app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
            if tab_key == '3' {
                assert!(app.net_state.focused, "/ on Net should focus net search");
                assert!(!app.files_state.focused, "files search should remain off");
            } else {
                assert!(
                    app.files_state.focused,
                    "/ on Files should focus files search"
                );
                assert!(!app.net_state.focused, "net search should remain off");
            }
        }
    }

    #[test]
    fn key_slash_noop_on_dash() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        assert!(!app.proc_state.focused);
        assert!(!app.net_state.focused);
        assert!(!app.files_state.focused);
    }

    #[test]
    fn net_and_files_search_esc_clears() {
        for (tab_key, query_val) in [('3', 'e'), ('4', 'm')] {
            let mut app = App::new();
            app.handle_key(KeyEvent::new(KeyCode::Char(tab_key), KeyModifiers::NONE));
            app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
            app.handle_key(KeyEvent::new(KeyCode::Char(query_val), KeyModifiers::NONE));
            if tab_key == '3' {
                assert_eq!(app.net_state.query, "e", "net query should be set");
            } else {
                assert_eq!(app.files_state.query, "m", "files query should be set");
            }
            app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
            assert!(app.net_state.query.is_empty(), "net query cleared");
            assert!(app.files_state.query.is_empty(), "files query cleared");
            assert!(!app.net_state.focused, "net search unfocused");
            assert!(!app.files_state.focused, "files search unfocused");
        }
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
            tab_orientation: TabOrientation::Sidebar,
            ..Config::default()
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
            tab_orientation: TabOrientation::Sidebar,
            ..Config::default()
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
    fn tab_index_matches_all() {
        for (i, tab) in Tab::ALL.iter().enumerate() {
            assert_eq!(tab.index(), i, "{tab:?} index should be {i}");
        }
    }

    fn parse_config(args: &[&str]) -> Config {
        let input: Vec<String> = args.iter().map(ToString::to_string).collect();
        match parse_args(&input) {
            CliAction::Config(cfg) => cfg,
            _ => panic!("expected Config, got {input:?}"),
        }
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
                "key '{key:?}' should set sort field to {expected_field:?}",
            );
            assert_eq!(
                app.proc_sort_asc, *expected_asc,
                "key '{key:?}' should set sort_asc to {expected_asc}",
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

    #[test]
    fn push_history_populates_all_histories() {
        let mut app = App::new();
        app.history_window = 10;
        let samples = Samples {
            cpu_usage: 42.0,
            mem_used: 8_000_000_000,
            mem_total: 16_000_000_000,
            mem_available: 8_000_000_000,
            mem_free: 4_000_000_000,
            swap_used: 1_000_000_000,
            swap_total: 4_000_000_000,
            net_rx_rate: 1_000_000,
            net_tx_rate: 500_000,
            load_one: 0.5,
            load_five: 0.3,
            load_fifteen: 0.2,
            processes: vec![],
            interfaces: vec![],
            disks: vec![],
            disk_io: vec![],
            disk_read_rate: 2_000_000,
            disk_write_rate: 1_000_000,
            temperatures: vec![],
            cpus: vec![],
            sys_info: crate::samplers::SysInfo::default(),
        };
        app.push_history(&samples);
        assert_eq!(app.cpu_history.len(), 1);
        assert_eq!(*app.cpu_history.front().unwrap(), 42);
        assert_eq!(*app.mem_history.front().unwrap(), 50);
        assert_eq!(*app.net_rx_history.front().unwrap(), 1_000_000);
        assert_eq!(*app.net_tx_history.front().unwrap(), 500_000);
        assert_eq!(*app.disk_read_history.front().unwrap(), 2_000_000);
        assert_eq!(*app.disk_write_history.front().unwrap(), 1_000_000);
        assert_eq!(*app.temp_history.front().unwrap(), 0);
        assert_eq!(*app.disk_usage_history.front().unwrap(), 0);
        assert_eq!(*app.swap_history.front().unwrap(), 25);
    }

    #[test]
    fn push_history_respects_window() {
        let mut app = App::new();
        app.history_window = 2;
        let samples = Samples {
            cpu_usage: 42.0,
            mem_used: 0,
            mem_total: 0,
            mem_available: 0,
            mem_free: 0,
            swap_used: 0,
            swap_total: 0,
            net_rx_rate: 0,
            net_tx_rate: 0,
            load_one: 0.0,
            load_five: 0.0,
            load_fifteen: 0.0,
            processes: vec![],
            interfaces: vec![],
            disks: vec![],
            disk_io: vec![],
            disk_read_rate: 0,
            disk_write_rate: 0,
            temperatures: vec![],
            cpus: vec![],
            sys_info: crate::samplers::SysInfo::default(),
        };
        app.push_history(&samples);
        app.push_history(&samples);
        app.push_history(&samples);
        assert_eq!(app.cpu_history.len(), 2);
    }

    #[test]
    fn refresh_term_size_returns_bool() {
        let mut app = App::new();
        let result = app.refresh_term_size();
        assert!(result, "terminal size should be readable");
        assert!(app.term_width > 0);
        assert!(app.term_height > 0);
    }

    #[test]
    fn tab_from_horizontal_click_returns_correct_index() {
        let mut app = App::new();
        app.term_width = 80;
        // tab_width = (80-2)/9 = 8
        // cols 1-8 = tab 0, cols 9-16 = tab 1, etc.
        assert_eq!(app.tab_from_horizontal_click(5), Some(0));
        assert_eq!(app.tab_from_horizontal_click(10), Some(1));
        assert_eq!(app.tab_from_horizontal_click(17), Some(2));
        assert_eq!(app.tab_from_horizontal_click(73), None);
    }

    #[test]
    fn tab_from_horizontal_click_out_of_bounds() {
        let mut app = App::new();
        app.term_width = 10;
        // tab_width = (10-2)/9 = 0, max(1) = 1
        assert_eq!(app.tab_from_horizontal_click(100), None);
    }

    #[test]
    fn tab_orientation_default_sidebar() {
        assert_eq!(TabOrientation::default(), TabOrientation::Sidebar);
        let app = App::new();
        assert_eq!(app.tab_orientation, TabOrientation::Sidebar);
    }

    #[test]
    fn ctrl_t_toggles_tab_orientation() {
        let mut app = App::new();
        assert_eq!(app.tab_orientation, TabOrientation::Sidebar);
        app.handle_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));
        assert_eq!(app.tab_orientation, TabOrientation::Horizontal);
        app.handle_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));
        assert_eq!(app.tab_orientation, TabOrientation::HorizontalFooter);
        app.handle_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));
        assert_eq!(app.tab_orientation, TabOrientation::Sidebar);
    }

    #[test]
    fn ctrl_s_in_horizontal_footer_toggles_tab_bar() {
        let mut app = App::new();
        app.tab_orientation = TabOrientation::HorizontalFooter;
        assert!(app.tab_bar_visible);
        app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
        assert!(!app.tab_bar_visible);
        app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
        assert!(app.tab_bar_visible);
    }

    #[test]
    fn ctrl_s_in_horizontal_toggles_tab_bar() {
        let mut app = App::new();
        assert!(app.tab_bar_visible);
        app.handle_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));
        assert_eq!(app.tab_orientation, TabOrientation::Horizontal);
        assert!(app.sidebar_visible);
        app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
        assert!(!app.tab_bar_visible);
        app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
        assert!(app.tab_bar_visible);
    }

    #[test]
    fn ctrl_s_in_sidebar_toggles_sidebar() {
        let mut app = App::new();
        assert_eq!(app.tab_orientation, TabOrientation::Sidebar);
        assert!(app.sidebar_visible);
        app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
        assert!(!app.sidebar_visible);
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
    fn left_right_arrows_navigate_tabs_in_horizontal() {
        let mut app = App::new();
        app.tab_orientation = TabOrientation::Horizontal;
        assert_eq!(app.active_tab, Tab::Dash);
        app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        assert_eq!(app.active_tab, Tab::Proc);
        app.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        assert_eq!(app.active_tab, Tab::Dash);
    }

    #[test]
    fn left_right_arrows_navigate_tabs_in_footer() {
        let mut app = App::new();
        app.tab_orientation = TabOrientation::HorizontalFooter;
        assert_eq!(app.active_tab, Tab::Dash);
        app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        assert_eq!(app.active_tab, Tab::Proc);
        app.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        assert_eq!(app.active_tab, Tab::Dash);
    }

    #[test]
    fn left_right_arrows_noop_in_sidebar() {
        let mut app = App::new();
        assert_eq!(app.tab_orientation, TabOrientation::Sidebar);
        app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        assert_eq!(app.active_tab, Tab::Dash);
        app.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        assert_eq!(app.active_tab, Tab::Dash);
    }

    // --- Pause/Freeze ---

    #[test]
    fn space_toggles_paused() {
        let mut app = App::new();
        assert!(!app.paused);
        app.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        assert!(app.paused);
        app.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        assert!(!app.paused);
    }

    #[test]
    fn space_works_on_any_tab() {
        let mut app = App::new();
        app.active_tab = Tab::Proc;
        app.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        assert!(app.paused);
        app.active_tab = Tab::Net;
        app.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        assert!(!app.paused);
    }

    // --- Mouse sidebar click ---

    #[test]
    fn mouse_click_sidebar_switches_tab() {
        let mut app = App::new();
        assert_eq!(app.active_tab, Tab::Dash);
        // Click on tab index 1 (Proc) at col=5, row=2
        app.handle_mouse(5, 2, MouseEventKind::Down(MouseButton::Left));
        assert_eq!(app.active_tab, Tab::Proc);
        // Click on tab index 8 (Mem) at col=5, row=9
        app.handle_mouse(5, 9, MouseEventKind::Down(MouseButton::Left));
        assert_eq!(app.active_tab, Tab::Mem);
        // Click on tab index 0 (Dash) at col=5, row=1
        app.handle_mouse(5, 1, MouseEventKind::Down(MouseButton::Left));
        assert_eq!(app.active_tab, Tab::Dash);
    }

    #[test]
    fn mouse_click_outside_sidebar_ignored() {
        let mut app = App::new();
        app.handle_mouse(10, 2, MouseEventKind::Down(MouseButton::Left));
        assert_eq!(app.active_tab, Tab::Dash);
        app.handle_mouse(5, 0, MouseEventKind::Down(MouseButton::Left));
        assert_eq!(app.active_tab, Tab::Dash);
        app.handle_mouse(5, 10, MouseEventKind::Down(MouseButton::Left));
        assert_eq!(app.active_tab, Tab::Dash);
    }

    #[test]
    fn mouse_click_sidebar_hidden_ignored() {
        let mut app = App::new();
        app.sidebar_visible = false;
        app.handle_mouse(5, 2, MouseEventKind::Down(MouseButton::Left));
        assert_eq!(app.active_tab, Tab::Dash);
    }

    #[test]
    fn kill_state_dismissed_on_proc_click() {
        let mut app = App::new();
        app.active_tab = Tab::Proc;
        app.kill_state = Some(KillState::Confirm);
        app.handle_mouse(20, 5, MouseEventKind::Down(MouseButton::Left));
        assert!(app.kill_state.is_none(), "Proc click clears kill_state");
    }

    #[test]
    fn kill_state_dismissed_on_tab_switch_click() {
        let mut app = App::new();
        app.kill_state = Some(KillState::Confirm);
        app.handle_mouse(1, 3, MouseEventKind::Down(MouseButton::Left));
        assert_eq!(app.active_tab, Tab::Net, "click switches to Net tab");
        assert!(
            app.kill_state.is_none(),
            "tab switch click clears kill_state"
        );
    }

    // --- Mouse scroll ---

    #[test]
    fn selected_pid_cleared_on_scroll() {
        let mut app = App::new();
        app.active_tab = Tab::Proc;
        app.selected = Some(SelectionState {
            pid: 42,
            name: "bash".to_owned(),
        });
        app.kill_state = Some(KillState::Confirm);
        app.handle_mouse(0, 0, MouseEventKind::ScrollDown);
        assert!(app.selected.is_none(), "scroll clears selected");
        assert!(app.selected.is_none(), "scroll clears selected");
        assert!(app.kill_state.is_none(), "scroll clears kill_state");
    }

    #[test]
    fn mouse_scroll_up_on_proc_moves_selection_up() {
        let mut app = App::new();
        app.active_tab = Tab::Proc;
        app.proc_state.selection = 5;
        app.handle_mouse(0, 0, MouseEventKind::ScrollUp);
        assert_eq!(app.proc_state.selection, 2);
    }

    #[test]
    fn mouse_scroll_down_on_proc_moves_selection_down() {
        let mut app = App::new();
        app.active_tab = Tab::Proc;
        app.proc_state.selection = 5;
        app.handle_mouse(0, 0, MouseEventKind::ScrollDown);
        assert_eq!(app.proc_state.selection, 8);
    }

    #[test]
    fn mouse_scroll_wraps_at_zero() {
        let mut app = App::new();
        app.active_tab = Tab::Proc;
        app.proc_state.selection = 1;
        app.handle_mouse(0, 0, MouseEventKind::ScrollUp);
        assert_eq!(app.proc_state.selection, 0);
        app.handle_mouse(0, 0, MouseEventKind::ScrollUp);
        assert_eq!(app.proc_state.selection, 0);
    }

    #[test]
    fn mouse_scroll_noop_on_non_proc() {
        let mut app = App::new();
        app.active_tab = Tab::Dash;
        app.proc_state.selection = 5;
        app.handle_mouse(0, 0, MouseEventKind::ScrollDown);
        assert_eq!(app.proc_state.selection, 5);
        app.handle_mouse(0, 0, MouseEventKind::ScrollUp);
        assert_eq!(app.proc_state.selection, 5);
    }

    #[test]
    fn kill_state_dismissed_on_scroll_up() {
        let mut app = App::new();
        app.active_tab = Tab::Proc;
        app.kill_state = Some(KillState::Confirm);
        app.handle_mouse(0, 0, MouseEventKind::ScrollUp);
        assert!(app.kill_state.is_none(), "ScrollUp clears kill_state");
    }

    #[test]
    fn kill_state_dismissed_on_scroll_down() {
        let mut app = App::new();
        app.active_tab = Tab::Proc;
        app.kill_state = Some(KillState::Confirm);
        app.handle_mouse(0, 0, MouseEventKind::ScrollDown);
        assert!(app.kill_state.is_none(), "ScrollDown clears kill_state");
    }

    // --- Net/Files mouse scroll ---

    #[test]
    fn net_files_mouse_scroll() {
        let mut app = App::new();

        app.active_tab = Tab::Net;
        app.net_state.selection = 5;
        app.handle_mouse(0, 0, MouseEventKind::ScrollUp);
        assert_eq!(
            app.net_state.selection, 2,
            "Net scroll up by default step 3"
        );
        app.handle_mouse(0, 0, MouseEventKind::ScrollDown);
        assert_eq!(
            app.net_state.selection, 5,
            "Net scroll down by default step 3"
        );

        app.active_tab = Tab::Files;
        app.files_state.selection = 5;
        app.handle_mouse(0, 0, MouseEventKind::ScrollUp);
        assert_eq!(
            app.files_state.selection, 2,
            "Files scroll up by default step 3"
        );
        app.handle_mouse(0, 0, MouseEventKind::ScrollDown);
        assert_eq!(
            app.files_state.selection, 5,
            "Files scroll down by default step 3"
        );
    }

    #[test]
    fn net_files_mouse_scroll_wraps_at_zero() {
        let mut app = App::new();

        app.active_tab = Tab::Net;
        app.net_state.selection = 1;
        app.handle_mouse(0, 0, MouseEventKind::ScrollUp);
        assert_eq!(app.net_state.selection, 0);
        app.handle_mouse(0, 0, MouseEventKind::ScrollUp);
        assert_eq!(app.net_state.selection, 0);

        app.active_tab = Tab::Files;
        app.files_state.selection = 1;
        app.handle_mouse(0, 0, MouseEventKind::ScrollUp);
        assert_eq!(app.files_state.selection, 0);
        app.handle_mouse(0, 0, MouseEventKind::ScrollUp);
        assert_eq!(app.files_state.selection, 0);
    }

    #[test]
    fn net_files_mouse_scroll_noop_on_non_scrollable_tabs() {
        let mut app = App::new();
        app.net_state.selection = 3;
        app.files_state.selection = 7;
        app.active_tab = Tab::Dash;
        app.handle_mouse(0, 0, MouseEventKind::ScrollDown);
        assert_eq!(app.net_state.selection, 3, "Net unchanged on Dash");
        assert_eq!(app.files_state.selection, 7, "Files unchanged on Dash");
        app.handle_mouse(0, 0, MouseEventKind::ScrollUp);
        assert_eq!(app.net_state.selection, 3, "Net unchanged on Dash");
        assert_eq!(app.files_state.selection, 7, "Files unchanged on Dash");
    }

    // --- Mouse horizontal tab bar click ---

    #[test]
    fn mouse_click_horizontal_switches_tab() {
        let mut app = App::new();
        app.tab_orientation = TabOrientation::Horizontal;
        app.tab_bar_visible = true;
        app.term_width = 80;
        // term_width=80, area_width=78, tab_width=78/9=8
        // Tab 0 (Dash): cols 1-8
        // Tab 1 (Proc): cols 9-16
        // Tab 2 (Net): cols 17-24
        // Click on tab 1 (Proc) at col=10, row=1
        app.handle_mouse(10, 1, MouseEventKind::Down(MouseButton::Left));
        assert_eq!(app.active_tab, Tab::Proc);
        // Click on tab 0 (Dash) at col=5, row=1
        app.handle_mouse(5, 1, MouseEventKind::Down(MouseButton::Left));
        assert_eq!(app.active_tab, Tab::Dash);
    }

    #[test]
    fn mouse_click_horizontal_tab_bar_hidden_ignored() {
        let mut app = App::new();
        app.tab_orientation = TabOrientation::Horizontal;
        app.tab_bar_visible = false;
        app.term_width = 80;
        app.handle_mouse(10, 1, MouseEventKind::Down(MouseButton::Left));
        assert_eq!(app.active_tab, Tab::Dash);
    }

    #[test]
    fn mouse_click_horizontal_wrong_row_ignored() {
        let mut app = App::new();
        app.tab_orientation = TabOrientation::Horizontal;
        app.tab_bar_visible = true;
        app.term_width = 80;
        app.handle_mouse(10, 2, MouseEventKind::Down(MouseButton::Left));
        assert_eq!(app.active_tab, Tab::Dash);
    }

    #[test]
    fn mouse_click_footer_switches_tab() {
        let mut app = App::new();
        app.tab_orientation = TabOrientation::HorizontalFooter;
        app.tab_bar_visible = true;
        app.term_width = 80;
        app.term_height = 24;
        // footer tab bar at row = term_height - 3 = 21
        // tab_width = 78/9 = 8
        // Click on tab 8 (Mem) at col=70, row=21
        app.handle_mouse(70, 21, MouseEventKind::Down(MouseButton::Left));
        assert_eq!(app.active_tab, Tab::Mem);
    }

    // --- Tab-bar mouse wheel ---

    #[test]
    fn tab_bar_mouse_wheel_switches_tabs() {
        // Sidebar: scroll up on col 5 cycles backward
        let mut app = App::new();
        assert_eq!(app.active_tab, Tab::Dash);
        app.handle_mouse(5, 1, MouseEventKind::ScrollUp);
        assert_eq!(app.active_tab, Tab::Mem, "sidebar scroll up wraps to last");
        app.handle_mouse(5, 1, MouseEventKind::ScrollDown);
        assert_eq!(app.active_tab, Tab::Dash, "sidebar scroll down to first");

        // Horizontal: scroll on row 1
        app.tab_orientation = TabOrientation::Horizontal;
        app.tab_bar_visible = true;
        app.term_width = 80;
        app.active_tab = Tab::Dash;
        app.handle_mouse(10, 1, MouseEventKind::ScrollDown);
        assert_eq!(app.active_tab, Tab::Proc, "horizontal scroll down");
        app.handle_mouse(10, 1, MouseEventKind::ScrollUp);
        assert_eq!(app.active_tab, Tab::Dash, "horizontal scroll up");

        // HorizontalFooter: scroll on bottom row
        app.tab_orientation = TabOrientation::HorizontalFooter;
        app.term_height = 24;
        app.active_tab = Tab::Dash;
        app.handle_mouse(10, 21, MouseEventKind::ScrollDown);
        assert_eq!(app.active_tab, Tab::Proc, "footer scroll down");
        app.handle_mouse(10, 21, MouseEventKind::ScrollUp);
        assert_eq!(app.active_tab, Tab::Dash, "footer scroll up");
    }

    #[test]
    fn tab_bar_mouse_wheel_outside_tab_bar_ignored() {
        let mut app = App::new();
        app.active_tab = Tab::Dash;
        // Scroll at col=0 (outside sidebar) should not switch tabs
        app.handle_mouse(0, 2, MouseEventKind::ScrollDown);
        assert_eq!(app.active_tab, Tab::Dash, "col 0 not on sidebar");

        // Horizontal: scroll at row 2 (not tab bar) should not switch tabs
        app.tab_orientation = TabOrientation::Horizontal;
        app.tab_bar_visible = true;
        app.term_width = 80;
        app.handle_mouse(10, 2, MouseEventKind::ScrollDown);
        assert_eq!(app.active_tab, Tab::Dash, "not on horizontal tab bar row");
    }

    // --- Config expansion ---

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
    fn apply_config_sets_proc_sort() {
        let mut app = App::new();
        assert_eq!(app.proc_sort_field, ProcSortField::Cpu);
        let cfg = Config {
            proc_sort_default: ProcSortField::Memory,
            proc_sort_asc_default: true,
            ..Config::default()
        };
        app.apply_config(&cfg);
        assert_eq!(app.proc_sort_field, ProcSortField::Memory);
        assert!(app.proc_sort_asc);
    }

    #[test]
    fn apply_config_sets_history_window() {
        let mut app = App::new();
        assert_eq!(app.history_window, 60);
        let cfg = Config {
            history_window: 120,
            ..Config::default()
        };
        app.apply_config(&cfg);
        assert_eq!(app.history_window, 120);
        let cfg = Config {
            history_window: 9999,
            ..Config::default()
        };
        app.apply_config(&cfg);
        assert_eq!(app.history_window, 3600, "history_window capped at 3600");
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

    // --- Scroll step configuration ---

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

    #[test]
    fn apply_config_sets_scroll_step() {
        let mut app = App::new();
        assert_eq!(app.scroll_step, 3, "default in App");

        let cfg = Config {
            scroll_step: 10,
            ..Config::default()
        };
        app.apply_config(&cfg);
        assert_eq!(app.scroll_step, 10);

        // Cap at 100
        let cfg = Config {
            scroll_step: 999,
            ..Config::default()
        };
        app.apply_config(&cfg);
        assert_eq!(app.scroll_step, 100, "scroll_step capped at 100");

        // Floor at 1
        let cfg = Config {
            scroll_step: 0,
            ..Config::default()
        };
        app.apply_config(&cfg);
        assert_eq!(app.scroll_step, 1, "scroll_step floored at 1");

        // Verify configured step affects mouse scroll
        let cfg = Config {
            scroll_step: 5,
            ..Config::default()
        };
        app.apply_config(&cfg);
        app.active_tab = Tab::Net;
        app.net_state.selection = 20;
        app.handle_mouse(0, 0, MouseEventKind::ScrollUp);
        assert_eq!(
            app.net_state.selection, 15,
            "scroll uses configured step of 5"
        );
        app.handle_mouse(0, 0, MouseEventKind::ScrollDown);
        assert_eq!(
            app.net_state.selection, 20,
            "scroll down uses configured step"
        );
    }

    #[test]
    fn pct_values() {
        for (part, total, expected) in [
            (50, 100, 50.0),
            (0, 100, 0.0),
            (100, 100, 100.0),
            (0, 0, 0.0),
            (1, 3, 100.0 / 3.0),
        ] {
            let result = pct(part, total);
            assert!(
                (result - expected).abs() < 1e-12,
                "pct({part}, {total}) = {result}, expected {expected}"
            );
        }
    }
}
