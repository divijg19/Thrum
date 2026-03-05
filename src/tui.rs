//! Terminal UI rendering: tab dispatch, widgets, and per-tab render functions.
//!
//! Entry point: [`draw`]. Each tab implements [`TabWidget`] for dispatch via
//! [`RENDERERS`]. Shared UI utilities (formatting, layout constants, styles,
//! status bar) live in [`crate::ui`].

use std::borrow::Cow;

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Cell, Gauge, Paragraph, Row, Table};

use crate::app::{self, App, ProcSortField, SelectionState, Tab, TabOrientation, TabState};
use crate::samplers::{DiskInfo, NetInfo, ProcessInfo, Samples};
use crate::ui::*;

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

/// Filters the process list by name (substring, case-insensitive) or PID.
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

/// Sorts a slice of process references by the given field and direction.
fn sort_processes(filtered: &mut [&ProcessInfo], field: ProcSortField, asc: bool) {
    filtered.sort_unstable_by(|a, b| {
        let ord = match field {
            ProcSortField::Name => a.name.cmp(&b.name),
            ProcSortField::Pid => a.pid.cmp(&b.pid),
            ProcSortField::Cpu => a.cpu.total_cmp(&b.cpu),
            ProcSortField::Memory => a.memory.cmp(&b.memory),
            ProcSortField::VirtualMemory => a.virtual_memory.cmp(&b.virtual_memory),
            ProcSortField::RunTime => a.run_time.cmp(&b.run_time),
            ProcSortField::Status => a.status.cmp(b.status),
        };
        if asc { ord } else { ord.reverse() }
    });
}

/// Renders a filterable, scrollable table with search, selection, and pagination.
/// Returns `true` when items exist (or query is empty), `false` when filtered to zero.
#[expect(clippy::too_many_arguments)]
fn render_filtered_table<'a, T>(
    frame: &mut Frame,
    area: Rect,
    state: &mut TabState,
    items: &'a [T],
    title: impl Fn(usize, usize) -> String,
    empty_msg: &str,
    column_widths: &[Constraint],
    headers: &[&str],
    filter_fn: impl Fn(&T, &str) -> bool,
    row_fn: impl Fn(&'a T) -> Row<'a>,
) -> bool {
    let query = &state.query;
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
            .style(STYLE_DARK_GRAY)
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

#[expect(clippy::needless_pass_by_ref_mut)]
fn render_dash(frame: &mut Frame, area: Rect, app: &mut App, samples: &Samples) {
    let [_, gauges, cpu_spark, mem_spark, load, summary, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(DASH_GAUGE_HEIGHT),
        Constraint::Length(SPARKLINE_HEIGHT),
        Constraint::Length(SPARKLINE_HEIGHT),
        Constraint::Length(SINGLE_LINE_HEIGHT),
        Constraint::Length(SINGLE_LINE_HEIGHT),
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
        Constraint::Percentage(DASH_CPU_GAUGE_WIDTH),
        Constraint::Percentage(DASH_MEM_SWAP_GAUGE_WIDTH),
        Constraint::Percentage(DASH_MEM_SWAP_GAUGE_WIDTH),
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
        Span::styled("Load Average  ", STYLE_BOLD),
        Span::raw(format!(
            "{:.2} (1m)  {:.2} (5m)  {:.2} (15m)",
            samples.load_one, samples.load_five, samples.load_fifteen,
        )),
    ]))
    .alignment(Alignment::Center)
    .style(STYLE_GRAY);
    frame.render_widget(&l, area);
}

fn render_dash_summary(frame: &mut Frame, area: Rect, samples: &Samples) {
    let s = Paragraph::new(Line::from(vec![
        Span::styled("Net ", STYLE_BOLD),
        Span::raw(format!(
            "TX {}  RX {}",
            format_bytes(samples.net_tx_rate, false).trim(),
            format_bytes(samples.net_rx_rate, false).trim(),
        )),
        Span::raw("  "),
        Span::styled("Disk ", STYLE_BOLD),
        Span::raw(format!(
            "R {}  W {}",
            format_bytes(samples.disk_read_rate, false).trim(),
            format_bytes(samples.disk_write_rate, false).trim(),
        )),
    ]))
    .alignment(Alignment::Center)
    .style(STYLE_GRAY);
    frame.render_widget(&s, area);
}

