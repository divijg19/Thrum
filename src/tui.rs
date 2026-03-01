use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Gauge, Paragraph, Row, Sparkline, Table};

use crate::app::{App, KillState, MAX_QUERY_LEN, ProcSortField, Tab, TabOrientation};

const MAX_HINTS: usize = 4;

fn pct(part: u64, total: u64) -> f64 {
    if total > 0 {
        part as f64 / total as f64 * 100.0
    } else {
        0.0
    }
}

fn format_memory(gib: f64, pct: f64) -> String {
    format!("{gib:.1} GiB  {pct:.1}%")
}

fn clamp_scroll(selection: usize, scroll: &mut usize, count: usize, height: u16) -> (usize, usize) {
    let vis = (height as usize).saturating_sub(4);
    if selection < *scroll {
        *scroll = selection;
    } else if vis > 0 && selection >= *scroll + vis {
        *scroll = selection.saturating_add(1).saturating_sub(vis);
    }
    let clamped_scroll = (*scroll).min(count.saturating_sub(1));
    let max_visible = (height as usize).saturating_sub(3);
    let start = clamped_scroll;
    let end = count.min(start + max_visible);
    (start, end)
}
use crate::samplers::{DiskInfo, NetInfo, ProcessInfo, Samples};

const fn tab_color(tab: Tab) -> Color {
    match tab {
        Tab::Dash => Color::Green,
        Tab::Proc => Color::Cyan,
        Tab::Net => Color::Yellow,
        Tab::Files => Color::Magenta,
        Tab::Time => Color::Gray,
        Tab::Temp => Color::Red,
        Tab::Cores => Color::Blue,
        Tab::Disk => Color::White,
        Tab::Mem => Color::LightBlue,
    }
}

const PROC_HEADERS: &[(&str, ProcSortField)] = &[
    ("Name", ProcSortField::Name),
    ("PID", ProcSortField::Pid),
    ("CPU%", ProcSortField::Cpu),
    ("Memory", ProcSortField::Memory),
    ("Virtual", ProcSortField::VirtualMemory),
    ("Time", ProcSortField::RunTime),
    ("Status", ProcSortField::Status),
];

fn render_sidebar(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::default().borders(Borders::RIGHT);
    let inner = block.inner(area);
    frame.render_widget(&block, area);

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
    frame.render_widget(Paragraph::new(lines), inner);
}

fn render_dash(frame: &mut Frame, area: Rect, app: &App, samples: &Samples) {
    let [_, gauges, cpu_spark, mem_spark, load, summary, _] = Layout::vertical([
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
        .percent((samples.cpu_usage.min(100.0)) as u16)
        .label(format!("CPU: {:.1}%", samples.cpu_usage.min(100.0)));
    frame.render_widget(&g, cpu_area);

    let mem_pct_f = pct(samples.mem_used, samples.mem_total.max(1)).min(100.0);
    let mem_pct = mem_pct_f as u16;
    let mem_used_gb = samples.mem_used as f64 / 1_073_741_824.0;
    let mem_total_gb = samples.mem_total as f64 / 1_073_741_824.0;
    let mem_g = Gauge::default()
        .gauge_style(Style::new().fg(Color::Cyan))
        .percent(mem_pct)
        .label(format!(
            "Mem: {mem_pct_f:.1}%  {mem_used_gb:.1}/{mem_total_gb:.1} GiB"
        ));
    frame.render_widget(&mem_g, mem_area);

    let (swap_pct, swap_label) = if samples.swap_total > 0 {
        let pct = pct(samples.swap_used, samples.swap_total).min(100.0);
        let used_gb = samples.swap_used as f64 / 1_073_741_824.0;
        let total_gb = samples.swap_total as f64 / 1_073_741_824.0;
        (
            pct as u16,
            format!("Swap: {pct:.1}%  {used_gb:.1}/{total_gb:.1} GiB"),
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

    let s = Paragraph::new(Line::from(vec![
        Span::styled("Net ", Style::new().bold()),
        Span::raw(format!(
            "TX {}  RX {}",
            format_bytes(samples.net_tx_rate).trim(),
            format_bytes(samples.net_rx_rate).trim(),
        )),
        Span::raw("  "),
        Span::styled("Disk ", Style::new().bold()),
        Span::raw(format!(
            "R {}  W {}",
            format_bytes(samples.disk_read_rate).trim(),
            format_bytes(samples.disk_write_rate).trim(),
        )),
    ]))
    .alignment(Alignment::Center)
    .gray();
    frame.render_widget(&s, summary);
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_000_000_000_000 {
        format!("{:.1}TB", bytes as f64 / 1_000_000_000_000.0)
    } else if bytes >= 1_000_000_000 {
        format!("{:.1}GB", bytes as f64 / 1_000_000_000.0)
    } else if bytes >= 1_000_000 {
        format!("{:.1}MB", bytes as f64 / 1_000_000.0)
    } else if bytes >= 1_000 {
        format!("{:.1}KB", bytes as f64 / 1_000.0)
    } else {
        format!("{bytes}B")
    }
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
    temp.filter(|t| t.is_finite()).map_or_else(
        || format!("{:>9}", "N/A"),
        |t| format!("{:>9}", format!("{:.1}°C", t)),
    )
}

fn format_uptime(secs: u64) -> String {
    let d = secs / 86400;
    let h = (secs % 86400) / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if d > 0 {
        format!("{d}d {h}h {m}m")
    } else if h > 0 {
        format!("{h}h {m}m")
    } else if m > 0 && s == 0 {
        format!("{m}m")
    } else if m > 0 {
        format!("{m}m {s}s")
    } else {
        format!("{s}s")
    }
}

fn format_timestamp(secs: u64) -> String {
    let days_raw = secs / 86400;
    let rem = secs % 86400;
    let hour = rem / 3600;
    let min = (rem % 3600) / 60;
    let sec = rem % 60;

    let max_year = 2999u64;
    let max_cumulative = {
        let mut d = 0u64;
        for y in 1970..=max_year {
            d += 365;
            if y.is_multiple_of(4) && !y.is_multiple_of(100) || y.is_multiple_of(400) {
                d += 1;
            }
        }
        d - 1
    };
    let day = days_raw.min(max_cumulative);

    let mut year = 1970u64;
    let mut day = day;
    loop {
        let leap = year.is_multiple_of(4) && !year.is_multiple_of(100) || year.is_multiple_of(400);
        let diy = if leap { 366 } else { 365 };
        if day < diy {
            break;
        }
        day -= diy;
        year += 1;
    }

    let leap = year.is_multiple_of(4) && !year.is_multiple_of(100) || year.is_multiple_of(400);
    let month_days: [u64; 12] = if leap {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut mo = 0;
    for &md in &month_days {
        if day < md {
            break;
        }
        day -= md;
        mo += 1;
    }

    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        year,
        mo + 1,
        day + 1,
        hour,
        min,
        sec
    )
}

fn filter_processes<'a>(query: &str, processes: &'a [ProcessInfo]) -> Vec<&'a ProcessInfo> {
    if query.is_empty() {
        return processes.iter().collect();
    }
    let q = query.to_lowercase();
    let query_pid = query.parse::<u32>().ok();
    processes
        .iter()
        .filter(|p| p.name.to_lowercase().contains(&q) || query_pid.is_some_and(|pid| p.pid == pid))
        .collect()
}

fn render_search_bar(frame: &mut Frame, area: Rect, query: &str, focused: bool) -> Rect {
    let (search_area, remaining) = if !query.is_empty() || focused {
        let [s, r] = Layout::vertical([Constraint::Length(3), Constraint::Fill(1)]).areas(area);
        (Some(s), r)
    } else {
        (None, area)
    };

    if let Some(sa) = search_area {
        let cursor = if focused { "\u{258c}" } else { "" };
        let content: Vec<Span<'_>> = if query.is_empty() {
            if focused {
                vec![Span::styled(
                    format!("{cursor} Type to filter\u{2026}"),
                    Style::new().fg(Color::DarkGray),
                )]
            } else {
                vec![]
            }
        } else {
            let suffix = if query.len() >= MAX_QUERY_LEN {
                "\u{2026}"
            } else {
                ""
            };
            vec![Span::raw(format!("{query}{suffix}{cursor}"))]
        };
        frame.render_widget(
            Paragraph::new(Line::from(content)).block(Block::bordered().title(" Search ")),
            sa,
        );
    }
    remaining
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
        Line::from(vec![
            Span::styled("CPUs        ", Style::new().bold()),
            Span::raw(format!("{}", samples.sys_info.cpu_count)),
        ]),
        Line::from(vec![
            Span::styled("Distro      ", Style::new().bold()),
            Span::raw(&samples.sys_info.distro),
        ]),
        Line::from(vec![
            Span::styled("Boot        ", Style::new().bold()),
            Span::raw(format_timestamp(samples.sys_info.boot_time)),
        ]),
        Line::from(vec![
            Span::styled("Phys Cores  ", Style::new().bold()),
            Span::raw(format!("{}", samples.sys_info.physical_cores)),
        ]),
    ];

    let [_, info, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(9),
        Constraint::Fill(1),
    ])
    .areas(area);

    let p = Paragraph::new(lines).fg(Color::Gray);
    frame.render_widget(&p, info);
}

