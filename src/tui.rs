use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Gauge, Paragraph, Row, Sparkline, Table};

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

    let [cpu_area, mem_area] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(gauges);

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
            "Mem: {mem_pct_f:.1}%  {mem_used_gb:.1}/{mem_total_gb:.1} GB"
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

fn format_rate(bytes: u64) -> String {
    let s = if bytes >= 1_000_000 {
        format!("{:.1}MB/s", bytes as f64 / 1_000_000.0)
    } else if bytes >= 1_000 {
        format!("{:.1}KB/s", bytes as f64 / 1_000.0)
    } else {
        format!("{bytes}B/s")
    };
    format!("{s:>12}")
}

fn format_disk_size(bytes: u64) -> String {
    let b = bytes as f64;
    if b >= 1_099_511_627_776.0 {
        format!("{:.1}TB", b / 1_099_511_627_776.0)
    } else if b >= 1_073_741_824.0 {
        format!("{:.1}GB", b / 1_073_741_824.0)
    } else if b >= 1_048_576.0 {
        format!("{:.0}MB", b / 1_048_576.0)
    } else {
        format!("{bytes}B")
    }
}

fn format_uptime(secs: u64) -> String {
    let d = secs / 86400;
    let h = (secs % 86400) / 3600;
    let m = (secs % 3600) / 60;
    if d > 0 {
        format!("{d}d {h}h {m}m")
    } else if h > 0 {
        format!("{h}h {m}m")
    } else {
        format!("{m}m")
    }
}

fn render_time(frame: &mut Frame, area: Rect, samples: &Samples) {
    let lines = vec![
        Line::from(vec![
            Span::styled("Hostname    ", Style::new().bold()),
            Span::raw(&samples.sys_info.hostname),
        ]),
        Line::from(vec![
            Span::styled("OS          ", Style::new().bold()),
            Span::raw(&samples.sys_info.os),
        ]),
        Line::from(vec![
            Span::styled("Kernel      ", Style::new().bold()),
            Span::raw(&samples.sys_info.kernel),
        ]),
        Line::from(vec![
            Span::styled("Arch        ", Style::new().bold()),
            Span::raw(&samples.sys_info.arch),
        ]),
        Line::from(vec![
            Span::styled("Uptime      ", Style::new().bold()),
            Span::raw(format_uptime(samples.sys_info.uptime)),
        ]),
    ];

    let [_, info, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(5),
        Constraint::Fill(1),
    ])
    .areas(area);

    let p = Paragraph::new(lines).fg(Color::Gray);
    frame.render_widget(&p, info);
}

fn render_files(frame: &mut Frame, area: Rect, samples: &Samples) {
    let widths: [Constraint; 6] = [
        Constraint::Fill(1),
        Constraint::Length(6),
        Constraint::Length(9),
        Constraint::Length(9),
        Constraint::Length(6),
        Constraint::Length(7),
    ];

    let rows: Vec<Row> = samples
        .disks
        .iter()
        .map(|d| {
            Row::new(vec![
                Cell::from(d.mount.as_str()),
                Cell::from(d.fs.as_str()),
                Cell::from(Span::styled(
                    format_disk_size(d.total),
                    Style::new().fg(Color::Magenta),
                )),
                Cell::from(Span::styled(
                    format_disk_size(d.available),
                    Style::new().fg(Color::Magenta),
                )),
                Cell::from(Span::styled(
                    format!("{:.1}%", d.usage_pct),
                    Style::new().fg(Color::Magenta),
                )),
                Cell::from(d.kind.as_str()),
            ])
        })
        .collect();

    let table = Table::new(rows, widths)
        .header(Row::new(vec![
            "Mount", "FS", "Size", "Avail", "Use%", "Kind",
        ]))
        .block(Block::bordered().title(" Filesystems "));
    frame.render_widget(table, area);
}

