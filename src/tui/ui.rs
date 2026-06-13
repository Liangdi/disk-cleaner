use std::sync::LazyLock;

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph},
};
use ratatui_style::{NodeRef, Stylesheet, css};

use super::app::{AppState, DetailStats};

/// All styling for the interactive UI lives in this stylesheet, embedded at
/// compile time. Re-theme the app by editing it.
static THEME: LazyLock<Stylesheet> = css!("theme.css");

/// Compute the ratatui [`Style`] for a styled element (`Type` selector) with
/// optional variant classes. The common no-parent case.
fn sty(type_name: &str, classes: &[&str]) -> Style {
    THEME
        .compute(&NodeRef::new(type_name).classes(classes), None)
        .to_style()
}

/// Width of the percentage bar in characters.
const BAR_WIDTH: usize = 12;

/// Render the full TUI layout.
pub fn render(f: &mut Frame, state: &AppState, detail: &DetailStats) {
    if state.loading {
        render_loading(f, state);
        return;
    }

    // Clear entire frame to deep background
    f.render_widget(Paragraph::new("").style(sty("Root", &[])), f.area());

    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // title bar
            Constraint::Min(5),   // main area
            Constraint::Length(1), // status bar
        ])
        .split(f.area());

    render_title(f, state, outer[0]);

    // Left-right split for main area
    let main = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(55), // left: tree
            Constraint::Percentage(45), // right: detail
        ])
        .split(outer[1]);

    render_list(f, state, main[0]);
    render_detail(f, detail, main[1]);
    render_status(f, state, outer[2]);

    // Overlay: delete confirmation dialog
    if state.delete_target.is_some() {
        render_delete_dialog(f, state);
    }
}

// ── Title bar ──────────────────────────────────────────────

fn render_title(f: &mut Frame, state: &AppState, area: Rect) {
    let total = pretty_bytes(state.total_size as f64);
    let mut spans = vec![
        Span::styled(" ▌", sty("Title", &["glyph"])),
        Span::styled("disk cleaner", sty("Title", &["name"])),
        Span::styled(" ▐ ", sty("Title", &["glyph"])),
        Span::styled(
            format!("{} ", state.root_path),
            sty("Title", &["rootpath"]),
        ),
        Span::styled(format!("{}", total), sty("Title", &["total"])),
    ];

    if state.apparent {
        spans.push(Span::styled(" [apparent]", sty("Title", &["apparent"])));
    }

    if !state.search_query.is_empty() || state.search_active {
        spans.push(Span::styled(
            format!(" /{}", state.search_query),
            sty("Title", &["search"]),
        ));
        if state.search_active {
            spans.push(Span::styled("█", sty("Title", &["search"])));
        }
    }

    let para = Paragraph::new(Line::from(spans)).style(sty("Title", &[]));
    f.render_widget(para, area);
}

// ── Left: Tree list ────────────────────────────────────────

fn render_list(f: &mut Frame, state: &AppState, area: Rect) {
    let viewport_height = area.height as usize;
    let lines: Vec<Line> = state
        .visible
        .iter()
        .enumerate()
        .skip(state.scroll_offset)
        .take(viewport_height)
        .map(|(vis_idx, &item_idx)| render_item(state, vis_idx, item_idx))
        .collect();

    let para = Paragraph::new(lines).style(sty("TreePanel", &[]));
    f.render_widget(para, area);
}

