use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Gauge, Paragraph, Row, Sparkline, Table};
use ratatui::Frame;

use crate::app::{App, Tab};
use crate::samplers::Samples;

fn tab_color(tab: Tab) -> Color {
    match tab {
        Tab::Dash => Color::Green,
        Tab::Proc => Color::Cyan,
        Tab::Net => Color::Yellow,
        Tab::Files => Color::Magenta,
        Tab::Time => Color::Gray,
    }
}

fn render_sidebar(frame: &mut Frame, area: Rect, app: &App) {
    let border = Block::default().borders(Borders::RIGHT);
    frame.render_widget(&border, area);

    let mut lines = Vec::with_capacity(5);
    for tab in Tab::ALL {
        let is_active = tab == app.active_tab;
        let indicator = if is_active { "\u{25b6}" } else { "\u{25cb}" };
        let style = if is_active {
            Style::new().fg(tab_color(tab)).bold()
        } else {
            Style::new().fg(Color::DarkGray)
        };
        let label = format!("{} {:<5}", indicator, tab.label());
        lines.push(Line::from(Span::styled(label, style)));
    }
    frame.render_widget(Paragraph::new(lines), area);
}

fn render_dash(frame: &mut Frame, area: Rect, app: &App, samples: &Samples) {
    let [_, gauges, spark, help, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(3),
        Constraint::Length(3),
        Constraint::Length(1),
        Constraint::Fill(1),
    ])
    .areas(area);

    let [cpu_area, mem_area] = Layout::horizontal([
        Constraint::Percentage(50),
        Constraint::Percentage(50),
    ])
    .areas(gauges);

    let g = Gauge::default()
        .gauge_style(Style::new().fg(Color::Green))
        .percent(samples.cpu_usage as u16)
        .label(format!("CPU: {:.1}%", samples.cpu_usage));
    frame.render_widget(&g, cpu_area);

    let mem_pct_f = samples.mem_used as f64 / samples.mem_total.max(1) as f64 * 100.0;
    let mem_pct = mem_pct_f as u16;
    let mem_used_gb = samples.mem_used as f64 / 1_073_741_824.0;
    let mem_total_gb = samples.mem_total as f64 / 1_073_741_824.0;
    let mem_g = Gauge::default()
        .gauge_style(Style::new().fg(Color::Cyan))
        .percent(mem_pct)
        .label(format!(
            "Mem: {:.1}%  {:.1}/{:.1} GB",
            mem_pct_f, mem_used_gb, mem_total_gb
        ));
    frame.render_widget(&mem_g, mem_area);

    let s = Sparkline::default()
        .block(Block::bordered().title(" History "))
        .data(app.cpu_history.iter())
        .style(Style::new().fg(Color::Green));
    frame.render_widget(&s, spark);

    let h = Paragraph::new("press q to quit")
        .alignment(Alignment::Center)
        .gray();
    frame.render_widget(&h, help);
}

fn render_proc(frame: &mut Frame, area: Rect, samples: &Samples, scroll: usize) {
    let max_visible = (area.height as usize).saturating_sub(3);
    let start = scroll.min(samples.processes.len().saturating_sub(1));
    let end = samples.processes.len().min(start + max_visible);
    let visible = &samples.processes[start..end];

    let widths: [Constraint; 5] = [
        Constraint::Fill(1),
        Constraint::Length(7),
        Constraint::Length(7),
        Constraint::Length(10),
        Constraint::Length(10),
    ];

    let rows: Vec<Row> = visible
        .iter()
        .map(|p| {
            let mem_label = if p.memory >= 1_073_741_824 {
                format!("{:.1}GB", p.memory as f64 / 1_073_741_824.0)
            } else {
                format!("{:.0}MB", p.memory as f64 / 1_048_576.0)
            };
            Row::new(vec![
                Cell::from(p.name.as_str()),
                Cell::from(format!("{}", p.pid)),
                Cell::from(Span::styled(
                    format!("{:.1}", p.cpu),
                    Style::new().fg(Color::Green),
                )),
                Cell::from(Span::styled(mem_label, Style::new().fg(Color::Cyan))),
                Cell::from(p.status.as_str()),
            ])
        })
        .collect();

    let table = Table::new(rows, widths)
        .header(Row::new(vec!["Name", "PID", "CPU%", "Memory", "Status"]))
        .block(Block::bordered().title(" Processes "));
    frame.render_widget(table, area);
}

fn render_placeholder(frame: &mut Frame, area: Rect, message: &str) {
    let [_, text, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Fill(1),
    ])
    .areas(area);

    let p = Paragraph::new(message)
        .alignment(Alignment::Center)
        .gray();
    frame.render_widget(&p, text);
}

pub fn draw(frame: &mut Frame, app: &App, samples: &Samples) {
    let block = if app.sidebar_visible {
        Block::bordered().title(" Thrum ")
    } else {
        Block::bordered().title(format!(" Thrum | {} ", app.active_tab.label()))
    };
    frame.render_widget(&block, frame.area());
    let inner = block.inner(frame.area());

    let content_area = if app.sidebar_visible {
        let [sidebar_area, content_area] = Layout::horizontal([
            Constraint::Length(8),
            Constraint::Fill(1),
        ])
        .areas(inner);
        render_sidebar(frame, sidebar_area, app);
        content_area
    } else {
        inner
    };

    match app.active_tab {
        Tab::Dash => render_dash(frame, content_area, app, samples),
        Tab::Proc => render_proc(frame, content_area, samples, app.proc_scroll),
        Tab::Net => render_placeholder(frame, content_area, "Network I/O - v0.0.6"),
        Tab::Files => render_placeholder(frame, content_area, "Filesystem mounts - v0.0.7"),
        Tab::Time => render_placeholder(frame, content_area, "System info - v0.0.8"),
    }
}
