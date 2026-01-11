use std::collections::VecDeque;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tab {
    Dash,
    Proc,
    Net,
    Files,
    Time,
}

impl Tab {
    pub const ALL: [Tab; 5] = [Tab::Dash, Tab::Proc, Tab::Net, Tab::Files, Tab::Time];

    pub fn label(&self) -> &str {
        match self {
            Tab::Dash => "Dash",
            Tab::Proc => "Proc",
            Tab::Net => "Net",
            Tab::Files => "Files",
            Tab::Time => "Time",
        }
    }
}

pub struct App {
    pub active_tab: Tab,
    pub sidebar_visible: bool,
    pub cpu_history: VecDeque<u64>,
    pub proc_scroll: usize,
    pub should_quit: bool,
}

impl App {
    pub fn new() -> Self {
        Self {
            active_tab: Tab::Dash,
            sidebar_visible: true,
            cpu_history: VecDeque::with_capacity(60),
            proc_scroll: 0,
            should_quit: false,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
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
            KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.sidebar_visible = !self.sidebar_visible;
            }
            KeyCode::Up => self.proc_scroll = self.proc_scroll.saturating_sub(1),
            KeyCode::Down => self.proc_scroll = self.proc_scroll.saturating_add(1),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    #[test]
    fn tab_has_five_variants() {
        assert_eq!(Tab::ALL.len(), 5);
    }

    #[test]
    fn tab_label_matches() {
        assert_eq!(Tab::Dash.label(), "Dash");
        assert_eq!(Tab::Proc.label(), "Proc");
        assert_eq!(Tab::Net.label(), "Net");
        assert_eq!(Tab::Files.label(), "Files");
        assert_eq!(Tab::Time.label(), "Time");
    }

    #[test]
    fn app_new_defaults() {
        let app = App::new();
        assert_eq!(app.active_tab, Tab::Dash);
        assert!(app.sidebar_visible);
        assert!(!app.should_quit);
        assert_eq!(app.proc_scroll, 0);
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
        assert_eq!(app.active_tab, Tab::Dash);
    }

    #[test]
    fn key_backtab_cycles_backward() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('5'), KeyModifiers::NONE));
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
        assert_eq!(app.active_tab, Tab::Time);
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
    fn key_unknown_does_nothing() {
        let mut app = App::new();
        app.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        assert!(!app.should_quit);
        assert_eq!(app.active_tab, Tab::Dash);
        assert!(app.sidebar_visible);
    }
}