fn render_mem(frame: &mut Frame, area: Rect, app: &App, samples: &Samples) {
    let mem_total_gb = samples.mem_total as f64 / 1_073_741_824.0;
    let mem_used_gb = samples.mem_used as f64 / 1_073_741_824.0;
    let mem_avail_gb = samples.mem_available as f64 / 1_073_741_824.0;
    let mem_free_gb = samples.mem_free as f64 / 1_073_741_824.0;

    let mem_used_pct = pct(samples.mem_used, samples.mem_total);
    let mem_avail_pct = pct(samples.mem_available, samples.mem_total);
    let mem_free_pct = pct(samples.mem_free, samples.mem_total);

    let swap_total_gb = samples.swap_total as f64 / 1_073_741_824.0;
    let swap_used_gb = samples.swap_used as f64 / 1_073_741_824.0;
    let swap_free = samples.swap_total.saturating_sub(samples.swap_used);
    let swap_free_gb = swap_free as f64 / 1_073_741_824.0;

    let swap_used_pct = pct(samples.swap_used, samples.swap_total);
    let swap_free_pct = pct(swap_free, samples.swap_total);

    let lines = vec![
        Line::from(vec![
            Span::styled("Memory      ", Style::new().bold()),
            Span::raw(format!("{mem_total_gb:.1} GiB")),
        ]),
        Line::from(vec![
            Span::styled("Used        ", Style::new().bold()),
            Span::raw(format_memory(mem_used_gb, mem_used_pct)),
        ]),
        Line::from(vec![
            Span::styled("Available   ", Style::new().bold()),
            Span::raw(format_memory(mem_avail_gb, mem_avail_pct)),
        ]),
        Line::from(vec![
            Span::styled("Free        ", Style::new().bold()),
            Span::raw(format_memory(mem_free_gb, mem_free_pct)),
        ]),
        Line::from(Span::raw("")),
        Line::from(vec![
            Span::styled("Swap        ", Style::new().bold()),
            Span::raw(format!("{swap_total_gb:.1} GiB")),
        ]),
        Line::from(vec![
            Span::styled("Used        ", Style::new().bold()),
            Span::raw(format_memory(swap_used_gb, swap_used_pct)),
        ]),
        Line::from(vec![
            Span::styled("Free        ", Style::new().bold()),
            Span::raw(format_memory(swap_free_gb, swap_free_pct)),
        ]),
    ];

    let [_, info, mem_spark, swap_spark, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(8),
        Constraint::Length(3),
        Constraint::Length(3),
        Constraint::Fill(1),
    ])
    .areas(area);

    let p = Paragraph::new(lines).fg(Color::Gray);
    frame.render_widget(&p, info);

    let ms = Sparkline::default()
        .block(Block::bordered().title(" History "))
        .data(&app.mem_history)
        .style(Style::new().fg(Color::LightBlue));
    frame.render_widget(&ms, mem_spark);

    let ss = Sparkline::default()
        .block(Block::bordered().title(" Swap "))
        .data(&app.swap_history)
        .style(Style::new().fg(Color::Yellow));
    frame.render_widget(&ss, swap_spark);
}

