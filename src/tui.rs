use std::collections::VecDeque;

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Gauge, Paragraph, Row, Sparkline, Table};

use crate::app::{
    self, App, KillState, MAX_QUERY_LEN, ProcSortField, SelectionState, Tab, TabOrientation,
    TabState,
};
use crate::samplers::{DiskInfo, NetInfo, ProcessInfo, Samples};

const MAX_HINTS: usize = 4;

const SPARKLINE_HEIGHT: u16 = 3;

const SPARK_CPU_TITLE: &str = " CPU ";
const SPARK_CPU_COLOR: Color = Color::Green;
const SPARK_MEM_TITLE: &str = " Memory ";
const SPARK_MEM_COLOR: Color = Color::Cyan;
const SPARK_NET_RX_TITLE: &str = " RX ";
const SPARK_NET_RX_COLOR: Color = Color::Green;
const SPARK_NET_TX_TITLE: &str = " TX ";
const SPARK_NET_TX_COLOR: Color = Color::Yellow;
const SPARK_DISK_READ_TITLE: &str = " Read ";
const SPARK_DISK_READ_COLOR: Color = Color::Cyan;
const SPARK_DISK_WRITE_TITLE: &str = " Write ";
const SPARK_DISK_WRITE_COLOR: Color = Color::Yellow;
const SPARK_USAGE_TITLE: &str = " Usage ";
const SPARK_USAGE_COLOR: Color = Color::Magenta;
const SPARK_MEM_HISTORY_TITLE: &str = " History ";
const SPARK_MEM_HISTORY_COLOR: Color = Color::LightBlue;
const SPARK_SWAP_TITLE: &str = " Swap ";
const SPARK_SWAP_COLOR: Color = Color::Yellow;
const SPARK_TEMP_TITLE: &str = " History ";
const SPARK_TEMP_COLOR: Color = Color::Red;

const STYLE_GREEN: Style = Style::new().fg(Color::Green);
const STYLE_CYAN: Style = Style::new().fg(Color::Cyan);
const STYLE_YELLOW: Style = Style::new().fg(Color::Yellow);
const STYLE_MAGENTA: Style = Style::new().fg(Color::Magenta);
const STYLE_DARK_GRAY: Style = Style::new().fg(Color::DarkGray);
const STYLE_SELECTED: Style = Style::new().bg(Color::DarkGray);
const STYLE_RED: Style = Style::new().fg(Color::Red);

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

impl Tab {
    /// Returns the accent color associated with this tab.
    pub const fn color(self) -> Color {
        match self {
            Self::Dash => Color::Green,
            Self::Proc => Color::Cyan,
            Self::Net => Color::Yellow,
            Self::Files => Color::Magenta,
            Self::Time => Color::Gray,
            Self::Temp => Color::Red,
            Self::Cores => Color::Blue,
            Self::Disk => Color::White,
            Self::Mem => Color::LightBlue,
        }
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
            Style::new().fg(tab.color()).bold()
        } else {
            STYLE_DARK_GRAY
        };
        let label = format!("{} {:<5}", indicator, tab.label());
        lines.push(Line::from(Span::styled(label, style)));
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

fn render_dash(frame: &mut Frame, area: Rect, app: &mut App, samples: &Samples) {
    let [_, gauges, cpu_spark, mem_spark, load, summary, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(3),
        Constraint::Length(SPARKLINE_HEIGHT),
        Constraint::Length(SPARKLINE_HEIGHT),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Fill(1),
    ])
    .areas(area);

    render_dash_gauges(frame, gauges, samples);
    render_sparkline(
        frame,
        cpu_spark,
        SPARK_CPU_TITLE,
        &app.cpu_history,
        SPARK_CPU_COLOR,
    );
    render_sparkline(
        frame,
        mem_spark,
        SPARK_MEM_TITLE,
        &app.mem_history,
        SPARK_MEM_COLOR,
    );
    render_dash_load(frame, load, samples);
    render_dash_summary(frame, summary, samples);
}

fn render_dash_gauges(frame: &mut Frame, area: Rect, samples: &Samples) {
    let [cpu_area, mem_area, swap_area] = Layout::horizontal([
        Constraint::Percentage(34),
        Constraint::Percentage(33),
        Constraint::Percentage(33),
    ])
    .areas(area);

    let g = Gauge::default()
        .gauge_style(STYLE_GREEN)
        .percent((samples.cpu_usage.min(100.0)) as u16)
        .label(format!("CPU: {:.1}%", samples.cpu_usage.min(100.0)));
    frame.render_widget(&g, cpu_area);

    let mem_pct_f = app::pct(samples.mem_used, samples.mem_total.max(1)).min(100.0);
    let mem_pct = mem_pct_f as u16;
    let mem_used_gb = samples.mem_used as f64 / 1_073_741_824.0;
    let mem_total_gb = samples.mem_total as f64 / 1_073_741_824.0;
    let mem_g = Gauge::default()
        .gauge_style(STYLE_CYAN)
        .percent(mem_pct)
        .label(format!(
            "Mem: {mem_pct_f:.1}%  {mem_used_gb:.1}/{mem_total_gb:.1} GiB"
        ));
    frame.render_widget(&mem_g, mem_area);

    let (swap_pct, swap_label) = if samples.swap_total > 0 {
        let swap_pct_val = app::pct(samples.swap_used, samples.swap_total).min(100.0);
        let used_gb = samples.swap_used as f64 / 1_073_741_824.0;
        let total_gb = samples.swap_total as f64 / 1_073_741_824.0;
        (
            swap_pct_val as u16,
            format!("Swap: {swap_pct_val:.1}%  {used_gb:.1}/{total_gb:.1} GiB"),
        )
    } else {
        (0, "Swap: N/A".to_string())
    };
    let swap_g = Gauge::default()
        .gauge_style(STYLE_YELLOW)
        .percent(swap_pct)
        .label(swap_label);
    frame.render_widget(&swap_g, swap_area);
}

fn render_dash_load(frame: &mut Frame, area: Rect, samples: &Samples) {
    let l = Paragraph::new(Line::from(vec![
        Span::styled("Load Average  ", Style::new().bold()),
        Span::raw(format!(
            "{:.2} (1m)  {:.2} (5m)  {:.2} (15m)",
            samples.load_one, samples.load_five, samples.load_fifteen,
        )),
    ]))
    .alignment(Alignment::Center)
    .gray();
    frame.render_widget(&l, area);
}