fn render_item(state: &AppState, vis_idx: usize, item_idx: usize) -> Line<'static> {
    let item = &state.items[item_idx];
    let is_selected = vis_idx == state.selected;

    let indent = "  ".repeat(item.depth);
    let indicator = if item.has_children {
        if state.expanded.contains(&item_idx) {
            "◉ "
        } else {
            "◎ "
        }
    } else {
        "  "
    };

    let pct = if item.parent_size > 0 {
        100.0 * (item.disk_size as f64 / item.parent_size as f64)
    } else {
        100.0
    };

    let filled = if item.parent_size > 0 {
        ((item.disk_size as f64 / item.parent_size as f64) * BAR_WIDTH as f64).round() as usize
    } else {
        BAR_WIDTH
    };
    let filled = filled.min(BAR_WIDTH);

    // Threshold-driven color, shared by the % text and the filled bar.
    let pct_cls: &[&str] = if item.depth == 0 {
        &["root"]
    } else if pct > 20.0 {
        &["high"]
    } else if pct > 10.0 {
        &["mid"]
    } else {
        &["low"]
    };
    let pct_style = sty("Pct", pct_cls);

    let size_str = pretty_bytes(item.disk_size as f64);
    let name = if item.has_children {
        format!("{}/", item.name)
    } else {
        item.name.clone()
    };

    let name_cls: &[&str] = if is_selected {
        &["selected"]
    } else if item.has_children {
        &["dir"]
    } else {
        &[]
    };
    let ind_cls: &[&str] = if is_selected { &["selected"] } else { &[] };
    let sel_cls: &[&str] = if is_selected { &["selected"] } else { &[] };

    // Row background (CSS guarantees TreeItem declares a background).
    let row_bg = sty("TreeItem", sel_cls).bg.unwrap_or(Color::Reset);

    Line::from(vec![
        Span::styled(
            format!("{}{}", indent, indicator),
            sty("Indicator", ind_cls).bg(row_bg),
        ),
        Span::styled(format!("{:>6.1}% ", pct), pct_style.bg(row_bg)),
        Span::styled("█".repeat(filled), pct_style.bg(row_bg)),
        Span::styled(
            "░".repeat(BAR_WIDTH - filled),
            sty("Bar", &["empty"]).bg(row_bg),
        ),
        Span::styled(
            format!(" {:>10}", size_str),
            sty("Size", &[]).bg(row_bg),
        ),
        Span::styled(format!(" {}", name), sty("Name", name_cls).bg(row_bg)),
    ])
}

// ── Right: Detail panel ────────────────────────────────────

fn render_detail(f: &mut Frame, stats: &DetailStats, area: Rect) {
    // CSS `border-left: single var(--border-dim)` yields exactly the LEFT border.
    // Bind the computed style: `to_block()` borrows it.
    let detail = THEME.compute(&NodeRef::new("DetailPanel"), None);
    let block = detail.to_block();
    let inner = block.inner(area);
    f.render_widget(block, area);

    // Split detail into sections
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(6),  // breadcrumb + size gauge + stats
            Constraint::Min(5),    // type distribution + histogram
            Constraint::Min(4),    // top largest
        ])
        .split(inner);

    render_header_section(f, stats, chunks[0]);
    render_charts_section(f, stats, chunks[1]);
    render_top_largest(f, stats, chunks[2]);
}

