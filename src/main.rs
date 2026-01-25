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

        app.cpu_history.push_back(samples.cpu_usage as u64);
        if app.cpu_history.len() > 60 {
            app.cpu_history.pop_front();
        }

        let mem_pct = if samples.mem_total > 0 {
            (samples.mem_used as f64 / samples.mem_total as f64 * 100.0) as u64
        } else {
            0
        };
        app.mem_history.push_back(mem_pct);
        if app.mem_history.len() > 60 {
            app.mem_history.pop_front();
        }

        app.net_rx_history.push_back(samples.net_rx_rate);
        if app.net_rx_history.len() > 60 {
            app.net_rx_history.pop_front();
        }
        app.net_tx_history.push_back(samples.net_tx_rate);
        if app.net_tx_history.len() > 60 {
            app.net_tx_history.pop_front();
        }

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