fn render_dash_summary(frame: &mut Frame, area: Rect, samples: &Samples) {
    let s = Paragraph::new(Line::from(vec![
        Span::styled("Net ", Style::new().bold()),
        Span::raw(format!(
            "TX {}  RX {}",
            format_bytes(samples.net_tx_rate, false).trim(),
            format_bytes(samples.net_rx_rate, false).trim(),
        )),
        Span::raw("  "),
        Span::styled("Disk ", Style::new().bold()),
        Span::raw(format!(
            "R {}  W {}",
            format_bytes(samples.disk_read_rate, false).trim(),
            format_bytes(samples.disk_write_rate, false).trim(),
        )),
    ]))
    .alignment(Alignment::Center)
    .gray();
    frame.render_widget(&s, area);
}

fn format_bytes(bytes: u64, binary: bool) -> String {
    let b = bytes as f64;
    if binary {
        if b >= 1_099_511_627_776.0 {
            format!("{:.1}TiB", b / 1_099_511_627_776.0)
        } else if b >= 1_073_741_824.0 {
            format!("{:.1}GiB", b / 1_073_741_824.0)
        } else if b >= 1_048_576.0 {
            format!("{:.1}MiB", b / 1_048_576.0)
        } else {
            format!("{bytes}B")
        }
    } else if b >= 1_000_000_000_000.0 {
        format!("{:.1}TB", b / 1_000_000_000_000.0)
    } else if b >= 1_000_000_000.0 {
        format!("{:.1}GB", b / 1_000_000_000.0)
    } else if b >= 1_000_000.0 {
        format!("{:.1}MB", b / 1_000_000.0)
    } else if b >= 1000.0 {
        format!("{:.1}KB", b / 1000.0)
    } else {
        format!("{bytes}B")
    }
}

fn format_memory_label(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.1}GiB", bytes as f64 / 1_073_741_824.0)
    } else {
        format!("{:.0}MiB", bytes as f64 / 1_048_576.0)
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

/// Days from 1970-01-01 to 2999-12-31 (exclusive upper bound), computed at compile time.
const MAX_CUMULATIVE: u64 = {
    let mut d = 0u64;
    let mut y = 1970u64;
    while y <= 2999 {
        d += 365;
        if y.is_multiple_of(4) && !y.is_multiple_of(100) || y.is_multiple_of(400) {
            d += 1;
        }
        y += 1;
    }
    d - 1
};

fn format_timestamp(secs: u64) -> String {
    let days_raw = secs / 86400;
    let rem = secs % 86400;
    let hour = rem / 3600;
    let min = (rem % 3600) / 60;
    let sec = rem % 60;

    let day = days_raw.min(MAX_CUMULATIVE);

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
    let q_bytes = q.as_bytes();
    processes
        .iter()
        .filter(|p| {
            if query_pid.is_some_and(|pid| p.pid == pid) {
                return true;
            }
            p.name
                .as_bytes()
                .windows(q_bytes.len())
                .any(|w| w.eq_ignore_ascii_case(q_bytes))
        })
        .collect()
}

fn sort_processes(filtered: &mut [&ProcessInfo], field: ProcSortField, asc: bool) {
    filtered.sort_unstable_by(|a, b| {
        let ord = match field {
            ProcSortField::Name => a.name.cmp(&b.name),
            ProcSortField::Pid => a.pid.cmp(&b.pid),
            ProcSortField::Cpu => a.cpu.total_cmp(&b.cpu),
            ProcSortField::Memory => a.memory.cmp(&b.memory),
            ProcSortField::VirtualMemory => a.virtual_memory.cmp(&b.virtual_memory),
            ProcSortField::RunTime => a.run_time.cmp(&b.run_time),
            ProcSortField::Status => a.status.cmp(&b.status),
        };
        if asc { ord } else { ord.reverse() }
    });
}

#[expect(clippy::too_many_arguments)]
fn render_filtered_table<'a, T>(
    frame: &mut Frame,
    area: Rect,
    state: &mut TabState,
    query: &str,
    items: &'a [T],
    title: impl Fn(usize, usize) -> String,
    empty_msg: &str,
    column_widths: &[Constraint],
    headers: &[&str],
    filter_fn: impl Fn(&T, &str) -> bool,
    row_fn: impl Fn(&'a T) -> Row<'a>,
) -> bool {
    let has_query = !query.is_empty();

    let filtered: Vec<&T> = if has_query {
        let q = query.to_lowercase();
        items.iter().filter(|item| filter_fn(item, &q)).collect()
    } else {
        items.iter().collect()
    };

    let count = filtered.len();
    state.selection = state.selection.min(count.saturating_sub(1));

    if count == 0 && has_query {
        let p = Paragraph::new(empty_msg)
            .fg(Color::DarkGray)
            .alignment(Alignment::Center);
        frame.render_widget(p, area);
        return false;
    }

    let (start, end) = clamp_scroll(state.selection, &mut state.scroll, count, area.height);
    let visible = &filtered[start..end];
    let rel_sel = state.selection.saturating_sub(start);

    let rows: Vec<Row> = visible
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let is_selected = i == rel_sel;
            let style = if is_selected {
                STYLE_SELECTED
            } else {
                Style::default()
            };
            row_fn(item).style(style)
        })
        .collect();

    let table = Table::new(rows, column_widths)
        .header(Row::new(headers.iter().map(|h| Cell::from(*h))))
        .block(Block::bordered().title(title(count, items.len())));
    frame.render_widget(table, area);
    true
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
                    STYLE_DARK_GRAY,
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

fn render_info_block(frame: &mut Frame, area: Rect, items: &[(&str, String)]) {
    let [_, info, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(items.len() as u16),
        Constraint::Fill(1),
    ])
    .areas(area);

    let lines: Vec<Line> = items
        .iter()
        .map(|(label, value)| {
            Line::from(vec![
                Span::styled(*label, Style::new().bold()),
                Span::raw(value),
            ])
        })
        .collect();
    frame.render_widget(Paragraph::new(lines).fg(Color::Gray), info);
}

