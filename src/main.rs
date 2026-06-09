use std::collections::VecDeque;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode};
use ratatui::layout::{Alignment, Constraint, Layout};
use ratatui::style::{Color, Style, Stylize};
use ratatui::widgets::{Block, Gauge, Paragraph, Sparkline};

fn main() -> std::io::Result<()> {
    let mut terminal = ratatui::init();
    let mut sys = sysinfo::System::new();
    let mut history: VecDeque<u64> = VecDeque::with_capacity(60);

    loop {
        sys.refresh_cpu_all();
        let cpu = sys.global_cpu_usage();

        history.push_back(cpu as u64);
        if history.len() > 60 {
            history.pop_front();
        }

        terminal.draw(|f| {
            let block = Block::bordered().title(" Thrum ");
            f.render_widget(&block, f.area());

            let inner = block.inner(f.area());
            let [_, gauge, spark, help, _] = Layout::vertical([
                Constraint::Fill(1),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(1),
                Constraint::Fill(1),
            ])
            .areas(inner);

            let g = Gauge::default()
                .gauge_style(Style::new().fg(Color::Green))
                .percent(cpu as u16)
                .label(format!("CPU: {:.1}%", cpu));
            f.render_widget(&g, gauge);

            let s = Sparkline::default()
                .block(Block::bordered().title(" History "))
                .data(history.iter())
                .style(Style::new().fg(Color::Green));
            f.render_widget(&s, spark);

            let h = Paragraph::new("press q to quit")
                .alignment(Alignment::Center)
                .gray();
            f.render_widget(&h, help);
        })?;

        if event::poll(Duration::from_secs(1))? {
            if let Event::Key(key) = event::read()? {
                if key.code == KeyCode::Char('q') {
                    break;
                }
            }
        }
    }

    ratatui::restore();
    Ok(())
}