fn render_time(frame: &mut Frame, area: Rect, _app: &mut App, samples: &Samples) {
    render_info_block(
        frame,
        area,
        &[
            ("Hostname    ", Cow::Borrowed(&samples.sys_info.hostname)),
            ("OS          ", Cow::Borrowed(&samples.sys_info.os)),
            ("Kernel      ", Cow::Borrowed(&samples.sys_info.kernel)),
            ("Arch        ", Cow::Borrowed(&samples.sys_info.arch)),
            (
                "Uptime      ",
                Cow::Owned(format_uptime(samples.sys_info.uptime)),
            ),
            (
                "CPUs        ",
                Cow::Owned(format!("{}", samples.sys_info.cpu_count)),
            ),
            ("Distro      ", Cow::Borrowed(&samples.sys_info.distro)),
            (
                "Boot        ",
                Cow::Owned(format_timestamp(samples.sys_info.boot_time)),
            ),
            (
                "Phys Cores  ",
                Cow::Owned(format!("{}", samples.sys_info.physical_cores)),
            ),
        ],
    );
}

#[expect(clippy::needless_pass_by_ref_mut)]
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
        Constraint::Length(INFO_BLOCK_HEIGHT),
        Constraint::Length(SPARKLINE_HEIGHT),
        Constraint::Length(SPARKLINE_HEIGHT),
        Constraint::Fill(1),
    ])
    .areas(area);

    render_info_block(
        frame,
        info,
        &[
            ("Memory      ", Cow::Owned(format!("{mem_total_gb:.1} GiB"))),
            (
                "Used        ",
                Cow::Owned(format_memory(mem_used_gb, mem_used_pct)),
            ),
            (
                "Available   ",
                Cow::Owned(format_memory(mem_avail_gb, mem_avail_pct)),
            ),
            (
                "Free        ",
                Cow::Owned(format_memory(mem_free_gb, mem_free_pct)),
            ),
            ("", Cow::Borrowed("")),
            (
                "Swap        ",
                Cow::Owned(format!("{swap_total_gb:.1} GiB")),
            ),
            (
                "Used        ",
                Cow::Owned(format_memory(swap_used_gb, swap_used_pct)),
            ),
            (
                "Free        ",
                Cow::Owned(format_memory(swap_free_gb, swap_free_pct)),
            ),
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

#[expect(clippy::needless_pass_by_ref_mut)]
fn render_temp(frame: &mut Frame, area: Rect, app: &mut App, samples: &Samples) {
    let widths: [Constraint; 4] = [
        Constraint::Fill(1),
        Constraint::Length(TEMP_COL_WIDTH),
        Constraint::Length(TEMP_COL_WIDTH),
        Constraint::Length(TEMP_COL_WIDTH),
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

    let constraints = vec![Constraint::Length(SINGLE_LINE_HEIGHT); core_count];
    let chunks = Layout::vertical(&constraints).split(inner);

    for (i, cpu) in samples.cpus.iter().enumerate() {
        if let Some(chunk) = chunks.get(i) {
            let gauge = Gauge::default()
                .gauge_style(STYLE_BLUE)
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

    if !render_filtered_table(
        frame,
        table_area,
        &mut app.files_state,
        &samples.disks,
        |count, total| format!(" Filesystems ({count}/{total}) "),
        "No matching filesystems",
        &[
            Constraint::Length(FILES_DEVICE_WIDTH),
            Constraint::Fill(1),
            Constraint::Length(FILES_FS_WIDTH),
            Constraint::Length(FILES_SIZE_WIDTH),
            Constraint::Length(FILES_SIZE_WIDTH),
            Constraint::Length(FILES_USEPCT_WIDTH),
            Constraint::Length(FILES_KIND_WIDTH),
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

#[expect(clippy::needless_pass_by_ref_mut)]
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
        Constraint::Length(DISK_IO_RW_WIDTH),
        Constraint::Length(DISK_IO_RW_WIDTH),
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

    if !render_filtered_table(
        frame,
        table_area,
        &mut app.net_state,
        &samples.interfaces,
        |count, total| format!(" Network I/O ({count}/{total}) "),
        "No matching interfaces",
        &[
            Constraint::Fill(1),
            Constraint::Length(NET_RX_TX_WIDTH),
            Constraint::Length(NET_RX_TX_WIDTH),
            Constraint::Length(NET_STATE_WIDTH),
            Constraint::Length(NET_MAC_WIDTH),
            Constraint::Fill(1),
        ],
        &["Interface", "RX", "TX", "State", "MAC", "IP"],
        |i: &NetInfo, q| i.name.to_lowercase().contains(q),
        |i: &NetInfo| {
            Row::new(vec![
                Cell::from(i.name.as_str()),
                Cell::from(Span::styled(format_bytes(i.rx_bytes, false), STYLE_YELLOW)),
                Cell::from(Span::styled(format_bytes(i.tx_bytes, false), STYLE_YELLOW)),
                Cell::from(i.state),
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
            .style(STYLE_DARK_GRAY)
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

/// Builds the process table widget with headers, sort arrows, and row selection.
fn build_proc_table<'a>(
    visible: &[&'a ProcessInfo],
    rel_sel: usize,
    app: &App,
    filtered: &[&ProcessInfo],
    samples: &Samples,
) -> Table<'a> {
    let widths: [Constraint; 7] = [
        Constraint::Fill(1),
        Constraint::Length(PROC_PID_WIDTH),
        Constraint::Length(PROC_PID_WIDTH),
        Constraint::Length(PROC_NUM_WIDTH),
        Constraint::Length(PROC_NUM_WIDTH),
        Constraint::Length(PROC_NUM_WIDTH),
        Constraint::Length(PROC_NUM_WIDTH),
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
                Cell::from(p.status),
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

/// Trait for tab-specific rendering, enabling v0.6.x widget-oriented dispatch.
pub trait TabWidget {
    /// Render the tab content into the given area.
    fn render(&self, frame: &mut Frame, area: Rect, app: &mut App, samples: &Samples);
}

macro_rules! impl_tab {
    ($name:ident, $render:ident) => {
        struct $name;
        impl TabWidget for $name {
            fn render(&self, frame: &mut Frame, area: Rect, app: &mut App, samples: &Samples) {
                $render(frame, area, app, samples);
            }
        }
    };
}

impl_tab!(DashTab, render_dash);
impl_tab!(ProcTab, render_proc);
impl_tab!(NetTab, render_net);
impl_tab!(FilesTab, render_files);
impl_tab!(TimeTab, render_time);
impl_tab!(TempTab, render_temp);
impl_tab!(CoresTab, render_cores);
impl_tab!(DiskTab, render_disk);
impl_tab!(MemTab, render_mem);

const RENDERERS: &[&dyn TabWidget; 9] = &[
    &DashTab, &ProcTab, &NetTab, &FilesTab, &TimeTab, &TempTab, &CoresTab, &DiskTab, &MemTab,
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
                Layout::vertical([Constraint::Length(SINGLE_LINE_HEIGHT), Constraint::Fill(1)])
                    .areas(inner);
            render_horizontal_tabs(frame, tab_area, app);
            content_area
        }
        _ => inner,
    };

    let has_footer = app.tab_orientation == TabOrientation::HorizontalFooter && app.tab_bar_visible;

    let (tab_area, status_area) = if has_footer {
        let chunks = Layout::vertical([
            Constraint::Fill(1),
            Constraint::Length(SINGLE_LINE_HEIGHT),
            Constraint::Length(SINGLE_LINE_HEIGHT),
        ])
        .split(content_area);
        render_horizontal_tabs(frame, chunks[1], app);
        (chunks[0], chunks[2])
    } else {
        let chunks =
            Layout::vertical([Constraint::Fill(1), Constraint::Length(SINGLE_LINE_HEIGHT)])
                .split(content_area);
        (chunks[0], chunks[1])
    };

    RENDERERS[app.active_tab.index()].render(frame, tab_area, app, samples);

    render_status_bar(frame, status_area, app);
    render_overlays(frame, tab_area, app);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_processes_queries() {
        let procs = [
            ProcessInfo {
                name: "firefox".into(),
                pid: 100,
                cpu: 0.0,
                memory: 0,
                virtual_memory: 0,
                run_time: 0,
                status: "Running",
            },
            ProcessInfo {
                name: "bash".into(),
                pid: 200,
                cpu: 0.0,
                memory: 0,
                virtual_memory: 0,
                run_time: 0,
                status: "Sleep",
            },
        ];
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
        let procs = [
            ProcessInfo {
                name: "firefox".into(),
                pid: 100,
                cpu: 0.0,
                memory: 0,
                virtual_memory: 0,
                run_time: 0,
                status: "Running",
            },
            ProcessInfo {
                name: "bash".into(),
                pid: 200,
                cpu: 0.0,
                memory: 0,
                virtual_memory: 0,
                run_time: 0,
                status: "Sleep",
            },
        ];
        for (query, expected_pid) in [("FIRE", 100), ("BASH", 200)] {
            let result = filter_processes(query, &procs);
            assert_eq!(result.len(), 1, "query '{query}'");
            assert_eq!(result[0].pid, expected_pid, "query '{query}'");
        }
    }

    #[test]
    fn sort_processes_sorts_by_name_ascending() {
        let procs = [
            ProcessInfo {
                name: "firefox".into(),
                pid: 100,
                cpu: 0.0,
                memory: 0,
                virtual_memory: 0,
                run_time: 0,
                status: "Running",
            },
            ProcessInfo {
                name: "bash".into(),
                pid: 200,
                cpu: 0.0,
                memory: 0,
                virtual_memory: 0,
                run_time: 0,
                status: "Sleep",
            },
        ];
        let mut refs: Vec<&ProcessInfo> = procs.iter().collect();
        sort_processes(&mut refs, ProcSortField::Name, true);
        assert_eq!(refs[0].name, "bash");
        assert_eq!(refs[1].name, "firefox");
    }

    #[test]
    fn sort_processes_sorts_by_name_descending() {
        let procs = [
            ProcessInfo {
                name: "firefox".into(),
                pid: 100,
                cpu: 0.0,
                memory: 0,
                virtual_memory: 0,
                run_time: 0,
                status: "Running",
            },
            ProcessInfo {
                name: "bash".into(),
                pid: 200,
                cpu: 0.0,
                memory: 0,
                virtual_memory: 0,
                run_time: 0,
                status: "Sleep",
            },
        ];
        let mut refs: Vec<&ProcessInfo> = procs.iter().collect();
        sort_processes(&mut refs, ProcSortField::Name, false);
        assert_eq!(refs[0].name, "firefox");
        assert_eq!(refs[1].name, "bash");
    }

    #[test]
    fn sort_processes_sorts_by_pid() {
        let procs = [
            ProcessInfo {
                name: "firefox".into(),
                pid: 100,
                cpu: 0.0,
                memory: 0,
                virtual_memory: 0,
                run_time: 0,
                status: "Running",
            },
            ProcessInfo {
                name: "bash".into(),
                pid: 200,
                cpu: 0.0,
                memory: 0,
                virtual_memory: 0,
                run_time: 0,
                status: "Sleep",
            },
        ];
        let mut refs: Vec<&ProcessInfo> = procs.iter().collect();
        sort_processes(&mut refs, ProcSortField::Pid, true);
        assert_eq!(refs[0].pid, 100);
        assert_eq!(refs[1].pid, 200);
    }

    #[test]
    fn sort_processes_sorts_by_pid_descending() {
        let procs = [
            ProcessInfo {
                name: "firefox".into(),
                pid: 100,
                cpu: 0.0,
                memory: 0,
                virtual_memory: 0,
                run_time: 0,
                status: "Running",
            },
            ProcessInfo {
                name: "bash".into(),
                pid: 200,
                cpu: 0.0,
                memory: 0,
                virtual_memory: 0,
                run_time: 0,
                status: "Sleep",
            },
        ];
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
                status: "Sleep",
            },
            ProcessInfo {
                name: "firefox".into(),
                pid: 2,
                cpu: 50.0,
                memory: 1000,
                virtual_memory: 5000,
                run_time: 500,
                status: "Running",
            },
            ProcessInfo {
                name: "chrome".into(),
                pid: 3,
                cpu: 30.0,
                memory: 500,
                virtual_memory: 3000,
                run_time: 300,
                status: "Running",
            },
            ProcessInfo {
                name: "sshd".into(),
                pid: 4,
                cpu: 0.5,
                memory: 10,
                virtual_memory: 50,
                run_time: 2000,
                status: "Idle",
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