fn render_temp(frame: &mut Frame, area: Rect, app: &App, samples: &Samples) {
    let widths: [Constraint; 4] = [
        Constraint::Fill(1),
        Constraint::Length(9),
        Constraint::Length(9),
        Constraint::Length(9),
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

    let [table_area, spark_area] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(3)]).areas(area);

    let table = Table::new(rows, widths)
        .header(Row::new(vec!["Sensor", "Temp", "Max", "Critical"]))
        .block(Block::bordered().title(" Temperature "));
    frame.render_widget(table, table_area);

    let ts = Sparkline::default()
        .block(Block::bordered().title(" History "))
        .data(&app.temp_history)
        .style(Style::new().fg(Color::Red));
    frame.render_widget(&ts, spark_area);
}

fn render_cores(frame: &mut Frame, area: Rect, samples: &Samples) {
    let block = Block::bordered().title(" Per-Core CPU ");
    frame.render_widget(&block, area);
    let inner = block.inner(area);

    let core_count = samples.cpus.len();
    if core_count == 0 {
        return;
    }

    let constraints = vec![Constraint::Length(1); core_count];
    let chunks = Layout::vertical(&constraints).split(inner);

    for (i, cpu) in samples.cpus.iter().enumerate() {
        if let Some(chunk) = chunks.get(i) {
            let gauge = Gauge::default()
                .gauge_style(Style::new().fg(Color::Blue))
                .percent((cpu.usage.min(100.0)) as u16)
                .label(format!("{}  {:.1}%  {}MHz", cpu.label, cpu.usage, cpu.freq));
            frame.render_widget(&gauge, *chunk);
        }
    }
}

fn render_files(frame: &mut Frame, area: Rect, app: &mut App, samples: &Samples) {
    let has_query = !app.files_query.is_empty();

    let content_area = render_search_bar(frame, area, &app.files_query, app.files_search_focused);

    let [table_area, spark_area] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(3)]).areas(content_area);

    let filtered: Vec<&DiskInfo> = if has_query {
        let q = app.files_query.to_lowercase();
        samples
            .disks
            .iter()
            .filter(|d| d.mount.to_lowercase().contains(&q) || d.device.to_lowercase().contains(&q))
            .collect()
    } else {
        samples.disks.iter().collect()
    };

    let count = filtered.len();
    app.files_selection = app.files_selection.min(count.saturating_sub(1));

    if count == 0 && !app.files_query.is_empty() {
        let p = Paragraph::new("No matching filesystems")
            .fg(Color::DarkGray)
            .alignment(Alignment::Center);
        frame.render_widget(p, table_area);
        return;
    }

    let (start, end) = clamp_scroll(
        app.files_selection,
        &mut app.files_scroll,
        count,
        table_area.height,
    );
    let visible = &filtered[start..end];
    let rel_sel = app.files_selection.saturating_sub(start);

    let widths: [Constraint; 7] = [
        Constraint::Length(14),
        Constraint::Fill(1),
        Constraint::Length(6),
        Constraint::Length(9),
        Constraint::Length(9),
        Constraint::Length(6),
        Constraint::Length(7),
    ];

    let rows: Vec<Row> = visible
        .iter()
        .enumerate()
        .map(|(i, d)| {
            let is_selected = i == rel_sel;
            let style = if is_selected {
                Style::new().bg(Color::DarkGray)
            } else {
                Style::default()
            };
            Row::new(vec![
                Cell::from(d.device.as_str()),
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
            .style(style)
        })
        .collect();

    let table = Table::new(rows, widths)
        .header(Row::new(vec![
            "Device", "Mount", "FS", "Size", "Avail", "Use%", "Kind",
        ]))
        .block(
            Block::bordered().title(format!(" Filesystems ({count}/{}) ", samples.disks.len(),)),
        );
    frame.render_widget(table, table_area);

    let fus = Sparkline::default()
        .block(Block::bordered().title(" Usage "))
        .data(&app.disk_usage_history)
        .style(Style::new().fg(Color::Magenta));
    frame.render_widget(&fus, spark_area);
}

fn render_disk(frame: &mut Frame, area: Rect, app: &App, samples: &Samples) {
    let [table_area, read_spark, write_spark] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(3),
        Constraint::Length(3),
    ])
    .areas(area);

    let rows: Vec<Row> = samples
        .disk_io
        .iter()
        .map(|d| {
            Row::new(vec![
                Cell::from(d.mount_point.as_str()),
                Cell::from(Span::styled(
                    format_bytes(d.read_rate),
                    Style::new().fg(Color::Cyan),
                )),
                Cell::from(Span::styled(
                    format_bytes(d.write_rate),
                    Style::new().fg(Color::Yellow),
                )),
            ])
        })
        .collect();

    let widths: [Constraint; 3] = [
        Constraint::Fill(1),
        Constraint::Length(12),
        Constraint::Length(12),
    ];

    let table = Table::new(rows, widths)
        .header(Row::new(vec!["Mount", "Read", "Write"]))
        .block(Block::bordered().title(" Disk I/O "));
    frame.render_widget(table, table_area);

    let rs = Sparkline::default()
        .block(Block::bordered().title(" Read "))
        .data(app.disk_read_history.iter())
        .style(Style::new().fg(Color::Cyan));
    frame.render_widget(&rs, read_spark);

    let ws = Sparkline::default()
        .block(Block::bordered().title(" Write "))
        .data(app.disk_write_history.iter())
        .style(Style::new().fg(Color::Yellow));
    frame.render_widget(&ws, write_spark);
}

