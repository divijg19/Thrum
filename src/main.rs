#![allow(
    clippy::cast_possible_truncation, // sparklines/gauges: f32/f64→u16/u64
    clippy::cast_precision_loss,       // percentages→display units
    clippy::cast_sign_loss,            // unsigned counters always non-negative
    clippy::similar_names              // intentional: mem_used/mem_total etc.
)]

const WINDOW: usize = 60;

use std::time::Duration;

use crossterm::event::{self, Event};

mod app;
mod samplers;
mod tui;

fn main() -> std::io::Result<()> {
    let cfg = app::parse_args();
    let mut terminal = ratatui::init();
    let mut app = app::App::new();
    app.apply_config(&cfg);
    let mut samplers = samplers::Samplers::new();
    let refresh = Duration::from_millis(cfg.refresh_ms);

    loop {
        let refresh_proc = app.active_tab == app::Tab::Proc;
        let samples = samplers.sample(refresh_proc);

        app::push_bounded(&mut app.cpu_history, samples.cpu_usage as u64, WINDOW);

        let mem_pct = if samples.mem_total > 0 {
            (samples.mem_used as f64 / samples.mem_total as f64 * 100.0) as u64
        } else {
            0
        };
        app::push_bounded(&mut app.mem_history, mem_pct, WINDOW);

        app::push_bounded(&mut app.net_rx_history, samples.net_rx_rate, WINDOW);
        app::push_bounded(&mut app.net_tx_history, samples.net_tx_rate, WINDOW);

        let total_disk_read: u64 = samples.disk_io.iter().map(|d| d.read_rate).sum();
        let total_disk_write: u64 = samples.disk_io.iter().map(|d| d.write_rate).sum();
        app::push_bounded(&mut app.disk_read_history, total_disk_read, WINDOW);
        app::push_bounded(&mut app.disk_write_history, total_disk_write, WINDOW);

        terminal.draw(|f| tui::draw(f, &app, &samples))?;

        if event::poll(refresh)?
            && let Event::Key(key) = event::read()?
        {
            app.handle_key(key);
            if app.should_quit {
                break;
            }
        }
    }

    ratatui::restore();
    Ok(())
}
