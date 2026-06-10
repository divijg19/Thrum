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

        app.proc_scroll = app
            .proc_scroll
            .min(samples.processes.len().saturating_sub(1));

        app.cpu_history.push_back(samples.cpu_usage as u64);
        if app.cpu_history.len() > 60 {
            app.cpu_history.pop_front();
        }

        terminal.draw(|f| tui::draw(f, &app, &samples))?;

        if event::poll(Duration::from_secs(1))? {
            if let Event::Key(key) = event::read()? {
                app.handle_key(key);
                if app.should_quit {
                    break;
                }
            }
        }
    }

    ratatui::restore();
    Ok(())
}