fn render_time(frame: &mut Frame, area: Rect, _app: &mut App, samples: &Samples) {
    render_info_block(
        frame,
        area,
        &[
            ("Hostname    ", samples.sys_info.hostname.clone()),
            ("OS          ", samples.sys_info.os.clone()),
            ("Kernel      ", samples.sys_info.kernel.clone()),
            ("Arch        ", samples.sys_info.arch.clone()),
            ("Uptime      ", format_uptime(samples.sys_info.uptime)),
            ("CPUs        ", format!("{}", samples.sys_info.cpu_count)),
            ("Distro      ", samples.sys_info.distro.clone()),
            ("Boot        ", format_timestamp(samples.sys_info.boot_time)),
            (
                "Phys Cores  ",
                format!("{}", samples.sys_info.physical_cores),
            ),
        ],
    );
}

fn render_mem(frame: &mut Frame, area: Rect, app: &mut App, samples: &Samples) {
    let mem_total_gb = samples.mem_total as f64 / 1_073_741_824.0;
    let mem_used_gb = samples.mem_used as f64 / 1_073_741_824.0;
    let mem_avail_gb = samples.mem_available as f64 / 1_073_741_824.0;
    let mem_free_gb = samples.mem_free as f64 / 1_073_741_824.0;

    let mem_used_pct = app::pct(samples.mem_used, samples.mem_total);
    let mem_avail_pct = app::pct(samples.mem_available, samples.mem_total);
    let mem_free_pct = app::pct(samples.mem_free, samples.mem_total);

    let swap_total_gb = samples.swap_total as f64 / 1_073_741_824.0;
    let swap_used_gb = samples.swap_used as f64 / 1_073_741_824.0;
    let swap_free = samples.swap_total.saturating_sub(samples.swap_used);
    let swap_free_gb = swap_free as f64 / 1_073_741_824.0;

    let swap_used_pct = app::pct(samples.swap_used, samples.swap_total);
    let swap_free_pct = app::pct(swap_free, samples.swap_total);

    let [_, info, mem_spark, swap_spark, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(8),
        Constraint::Length(SPARKLINE_HEIGHT),
        Constraint::Length(SPARKLINE_HEIGHT),
        Constraint::Fill(1),
    ])
    .areas(area);

    render_info_block(
        frame,
        info,
        &[
            ("Memory      ", format!("{mem_total_gb:.1} GiB")),
            ("Used        ", format_memory(mem_used_gb, mem_used_pct)),
            ("Available   ", format_memory(mem_avail_gb, mem_avail_pct)),
            ("Free        ", format_memory(mem_free_gb, mem_free_pct)),
            ("", String::new()),
            ("Swap        ", format!("{swap_total_gb:.1} GiB")),
            ("Used        ", format_memory(swap_used_gb, swap_used_pct)),
            ("Free        ", format_memory(swap_free_gb, swap_free_pct)),
        ],
    );

    render_sparkline(
        frame,
        mem_spark,
        SPARK_MEM_HISTORY_TITLE,
        &app.mem_history,
        SPARK_MEM_HISTORY_COLOR,
    );
    render_sparkline(
        frame,
        swap_spark,
        SPARK_SWAP_TITLE,
        &app.swap_history,
        SPARK_SWAP_COLOR,
    );
}

fn render_temp(frame: &mut Frame, area: Rect, app: &mut App, samples: &Samples) {
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
                Cell::from(Span::styled(format_temp(t.temperature), STYLE_RED)),
                Cell::from(format_temp(t.max)),
                Cell::from(format_temp(t.critical)),
            ])
        })
        .collect();

    let [table_area, spark_area] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(SPARKLINE_HEIGHT)]).areas(area);

    let table = Table::new(rows, widths)
        .header(Row::new(vec!["Sensor", "Temp", "Max", "Critical"]))
        .block(Block::bordered().title(" Temperature "));
    frame.render_widget(table, table_area);

    render_sparkline(
        frame,
        spark_area,
        SPARK_TEMP_TITLE,
        &app.temp_history,
        SPARK_TEMP_COLOR,
    );
}

fn render_cores(frame: &mut Frame, area: Rect, _app: &mut App, samples: &Samples) {
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
    let content_area =
        render_search_bar(frame, area, &app.files_state.query, app.files_state.focused);

    let [table_area, spark_area] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(SPARKLINE_HEIGHT)])
            .areas(content_area);

    let query = app.files_state.query.clone();
    if !render_filtered_table(
        frame,
        table_area,
        &mut app.files_state,
        &query,
        &samples.disks,
        |count, total| format!(" Filesystems ({count}/{total}) "),
        "No matching filesystems",
        &[
            Constraint::Length(14),
            Constraint::Fill(1),
            Constraint::Length(6),
            Constraint::Length(9),
            Constraint::Length(9),
            Constraint::Length(6),
            Constraint::Length(7),
        ],
        &["Device", "Mount", "FS", "Size", "Avail", "Use%", "Kind"],
        |d: &DiskInfo, q| {
            d.mount_point.to_lowercase().contains(q) || d.device.to_lowercase().contains(q)
        },
        |d: &DiskInfo| {
            Row::new(vec![
                Cell::from(d.device.as_str()),
                Cell::from(d.mount_point.as_str()),
                Cell::from(d.fs.as_str()),
                Cell::from(Span::styled(format_bytes(d.total, true), STYLE_MAGENTA)),
                Cell::from(Span::styled(format_bytes(d.available, true), STYLE_MAGENTA)),
                Cell::from(Span::styled(format!("{:.1}%", d.usage_pct), STYLE_MAGENTA)),
                Cell::from(d.kind.as_str()),
            ])
        },
    ) {
        return;
    }

    render_sparkline(
        frame,
        spark_area,
        SPARK_USAGE_TITLE,
        &app.disk_usage_history,
        SPARK_USAGE_COLOR,
    );
}