/// Top section: breadcrumb, proportion gauge, quick stats.
fn render_header_section(f: &mut Frame, stats: &DetailStats, area: Rect) {
    let w = area.width as usize;
    let mut lines = Vec::new();

    // Breadcrumb: root ▸ src ▸ tui
    let crumb = if stats.breadcrumb.len() <= 1 {
        stats.breadcrumb.first().cloned().unwrap_or_default()
    } else {
        stats
            .breadcrumb
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(" ▸ ")
    };
    lines.push(Line::from(vec![
        Span::styled(" ", sty("Dim", &[])),
        Span::styled(truncate_str(&crumb, w.saturating_sub(2)), sty("Crumb", &[])),
    ]));

    // Proportion gauge: big visual bar of total share
    let gauge_w = w.saturating_sub(14);
    let filled = if stats.pct_of_total > 0.0 && gauge_w > 0 {
        ((stats.pct_of_total / 100.0) * gauge_w as f64).round() as usize
    } else {
        0
    };
    let filled = filled.min(gauge_w);
    let gauge_cls: &[&str] = if stats.pct_of_total > 50.0 {
        &["high"]
    } else if stats.pct_of_total > 20.0 {
        &["mid"]
    } else {
        &["low"]
    };
    lines.push(Line::from(vec![
        Span::styled(" ╺", sty("GaugeCap", &[])),
        Span::styled("━".repeat(filled), sty("Gauge", gauge_cls)),
        Span::styled(
            "─".repeat(gauge_w.saturating_sub(filled)),
            sty("GaugeTrack", &[]),
        ),
        Span::styled("╸ ", sty("GaugeCap", &[])),
        Span::styled(
            format!("{:.1}%", stats.pct_of_total),
            sty("Gauge", gauge_cls).add_modifier(Modifier::BOLD),
        ),
    ]));

    // Quick stats: size │ files │ dirs │ avg
    let size_str = pretty_bytes(stats.size as f64);
    let avg_str = if stats.avg_file_size > 0 {
        pretty_bytes(stats.avg_file_size as f64)
    } else {
        "-".to_string()
    };
    lines.push(Line::from(vec![
        Span::styled(" ", sty("Dim", &[])),
        Span::styled(size_str, sty("StatSize", &[])),
        Span::styled(" │ ", sty("Sep", &[])),
        Span::styled(format!("{} files", stats.file_count), sty("StatLabel", &[])),
        Span::styled(" │ ", sty("Sep", &[])),
        Span::styled(format!("{} dirs", stats.dir_count), sty("StatLabel", &[])),
        Span::styled(" │ ", sty("Sep", &[])),
        Span::styled(format!("avg {}", avg_str), sty("Dim", &[])),
    ]));

    // Largest child
    if let Some((ref name, sz)) = stats.largest_child {
        lines.push(Line::from(vec![
            Span::styled(" ◈ ", sty("Largest", &[])),
            Span::styled(pretty_bytes(sz as f64), sty("Largest", &[])),
            Span::styled(
                format!(" {}", truncate_str(name, w.saturating_sub(16))),
                sty("LargestName", &[]),
            ),
        ]));
    }

    // Separator
    lines.push(Line::from(Span::styled(
        format!(" {}", "─".repeat(w.saturating_sub(2))),
        sty("Hr", &[]),
    )));

    let para = Paragraph::new(lines);
    f.render_widget(para, area);
}

/// Middle section: file types + size histogram side by side.
fn render_charts_section(f: &mut Frame, stats: &DetailStats, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(55), // type distribution
            Constraint::Percentage(45), // size histogram
        ])
        .split(area);

    render_type_distribution(f, stats, chunks[0]);
    render_size_histogram(f, stats, chunks[1]);
}

fn render_type_distribution(f: &mut Frame, stats: &DetailStats, area: Rect) {
    let max_rows = area.height as usize;
    let mut lines = vec![Line::from(Span::styled(
        " ◈ Types",
        sty("SectionTitle", &[]),
    ))];

    if stats.type_distribution.is_empty() {
        lines.push(Line::from(Span::styled("  (empty)", sty("Empty", &[]))));
        let para = Paragraph::new(lines);
        f.render_widget(para, area);
        return;
    }

    let bar_w = 8;
    let max_size = stats.type_distribution.iter().map(|t| t.1).max().unwrap_or(1);

    for (ext, size, count) in stats.type_distribution.iter().take(max_rows.saturating_sub(1)) {
        let ext_display = if ext.len() > 8 { &ext[..7] } else { ext };
        let filled = if max_size > 0 {
            ((*size as f64 / max_size as f64) * bar_w as f64).round() as usize
        } else {
            0
        };
        let filled = filled.min(bar_w);

        let ext_style = sty("TypeExt", &[]);
        lines.push(Line::from(vec![
            Span::styled(format!(" {:>8}", ext_display), ext_style),
            Span::styled("█".repeat(filled), ext_style),
            Span::styled(
                "░".repeat(bar_w.saturating_sub(filled)),
                sty("Bar", &["empty"]),
            ),
            Span::styled(
                format!(" {:>9}", pretty_bytes(*size as f64)),
                sty("TypeSize", &[]),
            ),
            Span::styled(format!("({:>3})", count), sty("Dim", &[])),
        ]));
    }

    let para = Paragraph::new(lines);
    f.render_widget(para, area);
}

