//! Shared UI utilities: formatting, layout constants, style presets, and sparkline config.
//!
//! These are consumed by the per-tab render functions in [`crate::tui`].

use std::borrow::Cow;
use std::collections::VecDeque;

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Sparkline};

use crate::app::{self, App, KillState, MAX_QUERY_LEN, ProcSortField, Tab};

const MAX_HINTS: usize = 4;

pub const SPARKLINE_HEIGHT: u16 = 3;

pub const SPARK_CPU_TITLE: &str = " CPU ";
pub const SPARK_CPU_COLOR: Color = Color::Green;
pub const SPARK_MEM_TITLE: &str = " Memory ";
pub const SPARK_MEM_COLOR: Color = Color::Cyan;
pub const SPARK_NET_RX_TITLE: &str = " RX ";
pub const SPARK_NET_RX_COLOR: Color = Color::Green;
pub const SPARK_NET_TX_TITLE: &str = " TX ";
pub const SPARK_NET_TX_COLOR: Color = Color::Yellow;
pub const SPARK_DISK_READ_TITLE: &str = " Read ";
pub const SPARK_DISK_READ_COLOR: Color = Color::Cyan;
pub const SPARK_DISK_WRITE_TITLE: &str = " Write ";
pub const SPARK_DISK_WRITE_COLOR: Color = Color::Yellow;
pub const SPARK_USAGE_TITLE: &str = " Usage ";
pub const SPARK_USAGE_COLOR: Color = Color::Magenta;
pub const SPARK_MEM_HISTORY_TITLE: &str = " History ";
pub const SPARK_MEM_HISTORY_COLOR: Color = Color::LightBlue;
pub const SPARK_SWAP_TITLE: &str = " Swap ";
pub const SPARK_SWAP_COLOR: Color = Color::Yellow;
pub const SPARK_TEMP_TITLE: &str = " Thermal ";
pub const SPARK_TEMP_COLOR: Color = Color::Red;

pub const STYLE_GREEN: Style = Style::new().fg(Color::Green);
pub const STYLE_CYAN: Style = Style::new().fg(Color::Cyan);
pub const STYLE_YELLOW: Style = Style::new().fg(Color::Yellow);
pub const STYLE_MAGENTA: Style = Style::new().fg(Color::Magenta);
pub const STYLE_DARK_GRAY: Style = Style::new().fg(Color::DarkGray);
pub const STYLE_SELECTED: Style = Style::new().bg(Color::DarkGray);
pub const STYLE_RED: Style = Style::new().fg(Color::Red);
pub const STYLE_GRAY: Style = Style::new().fg(Color::Gray);
pub const STYLE_BLUE: Style = Style::new().fg(Color::Blue);
pub const STYLE_BOLD: Style = Style::new().bold();

// Semantic observation colors — used only by observation_to_line in tui.rs
// These colors carry meaning (not decoration) and are distinct from the metric palette.
pub const STYLE_NEUTRAL: Style = Style::new().fg(Color::DarkGray);
pub const STYLE_ACTIVITY: Style = Style::new().fg(Color::Cyan);
pub const STYLE_ATTENTION: Style = Style::new().fg(Color::Yellow);
pub const STYLE_CRITICAL: Style = Style::new().fg(Color::Red);

// Layout constants
pub const DASH_GAUGE_HEIGHT: u16 = 3;
const SEARCH_BAR_HEIGHT: u16 = 3;
pub const INFO_BLOCK_HEIGHT: u16 = 8;
pub const SINGLE_LINE_HEIGHT: u16 = 1;
pub const STATUS_HINTS_MIN_WIDTH: u16 = 20;

/// Bytes per gibibyte (1024³).
pub const GIB: u64 = 1 << 30;

/// Bytes per mebibyte (1024²).
const MIB: u64 = 1 << 20;

// Dash gauge widths
pub const DASH_CPU_GAUGE_WIDTH: u16 = 34;
pub const DASH_MEM_SWAP_GAUGE_WIDTH: u16 = 33;