fn render_net(frame: &mut Frame, area: Rect, app: &mut App, samples: &Samples) {
    let has_query = !app.net_query.is_empty();

    let content_area = render_search_bar(frame, area, &app.net_query, app.net_search_focused);

    let [table_area, rx_spark, tx_spark] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(3),
        Constraint::Length(3),
    ])
    .areas(content_area);

    let filtered: Vec<&NetInfo> = if has_query {
        let q = app.net_query.to_lowercase();
        samples
            .interfaces
            .iter()
            .filter(|i| i.name.to_lowercase().contains(&q))
            .collect()
    } else {
        samples.interfaces.iter().collect()
    };

    let count = filtered.len();
    app.net_selection = app.net_selection.min(count.saturating_sub(1));

    if count == 0 && !app.net_query.is_empty() {
        let p = Paragraph::new("No matching interfaces")
            .fg(Color::DarkGray)
            .alignment(Alignment::Center);
        frame.render_widget(p, table_area);
        return;
    }

    let (start, end) = clamp_scroll(
        app.net_selection,
        &mut app.net_scroll,
        count,
        table_area.height,
    );
    let visible = &filtered[start..end];
    let rel_sel = app.net_selection.saturating_sub(start);

    let widths: [Constraint; 6] = [
        Constraint::Fill(1),
        Constraint::Length(12),
        Constraint::Length(12),
        Constraint::Length(8),
        Constraint::Length(17),
        Constraint::Fill(1),
    ];

    let rows: Vec<Row> = visible
        .iter()
        .enumerate()
        .map(|(i, iface)| {
            let is_selected = i == rel_sel;
            let style = if is_selected {
                Style::new().bg(Color::DarkGray)
            } else {
                Style::default()
            };
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
                Cell::from(iface.mac.as_str()),
                Cell::from(iface.ip.as_str()),
            ])
            .style(style)
        })
        .collect();

    let table = Table::new(rows, widths)
        .header(Row::new(vec![
            "Interface",
            "RX",
            "TX",
            "State",
            "MAC",
            "IP",
        ]))
        .block(Block::bordered().title(format!(
            " Network I/O ({count}/{}) ",
            samples.interfaces.len(),
        )));
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

fn render_help(frame: &mut Frame, area: Rect) {
    let lines = vec![
        Line::from(""),
        Line::from("  ?            Toggle help           Ctrl+T          Cycle orientation"),
        Line::from(
            "  \u{2190}/\u{2192}    Prev/next tab (h)     Ctrl+S          Toggle sidebar/tab bar",
        ),
        Line::from("  Tab          Next tab               Shift+Tab       Previous tab"),
        Line::from(format!("  1-{}          Jump to tab", Tab::ALL.len())),
        Line::from("  /            Search / Filter       Esc             Clear"),
        Line::from("  Space        Pause / Resume"),
        Line::from("  n            Sort by name            p               Sort by PID"),
        Line::from("  c            Sort by CPU             m               Sort by memory"),
        Line::from("  v            Sort by virtual mem     t               Sort by run time"),
        Line::from("  s            Sort by status          r               Toggle sort order"),
        Line::from("  \u{2191}/\u{2193}    Select (Proc)           PgUp/PgDn       Page select"),
        Line::from("  Delete      Kill dialog               Ctrl+K       Kill now (SIGKILL)"),
        Line::from("  q / Ctrl+C  Quit"),
        Line::from(""),
        Line::from("  (h)  horizontal tab modes only"),
        Line::from("  Press ? to close"),
    ];

    let height = lines.len() as u16 + 2;
    let [_, inner, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(height),
        Constraint::Fill(1),
    ])
    .areas(area);

    let [_, centered, _] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Min(48),
        Constraint::Fill(1),
    ])
    .areas(inner);

    let p = Paragraph::new(lines)
        .block(Block::bordered().title(" Help "))
        .fg(Color::Gray);
    frame.render_widget(&p, centered);
}

struct StatusBar {
    ctx: String,
    hints: Vec<String>,
}

const HINT_SEP: &str = " │ ";

impl StatusBar {
    fn build(app: &App) -> Self {
        if let Some(ref fb) = app.kill_feedback {
            return Self {
                ctx: fb.clone(),
                hints: vec![],
            };
        }
        if app.paused {
            return Self {
                ctx: "PAUSED (Space to resume)".to_owned(),
                hints: vec![],
            };
        }
        if app.help_visible {
            return Self {
                ctx: "Help (? to close)".to_owned(),
                hints: vec![],
            };
        }
        if app.kill_state == Some(KillState::Confirm) {
            let pid = app.selected_pid.unwrap_or(0);
            let name = app.selected_name.as_deref().unwrap_or("?");
            return Self {
                ctx: format!("Kill? PID {pid} ({name})"),
                hints: vec![
                    "1 SIGTERM".to_owned(),
                    "2 SIGKILL".to_owned(),
                    "3-6 More".to_owned(),
                    "any Cancel".to_owned(),
                ],
            };
        }

        if let Some(ref err) = app.error_msg {
            return Self {
                ctx: format!("Error: {err}"),
                hints: vec![],
            };
        }

        let ctx = {
            let label = app.active_tab.label();
            let sort = if app.active_tab == Tab::Proc {
                let arrow = if app.proc_sort_asc { "↑" } else { "↓" };
                Some(format!(" [{} {}{}]", label, app.proc_sort_field, arrow))
            } else {
                None
            };
            sort.unwrap_or_else(|| label.to_owned())
        };

        let mut hints: Vec<String> = Vec::with_capacity(3);

        match app.tab_orientation {
            TabOrientation::Sidebar => {
                let label = if app.sidebar_visible { "Hide" } else { "Show" };
                hints.push(format!("Ctrl+S {label} Sidebar"));
            }
            TabOrientation::Horizontal | TabOrientation::HorizontalFooter => {
                let label = if app.tab_bar_visible { "Hide" } else { "Show" };
                hints.push(format!("Ctrl+S {label} Tab Bar"));
            }
        }

        if matches!(app.active_tab, Tab::Proc | Tab::Net | Tab::Files) {
            let active_query = match app.active_tab {
                Tab::Proc => &app.proc_query,
                Tab::Net => &app.net_query,
                Tab::Files => &app.files_query,
                _ => "",
            };
            let focused = match app.active_tab {
                Tab::Proc => app.proc_search_focused,
                Tab::Net => app.net_search_focused,
                Tab::Files => app.files_search_focused,
                _ => false,
            };
            if !focused && active_query.is_empty() {
                hints.push("/ Search".to_owned());
            } else if !active_query.is_empty() {
                hints.push("Esc Clear".to_owned());
            }
        }

        if app.active_tab == Tab::Proc && app.selected_pid.is_some() {
            hints.push("Delete Kill".to_owned());
            hints.push("Ctrl+K Kill!".to_owned());
        }

        if hints.len() < MAX_HINTS {
            match app.active_tab {
                Tab::Proc => hints.push("\u{2191}\u{2193} Select".to_owned()),
                _ => hints.push("1-9 Tab".to_owned()),
            }
        }

        if hints.len() > MAX_HINTS {
            hints.truncate(MAX_HINTS);
        }

        Self { ctx, hints }
    }