fn render_size_histogram(f: &mut Frame, stats: &DetailStats, area: Rect) {
    let max_rows = area.height as usize;
    let mut lines = vec![Line::from(Span::styled(
        " ◈ Size Dist",
        sty("SectionTitle", &["purple"]),
    ))];

    if stats.size_histogram.is_empty() {
        lines.push(Line::from(Span::styled("  (no files)", sty("Empty", &[]))));
        let para = Paragraph::new(lines);
        f.render_widget(para, area);
        return;
    }

    let max_count = stats.size_histogram.iter().map(|t| t.1).max().unwrap_or(1);
    let bar_w = 10;

    for (label, count) in stats.size_histogram.iter().take(max_rows.saturating_sub(1)) {
        let filled = if max_count > 0 {
            ((*count as f64 / max_count as f64) * bar_w as f64).round() as usize
        } else {
            0
        };
        let filled = filled.min(bar_w);

        let lbl_style = sty("HistLabel", &[]);
        lines.push(Line::from(vec![
            Span::styled(format!(" {:>7}", label), lbl_style),
            Span::styled("█".repeat(filled), lbl_style),
            Span::styled(
                "░".repeat(bar_w.saturating_sub(filled)),
                sty("HistEmpty", &[]),
            ),
            Span::styled(format!(" {:>5}", count), sty("HistSize", &[])),
        ]));
    }

    let para = Paragraph::new(lines);
    f.render_widget(para, area);
}

fn render_top_largest(f: &mut Frame, stats: &DetailStats, area: Rect) {
    if stats.top_largest.is_empty() {
        return;
    }

    let max_rows = area.height as usize;
    let mut lines = vec![Line::from(Span::styled(
        " ◈ Top Largest",
        sty("SectionTitle", &["amber"]),
    ))];

    for (i, (name, size, _depth)) in stats.top_largest.iter().enumerate().take(max_rows.saturating_sub(1)) {
        lines.push(Line::from(vec![
            Span::styled(format!(" {:>2}▸", i + 1), sty("Dim", &[])),
            Span::styled(format!("{:>9}", pretty_bytes(*size as f64)), sty("TopSize", &[])),
            Span::styled(
                format!(" {}", truncate_str(name, 20)),
                sty("TopName", &[]),
            ),
        ]));
    }

    let para = Paragraph::new(lines);
    f.render_widget(para, area);
}

// ── Delete confirmation dialog (HUD style) ──────────────────

