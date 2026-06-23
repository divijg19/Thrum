#![deny(unsafe_code, trivial_casts)]
#![expect(
    clippy::must_use_candidate,
    reason = "all public functions are potentially must-use; marking each individually is noisy"
)]
#![expect(
    clippy::multiple_crate_versions,
    reason = "ratatui→kasuari→hashbrown 0.16 vs ratatui→hashbrown 0.17"
)]
#![expect(
    clippy::cast_possible_truncation, // sparklines/gauges: f32/f64→u16/u64
    clippy::cast_precision_loss,       // percentages→display units
    clippy::cast_sign_loss,            // float→unsigned (cpu_usage etc.)
    clippy::similar_names,
    reason = "intentional: mem_used/mem_total, rx_bytes/tx_bytes etc."
)]

mod app;
mod cli;
mod samplers;
mod tui;
mod ui;

use std::time::Duration;

use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event};
use crossterm::execute;

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

/// Run the Thrum TUI with the given configuration.
///
/// # Errors
///
/// Returns an error if terminal initialization, mouse capture, or
/// the event loop encounters an I/O error.
pub fn run(mut config: Config) -> std::io::Result<()> {
    let mut terminal = ratatui::init();
    execute!(std::io::stdout(), EnableMouseCapture)?;
    let _guard = TerminalGuard;

    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        prev_hook(info);
    }));
    let mut app = app::App::new();
    app.error_msg = config.config_warning.take();
    app.apply_config(&config);
    let _ = app.refresh_term_size();
    let mut samplers = samplers::Samplers::new();
    let refresh = Duration::from_millis(config.refresh_ms);

    let mut last_samples = samplers::Samples::default();

    loop {
        if !app.paused {
            let refresh_proc = app.active_tab.is_proc();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                last_samples = samplers.sample(refresh_proc);
            }));
            match result {
                Ok(()) => {
                    app.error_msg = None;
                    app.push_history(&last_samples);
                }
                Err(e) => {
                    samplers = samplers::Samplers::new();
                    let msg = e
                        .downcast_ref::<&str>()
                        .map(ToString::to_string)
                        .or_else(|| e.downcast_ref::<String>().cloned())
                        .unwrap_or_else(|| "sampling failed".to_owned());
                    app.error_msg = Some(msg);
                }
            }
        }

        terminal.draw(|f| tui::draw(f, &mut app, &last_samples))?;

        if event::poll(refresh)? {
            let _ = app.refresh_term_size();
            match event::read()? {
                Event::Key(key) => {
                    app.handle_key(key);
                    let kill_dispatch = app.kill_state.take();
                    if let Some(app::KillState::Dispatch(signal)) = kill_dispatch
                        && let Some(ref sel) = app.selected
                    {
                        app.kill_feedback =
                            Some(samplers.kill_process(sel.pid, signal).message(sel.pid));
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

pub use app::{App, KillState, ProcSortField, Tab, TabOrientation, TabState};
pub use cli::{CliAction, Config, parse_args, print_help};
pub use samplers::Samples;
pub use tui::draw;
