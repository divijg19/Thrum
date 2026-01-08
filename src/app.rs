use std::collections::VecDeque;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Clone, Copy, PartialEq)]
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