fn render_delete_dialog(f: &mut Frame, state: &AppState) {
    let idx = match state.delete_target {
        Some(i) => i,
        None => return,
    };
    let item = &state.items[idx];
    let is_dir = item.has_children;
    let kind = if is_dir { "DIRECTORY" } else { "FILE" };
    let name = &item.name;
    let size = pretty_bytes(item.disk_size as f64);
    let w = 52usize;

    // HUD frame — manual box-drawing for sci-fi feel
    let top_left = "╔";
    let top_right = "╗";
    let bot_left = "╚";
    let bot_right = "╝";
    let h_line = "═";
    let _ = (); // border chars used inline below

    let text = vec![
        // Top border with title
        Line::from(vec![
            Span::styled(top_left, sty("DialogBorder", &[])),
            Span::styled(" DELETE CONFIRM ", sty("DialogTitle", &[])),
            Span::styled(h_line.repeat(w - 17), sty("DialogBorder", &[])),
            Span::styled(top_right, sty("DialogBorder", &[])),
        ]),
        // Divider
        Line::from(vec![
            Span::styled("║", sty("DialogDivider", &[])),
            Span::styled(
                format!(" {:<width$}", format!("▸ Target: {}", kind), width = w - 2),
                sty("DialogTarget", &[]),
            ),
            Span::styled("║", sty("DialogDivider", &[])),
        ]),
        // Name + size
        Line::from(vec![
            Span::styled("║", sty("DialogDivider", &[])),
            Span::styled(
                format!(" {:<width$}", format!("  {}  [{}]", name, size), width = w - 2),
                sty("DialogName", &[]),
            ),
            Span::styled("║", sty("DialogDivider", &[])),
        ]),
        // Divider with warning
        Line::from(vec![
            Span::styled("║", sty("DialogDivider", &[])),
            Span::styled(
                format!(" {:<width$}", "⚠  This action cannot be undone.", width = w - 2),
                sty("DialogWarn", &[]),
            ),
            Span::styled("║", sty("DialogDivider", &[])),
        ]),
        // Bottom border
        Line::from(vec![
            Span::styled(bot_left, sty("DialogBorder", &[])),
            Span::styled(h_line.repeat(w - 2), sty("DialogBorder", &[])),
            Span::styled(bot_right, sty("DialogBorder", &[])),
        ]),
        // Prompt below the box
        Line::from(vec![
            Span::styled("  [", sty("DialogKey", &[])),
            Span::styled("y", sty("DialogConfirm", &[])),
            Span::styled("] confirm   [", sty("DialogKey", &[])),
            Span::styled("n/Esc", sty("DialogAbort", &[])),
            Span::styled("] abort", sty("DialogKey", &[])),
        ]),
    ];

    let dialog_width = (w + 2) as usize;
    let dialog_height = 6usize;
    let area = centered_rect(dialog_width, dialog_height, f.area());

    f.render_widget(Clear, area);
    let para = Paragraph::new(text).style(sty("Dialog", &[]));
    f.render_widget(para, area);
}

/// Return a centered Rect of the given width/height within `r`.
fn centered_rect(width: usize, height: usize, r: Rect) -> Rect {
    let x = (r.width as usize).saturating_sub(width) / 2;
    let y = (r.height as usize).saturating_sub(height) / 2;
    Rect::new(
        x as u16,
        y as u16,
        width.min(r.width as usize) as u16,
        height.min(r.height as usize) as u16,
    )
}

// ── Status bar ─────────────────────────────────────────────

fn render_status(f: &mut Frame, state: &AppState, area: Rect) {
    let hidden_cls: &[&str] = if state.show_hidden { &["on"] } else { &[] };
    let status = Line::from(vec![
        Span::styled(" ▌", sty("StatusGlyph", &[])),
        Span::styled(
            format!("{}", state.visible.len()),
            sty("StatusCount", &[]),
        ),
        Span::styled("▐ ", sty("StatusGlyph", &[])),
        Span::styled("q", sty("StatusKey", &[])),
        Span::styled("uit ", sty("StatusHint", &[])),
        Span::styled("/", sty("StatusKey", &[])),
        Span::styled("find ", sty("StatusHint", &[])),
        Span::styled(".", sty("StatusHidden", hidden_cls)),
        Span::styled("hide ", sty("StatusHint", &[])),
        Span::styled("x", sty("StatusKey", &[])),
        Span::styled("del ", sty("StatusHint", &[])),
        Span::styled("a", sty("StatusKey", &[])),
        Span::styled("size ", sty("StatusHint", &[])),
        Span::styled("d", sty("StatusKey", &[])),
        Span::styled("cd ", sty("StatusHint", &[])),
        Span::styled("u", sty("StatusKey", &[])),
        Span::styled("up", sty("StatusHint", &[])),
    ]);
    let para = Paragraph::new(status).style(sty("Status", &[]));
    f.render_widget(para, area);
}