fn render_net(frame: &mut Frame, area: Rect, samples: &Samples) {
    let rows: Vec<Row> = samples
        .interfaces
        .iter()
        .map(|iface| {
            Row::new(vec![
                Cell::from(iface.name.as_str()),
                Cell::from(Span::styled(
                    format_rate(iface.rx_bytes),
                    Style::new().fg(Color::Yellow),
                )),
                Cell::from(Span::styled(
                    format_rate(iface.tx_bytes),
                    Style::new().fg(Color::Yellow),
                )),
                Cell::from(iface.state.as_str()),
            ])
        })
        .collect();

    let widths: [Constraint; 4] = [
        Constraint::Fill(1),
        Constraint::Length(12),
        Constraint::Length(12),
        Constraint::Length(8),
    ];

    let table = Table::new(rows, widths)
        .header(Row::new(vec!["Interface", "RX/s", "TX/s", "State"]))
        .block(Block::bordered().title(" Network I/O "));
    frame.render_widget(table, area);
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

pub fn draw(frame: &mut Frame, app: &App, samples: &Samples) {
    let block = if app.sidebar_visible {
        Block::bordered().title(" Thrum ")
    } else {
        Block::bordered().title(format!(" Thrum | {} ", app.active_tab.label()))
    };
    frame.render_widget(&block, frame.area());
    let inner = block.inner(frame.area());

    let content_area = if app.sidebar_visible {
        let [sidebar_area, content_area] =
            Layout::horizontal([Constraint::Length(8), Constraint::Fill(1)]).areas(inner);
        render_sidebar(frame, sidebar_area, app);
        content_area
    } else {
        inner
    };

    match app.active_tab {
        Tab::Dash => render_dash(frame, content_area, app, samples),
        Tab::Proc => render_proc(frame, content_area, samples, app.proc_scroll),
        Tab::Net => render_net(frame, content_area, samples),
        Tab::Files => render_files(frame, content_area, samples),
        Tab::Time => render_time(frame, content_area, samples),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_rate_zero() {
        assert_eq!(format_rate(0), format!("{:>12}", "0B/s"));
    }

    #[test]
    fn format_rate_bytes() {
        assert_eq!(format_rate(500), format!("{:>12}", "500B/s"));
    }

    #[test]
    fn format_rate_kilobytes() {
        assert_eq!(format_rate(1500), format!("{:>12}", "1.5KB/s"));
    }

    #[test]
    fn format_rate_megabytes() {
        assert_eq!(format_rate(2_000_000), format!("{:>12}", "2.0MB/s"));
    }

    #[test]
    fn format_disk_size_bytes() {
        assert_eq!(format_disk_size(500), "500B");
    }

    #[test]
    fn format_disk_size_megabytes() {
        assert_eq!(format_disk_size(1_048_576), "1MB");
    }

    #[test]
    fn format_disk_size_gigabytes() {
        assert_eq!(format_disk_size(1_073_741_824), "1.0GB");
    }

    #[test]
    fn format_disk_size_terabytes() {
        let two_tb = 2 * 1_099_511_627_776;
        assert_eq!(format_disk_size(two_tb), "2.0TB");
    }

    #[test]
    fn format_uptime_minutes() {
        assert_eq!(format_uptime(0), "0m");
        assert_eq!(format_uptime(30), "0m");
        assert_eq!(format_uptime(119), "1m");
        assert_eq!(format_uptime(120), "2m");
    }

    #[test]
    fn format_uptime_hours() {
        assert_eq!(format_uptime(3600), "1h 0m");
        assert_eq!(format_uptime(3661), "1h 1m");
    }

    #[test]
    fn format_uptime_days() {
        assert_eq!(format_uptime(86400), "1d 0h 0m");
        assert_eq!(format_uptime(90061), "1d 1h 1m");
    }

    #[test]
    fn tab_color_matches_tab() {
        assert_eq!(tab_color(Tab::Dash), Color::Green);
        assert_eq!(tab_color(Tab::Proc), Color::Cyan);
        assert_eq!(tab_color(Tab::Net), Color::Yellow);
        assert_eq!(tab_color(Tab::Files), Color::Magenta);
        assert_eq!(tab_color(Tab::Time), Color::Gray);
    }
}
