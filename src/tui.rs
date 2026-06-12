use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Gauge, Paragraph, Row, Sparkline, Table};

use crate::app::{App, ProcSortField, Tab};
use crate::samplers::{ProcessInfo, Samples};

fn tab_color(tab: Tab) -> Color {
    match tab {
        Tab::Dash => Color::Green,
        Tab::Proc => Color::Cyan,
        Tab::Net => Color::Yellow,
        Tab::Files => Color::Magenta,
        Tab::Time => Color::Gray,
        Tab::Temp => Color::Red,
    }
}

fn render_sidebar(frame: &mut Frame, area: Rect, app: &App) {
    let border = Block::default().borders(Borders::RIGHT);
    frame.render_widget(&border, area);

    let mut lines = Vec::with_capacity(Tab::ALL.len());
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
    let [_, gauges, cpu_spark, mem_spark, load, help, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(3),
        Constraint::Length(3),
        Constraint::Length(3),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Fill(1),
    ])
    .areas(area);

    let [cpu_area, mem_area, swap_area] = Layout::horizontal([
        Constraint::Percentage(34),
        Constraint::Percentage(33),
        Constraint::Percentage(33),
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
            "Mem: {mem_pct_f:.1}%  {mem_used_gb:.1}/{mem_total_gb:.1} GB"
        ));
    frame.render_widget(&mem_g, mem_area);

    let (swap_pct, swap_label) = if samples.swap_total > 0 {
        let pct = samples.swap_used as f64 / samples.swap_total as f64 * 100.0;
        let used_gb = samples.swap_used as f64 / 1_073_741_824.0;
        let total_gb = samples.swap_total as f64 / 1_073_741_824.0;
        (
            pct as u16,
            format!("Swap: {pct:.1}%  {used_gb:.1}/{total_gb:.1} GB"),
        )
    } else {
        (0, "Swap: N/A".to_string())
    };
    let swap_g = Gauge::default()
        .gauge_style(Style::new().fg(Color::Yellow))
        .percent(swap_pct)
        .label(swap_label);
    frame.render_widget(&swap_g, swap_area);

    let s = Sparkline::default()
        .block(Block::bordered().title(" CPU "))
        .data(app.cpu_history.iter())
        .style(Style::new().fg(Color::Green));
    frame.render_widget(&s, cpu_spark);

    let ms = Sparkline::default()
        .block(Block::bordered().title(" Memory "))
        .data(app.mem_history.iter())
        .style(Style::new().fg(Color::Cyan));
    frame.render_widget(&ms, mem_spark);

    let l = Paragraph::new(Line::from(vec![
        Span::styled("Load Average  ", Style::new().bold()),
        Span::raw(format!(
            "{:.2} (1m)  {:.2} (5m)  {:.2} (15m)",
            samples.load_one, samples.load_five, samples.load_fifteen,
        )),
    ]))
    .alignment(Alignment::Center)
    .gray();
    frame.render_widget(&l, load);

    let h = Paragraph::new("press q to quit")
        .alignment(Alignment::Center)
        .gray();
    frame.render_widget(&h, help);
}

fn format_bytes(bytes: u64) -> String {
    let s = if bytes >= 1_000_000_000_000 {
        format!("{:.1}TB", bytes as f64 / 1_000_000_000_000.0)
    } else if bytes >= 1_000_000_000 {
        format!("{:.1}GB", bytes as f64 / 1_000_000_000.0)
    } else if bytes >= 1_000_000 {
        format!("{:.1}MB", bytes as f64 / 1_000_000.0)
    } else if bytes >= 1_000 {
        format!("{:.1}KB", bytes as f64 / 1_000.0)
    } else {
        format!("{bytes}B")
    };
    format!("{s:>12}")
}

fn format_disk_size(bytes: u64) -> String {
    let b = bytes as f64;
    if b >= 1_099_511_627_776.0 {
        format!("{:.1}TiB", b / 1_099_511_627_776.0)
    } else if b >= 1_073_741_824.0 {
        format!("{:.1}GiB", b / 1_073_741_824.0)
    } else if b >= 1_048_576.0 {
        format!("{:.0}MiB", b / 1_048_576.0)
    } else {
        format!("{bytes}B")
    }
}