// ── Loading ────────────────────────────────────────────────
//
// The splash is a procedural, per-frame animation (cycling title glow,
// traveling scan pulse, drifting particles). It is intentionally NOT driven by
// the CSS theme — these colors are animation, not discrete themable states.

const SPINNER: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// Wave characters for the animated line.
const WAVE_CHARS: &[char] = &['░', '▒', '▓', '█', '▓', '▒'];
/// Glow colors for the title.
const GLOW_COLORS: &[Color] = &[
    Color::Cyan,
    Color::Rgb(0, 230, 230),
    Color::Rgb(80, 255, 255),
    Color::White,
    Color::Rgb(80, 255, 255),
    Color::Rgb(0, 230, 230),
];

fn render_loading(f: &mut Frame, state: &AppState) {
    let frame = state.loading_frame;
    let area = f.area();
    let width = area.width as usize;
    let height = area.height as usize;

    // Clear screen to black
    f.render_widget(
        Paragraph::new("").style(Style::default().bg(Color::Black)),
        area,
    );

    let center_y = height / 2;
    let mut lines: Vec<Line> = Vec::new();

    // Padding to center vertically (we'll draw ~11 lines)
    let splash_height = 13;
    let top_pad = center_y.saturating_sub(splash_height / 2);

    // ── Top wave line ──
    lines.push(make_wave_line(width, frame, Color::Rgb(0, 80, 80)));

    // ── Spacer ──
    lines.push(Line::raw(" ".repeat(width)));

    // ── Title: D I S K  C L E A N E R with glow ──
    let title = "D I S K  C L E A N E R";
    let glow_idx = frame % GLOW_COLORS.len();
    let glow_color = GLOW_COLORS[glow_idx];
    let title_pad = (width / 2).saturating_sub(title.len() / 2);
    let mut title_spans = vec![Span::raw(" ".repeat(title_pad))];
    for (i, ch) in title.chars().enumerate() {
        // Each letter gets a slight wave in brightness
        let letter_frame = (frame + i) % GLOW_COLORS.len();
        let letter_color = if i % 2 == 0 { glow_color } else { GLOW_COLORS[letter_frame] };
        title_spans.push(Span::styled(
            ch.to_string(),
            Style::default()
                .fg(letter_color)
                .add_modifier(Modifier::BOLD),
        ));
    }
    lines.push(Line::from(title_spans));

    // ── Subtitle ──
    let sub = "disk usage analyzer and cleaner";
    let sub_pad = (width / 2).saturating_sub(sub.len() / 2);
    lines.push(Line::from(vec![
        Span::raw(" ".repeat(sub_pad)),
        Span::styled(
            sub,
            Style::default().fg(Color::Rgb(80, 80, 100)),
        ),
    ]));

    // ── Spacer ──
    lines.push(Line::raw(" ".repeat(width)));

    // ── Animated scan bar ──
    lines.push(make_scan_bar(width, frame));

    // ── Spacer ──
    lines.push(Line::raw(" ".repeat(width)));

    // ── Scanning message with spinner ──
    let spinner = SPINNER[frame % SPINNER.len()];
    let dots = ".".repeat((frame % 4) + 1);
    let msg = format!("{} Scanning {}{}", spinner, state.loading_message, dots);
    let msg_pad = (width / 2).saturating_sub(msg.len() / 2);
    lines.push(Line::from(vec![
        Span::raw(" ".repeat(msg_pad)),
        Span::styled(
            spinner.to_string(),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" Scanning {}{}", state.loading_message, dots),
            Style::default().fg(Color::Rgb(180, 180, 200)),
        ),
    ]));

    // ── AI processing particles ──
    lines.push(make_particles_line(width, frame));

    // ── Bottom wave line ──
    lines.push(make_wave_line(width, frame + 3, Color::Rgb(0, 80, 80)));

    // ── Spacer with hint ──
    lines.push(Line::raw(" ".repeat(width)));
    let quit_hint = "press q to cancel";
    let quit_pad = (width / 2).saturating_sub(quit_hint.len() / 2);
    lines.push(Line::from(vec![
        Span::raw(" ".repeat(quit_pad)),
        Span::styled(
            quit_hint,
            Style::default().fg(Color::Rgb(60, 60, 80)),
        ),
    ]));

    // Render with top padding
    let mut all_lines: Vec<Line> = (0..top_pad).map(|_| Line::raw(" ".repeat(width))).collect();
    all_lines.extend(lines);
    // Truncate to screen height
    all_lines.truncate(height);

    let para = Paragraph::new(all_lines).style(Style::default().bg(Color::Black));
    f.render_widget(para, area);
}