fn render_disk(frame: &mut Frame, area: Rect, app: &mut App, samples: &Samples) {
    let [table_area, read_spark, write_spark] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(SPARKLINE_HEIGHT),
        Constraint::Length(SPARKLINE_HEIGHT),
    ])
    .areas(area);

    let rows: Vec<Row> = samples
        .disk_io
        .iter()
        .map(|d| {
            Row::new(vec![
                Cell::from(d.mount_point.as_str()),
                Cell::from(Span::styled(format_bytes(d.read_rate, false), STYLE_CYAN)),
                Cell::from(Span::styled(
                    format_bytes(d.write_rate, false),
                    STYLE_YELLOW,
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

    render_sparkline(
        frame,
        read_spark,
        SPARK_DISK_READ_TITLE,
        &app.disk_read_history,
        SPARK_DISK_READ_COLOR,
    );
    render_sparkline(
        frame,
        write_spark,
        SPARK_DISK_WRITE_TITLE,
        &app.disk_write_history,
        SPARK_DISK_WRITE_COLOR,
    );
}

fn render_net(frame: &mut Frame, area: Rect, app: &mut App, samples: &Samples) {
    let content_area = render_search_bar(frame, area, &app.net_state.query, app.net_state.focused);

    let [table_area, rx_spark, tx_spark] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(SPARKLINE_HEIGHT),
        Constraint::Length(SPARKLINE_HEIGHT),
    ])
    .areas(content_area);

    let query = app.net_state.query.clone();
    if !render_filtered_table(
        frame,
        table_area,
        &mut app.net_state,
        &query,
        &samples.interfaces,
        |count, total| format!(" Network I/O ({count}/{total}) "),
        "No matching interfaces",
        &[
            Constraint::Fill(1),
            Constraint::Length(12),
            Constraint::Length(12),
            Constraint::Length(8),
            Constraint::Length(17),
            Constraint::Fill(1),
        ],
        &["Interface", "RX", "TX", "State", "MAC", "IP"],
        |i: &NetInfo, q| i.name.to_lowercase().contains(q),
        |i: &NetInfo| {
            Row::new(vec![
                Cell::from(i.name.as_str()),
                Cell::from(Span::styled(format_bytes(i.rx_bytes, false), STYLE_YELLOW)),
                Cell::from(Span::styled(format_bytes(i.tx_bytes, false), STYLE_YELLOW)),
                Cell::from(i.state.as_str()),
                Cell::from(i.mac.as_str()),
                Cell::from(i.ip.as_str()),
            ])
        },
    ) {
        return;
    }

    render_sparkline(
        frame,
        rx_spark,
        SPARK_NET_RX_TITLE,
        &app.net_rx_history,
        SPARK_NET_RX_COLOR,
    );
    render_sparkline(
        frame,
        tx_spark,
        SPARK_NET_TX_TITLE,
        &app.net_tx_history,
        SPARK_NET_TX_COLOR,
    );
}

fn sort_arrow(field: ProcSortField, sort_field: ProcSortField, asc: bool) -> &'static str {
    if field != sort_field {
        return "";
    }
    if asc { "\u{2191}" } else { "\u{2193}" }
}

fn render_sparkline(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    data: &VecDeque<u64>,
    color: Color,
) {
    let s = Sparkline::default()
        .block(Block::bordered().title(title))
        .data(data.iter())
        .style(Style::new().fg(color));
    frame.render_widget(&s, area);
}

fn render_overlay(frame: &mut Frame, area: Rect, lines: Vec<Line>, title: &str, min_width: u16) {
    let height = lines.len() as u16 + 2;
    let [_, inner, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(height),
        Constraint::Fill(1),
    ])
    .areas(area);

    let [_, centered, _] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Min(min_width),
        Constraint::Fill(1),
    ])
    .areas(inner);

    let p = Paragraph::new(lines)
        .block(Block::bordered().title(format!(" {title} ")))
        .fg(Color::Gray);
    frame.render_widget(&p, centered);
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

    render_overlay(frame, area, lines, "Help", 48);
}

struct StatusBar;

const HINT_SEP: &str = " │ ";

impl StatusBar {
    fn ctx_string(app: &App) -> String {
        if let Some(ref fb) = app.kill_feedback {
            return fb.clone();
        }
        if app.paused {
            return "PAUSED (Space to resume)".to_owned();
        }
        if app.help_visible {
            return "Help (? to close)".to_owned();
        }
        if app.kill_state == Some(KillState::Confirm) {
            let pid = app.selected_pid();
            let name = app.selected_name();
            return format!("Kill? PID {pid} ({name})");
        }
        if let Some(ref err) = app.error_msg {
            return format!("Error: {err}");
        }
        let label = app.active_tab.label();
        let sort = if app.active_tab.is_proc() {
            let arrow = if app.proc_sort_asc { "↑" } else { "↓" };
            Some(format!(" [{} {}{}]", label, app.proc_sort_field, arrow))
        } else {
            None
        };
        sort.unwrap_or_else(|| label.to_owned())
    }

    fn status_hints(app: &App) -> Vec<String> {
        if app.kill_feedback.is_some() || app.paused || app.help_visible || app.error_msg.is_some()
        {
            return vec![];
        }
        if app.kill_state == Some(KillState::Confirm) {
            let sig_hints: Vec<String> = app::KILL_SIGNAL_MAP
                .iter()
                .take(2)
                .map(|(k, label, _)| format!("{k} {label}"))
                .collect();
            return vec![
                sig_hints[0].clone(),
                sig_hints[1].clone(),
                format!(
                    "{}-{} More",
                    app::KILL_SIGNAL_MAP[2].0,
                    app::KILL_SIGNAL_MAP.last().unwrap().0
                ),
                "any Cancel".to_owned(),
            ];
        }

        let mut hints: Vec<String> = Vec::with_capacity(3);

        if app.tab_orientation.is_horizontal() {
            let label = if app.tab_bar_visible { "Hide" } else { "Show" };
            hints.push(format!("Ctrl+S {label} Tab Bar"));
        } else {
            let label = if app.sidebar_visible { "Hide" } else { "Show" };
            hints.push(format!("Ctrl+S {label} Sidebar"));
        }

        if app.active_tab.has_searchable_state() {
            let active_query = app.tab_state().map_or("", |s| s.query.as_str());
            let focused = app.tab_state().is_some_and(|s| s.focused);
            if !focused && active_query.is_empty() {
                hints.push("/ Search".to_owned());
            } else if !active_query.is_empty() {
                hints.push("Esc Clear".to_owned());
            }
        }

        if app.active_tab.is_proc() && app.selected.is_some() {
            hints.push("Delete Kill".to_owned());
            hints.push("Ctrl+K Kill!".to_owned());
        }

        if hints.len() < MAX_HINTS {
            if app.active_tab.is_proc() {
                hints.push("\u{2191}\u{2193} Select".to_owned());
            } else {
                hints.push("1-9 Tab".to_owned());
            }
        }

        if hints.len() > MAX_HINTS {
            hints.truncate(MAX_HINTS);
        }

        hints
    }

