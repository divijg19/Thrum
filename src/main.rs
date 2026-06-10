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
        sys.refresh_memory();
        let cpu = sys.global_cpu_usage();
        let mem_used = sys.used_memory();
        let mem_total = sys.total_memory();

        history.push_back(cpu as u64);
        if history.len() > 60 {
            history.pop_front();
        }

        terminal.draw(|f| {
            let block = Block::bordered().title(" Thrum ");
            f.render_widget(&block, f.area());

            let inner = block.inner(f.area());
            let [_, gauges, spark, help, _] = Layout::vertical([
                Constraint::Fill(1),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(1),
                Constraint::Fill(1),
            ])
            .areas(inner);

            let [cpu_area, mem_area] = Layout::horizontal([
                Constraint::Percentage(50),
                Constraint::Percentage(50),
            ])
            .areas(gauges);

            let g = Gauge::default()
                .gauge_style(Style::new().fg(Color::Green))
                .percent(cpu as u16)
                .label(format!("CPU: {:.1}%", cpu));
            f.render_widget(&g, cpu_area);

            let mem_pct_f = mem_used as f64 / mem_total.max(1) as f64 * 100.0;
            let mem_pct = mem_pct_f as u16;
            let mem_used_gb = mem_used as f64 / 1_073_741_824.0;
            let mem_total_gb = mem_total as f64 / 1_073_741_824.0;
            let mem_g = Gauge::default()
                .gauge_style(Style::new().fg(Color::Cyan))
                .percent(mem_pct)
                .label(format!("Mem: {:.1}%  {:.1}/{:.1} GB",
                    mem_pct_f, mem_used_gb, mem_total_gb));
            f.render_widget(&mem_g, mem_area);

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