    fn display(&self) -> (String, String) {
        if self.hints.is_empty() {
            (self.ctx.clone(), String::new())
        } else {
            (self.ctx.clone(), self.hints.join(HINT_SEP))
        }
    }
}

fn render_horizontal_tabs(frame: &mut Frame, area: Rect, app: &App) {
    let mut spans = Vec::with_capacity(Tab::ALL.len() * 2 - 1);
    for (i, tab) in Tab::ALL.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" │ ", Style::new().fg(Color::DarkGray)));
        }
        let is_active = tab == &app.active_tab;
        let style = if is_active {
            Style::new().fg(tab_color(*tab)).bold()
        } else {
            Style::new().fg(Color::DarkGray)
        };
        spans.push(Span::styled(tab.label(), style));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_proc(frame: &mut Frame, area: Rect, app: &mut App, samples: &Samples) {
    let mut filtered = filter_processes(&app.proc_query, &samples.processes);

    filtered.sort_unstable_by(|a, b| {
        let ord = match app.proc_sort_field {
            ProcSortField::Name => a.name.cmp(&b.name),
            ProcSortField::Pid => a.pid.cmp(&b.pid),
            ProcSortField::Cpu => a.cpu.total_cmp(&b.cpu),
            ProcSortField::Memory => a.memory.cmp(&b.memory),
            ProcSortField::VirtualMemory => a.virtual_memory.cmp(&b.virtual_memory),
            ProcSortField::RunTime => a.run_time.cmp(&b.run_time),
            ProcSortField::Status => a.status.cmp(&b.status),
        };
        if app.proc_sort_asc {
            ord
        } else {
            ord.reverse()
        }
    });

    let count = filtered.len();
    app.proc_selection = app.proc_selection.min(count.saturating_sub(1));
    app.selected_pid = filtered.get(app.proc_selection).map(|p| p.pid);
    app.selected_name = filtered.get(app.proc_selection).map(|p| p.name.clone());

    let table_area = render_search_bar(frame, area, &app.proc_query, app.proc_search_focused);

    if count == 0 && !app.proc_query.is_empty() {
        let p = Paragraph::new("No matching processes")
            .fg(Color::DarkGray)
            .alignment(Alignment::Center);
        frame.render_widget(p, table_area);
        return;
    }

    let (start, end) = clamp_scroll(
        app.proc_selection,
        &mut app.proc_scroll,
        count,
        table_area.height,
    );
    let visible = &filtered[start..end];
    let rel_sel = app.proc_selection.saturating_sub(start);

    let widths: [Constraint; 7] = [
        Constraint::Fill(1),
        Constraint::Length(7),
        Constraint::Length(7),
        Constraint::Length(10),
        Constraint::Length(10),
        Constraint::Length(10),
        Constraint::Length(10),
    ];

    let rows: Vec<Row> = visible
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let is_selected = i == rel_sel;
            let style = if is_selected {
                Style::new().bg(Color::DarkGray)
            } else {
                Style::default()
            };
            let mem_label = if p.memory >= 1_073_741_824 {
                format!("{:.1}GiB", p.memory as f64 / 1_073_741_824.0)
            } else {
                format!("{:.0}MiB", p.memory as f64 / 1_048_576.0)
            };
            let virt_label = if p.virtual_memory >= 1_073_741_824 {
                format!("{:.1}GiB", p.virtual_memory as f64 / 1_073_741_824.0)
            } else {
                format!("{:.0}MiB", p.virtual_memory as f64 / 1_048_576.0)
            };
            Row::new(vec![
                Cell::from(p.name.as_str()),
                Cell::from(format!("{}", p.pid)),
                Cell::from(Span::styled(
                    format!("{:.1}", p.cpu),
                    Style::new().fg(Color::Green),
                )),
                Cell::from(Span::styled(mem_label, Style::new().fg(Color::Cyan))),
                Cell::from(Span::styled(virt_label, Style::new().fg(Color::Magenta))),
                Cell::from(format_uptime(p.run_time)),
                Cell::from(p.status.as_str()),
            ])
            .style(style)
        })
        .collect();

    let table = Table::new(rows, widths)
        .header(Row::new(PROC_HEADERS.iter().map(|&(label, field)| {
            Cell::from(format!(
                "{}{}",
                label,
                sort_arrow(field, app.proc_sort_field, app.proc_sort_asc)
            ))
        })))
        .block(Block::bordered().title(format!(
            " Processes ({}/{}) ",
            filtered.len(),
            samples.processes.len(),
        )));
    frame.render_widget(table, table_area);
}