fn format_temp(temp: Option<f32>) -> String {
    match temp.filter(|t| t.is_finite()) {
        Some(t) => format!("{:>8}", format!("{:.1}°C", t)),
        None => format!("{:>8}", "N/A"),
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

fn render_temp(frame: &mut Frame, area: Rect, samples: &Samples) {
    let widths: [Constraint; 4] = [
        Constraint::Fill(1),
        Constraint::Length(8),
        Constraint::Length(8),
        Constraint::Length(8),
    ];

    let rows: Vec<Row> = samples
        .temperatures
        .iter()
        .map(|t| {
            Row::new(vec![
                Cell::from(t.label.as_str()),
                Cell::from(Span::styled(
                    format_temp(t.temperature),
                    Style::new().fg(Color::Red),
                )),
                Cell::from(format_temp(t.max)),
                Cell::from(format_temp(t.critical)),
            ])
        })
        .collect();

    let table = Table::new(rows, widths)
        .header(Row::new(vec!["Sensor", "Temp", "Max", "Critical"]))
        .block(Block::bordered().title(" Temperature "));
    frame.render_widget(table, area);
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

fn render_net(frame: &mut Frame, area: Rect, app: &App, samples: &Samples) {
    let [table_area, rx_spark, tx_spark] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(3),
        Constraint::Length(3),
    ])
    .areas(area);

    let rows: Vec<Row> = samples
        .interfaces
        .iter()
        .map(|iface| {
            Row::new(vec![
                Cell::from(iface.name.as_str()),
                Cell::from(Span::styled(
                    format_bytes(iface.rx_bytes),
                    Style::new().fg(Color::Yellow),
                )),
                Cell::from(Span::styled(
                    format_bytes(iface.tx_bytes),
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
        .header(Row::new(vec!["Interface", "RX", "TX", "State"]))
        .block(Block::bordered().title(" Network I/O "));
    frame.render_widget(table, table_area);

    let rs = Sparkline::default()
        .block(Block::bordered().title(" RX "))
        .data(app.net_rx_history.iter())
        .style(Style::new().fg(Color::Green));
    frame.render_widget(&rs, rx_spark);

    let ts = Sparkline::default()
        .block(Block::bordered().title(" TX "))
        .data(app.net_tx_history.iter())
        .style(Style::new().fg(Color::Yellow));
    frame.render_widget(&ts, tx_spark);
}

fn sort_arrow(field: ProcSortField, sort_field: ProcSortField, asc: bool) -> &'static str {
    if field != sort_field {
        return "";
    }
    if asc { "\u{2191}" } else { "\u{2193}" }
}

#[allow(clippy::too_many_arguments)]
fn render_proc(
    frame: &mut Frame,
    area: Rect,
    samples: &Samples,
    scroll: usize,
    query: &str,
    searching: bool,
    sort_field: ProcSortField,
    sort_asc: bool,
) {
    let has_query = !query.is_empty();
    let (search_area, table_area) = if has_query || searching {
        let [s, t] = Layout::vertical([Constraint::Length(3), Constraint::Fill(1)]).areas(area);
        (Some(s), t)
    } else {
        (None, area)
    };

    if let Some(sa) = search_area {
        let cursor = if searching { "\u{258c}" } else { "" };
        let content = if query.is_empty() {
            cursor.to_string()
        } else {
            format!("{query}{cursor}")
        };
        let block = Paragraph::new(content).block(Block::bordered().title(" Search "));
        frame.render_widget(&block, sa);
    }

    let mut filtered: Vec<&ProcessInfo> = if has_query {
        let q = query.to_lowercase();
        samples
            .processes
            .iter()
            .filter(|p| p.name.to_lowercase().contains(&q))
            .collect()
    } else {
        samples.processes.iter().collect()
    };

    filtered.sort_unstable_by(|a, b| {
        let ord = match sort_field {
            ProcSortField::Name => a.name.cmp(&b.name),
            ProcSortField::Pid => a.pid.cmp(&b.pid),
            ProcSortField::Cpu => a.cpu.total_cmp(&b.cpu),
            ProcSortField::Memory => a.memory.cmp(&b.memory),
        };
        if sort_asc { ord } else { ord.reverse() }
    });

    let count = filtered.len();
    let scroll = scroll.min(count.saturating_sub(1));
    let max_visible = (table_area.height as usize).saturating_sub(3);
    let start = scroll;
    let end = count.min(start + max_visible);
    let visible = &filtered[start..end];

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
                format!("{:.1}GiB", p.memory as f64 / 1_073_741_824.0)
            } else {
                format!("{:.0}MiB", p.memory as f64 / 1_048_576.0)
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
        .header(Row::new(vec![
            format!(
                "Name{}",
                sort_arrow(ProcSortField::Name, sort_field, sort_asc)
            ),
            format!(
                "PID{}",
                sort_arrow(ProcSortField::Pid, sort_field, sort_asc)
            ),
            format!(
                "CPU%{}",
                sort_arrow(ProcSortField::Cpu, sort_field, sort_asc)
            ),
            format!(
                "Memory{}",
                sort_arrow(ProcSortField::Memory, sort_field, sort_asc)
            ),
            "Status".to_string(),
        ]))
        .block(Block::bordered().title(format!(
            " Processes ({}/{}) ",
            filtered.len(),
            samples.processes.len(),
        )));
    frame.render_widget(table, table_area);
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
        Tab::Proc => render_proc(
            frame,
            content_area,
            samples,
            app.proc_scroll,
            &app.proc_query,
            app.proc_search_focused,
            app.proc_sort_field,
            app.proc_sort_asc,
        ),
        Tab::Net => render_net(frame, content_area, app, samples),
        Tab::Files => render_files(frame, content_area, samples),
        Tab::Time => render_time(frame, content_area, samples),
        Tab::Temp => render_temp(frame, content_area, samples),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_bytes_zero() {
        assert_eq!(format_bytes(0), format!("{:>12}", "0B"));
    }

    #[test]
    fn format_bytes_bytes() {
        assert_eq!(format_bytes(500), format!("{:>12}", "500B"));
    }

    #[test]
    fn format_bytes_kilobytes() {
        assert_eq!(format_bytes(1500), format!("{:>12}", "1.5KB"));
    }

    #[test]
    fn format_bytes_megabytes() {
        assert_eq!(format_bytes(2_000_000), format!("{:>12}", "2.0MB"));
    }

    #[test]
    fn format_bytes_gigabytes() {
        assert_eq!(format_bytes(2_000_000_000), format!("{:>12}", "2.0GB"));
    }

    #[test]
    fn format_bytes_terabytes() {
        assert_eq!(format_bytes(2_000_000_000_000), format!("{:>12}", "2.0TB"));
    }

    #[test]
    fn format_disk_size_bytes() {
        assert_eq!(format_disk_size(500), "500B");
    }

    #[test]
    fn format_disk_size_megabytes() {
        assert_eq!(format_disk_size(1_048_576), "1MiB");
    }

    #[test]
    fn format_disk_size_gigabytes() {
        assert_eq!(format_disk_size(1_073_741_824), "1.0GiB");
    }

    #[test]
    fn format_disk_size_terabytes() {
        let two_tb = 2 * 1_099_511_627_776;
        assert_eq!(format_disk_size(two_tb), "2.0TiB");
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
        assert_eq!(tab_color(Tab::Temp), Color::Red);
    }

    #[test]
    fn format_temp_value() {
        assert_eq!(format_temp(Some(65.0)), "  65.0°C");
        assert_eq!(format_temp(Some(0.0)), "   0.0°C");
    }

    #[test]
    fn format_temp_none() {
        assert_eq!(format_temp(None), "     N/A");
    }

    #[test]
    fn format_temp_nan() {
        assert_eq!(format_temp(Some(f32::NAN)), "     N/A");
    }

    #[test]
    fn format_temp_large() {
        assert_eq!(format_temp(Some(100.5)), " 100.5°C");
    }
}