// Table column widths
pub const TEMP_COL_WIDTH: u16 = 9;
pub const FILES_DEVICE_WIDTH: u16 = 14;
pub const FILES_FS_WIDTH: u16 = 6;
pub const FILES_SIZE_WIDTH: u16 = 9;
pub const FILES_USEPCT_WIDTH: u16 = 6;
pub const FILES_KIND_WIDTH: u16 = 7;
pub const DISK_IO_RW_WIDTH: u16 = 12;
pub const NET_RX_TX_WIDTH: u16 = 12;
pub const NET_STATE_WIDTH: u16 = 8;
pub const NET_MAC_WIDTH: u16 = 17;
pub const PROC_PID_WIDTH: u16 = 7;
pub const PROC_NUM_WIDTH: u16 = 9;

const HINT_SEP: &str = " │ ";

pub const PROC_HEADERS: &[(&str, ProcSortField)] = &[
    ("Name", ProcSortField::Name),
    ("PID", ProcSortField::Pid),
    ("CPU%", ProcSortField::Cpu),
    ("Memory", ProcSortField::Memory),
    ("Virtual", ProcSortField::VirtualMemory),
    ("Time", ProcSortField::RunTime),
    ("Status", ProcSortField::Status),
];

pub fn format_memory(gib: f64, pct: f64) -> String {
    format!("{gib:.1} GiB  {pct:.1}%")
}

pub fn format_bytes(bytes: u64, binary: bool) -> String {
    if binary {
        let units = ["B", "KiB", "MiB", "GiB", "TiB"];
        let divisor: f64 = 1024.0;
        let mut val = bytes as f64;
        let mut unit_idx = 0;
        while val >= divisor && unit_idx < units.len() - 1 {
            val /= divisor;
            unit_idx += 1;
        }
        if unit_idx == 0 || bytes == 0 {
            format!("{}{}", bytes, units[unit_idx])
        } else if val < 10.0 {
            format!("{val:.1}{}", units[unit_idx])
        } else {
            format!("{val:.0}{}", units[unit_idx])
        }
    } else {
        let units = ["B", "KB", "MB", "GB", "TB"];
        let divisor: f64 = 1000.0;
        let mut val = bytes as f64;
        let mut unit_idx = 0;
        while val >= divisor && unit_idx < units.len() - 1 {
            val /= divisor;
            unit_idx += 1;
        }
        if unit_idx == 0 || bytes == 0 {
            format!("{}{}", bytes, units[unit_idx])
        } else if val < 10.0 {
            format!("{val:.1}{}", units[unit_idx])
        } else {
            format!("{val:.0}{}", units[unit_idx])
        }
    }
}