pub fn draw(frame: &mut Frame, app: &mut App, samples: &Samples) {
    let block = match app.tab_orientation {
        TabOrientation::Horizontal | TabOrientation::HorizontalFooter => {
            Block::bordered().title(" Thrum ")
        }
        TabOrientation::Sidebar if app.sidebar_visible => Block::bordered().title(" Thrum "),
        TabOrientation::Sidebar => {
            Block::bordered().title(format!(" Thrum | {} ", app.active_tab.label()))
        }
    };
    frame.render_widget(&block, frame.area());
    let inner = block.inner(frame.area());

    let content_area = match app.tab_orientation {
        TabOrientation::Sidebar if app.sidebar_visible => {
            let [sidebar_area, content_area] =
                Layout::horizontal([Constraint::Length(9), Constraint::Fill(1)]).areas(inner);
            render_sidebar(frame, sidebar_area, app);
            content_area
        }
        TabOrientation::Horizontal if app.tab_bar_visible => {
            let [tab_area, content_area] =
                Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).areas(inner);
            render_horizontal_tabs(frame, tab_area, app);
            content_area
        }
        _ => inner,
    };

    let has_footer = app.tab_orientation == TabOrientation::HorizontalFooter && app.tab_bar_visible;

    let (tab_area, status_area) = if has_footer {
        let chunks = Layout::vertical([
            Constraint::Fill(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(content_area);
        render_horizontal_tabs(frame, chunks[1], app);
        (chunks[0], chunks[2])
    } else {
        let chunks =
            Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).split(content_area);
        (chunks[0], chunks[1])
    };

    match app.active_tab {
        Tab::Dash => render_dash(frame, tab_area, app, samples),
        Tab::Proc => render_proc(frame, tab_area, app, samples),
        Tab::Net => render_net(frame, tab_area, app, samples),
        Tab::Files => render_files(frame, tab_area, app, samples),
        Tab::Time => render_time(frame, tab_area, samples),
        Tab::Temp => render_temp(frame, tab_area, app, samples),
        Tab::Cores => render_cores(frame, tab_area, samples),
        Tab::Disk => render_disk(frame, tab_area, app, samples),
        Tab::Mem => render_mem(frame, tab_area, app, samples),
    }

    let (ctx, hints) = StatusBar::build(app).display();
    let [ctx_area, hints_area] =
        Layout::horizontal([Constraint::Fill(1), Constraint::Min(20)]).areas(status_area);
    frame.render_widget(
        Paragraph::new(ctx)
            .alignment(Alignment::Left)
            .fg(Color::Gray),
        ctx_area,
    );
    frame.render_widget(
        Paragraph::new(hints)
            .alignment(Alignment::Right)
            .fg(Color::Gray),
        hints_area,
    );

    if let Some(fb) = app.kill_feedback.take() {
        render_kill_feedback(frame, tab_area, &fb);
    }

    if app.help_visible {
        render_help(frame, tab_area);
    }

    if app.kill_state == Some(KillState::Confirm) {
        let pid = app.selected_pid.unwrap_or(0);
        let name = app.selected_name.as_deref().unwrap_or("?");
        render_kill_confirm(frame, tab_area, pid, name);
    }
}

fn render_kill_feedback(frame: &mut Frame, area: Rect, feedback: &str) {
    let lines = vec![
        Line::from(""),
        Line::from(format!("  {feedback}  ")),
        Line::from(""),
    ];

    let height = lines.len() as u16 + 2;
    let [_, inner, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(height),
        Constraint::Fill(1),
    ])
    .areas(area);

    let [_, centered, _] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Min(30),
        Constraint::Fill(1),
    ])
    .areas(inner);

    let p = Paragraph::new(lines)
        .block(Block::bordered().title(" Kill Result "))
        .fg(Color::Gray);
    frame.render_widget(&p, centered);
}