    fn display(app: &App) -> (String, String) {
        let ctx = Self::ctx_string(app);
        let hints = Self::status_hints(app);
        if hints.is_empty() {
            (ctx, String::new())
        } else {
            (ctx, hints.join(HINT_SEP))
        }
    }
}

fn render_horizontal_tabs(frame: &mut Frame, area: Rect, app: &App) {
    let mut spans = Vec::with_capacity(Tab::ALL.len() * 2 - 1);
    for (i, tab) in Tab::ALL.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" │ ", STYLE_DARK_GRAY));
        }
        let is_active = tab == &app.active_tab;
        let style = if is_active {
            Style::new().fg(tab.color()).bold()
        } else {
            STYLE_DARK_GRAY
        };
        spans.push(Span::styled(tab.label(), style));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_proc(frame: &mut Frame, area: Rect, app: &mut App, samples: &Samples) {
    let mut filtered = filter_processes(&app.proc_state.query, &samples.processes);
    sort_processes(&mut filtered, app.proc_sort_field, app.proc_sort_asc);

    let count = filtered.len();
    app.proc_state.selection = app.proc_state.selection.min(count.saturating_sub(1));
    app.selected = filtered
        .get(app.proc_state.selection)
        .map(|p| SelectionState {
            pid: p.pid,
            name: p.name.clone(),
        });

    let content_area =
        render_search_bar(frame, area, &app.proc_state.query, app.proc_state.focused);

    if count == 0 && !app.proc_state.query.is_empty() {
        let p = Paragraph::new("No matching processes")
            .fg(Color::DarkGray)
            .alignment(Alignment::Center);
        frame.render_widget(p, content_area);
        return;
    }

    let [table_area, cpu_spark, mem_spark] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(SPARKLINE_HEIGHT),
        Constraint::Length(SPARKLINE_HEIGHT),
    ])
    .areas(content_area);

    let (start, end) = clamp_scroll(
        app.proc_state.selection,
        &mut app.proc_state.scroll,
        count,
        table_area.height,
    );
    let visible = &filtered[start..end];
    let rel_sel = app.proc_state.selection.saturating_sub(start);

    let table = build_proc_table(visible, rel_sel, app, &filtered, samples);
    frame.render_widget(table, table_area);

    render_sparkline(
        frame,
        cpu_spark,
        SPARK_CPU_TITLE,
        &app.cpu_history,
        SPARK_CPU_COLOR,
    );
    render_sparkline(
        frame,
        mem_spark,
        SPARK_MEM_TITLE,
        &app.mem_history,
        SPARK_MEM_COLOR,
    );
}

fn build_proc_table<'a>(
    visible: &[&'a ProcessInfo],
    rel_sel: usize,
    app: &App,
    filtered: &[&ProcessInfo],
    samples: &Samples,
) -> Table<'a> {
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
                STYLE_SELECTED
            } else {
                Style::default()
            };
            let mem_label = format_memory_label(p.memory);
            let virt_label = format_memory_label(p.virtual_memory);
            Row::new(vec![
                Cell::from(p.name.as_str()),
                Cell::from(format!("{}", p.pid)),
                Cell::from(Span::styled(format!("{:.1}", p.cpu), STYLE_GREEN)),
                Cell::from(Span::styled(mem_label, STYLE_CYAN)),
                Cell::from(Span::styled(virt_label, STYLE_MAGENTA)),
                Cell::from(format_uptime(p.run_time)),
                Cell::from(p.status.as_str()),
            ])
            .style(style)
        })
        .collect();

    Table::new(rows, widths)
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
        )))
}

type TabRenderer = fn(&mut Frame, Rect, &mut App, &Samples);

const RENDERERS: [TabRenderer; 9] = [
    render_dash,
    render_proc,
    render_net,
    render_files,
    render_time,
    render_temp,
    render_cores,
    render_disk,
    render_mem,
];

