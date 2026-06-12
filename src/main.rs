#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::similar_names
)]

use std::time::Duration;

use crossterm::event::{self, Event};

mod app;
mod samplers;
mod tui;

fn main() -> std::io::Result<()> {
    let mut terminal = ratatui::init();
    let mut app = app::App::new();
    let mut samplers = samplers::Samplers::new();

    loop {
        let refresh_proc = app.active_tab == app::Tab::Proc;
        let samples = samplers.sample(refresh_proc);

        app::push_bounded(&mut app.cpu_history, samples.cpu_usage as u64, 60);

        let mem_pct = if samples.mem_total > 0 {
            (samples.mem_used as f64 / samples.mem_total as f64 * 100.0) as u64
        } else {
            0
        };
        app::push_bounded(&mut app.mem_history, mem_pct, 60);

        app::push_bounded(&mut app.net_rx_history, samples.net_rx_rate, 60);
        app::push_bounded(&mut app.net_tx_history, samples.net_tx_rate, 60);

        terminal.draw(|f| tui::draw(f, &app, &samples))?;

        if event::poll(Duration::from_secs(1))?
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