fn render_kill_confirm(frame: &mut Frame, area: Rect, pid: u32, name: &str) {
    let lines = vec![
        Line::from(""),
        Line::from(format!("  Kill PID {pid} ({name})?")),
        Line::from(""),
        Line::from("  1  SIGTERM    2  SIGKILL"),
        Line::from("  3  SIGINT      4  SIGHUP"),
        Line::from("  5  SIGSTOP    6  SIGCONT"),
        Line::from("  any  Cancel"),
        Line::from(""),
    ];

    let height = lines.len() as u16 + 2;
    let [_, inner, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(height),
        Constraint::Fill(1),
    ])
    .areas(area);

    let [_, centered, _] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Min(42),
        Constraint::Fill(1),
    ])
    .areas(inner);

    let p = Paragraph::new(lines)
        .block(Block::bordered().title(" Kill Process "))
        .fg(Color::Gray);
    frame.render_widget(&p, centered);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_bytes_zero() {
        assert_eq!(format_bytes(0), "0B");
    }

    #[test]
    fn format_bytes_bytes() {
        assert_eq!(format_bytes(500), "500B");
    }

    #[test]
    fn format_bytes_kilobytes() {
        assert_eq!(format_bytes(1500), "1.5KB");
    }

    #[test]
    fn format_bytes_megabytes() {
        assert_eq!(format_bytes(2_000_000), "2.0MB");
    }

    #[test]
    fn format_bytes_gigabytes() {
        assert_eq!(format_bytes(2_000_000_000), "2.0GB");
    }

    #[test]
    fn format_bytes_terabytes() {
        assert_eq!(format_bytes(2_000_000_000_000), "2.0TB");
    }

    #[test]
    fn format_bytes_large_no_overflow() {
        let result = format_bytes(18_446_744_073_709_551_615);
        assert!(
            !result.contains("  "),
            "no extra whitespace padding: {result:?}"
        );
        assert!(result.ends_with("TB"), "largest value is in TB");
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
    fn format_uptime_seconds() {
        assert_eq!(format_uptime(0), "0s");
        assert_eq!(format_uptime(30), "30s");
        assert_eq!(format_uptime(59), "59s");
    }

    #[test]
    fn format_uptime_minutes() {
        assert_eq!(format_uptime(119), "1m 59s");
        assert_eq!(format_uptime(120), "2m");
        assert_eq!(format_uptime(150), "2m 30s");
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
        assert_eq!(tab_color(Tab::Cores), Color::Blue);
        assert_eq!(tab_color(Tab::Disk), Color::White);
        assert_eq!(tab_color(Tab::Mem), Color::LightBlue);
    }

    #[test]
    fn format_temp_value() {
        assert_eq!(format_temp(Some(65.0)), "   65.0°C");
        assert_eq!(format_temp(Some(0.0)), "    0.0°C");
    }

    #[test]
    fn format_temp_none() {
        assert_eq!(format_temp(None), "      N/A");
    }

    #[test]
    fn format_temp_nan() {
        assert_eq!(format_temp(Some(f32::NAN)), "      N/A");
    }

    #[test]
    fn format_temp_large() {
        assert_eq!(format_temp(Some(100.5)), "  100.5°C");
    }

    #[test]
    fn filter_processes_by_name() {
        let p1 = ProcessInfo {
            name: "firefox".into(),
            pid: 100,
            cpu: 0.0,
            memory: 0,
            virtual_memory: 0,
            run_time: 0,
            status: "Running".into(),
        };
        let p2 = ProcessInfo {
            name: "bash".into(),
            pid: 200,
            cpu: 0.0,
            memory: 0,
            virtual_memory: 0,
            run_time: 0,
            status: "Sleep".into(),
        };
        let procs = vec![p1, p2];
        let result = filter_processes("fire", &procs);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].pid, 100);
    }

    #[test]
    fn filter_processes_by_pid() {
        let p1 = ProcessInfo {
            name: "firefox".into(),
            pid: 100,
            cpu: 0.0,
            memory: 0,
            virtual_memory: 0,
            run_time: 0,
            status: "Running".into(),
        };
        let p2 = ProcessInfo {
            name: "bash".into(),
            pid: 200,
            cpu: 0.0,
            memory: 0,
            virtual_memory: 0,
            run_time: 0,
            status: "Sleep".into(),
        };
        let procs = vec![p1, p2];
        let result = filter_processes("200", &procs);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].pid, 200);
    }

    #[test]
    fn filter_processes_empty_query_returns_all() {
        let p1 = ProcessInfo {
            name: "firefox".into(),
            pid: 100,
            cpu: 0.0,
            memory: 0,
            virtual_memory: 0,
            run_time: 0,
            status: "Running".into(),
        };
        let p2 = ProcessInfo {
            name: "bash".into(),
            pid: 200,
            cpu: 0.0,
            memory: 0,
            virtual_memory: 0,
            run_time: 0,
            status: "Sleep".into(),
        };
        let procs = vec![p1, p2];
        let result = filter_processes("", &procs);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn filter_processes_name_and_pid() {
        let p1 = ProcessInfo {
            name: "firefox".into(),
            pid: 100,
            cpu: 0.0,
            memory: 0,
            virtual_memory: 0,
            run_time: 0,
            status: "Running".into(),
        };
        let p2 = ProcessInfo {
            name: "bash".into(),
            pid: 200,
            cpu: 0.0,
            memory: 0,
            virtual_memory: 0,
            run_time: 0,
            status: "Sleep".into(),
        };
        let procs = vec![p1, p2];
        let result = filter_processes("100", &procs);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].pid, 100);
    }

    #[test]
    fn status_bar_search_hint_in_hints() {
        let mut app = App::new();
        for tab in [Tab::Proc, Tab::Net, Tab::Files] {
            app.active_tab = tab;
            let (_, hints) = StatusBar::build(&app).display();
            assert!(
                hints.contains("/ Search"),
                "{tab:?} should show search hint",
            );
        }
        app.active_tab = Tab::Dash;
        let (_, hints) = StatusBar::build(&app).display();
        assert!(
            !hints.contains("/ Search"),
            "Dash should not show search hint"
        );
    }

    #[test]
    fn status_bar_search_hidden_when_focused() {
        let mut app = App::new();
        app.active_tab = Tab::Proc;
        app.proc_search_focused = true;
        let (_, hints) = StatusBar::build(&app).display();
        assert!(
            !hints.contains("/ Search"),
            "search hint hidden when focused"
        );
    }

    #[test]
    fn status_bar_kill_hints_only_with_pid_and_proc() {
        let mut app = App::new();
        app.active_tab = Tab::Proc;
        app.selected_pid = Some(42);
        app.selected_name = Some("bash".to_owned());
        let (_, hints) = StatusBar::build(&app).display();
        assert!(hints.contains("Delete Kill"), "kill hint with PID on Proc");
        assert!(
            hints.contains("Ctrl+K Kill!"),
            "Ctrl+K hint with PID on Proc"
        );

        app.selected_pid = None;
        let (_, hints) = StatusBar::build(&app).display();
        assert!(!hints.contains("Delete Kill"), "no kill hint without PID");
    }

    #[test]
    fn status_bar_sidebar_hints_match() {
        let mut app = App::new();
        app.sidebar_visible = true;
        let (_, hints) = StatusBar::build(&app).display();
        assert!(hints.contains("Hide"), "should say 'Hide' when visible");
        app.sidebar_visible = false;
        let (_, hints) = StatusBar::build(&app).display();
        assert!(hints.contains("Show"), "should say 'Show' when hidden");
    }

    #[test]
    fn status_bar_ctx_shows_tab_name() {
        let app = App::new();
        let (ctx, _) = StatusBar::build(&app).display();
        assert_eq!(ctx, "Dash", "default ctx is tab label");
    }

    #[test]
    fn status_bar_ctx_help_mode() {
        let mut app = App::new();
        app.help_visible = true;
        let (ctx, _) = StatusBar::build(&app).display();
        assert_eq!(ctx, "Help (? to close)");
    }

    #[test]
    fn status_bar_ctx_kill_confirm() {
        let mut app = App::new();
        app.kill_state = Some(KillState::Confirm);
        app.selected_pid = Some(42);
        app.selected_name = Some("bash".to_owned());
        let (ctx, _) = StatusBar::build(&app).display();
        assert_eq!(ctx, "Kill? PID 42 (bash)");
    }

    #[test]
    fn status_bar_ctx_kill_feedback() {
        let mut app = App::new();
        app.kill_feedback = Some("Killed PID 42".to_owned());
        let (ctx, _) = StatusBar::build(&app).display();
        assert_eq!(ctx, "Killed PID 42");
    }

    #[test]
    fn status_bar_hints_max_three() {
        let mut app = App::new();
        app.active_tab = Tab::Proc;
        app.selected_pid = Some(42);
        let (_, hints) = StatusBar::build(&app).display();
        let count = hints.matches(HINT_SEP).count() + 1;
        assert!(
            count <= MAX_HINTS,
            "at most {MAX_HINTS} hints, got {count}: {hints}"
        );
    }

    #[test]
    fn status_bar_no_quit_or_help_hints() {
        let app = App::new();
        let (_, hints) = StatusBar::build(&app).display();
        assert!(!hints.contains("q/Ctrl+C"), "quit hint removed");
        assert!(!hints.contains("? Help"), "help hint removed");
    }

    #[test]
    fn format_timestamp_epoch() {
        assert_eq!(format_timestamp(0), "1970-01-01 00:00:00");
    }

    #[test]
    fn format_timestamp_one_second() {
        assert_eq!(format_timestamp(1), "1970-01-01 00:00:01");
    }

    #[test]
    fn format_timestamp_end_of_january() {
        assert_eq!(format_timestamp(2_678_399), "1970-01-31 23:59:59");
    }

    #[test]
    fn format_timestamp_february_first() {
        assert_eq!(format_timestamp(2_678_400), "1970-02-01 00:00:00");
    }

    #[test]
    fn format_timestamp_non_leap_feb_end() {
        assert_eq!(format_timestamp(5_097_599), "1970-02-28 23:59:59");
    }

    #[test]
    fn format_timestamp_march_first_non_leap() {
        assert_eq!(format_timestamp(5_097_600), "1970-03-01 00:00:00");
    }

    #[test]
    fn format_timestamp_year_boundary() {
        assert_eq!(format_timestamp(31_536_000), "1971-01-01 00:00:00");
    }

    #[test]
    fn format_timestamp_far_future_capped() {
        let result = format_timestamp(1_000_000_000_000);
        assert!(
            result.starts_with("2999-"),
            "far-future timestamps capped at year 2999, got: {result}"
        );
    }

    #[test]
    fn format_timestamp_leap_year_march() {
        assert_eq!(format_timestamp(68_256_000), "1972-03-01 00:00:00");
    }

    // --- Status bar paused ---

    #[test]
    fn status_bar_ctx_paused() {
        let mut app = App::new();
        app.paused = true;
        let (ctx, _) = StatusBar::build(&app).display();
        assert_eq!(ctx, "PAUSED (Space to resume)");
    }

    // --- Status bar tab-specific hints ---

    #[test]
    fn status_bar_proc_shows_select_hint() {
        let mut app = App::new();
        app.active_tab = Tab::Proc;
        let (_, hints) = StatusBar::build(&app).display();
        assert!(hints.contains("↑↓ Select"), "Proc tab shows select hint");
    }

    #[test]
    fn status_bar_non_proc_shows_tab_hint() {
        for tab in [
            Tab::Dash,
            Tab::Net,
            Tab::Files,
            Tab::Time,
            Tab::Temp,
            Tab::Cores,
            Tab::Disk,
            Tab::Mem,
        ] {
            let mut app = App::new();
            app.active_tab = tab;
            let (_, hints) = StatusBar::build(&app).display();
            assert!(hints.contains("1-9 Tab"), "{tab:?} shows 1-9 Tab hint");
        }
    }

    // --- Status bar Esc Clear hint ---

    #[test]
    fn status_bar_esc_clear_when_filter_active() {
        let mut app = App::new();
        app.active_tab = Tab::Proc;
        app.proc_query = "fire".to_owned();
        let (_, hints) = StatusBar::build(&app).display();
        assert!(
            hints.contains("Esc Clear"),
            "shows Esc Clear when filter active"
        );
        assert!(
            !hints.contains("/ Search"),
            "no search hint when filter active"
        );
    }

    #[test]
    fn status_bar_search_hint_when_no_filter() {
        let mut app = App::new();
        app.active_tab = Tab::Proc;
        let (_, hints) = StatusBar::build(&app).display();
        assert!(
            hints.contains("/ Search"),
            "shows / Search when no filter active"
        );
    }

    #[test]
    fn status_bar_esc_clear_on_net_tab() {
        let mut app = App::new();
        app.active_tab = Tab::Net;
        app.net_query = "eth".to_owned();
        let (_, hints) = StatusBar::build(&app).display();
        assert!(hints.contains("Esc Clear"), "Esc Clear on Net with filter");
    }

    #[test]
    fn status_bar_hints_keep_kill_with_tab_hint() {
        let mut app = App::new();
        app.active_tab = Tab::Proc;
        app.selected_pid = Some(42);
        app.selected_name = Some("bash".to_owned());
        let (_, hints) = StatusBar::build(&app).display();
        assert!(
            hints.contains("Delete Kill"),
            "kill hint preserved when PID selected"
        );
        assert!(
            hints.contains("Ctrl+K"),
            "Ctrl+K hint preserved when PID selected"
        );
    }

    // --- Status bar error_msg ---

    #[test]
    fn status_bar_shows_error_msg() {
        let mut app = App::new();
        app.error_msg = Some("sampling failed".to_owned());
        let (ctx, hints) = StatusBar::build(&app).display();
        assert_eq!(ctx, "Error: sampling failed");
        assert!(hints.is_empty(), "no hints when showing error");
    }

    #[test]
    fn status_bar_paused_takes_priority_over_error() {
        let mut app = App::new();
        app.error_msg = Some("disk error".to_owned());
        app.paused = true;
        let (ctx, _) = StatusBar::build(&app).display();
        assert_eq!(
            ctx, "PAUSED (Space to resume)",
            "paused wins over error_msg"
        );
    }

    #[test]
    fn status_bar_kill_confirm_takes_priority_over_error() {
        let mut app = App::new();
        app.error_msg = Some("stale data".to_owned());
        app.kill_state = Some(KillState::Confirm);
        app.selected_pid = Some(42);
        app.selected_name = Some("bash".to_owned());
        let (ctx, _) = StatusBar::build(&app).display();
        assert_eq!(ctx, "Kill? PID 42 (bash)", "kill confirm wins over error");
    }
}