/// Create an animated wave line across the full width.
fn make_wave_line(width: usize, frame: usize, color: Color) -> Line<'static> {
    let mut spans = Vec::new();
    let mut buf = String::new();
    for i in 0..width {
        let wave_idx = (i + frame) % WAVE_CHARS.len();
        buf.push(WAVE_CHARS[wave_idx]);
    }
    spans.push(Span::styled(buf, Style::default().fg(color)));
    Line::from(spans)
}

/// Create a pulsing scan bar — bright pulse with fading tail traveling right.
fn make_scan_bar(width: usize, frame: usize) -> Line<'static> {
    let bar_width = width.clamp(10, 50);
    let pad = (width / 2).saturating_sub(bar_width / 2);

    // Pulse position: travels 0 → bar_width, then wraps
    let pulse_pos = frame % bar_width;

    // Trail length: how many chars behind the pulse glow
    let trail_len = 8usize;

    // Characters for the pulse and trail
    let pulse_chars = ['◈', '◆', '◇', '▪', '∙', '·', '·', '·'];
    let trail_colors: [Color; 8] = [
        Color::White,
        Color::Rgb(100, 255, 255),
        Color::Rgb(0, 210, 210),
        Color::Rgb(0, 150, 150),
        Color::Rgb(0, 100, 100),
        Color::Rgb(0, 60, 60),
        Color::Rgb(0, 35, 35),
        Color::Rgb(0, 20, 20),
    ];

    let mut spans = vec![Span::raw(" ".repeat(pad))];

    // Left cap
    spans.push(Span::styled("╺", Style::default().fg(Color::Rgb(0, 100, 100))));

    for i in 0..bar_width {
        // Distance behind the pulse (wrapping)
        let dist = (pulse_pos + bar_width - i) % bar_width;
        if dist < trail_len {
            let ci = dist;
            spans.push(Span::styled(
                pulse_chars[ci].to_string(),
                Style::default().fg(trail_colors[ci]),
            ));
        } else {
            spans.push(Span::styled("─", Style::default().fg(Color::Rgb(15, 20, 30))));
        }
    }

    // Right cap
    spans.push(Span::styled("╸", Style::default().fg(Color::Rgb(0, 100, 100))));

    Line::from(spans)
}

/// Create a line of floating AI-style particles.
fn make_particles_line(width: usize, frame: usize) -> Line<'static> {
    let chars = ['·', '◦', '◇', '◆', '◇', '◦'];
    let mut spans = Vec::new();
    let mut buf = String::new();

    for i in 0..width {
        // Sparse particles that move
        let pos = (i + frame * 2) % 17;
        if pos == 0 {
            let ci = (i + frame) % chars.len();
            let brightness = match chars[ci] {
                '◆' => Color::Cyan,
                '◇' => Color::Rgb(0, 100, 100),
                '◦' => Color::Rgb(0, 60, 60),
                _ => Color::Rgb(0, 30, 30),
            };
            if !buf.is_empty() {
                spans.push(Span::styled(
                    std::mem::take(&mut buf),
                    Style::default().fg(Color::Rgb(0, 30, 30)),
                ));
            }
            spans.push(Span::styled(
                chars[ci].to_string(),
                Style::default().fg(brightness),
            ));
        } else {
            buf.push(' ');
        }
    }
    if !buf.is_empty() {
        spans.push(Span::raw(buf));
    }
    Line::from(spans)
}