/// Entry point for rendering a single frame: tabs, content, status bar, and overlays.
pub fn draw(frame: &mut Frame, app: &mut App, samples: &Samples) {
    let block = if app.tab_orientation.is_horizontal() || app.sidebar_visible {
        Block::bordered().title(" Thrum ")
    } else {
        Block::bordered().title(format!(" Thrum | {} ", app.active_tab.label()))
    };
    frame.render_widget(&block, frame.area());
    let inner = block.inner(frame.area());

    let content_area = match app.tab_orientation {
        TabOrientation::Sidebar if app.sidebar_visible => {
            let [sidebar_area, content_area] =
                Layout::horizontal([Constraint::Length(app::SIDEBAR_WIDTH), Constraint::Fill(1)])
                    .areas(inner);
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

    RENDERERS[app.active_tab.index()](frame, tab_area, app, samples);

    render_status_bar(frame, status_area, app);
    render_overlays(frame, tab_area, app);
}

fn render_status_bar(frame: &mut Frame, status_area: Rect, app: &App) {
    let (ctx, hints) = StatusBar::display(app);
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
}

fn render_overlays(frame: &mut Frame, tab_area: Rect, app: &mut App) {
    if let Some(fb) = app.kill_feedback.take() {
        render_kill_feedback(frame, tab_area, &fb);
    }

    if app.help_visible {
        render_help(frame, tab_area);
    }

    if app.kill_state == Some(KillState::Confirm) {
        let pid = app.selected_pid();
        let name = app.selected_name();
        render_kill_confirm(frame, tab_area, pid, name);
    }
}

fn render_kill_feedback(frame: &mut Frame, area: Rect, feedback: &str) {
    let lines = vec![
        Line::from(""),
        Line::from(format!("  {feedback}  ")),
        Line::from(""),
    ];

    render_overlay(frame, area, lines, "Kill Result", 30);
}

fn render_kill_confirm(frame: &mut Frame, area: Rect, pid: u32, name: &str) {
    let signal_lines: Vec<Line> = app::KILL_SIGNAL_MAP
        .chunks(2)
        .map(|chunk| {
            let parts: Vec<String> = chunk
                .iter()
                .map(|(k, label, _)| format!("  {k}  {label}"))
                .collect();
            Line::from(parts.join("    "))
        })
        .collect();
    let mut lines = vec![
        Line::from(""),
        Line::from(format!("  Kill PID {pid} ({name})?")),
        Line::from(""),
    ];
    lines.extend(signal_lines);
    lines.push(Line::from("  any  Cancel"));
    lines.push(Line::from(""));

    render_overlay(frame, area, lines, "Kill Process", 42);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_bytes_values() {
        for (input, expected) in [
            (0, "0B"),
            (500, "500B"),
            (1500, "1.5KB"),
            (2_000_000, "2.0MB"),
            (2_000_000_000, "2.0GB"),
            (2_000_000_000_000, "2.0TB"),
        ] {
            assert_eq!(format_bytes(input, false), expected);
        }
    }

    #[test]
    fn format_bytes_large_no_overflow() {
        let result = format_bytes(18_446_744_073_709_551_615, false);
        assert!(
            !result.contains("  "),
            "no extra whitespace padding: {result:?}"
        );
        assert!(result.ends_with("TB"), "largest value is in TB");
    }

    #[test]
    fn format_bytes_binary_values() {
        for (input, expected) in [
            (500, "500B"),
            (1_048_576, "1.0MiB"),
            (1_073_741_824, "1.0GiB"),
            (2 * 1_099_511_627_776, "2.0TiB"),
        ] {
            assert_eq!(format_bytes(input, true), expected);
        }
    }

    #[test]
    fn format_uptime_values() {
        for (input, expected) in [
            (0, "0s"),
            (30, "30s"),
            (59, "59s"),
            (119, "1m 59s"),
            (120, "2m"),
            (150, "2m 30s"),
            (3600, "1h 0m"),
            (3661, "1h 1m"),
            (86400, "1d 0h 0m"),
            (90061, "1d 1h 1m"),
        ] {
            assert_eq!(format_uptime(input), expected);
        }
    }

    #[test]
    fn tab_color_matches_tab() {
        for (tab, expected) in &[
            (Tab::Dash, Color::Green),
            (Tab::Proc, Color::Cyan),
            (Tab::Net, Color::Yellow),
            (Tab::Files, Color::Magenta),
            (Tab::Time, Color::Gray),
            (Tab::Temp, Color::Red),
            (Tab::Cores, Color::Blue),
            (Tab::Disk, Color::White),
            (Tab::Mem, Color::LightBlue),
        ] {
            assert_eq!(tab.color(), *expected);
        }
    }

    #[test]
    fn format_temp_values() {
        for (input, expected) in [
            (Some(65.0), "   65.0°C"),
            (Some(0.0), "    0.0°C"),
            (None, "      N/A"),
            (Some(f32::NAN), "      N/A"),
            (Some(100.5), "  100.5°C"),
        ] {
            assert_eq!(format_temp(input), expected);
        }
    }

    fn test_procs() -> Vec<ProcessInfo> {
        vec![
            ProcessInfo {
                name: "firefox".into(),
                pid: 100,
                cpu: 0.0,
                memory: 0,
                virtual_memory: 0,
                run_time: 0,
                status: "Running".into(),
            },
            ProcessInfo {
                name: "bash".into(),
                pid: 200,
                cpu: 0.0,
                memory: 0,
                virtual_memory: 0,
                run_time: 0,
                status: "Sleep".into(),
            },
        ]
    }

    #[test]
    fn filter_processes_queries() {
        let procs = test_procs();
        for (query, expected_len, expected_pid) in [
            ("fire", 1, Some(100)),
            ("200", 1, Some(200)),
            ("", 2, None),
            ("100", 1, Some(100)),
        ] {
            let result = filter_processes(query, &procs);
            assert_eq!(result.len(), expected_len, "query '{query}'");
            if let Some(pid) = expected_pid {
                assert_eq!(result[0].pid, pid, "query '{query}'");
            }
        }
    }

    #[test]
    fn filter_processes_case_insensitive() {
        let procs = test_procs();
        for (query, expected_pid) in [("FIRE", 100), ("BASH", 200)] {
            let result = filter_processes(query, &procs);
            assert_eq!(result.len(), 1, "query '{query}'");
            assert_eq!(result[0].pid, expected_pid, "query '{query}'");
        }
    }

    #[test]
    fn status_bar_search_hint_in_hints() {
        let mut app = App::new();
        for tab in [Tab::Proc, Tab::Net, Tab::Files] {
            app.active_tab = tab;
            let (_, hints) = StatusBar::display(&app);
            assert!(
                hints.contains("/ Search"),
                "{tab:?} should show search hint",
            );
        }
        app.active_tab = Tab::Dash;
        let (_, hints) = StatusBar::display(&app);
        assert!(
            !hints.contains("/ Search"),
            "Dash should not show search hint"
        );
    }

    #[test]
    fn status_bar_search_hidden_when_focused() {
        let mut app = App::new();
        app.active_tab = Tab::Proc;
        app.proc_state.focused = true;
        let (_, hints) = StatusBar::display(&app);
        assert!(
            !hints.contains("/ Search"),
            "search hint hidden when focused"
        );
    }

    #[test]
    fn status_bar_kill_hints_only_with_pid_and_proc() {
        let mut app = App::new();
        app.active_tab = Tab::Proc;
        app.selected = Some(SelectionState {
            pid: 42,
            name: "bash".to_owned(),
        });
        let (_, hints) = StatusBar::display(&app);
        assert!(hints.contains("Delete Kill"), "kill hint with PID on Proc");
        assert!(
            hints.contains("Ctrl+K Kill!"),
            "Ctrl+K hint with PID on Proc"
        );

        app.selected = None;
        let (_, hints) = StatusBar::display(&app);
        assert!(!hints.contains("Delete Kill"), "no kill hint without PID");
    }

    #[test]
    fn status_bar_sidebar_hints_match() {
        let mut app = App::new();
        app.sidebar_visible = true;
        let (_, hints) = StatusBar::display(&app);
        assert!(hints.contains("Hide"), "should say 'Hide' when visible");
        app.sidebar_visible = false;
        let (_, hints) = StatusBar::display(&app);
        assert!(hints.contains("Show"), "should say 'Show' when hidden");
    }

    #[test]
    fn status_bar_ctx_shows_tab_name() {
        let app = App::new();
        let (ctx, _) = StatusBar::display(&app);
        assert_eq!(ctx, "Dash", "default ctx is tab label");
    }

    #[test]
    fn status_bar_ctx_help_mode() {
        let mut app = App::new();
        app.help_visible = true;
        let (ctx, _) = StatusBar::display(&app);
        assert_eq!(ctx, "Help (? to close)");
    }

    #[test]
    fn status_bar_ctx_kill_confirm() {
        let mut app = App::new();
        app.kill_state = Some(KillState::Confirm);
        app.selected = Some(SelectionState {
            pid: 42,
            name: "bash".to_owned(),
        });
        let (ctx, _) = StatusBar::display(&app);
        assert_eq!(ctx, "Kill? PID 42 (bash)");
    }

    #[test]
    fn status_bar_ctx_kill_feedback() {
        let mut app = App::new();
        app.kill_feedback = Some("Killed PID 42".to_owned());
        let (ctx, _) = StatusBar::display(&app);
        assert_eq!(ctx, "Killed PID 42");
    }

    #[test]
    fn status_bar_hints_max_four() {
        let mut app = App::new();
        app.active_tab = Tab::Proc;
        app.selected = Some(SelectionState {
            pid: 42,
            name: String::new(),
        });
        let (_, hints) = StatusBar::display(&app);
        let count = hints.matches(HINT_SEP).count() + 1;
        assert!(
            count <= MAX_HINTS,
            "at most {MAX_HINTS} hints, got {count}: {hints}"
        );
    }

    #[test]
    fn status_bar_no_quit_or_help_hints() {
        let app = App::new();
        let (_, hints) = StatusBar::display(&app);
        assert!(!hints.contains("q/Ctrl+C"), "quit hint removed");
        assert!(!hints.contains("? Help"), "help hint removed");
    }

    #[test]
    fn format_timestamp_values() {
        for (input, expected) in [
            (0, "1970-01-01 00:00:00"),
            (1, "1970-01-01 00:00:01"),
            (2_678_399, "1970-01-31 23:59:59"),
            (2_678_400, "1970-02-01 00:00:00"),
            (5_097_599, "1970-02-28 23:59:59"),
            (5_097_600, "1970-03-01 00:00:00"),
            (31_536_000, "1971-01-01 00:00:00"),
            (68_256_000, "1972-03-01 00:00:00"),
        ] {
            assert_eq!(format_timestamp(input), expected);
        }
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
    fn format_timestamp_leap_year_2000() {
        assert_eq!(format_timestamp(951_782_400), "2000-02-29 00:00:00");
    }

    #[test]
    fn format_timestamp_century_non_leap_2100() {
        assert_eq!(format_timestamp(4_107_542_400), "2100-03-01 00:00:00");
    }

    // --- Status bar paused ---

    #[test]
    fn status_bar_ctx_paused() {
        let mut app = App::new();
        app.paused = true;
        let (ctx, _) = StatusBar::display(&app);
        assert_eq!(ctx, "PAUSED (Space to resume)");
    }

    // --- Status bar tab-specific hints ---

    #[test]
    fn status_bar_proc_shows_select_hint() {
        let mut app = App::new();
        app.active_tab = Tab::Proc;
        let (_, hints) = StatusBar::display(&app);
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
            let (_, hints) = StatusBar::display(&app);
            assert!(hints.contains("1-9 Tab"), "{tab:?} shows 1-9 Tab hint");
        }
    }

    // --- Status bar Esc Clear hint ---

    #[test]
    fn status_bar_esc_clear_when_filter_active() {
        let mut app = App::new();
        app.active_tab = Tab::Proc;
        app.proc_state.query = "fire".to_owned();
        let (_, hints) = StatusBar::display(&app);
        assert!(
            hints.contains("Esc Clear"),
            "shows Esc Clear when filter active"
        );
        assert!(
            !hints.contains("/ Search"),
            "no search hint when filter active"
        );

        app.active_tab = Tab::Net;
        app.net_state.query = "eth".to_owned();
        let (_, hints) = StatusBar::display(&app);
        assert!(hints.contains("Esc Clear"), "Esc Clear on Net with filter");
        assert!(
            !hints.contains("/ Search"),
            "no search hint on Net with filter"
        );
    }

    #[test]
    fn status_bar_hints_keep_kill_with_tab_hint() {
        let mut app = App::new();
        app.active_tab = Tab::Proc;
        app.selected = Some(SelectionState {
            pid: 42,
            name: "bash".to_owned(),
        });
        let (_, hints) = StatusBar::display(&app);
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
        let (ctx, hints) = StatusBar::display(&app);
        assert_eq!(ctx, "Error: sampling failed");
        assert!(hints.is_empty(), "no hints when showing error");
    }

    #[test]
    fn status_bar_paused_takes_priority_over_error() {
        let mut app = App::new();
        app.error_msg = Some("disk error".to_owned());
        app.paused = true;
        let (ctx, _) = StatusBar::display(&app);
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
        app.selected = Some(SelectionState {
            pid: 42,
            name: "bash".to_owned(),
        });
        let (ctx, _) = StatusBar::display(&app);
        assert_eq!(ctx, "Kill? PID 42 (bash)", "kill confirm wins over error");
    }

    #[test]
    fn clamp_scroll_noop_when_selection_visible() {
        let mut scroll = 5;
        let (start, end) = clamp_scroll(7, &mut scroll, 20, 10);
        assert_eq!(scroll, 5, "scroll unchanged");
        assert_eq!(start, 5);
        assert_eq!(end, 12);
    }

    #[test]
    fn clamp_scroll_moves_scroll_up_when_selection_above() {
        let mut scroll = 10;
        let (start, end) = clamp_scroll(3, &mut scroll, 20, 10);
        assert_eq!(scroll, 3, "scroll moves to selection");
        assert_eq!(start, 3);
        assert_eq!(end, 10);
    }

    #[test]
    fn clamp_scroll_moves_scroll_down_when_selection_below() {
        let mut scroll = 0;
        let (start, end) = clamp_scroll(15, &mut scroll, 20, 10);
        assert_eq!(scroll, 10, "scroll moves to reveal selection");
        assert_eq!(start, 10);
        assert_eq!(end, 17);
    }

    #[test]
    fn clamp_scroll_with_zero_height_avoids_underflow() {
        let mut scroll = 0;
        // height=4 => vis=0 => no scroll adjustment; max_visible=1
        let (start, end) = clamp_scroll(5, &mut scroll, 10, 4);
        assert_eq!(scroll, 0, "zero vis skips scroll adjustment");
        assert_eq!(start, 0);
        assert_eq!(end, 1);
    }

    #[test]
    fn clamp_scroll_clamps_to_count() {
        let mut scroll = 100;
        let (start, end) = clamp_scroll(5, &mut scroll, 10, 10);
        assert_eq!(scroll, 5, "scroll capped at count-1");
        assert_eq!(start, 5);
        assert_eq!(end, 10);
    }

    #[test]
    fn format_memory_label_shows_gib_above_threshold() {
        assert_eq!(format_memory_label(2_147_483_648), "2.0GiB");
        assert_eq!(format_memory_label(1_073_741_824), "1.0GiB");
        assert_eq!(format_memory_label(5_368_709_120), "5.0GiB");
    }

    #[test]
    fn format_memory_label_shows_mib_below_threshold() {
        assert_eq!(format_memory_label(0), "0MiB");
        assert_eq!(format_memory_label(1_048_576), "1MiB");
        assert_eq!(format_memory_label(524_288_000), "500MiB");
    }

    #[test]
    fn format_memory_label_edge_boundary() {
        assert_eq!(format_memory_label(1_073_741_823), "1024MiB");
        assert_eq!(format_memory_label(1_073_741_824), "1.0GiB");
    }

    #[test]
    fn format_memory_displays_gib_and_pct() {
        assert_eq!(format_memory(2.5, 45.0), "2.5 GiB  45.0%");
        assert_eq!(format_memory(0.0, 0.0), "0.0 GiB  0.0%");
        assert_eq!(format_memory(15.9, 100.0), "15.9 GiB  100.0%");
    }

    #[test]
    fn sort_processes_sorts_by_name_ascending() {
        let procs = test_procs();
        let mut refs: Vec<&ProcessInfo> = procs.iter().collect();
        sort_processes(&mut refs, ProcSortField::Name, true);
        assert_eq!(refs[0].name, "bash");
        assert_eq!(refs[1].name, "firefox");
    }

    #[test]
    fn sort_processes_sorts_by_name_descending() {
        let procs = test_procs();
        let mut refs: Vec<&ProcessInfo> = procs.iter().collect();
        sort_processes(&mut refs, ProcSortField::Name, false);
        assert_eq!(refs[0].name, "firefox");
        assert_eq!(refs[1].name, "bash");
    }

    #[test]
    fn sort_processes_sorts_by_pid() {
        let procs = test_procs();
        let mut refs: Vec<&ProcessInfo> = procs.iter().collect();
        sort_processes(&mut refs, ProcSortField::Pid, true);
        assert_eq!(refs[0].pid, 100);
        assert_eq!(refs[1].pid, 200);
    }

    #[test]
    fn sort_processes_sorts_by_pid_descending() {
        let procs = test_procs();
        let mut refs: Vec<&ProcessInfo> = procs.iter().collect();
        sort_processes(&mut refs, ProcSortField::Pid, false);
        assert_eq!(refs[0].pid, 200);
        assert_eq!(refs[1].pid, 100);
    }

    fn test_procs_diverse() -> Vec<ProcessInfo> {
        vec![
            ProcessInfo {
                name: "bash".into(),
                pid: 1,
                cpu: 1.0,
                memory: 50,
                virtual_memory: 200,
                run_time: 100,
                status: "Sleep".into(),
            },
            ProcessInfo {
                name: "firefox".into(),
                pid: 2,
                cpu: 50.0,
                memory: 1000,
                virtual_memory: 5000,
                run_time: 500,
                status: "Running".into(),
            },
            ProcessInfo {
                name: "chrome".into(),
                pid: 3,
                cpu: 30.0,
                memory: 500,
                virtual_memory: 3000,
                run_time: 300,
                status: "Running".into(),
            },
            ProcessInfo {
                name: "sshd".into(),
                pid: 4,
                cpu: 0.5,
                memory: 10,
                virtual_memory: 50,
                run_time: 2000,
                status: "Idle".into(),
            },
        ]
    }

    #[test]
    fn sort_processes_sorts_by_cpu_ascending() {
        let procs = test_procs_diverse();
        let mut refs: Vec<&ProcessInfo> = procs.iter().collect();
        sort_processes(&mut refs, ProcSortField::Cpu, true);
        assert_eq!(refs[0].pid, 4);
        assert_eq!(refs[1].pid, 1);
        assert_eq!(refs[2].pid, 3);
        assert_eq!(refs[3].pid, 2);
    }

    #[test]
    fn sort_processes_sorts_by_memory_ascending() {
        let procs = test_procs_diverse();
        let mut refs: Vec<&ProcessInfo> = procs.iter().collect();
        sort_processes(&mut refs, ProcSortField::Memory, true);
        assert_eq!(refs[0].pid, 4);
        assert_eq!(refs[1].pid, 1);
        assert_eq!(refs[2].pid, 3);
        assert_eq!(refs[3].pid, 2);
    }

    #[test]
    fn sort_processes_sorts_by_virtual_memory_ascending() {
        let procs = test_procs_diverse();
        let mut refs: Vec<&ProcessInfo> = procs.iter().collect();
        sort_processes(&mut refs, ProcSortField::VirtualMemory, true);
        assert_eq!(refs[0].pid, 4);
        assert_eq!(refs[1].pid, 1);
        assert_eq!(refs[2].pid, 3);
        assert_eq!(refs[3].pid, 2);
    }

    #[test]
    fn sort_processes_sorts_by_run_time_ascending() {
        let procs = test_procs_diverse();
        let mut refs: Vec<&ProcessInfo> = procs.iter().collect();
        sort_processes(&mut refs, ProcSortField::RunTime, true);
        assert_eq!(refs[0].pid, 1);
        assert_eq!(refs[1].pid, 3);
        assert_eq!(refs[2].pid, 2);
        assert_eq!(refs[3].pid, 4);
    }

    #[test]
    fn sort_processes_sorts_by_status_ascending() {
        let procs = test_procs_diverse();
        let mut refs: Vec<&ProcessInfo> = procs.iter().collect();
        sort_processes(&mut refs, ProcSortField::Status, true);
        assert_eq!(refs[0].pid, 4);
        assert_eq!(refs[1].pid, 2);
        assert_eq!(refs[2].pid, 3);
        assert_eq!(refs[3].pid, 1);
    }
}
