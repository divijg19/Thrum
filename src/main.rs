#![deny(unsafe_code, trivial_casts)]
#![expect(clippy::multiple_crate_versions)] // ratatui→kasuari→hashbrown 0.16 vs ratatui→hashbrown 0.17
#![allow(
    clippy::cast_possible_truncation, // sparklines/gauges: f32/f64→u16/u64
    clippy::cast_precision_loss,       // percentages→display units
    clippy::cast_sign_loss,            // unsigned counters always non-negative
    clippy::similar_names,             // intentional: mem_used/mem_total etc.
    clippy::uninlined_format_args      // intentional: positional args are more readable
)]

use std::time::Duration;

fn restore_terminal() {
    let _ = execute!(std::io::stdout(), DisableMouseCapture);
    ratatui::restore();
}

struct TerminalGuard;
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore_terminal();
    }
}

use crossterm::event::{self, Event};
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;

mod app;
mod samplers;
mod tui;

fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut cfg = match app::parse_args(&args) {
        app::CliAction::Help => {
            app::print_help();
            return Ok(());
        }
        app::CliAction::Version => {
            println!("thrum {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        app::CliAction::Error(msg) => {
            eprintln!("error: {msg}");
            std::process::exit(1);
        }
        app::CliAction::Config(cfg) => cfg,
    };
    let mut terminal = ratatui::init();
    execute!(std::io::stdout(), EnableMouseCapture)?;
    let _guard = TerminalGuard;

    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        prev_hook(info);
    }));
    let mut app = app::App::new();
    app.error_msg = cfg.config_warning.take();
    app.apply_config(&cfg);
    if let Ok((w, h)) = crossterm::terminal::size() {
        app.term_width = w;
        app.term_height = h;
    }
    let mut samplers = samplers::Samplers::new();
    let refresh = Duration::from_millis(cfg.refresh_ms);

    let mut last_samples = samplers::Samples::default();

    loop {
        if !app.paused {
            let refresh_proc = app.active_tab == app::Tab::Proc;
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                last_samples = samplers.sample(refresh_proc)
            }));
            match result {
                Ok(()) => {
                    app.error_msg = None;

                    app::push_bounded(
                        &mut app.cpu_history,
                        last_samples.cpu_usage as u64,
                        app.history_window,
                    );

                    let mem_pct = app::pct(last_samples.mem_used, last_samples.mem_total) as u64;
                    app::push_bounded(&mut app.mem_history, mem_pct, app.history_window);

                    app::push_bounded(
                        &mut app.net_rx_history,
                        last_samples.net_rx_rate,
                        app.history_window,
                    );
                    app::push_bounded(
                        &mut app.net_tx_history,
                        last_samples.net_tx_rate,
                        app.history_window,
                    );

                    app::push_bounded(
                        &mut app.disk_read_history,
                        last_samples.disk_read_rate,
                        app.history_window,
                    );
                    app::push_bounded(
                        &mut app.disk_write_history,
                        last_samples.disk_write_rate,
                        app.history_window,
                    );

                    let valid_temps: Vec<f32> = last_samples
                        .temperatures
                        .iter()
                        .filter_map(|t| t.temperature.filter(|t| t.is_finite()))
                        .collect();
                    let avg_temp = if valid_temps.is_empty() {
                        0.0
                    } else {
                        valid_temps.iter().sum::<f32>() / valid_temps.len() as f32
                    };
                    app::push_bounded(
                        &mut app.temp_history,
                        (avg_temp * 10.0) as u64,
                        app.history_window,
                    );

                    let avg_usage = if last_samples.disks.is_empty() {
                        0.0
                    } else {
                        last_samples.disks.iter().map(|d| d.usage_pct).sum::<f32>()
                            / last_samples.disks.len() as f32
                    };
                    app::push_bounded(
                        &mut app.disk_usage_history,
                        (avg_usage * 10.0) as u64,
                        app.history_window,
                    );

                    let swap_pct = app::pct(last_samples.swap_used, last_samples.swap_total) as u64;
                    app::push_bounded(&mut app.swap_history, swap_pct, app.history_window);
                }
                Err(e) => {
                    samplers = samplers::Samplers::new();
                    let msg = if let Some(s) = e.downcast_ref::<&str>() {
                        s.to_string()
                    } else if let Some(s) = e.downcast_ref::<String>() {
                        s.clone()
                    } else {
                        "sampling failed".to_owned()
                    };
                    app.error_msg = Some(msg);
                }
            }
        }

        terminal.draw(|f| tui::draw(f, &mut app, &last_samples))?;

        if event::poll(refresh)? {
            if let Ok((w, h)) = crossterm::terminal::size() {
                app.term_width = w;
                app.term_height = h;
            }
            match event::read()? {
                Event::Key(key) => {
                    app.handle_key(key);
                    let kill_dispatch = app.kill_state.take();
                    if let Some(app::KillState::Dispatch(signal)) = kill_dispatch
                        && let Some(pid) = app.selected_pid
                    {
                        app.kill_feedback = Some(samplers.kill_process(pid, signal).message(pid));
                    } else if matches!(kill_dispatch, Some(app::KillState::Dispatch(_))) {
                        app.kill_feedback = Some("No process selected".to_owned());
                    }
                }
                Event::Mouse(mouse) => {
                    app.handle_mouse(mouse.column, mouse.row, mouse.kind);
                }
                Event::Resize(w, h) => {
                    app.term_width = w;
                    app.term_height = h;
                }
                _ => {}
            }
            if app.should_quit {
                break;
            }
        }
    }

    Ok(())
}
