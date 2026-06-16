#![deny(unsafe_code, trivial_casts)]
#![allow(
    clippy::cast_possible_truncation, // sparklines/gauges: f32/f64→u16/u64
    clippy::cast_precision_loss,       // percentages→display units
    clippy::cast_sign_loss,            // unsigned counters always non-negative
    clippy::similar_names              // intentional: mem_used/mem_total etc.
)]

use std::time::Duration;

use crossterm::event::{self, Event};

mod app;
mod samplers;
mod tui;

fn main() -> std::io::Result<()> {
    let cfg = app::parse_args(&std::env::args().skip(1).collect::<Vec<_>>());
    let mut terminal = ratatui::init();
    let mut app = app::App::new();
    app.apply_config(&cfg);
    let mut samplers = samplers::Samplers::new();
    let refresh = Duration::from_millis(cfg.refresh_ms);

    loop {
        let refresh_proc = app.active_tab == app::Tab::Proc;
        let samples = samplers.sample(refresh_proc);

        app::push_bounded(&mut app.cpu_history, samples.cpu_usage as u64, app::WINDOW);

        let mem_pct = if samples.mem_total > 0 {
            (samples.mem_used as f64 / samples.mem_total as f64 * 100.0) as u64
        } else {
            0
        };
        app::push_bounded(&mut app.mem_history, mem_pct, app::WINDOW);

        app::push_bounded(&mut app.net_rx_history, samples.net_rx_rate, app::WINDOW);
        app::push_bounded(&mut app.net_tx_history, samples.net_tx_rate, app::WINDOW);

        app::push_bounded(
            &mut app.disk_read_history,
            samples.disk_read_rate,
            app::WINDOW,
        );
        app::push_bounded(
            &mut app.disk_write_history,
            samples.disk_write_rate,
            app::WINDOW,
        );

        let valid_temps: Vec<f32> = samples
            .temperatures
            .iter()
            .filter_map(|t| t.temperature.filter(|t| t.is_finite()))
            .collect();
        let avg_temp = if valid_temps.is_empty() {
            0.0
        } else {
            valid_temps.iter().sum::<f32>() / valid_temps.len() as f32
        };
        app::push_bounded(&mut app.temp_history, (avg_temp * 10.0) as u64, app::WINDOW);

        let avg_usage = if samples.disks.is_empty() {
            0.0
        } else {
            samples.disks.iter().map(|d| d.usage_pct).sum::<f32>() / samples.disks.len() as f32
        };
        app::push_bounded(
            &mut app.disk_usage_history,
            (avg_usage * 10.0) as u64,
            app::WINDOW,
        );

        let swap_pct = if samples.swap_total > 0 {
            (samples.swap_used as f64 / samples.swap_total as f64 * 100.0) as u64
        } else {
            0
        };
        app::push_bounded(&mut app.swap_history, swap_pct, app::WINDOW);

        terminal.draw(|f| tui::draw(f, &mut app, &samples))?;

        if event::poll(refresh)?
            && let Event::Key(key) = event::read()?
        {
            app.handle_key(key);
            if let Some(app::KillState::Dispatch(signal)) = app.kill_state.take()
                && let Some(pid) = app.selected_pid
            {
                let ok = samplers.kill_process(pid, signal);
                app.kill_feedback = Some(if ok {
                    format!("Killed PID {pid}")
                } else {
                    format!("Failed to kill PID {pid}")
                });
            }
            if app.should_quit {
                break;
            }
        }
    }

    ratatui::restore();
    Ok(())
}