// ── Helpers ────────────────────────────────────────────────

fn pretty_bytes(bytes: f64) -> String {
    pretty_bytes::converter::convert(bytes)
}

fn truncate_str(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_chars.saturating_sub(1)).collect();
        format!("{}…", truncated)
    }
}

#[cfg(test)]
mod tests {
    //! Render the CSS-driven UI into a TestBackend buffer and assert that key
    //! cells resolve to the exact palette colors they had before the refactor.
    //! This is the regression guard for "the UI must not change": if a CSS
    //! value drifts or a selector stops matching, these assertions fail.

    use super::*;
    use crate::DiskItem;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn render_buffer() -> ratatui::buffer::Buffer {
        let root = DiskItem {
            name: "root".into(),
            disk_size: 1000,
            children: Some(vec![
                DiskItem {
                    name: "big".into(),
                    disk_size: 800,
                    children: Some(vec![]),
                },
                DiskItem {
                    name: "a.txt".into(),
                    disk_size: 200,
                    children: None,
                },
            ]),
        };
        let mut state = AppState::from_disk_item_with_apparent(root, "/root".into(), 1000, false);
        let detail = state.detail_stats_cloned();

        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render(f, &state, &detail)).unwrap();
        terminal.backend().buffer().clone()
    }

    #[test]
    fn title_bar_uses_theme_colors() {
        let buf = render_buffer();
        // Row 0: " ▌disk cleaner…" — 'd' at x=2 is accent-cyan on bg-deep.
        let d = &buf[(2, 0)];
        assert_eq!(d.fg, Color::Rgb(0, 210, 210), "title 'disk cleaner' fg");
        assert_eq!(d.bg, Color::Rgb(8, 10, 18), "title bg");
    }

    #[test]
    fn selected_root_row_colors() {
        let buf = render_buffer();
        // Row 1: root item (depth 0, selected). Indicator '◉' at x=0.
        let ind = &buf[(0, 1)];
        assert_eq!(ind.fg, Color::Rgb(80, 255, 255), "selected indicator fg (glow)");
        assert_eq!(ind.bg, Color::Rgb(0, 35, 50), "selected row bg (sel-bg)");
        // pct '1' of "100.0%" at x=3 — root threshold → green.
        let pct = &buf[(3, 1)];
        assert_eq!(pct.fg, Color::Rgb(60, 230, 130), "root pct fg (green)");
        assert_eq!(pct.bg, Color::Rgb(0, 35, 50), "pct cell bg");
    }

    #[test]
    fn detail_panel_left_border_is_dim() {
        let buf = render_buffer();
        // Right panel starts at x=66 (55% of 120); its left border is border-dim.
        let border = &buf[(66, 10)];
        assert_eq!(border.symbol(), "│", "left border glyph");
        assert_eq!(border.fg, Color::Rgb(0, 50, 55), "detail left border fg (border-dim)");
    }

    #[test]
    fn status_bar_glyph_is_border_bright() {
        let buf = render_buffer();
        // Status bar is the last row (39); '▌' glyph at x=1.
        let glyph = &buf[(1, 39)];
        assert_eq!(glyph.fg, Color::Rgb(0, 120, 120), "status glyph fg (border-bright)");
    }

    #[test]
    fn non_selected_item_uses_panel_bg() {
        let buf = render_buffer();
        // Second visible row (the 'big' child) is not selected → bg-panel.
        // It sits at row 2. 'b' of "big/" — find any painted cell on row 2.
        let cell = (0..66)
            .map(|x| &buf[(x, 2)])
            .find(|c| c.fg == Color::Rgb(0, 190, 190))
            .expect("dir name in accent dir-cyan exists on row 2");
        assert_eq!(cell.bg, Color::Rgb(12, 15, 25), "non-selected row bg (bg-panel)");
    }
}