pub fn format_uptime(secs: u64) -> String {
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

pub fn format_temp(temp: Option<f32>) -> String {
    temp.filter(|t| t.is_finite())
        .map_or_else(|| "      N/A".to_owned(), |t| format!("{t:>7.1}°C"))
}

/// Days from 1970-01-01 to 2999-12-31 (exclusive upper bound), computed at compile time.
const MAX_CUMULATIVE: u64 = {
    let mut d = 0u64;
    let mut y = 1970u64;
    while y <= 2999 {
        d += 365;
        if y.is_multiple_of(4) && (!y.is_multiple_of(100) || y.is_multiple_of(400)) {
            d += 1;
        }
        y += 1;
    }
    d - 1
};

pub fn format_timestamp(secs: u64) -> String {
    let days_raw = secs / 86400;
    let rem = secs % 86400;
    let hour = rem / 3600;
    let min = (rem % 3600) / 60;
    let sec = rem % 60;

    let day = days_raw.min(MAX_CUMULATIVE);

    let mut year = 1970u64;
    let mut day = day;
    loop {
        let leap =
            year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
        let diy = if leap { 366 } else { 365 };
        if day < diy {
            break;
        }
        day -= diy;
        year += 1;
    }

    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
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

pub fn format_memory_label(bytes: u64) -> String {
    if bytes >= GIB {
        let gib = bytes as f64 / GIB as f64;
        format!("{gib:.1}GiB")
    } else {
        let mib = bytes as f64 / MIB as f64;
        if mib >= 1.0 {
            format!("{mib:.0}MiB")
        } else {
            format!("{mib}MiB")
        }
    }
}

pub fn clamp_scroll(
    selection: usize,
    scroll: &mut usize,
    count: usize,
    height: u16,
) -> (usize, usize) {
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

pub fn sort_arrow(field: ProcSortField, sort_field: ProcSortField, asc: bool) -> &'static str {
    if field != sort_field {
        return "";
    }
    if asc { "\u{2191}" } else { "\u{2193}" }
}

pub fn render_sidebar(frame: &mut Frame, area: Rect, app: &App) {
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

pub fn render_search_bar(frame: &mut Frame, area: Rect, query: &str, focused: bool) -> Rect {
    let (search_area, remaining) = if !query.is_empty() || focused {
        let [s, r] = Layout::vertical([Constraint::Length(SEARCH_BAR_HEIGHT), Constraint::Fill(1)])
            .areas(area);
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

pub fn render_info_block(frame: &mut Frame, area: Rect, items: &[(&str, Cow<'_, str>)]) {
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
                Span::styled(*label, STYLE_BOLD),
                Span::raw(value.as_ref()),
            ])
        })
        .collect();
    frame.render_widget(Paragraph::new(lines).style(STYLE_GRAY), info);
}

pub fn render_sparkline(
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
        .style(STYLE_GRAY);
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

impl StatusBar {
    fn ctx_string(app: &App) -> Cow<'static, str> {
        if let Some(ref fb) = app.kill_feedback {
            return Cow::Owned(fb.clone());
        }
        if app.paused {
            return Cow::Borrowed("PAUSED (Space to resume)");
        }
        if app.help_visible {
            return Cow::Borrowed("Help (? to close)");
        }
        if app.kill_state == Some(KillState::Confirm) {
            let pid = app.selected_pid();
            let name = app.selected_name();
            return Cow::Owned(format!("Kill? PID {pid} ({name})"));
        }
        if let Some(ref err) = app.error_msg {
            return Cow::Owned(format!("Error: {err}"));
        }
        let label = app.active_tab.label();
        if app.active_tab.is_proc() {
            let arrow = if app.proc_sort_asc {
                "\u{2191}"
            } else {
                "\u{2193}"
            };
            Cow::Owned(format!(" [{label} {}{arrow}]", app.proc_sort_field))
        } else {
            Cow::Borrowed(label)
        }
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
                    app::KILL_SIGNAL_MAP.last().map_or('6', |(k, _, _)| *k)
                ),
                "any Cancel".to_owned(),
            ];
        }

        let mut hints: Vec<String> = Vec::with_capacity(MAX_HINTS + 1);
        if app.active_tab.has_searchable_state() {
            let state = app.tab_state();
            let query_empty = state.is_none_or(|s| s.query.is_empty());
            let focused = state.is_some_and(|s| s.focused);
            if !focused && query_empty {
                hints.push("/ Search".to_owned());
            } else if !query_empty {
                hints.push("Esc Clear".to_owned());
            }
        }
        if app.active_tab.is_proc() {
            hints.push("\u{2191}\u{2193} Select".to_owned());
            if app.selected.is_some() {
                hints.push("Delete Kill".to_owned());
                hints.push("Ctrl+K Kill!".to_owned());
            }
        } else {
            hints.push("1-9 Tab".to_owned());
        }

        let toggle_label = if app.tab_orientation.is_horizontal() {
            if app.tab_bar_visible {
                "Hide Tab"
            } else {
                "Show Tab"
            }
        } else if app.sidebar_visible {
            "Hide Sidebar"
        } else {
            "Show Sidebar"
        };
        hints.push(format!("Ctrl+S {toggle_label}"));
        hints.truncate(MAX_HINTS);
        hints
    }

    fn display(app: &App) -> (Cow<'static, str>, String) {
        let ctx = Self::ctx_string(app);
        let hints = Self::status_hints(app).join(HINT_SEP);
        (ctx, hints)
    }
}

