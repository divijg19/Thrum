use std::collections::VecDeque;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ProcSortField {
    Name,
    Pid,
    Cpu,
    Memory,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tab {
    Dash,
    Proc,
    Net,
    Files,
    Time,
    Temp,
}

impl Tab {
    pub const ALL: [Tab; 6] = [
        Tab::Dash,
        Tab::Proc,
        Tab::Net,
        Tab::Files,
        Tab::Time,
        Tab::Temp,
    ];

    pub fn label(&self) -> &str {
        match self {
            Tab::Dash => "Dash",
            Tab::Proc => "Proc",
            Tab::Net => "Net",
            Tab::Files => "Files",
            Tab::Time => "Time",
            Tab::Temp => "Temp",
        }
    }
}

#[allow(clippy::struct_excessive_bools)]
pub struct App {
    pub active_tab: Tab,
    pub sidebar_visible: bool,
    pub cpu_history: VecDeque<u64>,
    pub mem_history: VecDeque<u64>,
    pub net_rx_history: VecDeque<u64>,
    pub net_tx_history: VecDeque<u64>,
    pub proc_scroll: usize,
    pub proc_query: String,
    pub proc_search_focused: bool,
    pub proc_sort_field: ProcSortField,
    pub proc_sort_asc: bool,
    pub should_quit: bool,
}

impl App {
    pub fn new() -> Self {
        Self {
            active_tab: Tab::Dash,
            sidebar_visible: true,
            cpu_history: VecDeque::with_capacity(60),
            mem_history: VecDeque::with_capacity(60),
            net_rx_history: VecDeque::with_capacity(60),
            net_tx_history: VecDeque::with_capacity(60),
            proc_scroll: 0,
            proc_query: String::new(),
            proc_search_focused: false,
            proc_sort_field: ProcSortField::Cpu,
            proc_sort_asc: false,
            should_quit: false,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if self.proc_search_focused && self.active_tab == Tab::Proc {
            match key.code {
                KeyCode::Char(c) if key.modifiers.is_empty() => {
                    self.proc_query.push(c);
                    return;
                }
                KeyCode::Backspace => {
                    self.proc_query.pop();
                    if self.proc_query.is_empty() {
                        self.proc_search_focused = false;
                    }
                    return;
                }
                KeyCode::Esc => {
                    self.proc_query.clear();
                    self.proc_search_focused = false;
                    return;
                }
                KeyCode::Enter => {
                    self.proc_search_focused = false;
                    return;
                }
                _ => {
                    self.proc_search_focused = false;
                }
            }
        }

        match key.code {
            KeyCode::Char('q') if key.modifiers.is_empty() => self.should_quit = true,
            KeyCode::Tab => {
                let idx = Tab::ALL.iter().position(|t| *t == self.active_tab).unwrap();
                self.active_tab = Tab::ALL[(idx + 1) % Tab::ALL.len()];
            }
            KeyCode::BackTab => {
                let idx = Tab::ALL.iter().position(|t| *t == self.active_tab).unwrap();
                self.active_tab = Tab::ALL[(idx + Tab::ALL.len() - 1) % Tab::ALL.len()];
            }
            KeyCode::Char('1') => self.active_tab = Tab::Dash,
            KeyCode::Char('2') => self.active_tab = Tab::Proc,
            KeyCode::Char('3') => self.active_tab = Tab::Net,
            KeyCode::Char('4') => self.active_tab = Tab::Files,
            KeyCode::Char('5') => self.active_tab = Tab::Time,
            KeyCode::Char('6') => self.active_tab = Tab::Temp,
            KeyCode::Char('/') if key.modifiers.is_empty() && self.active_tab == Tab::Proc => {
                self.proc_search_focused = true;
            }
            KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.sidebar_visible = !self.sidebar_visible;
            }
            KeyCode::Char('n') if key.modifiers.is_empty() && self.active_tab == Tab::Proc => {
                self.proc_sort_field = ProcSortField::Name;
                self.proc_sort_asc = true;
            }
            KeyCode::Char('p') if key.modifiers.is_empty() && self.active_tab == Tab::Proc => {
                self.proc_sort_field = ProcSortField::Pid;
                self.proc_sort_asc = true;
            }
            KeyCode::Char('c') if key.modifiers.is_empty() && self.active_tab == Tab::Proc => {
                self.proc_sort_field = ProcSortField::Cpu;
                self.proc_sort_asc = false;
            }
            KeyCode::Char('m') if key.modifiers.is_empty() && self.active_tab == Tab::Proc => {
                self.proc_sort_field = ProcSortField::Memory;
                self.proc_sort_asc = false;
            }
            KeyCode::Char('r') if key.modifiers.is_empty() && self.active_tab == Tab::Proc => {
                self.proc_sort_asc = !self.proc_sort_asc;
            }
            KeyCode::Up if self.active_tab == Tab::Proc => {
                self.proc_scroll = self.proc_scroll.saturating_sub(1);
            }
            KeyCode::Down if self.active_tab == Tab::Proc => {
                self.proc_scroll = self.proc_scroll.saturating_add(1);
            }
            _ => {}
        }
    }
}

pub fn push_bounded<T>(deque: &mut VecDeque<T>, value: T, max: usize) {
    deque.push_back(value);
    if deque.len() > max {
        deque.pop_front();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    #[test]
    fn tab_has_six_variants() {
        assert_eq!(Tab::ALL.len(), 6);
    }

    #[test]
    fn tab_label_matches() {
        assert_eq!(Tab::Dash.label(), "Dash");
        assert_eq!(Tab::Proc.label(), "Proc");
        assert_eq!(Tab::Net.label(), "Net");
        assert_eq!(Tab::Files.label(), "Files");
        assert_eq!(Tab::Time.label(), "Time");
        assert_eq!(Tab::Temp.label(), "Temp");
    }

    #[test]
    fn app_new_defaults() {
        let app = App::new();
        assert_eq!(app.active_tab, Tab::Dash);
        assert!(app.sidebar_visible);
        assert!(!app.should_quit);
        assert_eq!(app.proc_scroll, 0);
        assert!(app.proc_query.is_empty());
        assert!(!app.proc_search_focused);
        assert_eq!(app.mem_history.len(), 0);
        assert_eq!(app.net_rx_history.len(), 0);
        assert_eq!(app.net_tx_history.len(), 0);
        assert_eq!(app.proc_sort_field, ProcSortField::Cpu);
        assert!(!app.proc_sort_asc);
    }

    #[test]
    fn key_q_quits() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
        assert!(app.should_quit);
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
        assert_eq!(app.active_tab, Tab::Dash);
    }

    #[test]
    fn key_backtab_cycles_backward() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('6'), KeyModifiers::NONE));
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
        assert_eq!(app.active_tab, Tab::Temp);
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
    fn key_up_down_scrolls_proc() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
        assert_eq!(app.proc_scroll, 0);
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.proc_scroll, 1);
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.proc_scroll, 2);
        app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(app.proc_scroll, 1);
        app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(app.proc_scroll, 0);
        app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(app.proc_scroll, 0);
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
}
