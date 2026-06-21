#![deny(unsafe_code, trivial_casts)]
#![expect(clippy::multiple_crate_versions)] // ratatui→kasuari→hashbrown 0.16 vs ratatui→hashbrown 0.17
#![expect(
    clippy::cast_possible_truncation, // sparklines/gauges: f32/f64→u16/u64
    clippy::cast_precision_loss,       // percentages→display units
    clippy::cast_sign_loss,            // unsigned counters always non-negative
    clippy::similar_names              // intentional: mem_used/mem_total etc.
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
    let _ = app.refresh_term_size();
    let mut samplers = samplers::Samplers::new();
    let refresh = Duration::from_millis(cfg.refresh_ms);

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
                    #[expect(clippy::option_if_let_else)]
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