pub fn render_horizontal_tabs(frame: &mut Frame, area: Rect, app: &App) {
    let mut spans = Vec::with_capacity(Tab::ALL.len() * 2 - 1);
    for (i, tab) in Tab::ALL.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(HINT_SEP, STYLE_DARK_GRAY));
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

pub fn render_status_bar(frame: &mut Frame, status_area: Rect, app: &App) {
    let (ctx, hints) = StatusBar::display(app);
    let [ctx_area, hints_area] =
        Layout::horizontal([Constraint::Fill(1), Constraint::Min(STATUS_HINTS_MIN_WIDTH)])
            .areas(status_area);
    frame.render_widget(
        Paragraph::new(ctx)
            .alignment(Alignment::Left)
            .style(STYLE_GRAY),
        ctx_area,
    );
    frame.render_widget(
        Paragraph::new(hints)
            .alignment(Alignment::Right)
            .style(STYLE_GRAY),
        hints_area,
    );
}

pub fn render_overlays(frame: &mut Frame, tab_area: Rect, app: &mut App) {
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
            (MIB, "1.0MiB"),
            (GIB, "1.0GiB"),
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
        assert_eq!(format_memory_label(GIB), "1.0GiB");
        assert_eq!(format_memory_label(5_368_709_120), "5.0GiB");
    }

    #[test]
    fn format_memory_label_shows_mib_below_threshold() {
        assert_eq!(format_memory_label(0), "0MiB");
        assert_eq!(format_memory_label(MIB), "1MiB");
        assert_eq!(format_memory_label(524_288_000), "500MiB");
    }

    #[test]
    fn format_memory_label_edge_boundary() {
        assert_eq!(format_memory_label(1_073_741_823), "1024MiB");
        assert_eq!(format_memory_label(GIB), "1.0GiB");
    }

    #[test]
    fn format_memory_displays_gib_and_pct() {
        assert_eq!(format_memory(2.5, 45.0), "2.5 GiB  45.0%");
        assert_eq!(format_memory(0.0, 0.0), "0.0 GiB  0.0%");
        assert_eq!(format_memory(15.9, 100.0), "15.9 GiB  100.0%");
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
    fn sort_arrow_shows_ascending() {
        assert_eq!(
            sort_arrow(ProcSortField::Name, ProcSortField::Name, true),
            "\u{2191}"
        );
    }

    #[test]
    fn sort_arrow_shows_descending() {
        assert_eq!(
            sort_arrow(ProcSortField::Cpu, ProcSortField::Cpu, false),
            "\u{2193}"
        );
    }

    #[test]
    fn sort_arrow_empty_when_different_field() {
        assert_eq!(
            sort_arrow(ProcSortField::Name, ProcSortField::Cpu, true),
            ""
        );
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
        app.selected = Some(crate::app::SelectionState {
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
        app.selected = Some(crate::app::SelectionState {
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
        app.selected = Some(crate::app::SelectionState {
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
    fn status_bar_ctx_paused() {
        let mut app = App::new();
        app.paused = true;
        let (ctx, _) = StatusBar::display(&app);
        assert_eq!(ctx, "PAUSED (Space to resume)");
    }

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
        app.selected = Some(crate::app::SelectionState {
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
        app.selected = Some(crate::app::SelectionState {
            pid: 42,
            name: "bash".to_owned(),
        });
        let (ctx, _) = StatusBar::display(&app);
        assert_eq!(ctx, "Kill? PID 42 (bash)", "kill confirm wins over error");
    }
}
