use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use super::app::{AppState, DetailStats};

/// Width of the percentage bar in characters.
const BAR_WIDTH: usize = 12;

// ── Sci-fi color palette ────────────────────────────────────
const BG_DEEP: Color = Color::Rgb(8, 10, 18);
const BG_PANEL: Color = Color::Rgb(12, 15, 25);
const BORDER_DIM: Color = Color::Rgb(0, 50, 55);
const BORDER_BRIGHT: Color = Color::Rgb(0, 120, 120);
const ACCENT_CYAN: Color = Color::Rgb(0, 210, 210);
const ACCENT_GLOW: Color = Color::Rgb(80, 255, 255);
const TEXT_DIM: Color = Color::Rgb(55, 65, 85);
const TEXT_MID: Color = Color::Rgb(140, 150, 170);
const TEXT_BRIGHT: Color = Color::Rgb(200, 210, 225);
const AMBER: Color = Color::Rgb(255, 195, 60);
const AMBER_DIM: Color = Color::Rgb(160, 120, 40);
const PURPLE: Color = Color::Rgb(170, 80, 255);
const PURPLE_DIM: Color = Color::Rgb(80, 40, 120);
const RED_ACCENT: Color = Color::Rgb(255, 70, 70);
const GREEN_ACCENT: Color = Color::Rgb(60, 230, 130);
const SEL_BG: Color = Color::Rgb(0, 35, 50);
const BAR_EMPTY: Color = Color::Rgb(18, 22, 35);
const GAUGE_BG: Color = Color::Rgb(20, 25, 40);

/// Render the full TUI layout.
pub fn render(f: &mut Frame, state: &AppState, detail: &DetailStats) {
    if state.loading {
        render_loading(f, state);
        return;
    }

    // Clear entire frame to deep background
    f.render_widget(
        Paragraph::new("").style(Style::default().bg(BG_DEEP)),
        f.area(),
    );

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
        Span::styled(
            " ▌",
            Style::default().fg(ACCENT_GLOW),
        ),
        Span::styled(
            "disk cleaner",
            Style::default()
                .fg(ACCENT_CYAN)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            " ▐ ",
            Style::default().fg(ACCENT_GLOW),
        ),
        Span::styled(
            format!("{} ", state.root_path),
            Style::default().fg(Color::Rgb(80, 190, 190)),
        ),
        Span::styled(
            format!("{}", total),
            Style::default()
                .fg(AMBER)
                .add_modifier(Modifier::BOLD),
        ),
    ];

    if state.apparent {
        spans.push(Span::styled(
            " [apparent]",
            Style::default().fg(PURPLE),
        ));
    }

    if !state.search_query.is_empty() || state.search_active {
        spans.push(Span::styled(
            format!(" /{}", state.search_query),
            Style::default().fg(GREEN_ACCENT),
        ));
        if state.search_active {
            spans.push(Span::styled("█", Style::default().fg(GREEN_ACCENT)));
        }
    }

    let para = Paragraph::new(Line::from(spans)).style(Style::default().bg(BG_DEEP));
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

    let para = Paragraph::new(lines).style(Style::default().bg(BG_PANEL));
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

    let pct_color = if item.depth == 0 {
        GREEN_ACCENT
    } else if pct > 20.0 {
        RED_ACCENT
    } else if pct > 10.0 {
        AMBER
    } else {
        ACCENT_CYAN
    };

    let size_str = pretty_bytes(item.disk_size as f64);
    let name = if item.has_children {
        format!("{}/", item.name)
    } else {
        item.name.clone()
    };

    let name_color = if is_selected {
        ACCENT_GLOW
    } else if item.has_children {
        Color::Rgb(0, 190, 190)
    } else {
        TEXT_BRIGHT
    };

    let indicator_color = if is_selected { ACCENT_GLOW } else { ACCENT_CYAN };

    let sel_bg = if is_selected { SEL_BG } else { BG_PANEL };

    Line::from(vec![
        Span::styled(
            format!("{}{}", indent, indicator),
            Style::default().fg(indicator_color).bg(sel_bg),
        ),
        Span::styled(
            format!("{:>6.1}% ", pct),
            Style::default().fg(pct_color).bg(sel_bg),
        ),
        Span::styled(
            "█".repeat(filled),
            Style::default().fg(pct_color).bg(sel_bg),
        ),
        Span::styled(
            "░".repeat(BAR_WIDTH - filled),
            Style::default().fg(BAR_EMPTY).bg(sel_bg),
        ),
        Span::styled(
            format!(" {:>10}", size_str),
            Style::default().fg(AMBER_DIM).bg(sel_bg),
        ),
        Span::styled(
            format!(" {}", name),
            Style::default().fg(name_color).bg(sel_bg).add_modifier(
                if is_selected { Modifier::BOLD } else { Modifier::empty() },
            ),
        ),
    ])
}

// ── Right: Detail panel ────────────────────────────────────

fn render_detail(f: &mut Frame, stats: &DetailStats, area: Rect) {
    let block = Block::default()
        .borders(Borders::LEFT)
        .border_style(Style::default().fg(BORDER_DIM))
        .style(Style::default().bg(BG_PANEL));
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
        Span::styled(" ", Style::default().fg(TEXT_DIM)),
        Span::styled(
            truncate_str(&crumb, w.saturating_sub(2)),
            Style::default()
                .fg(ACCENT_CYAN)
                .add_modifier(Modifier::BOLD),
        ),
    ]));

    // Proportion gauge: big visual bar of total share
    let gauge_w = w.saturating_sub(14);
    let filled = if stats.pct_of_total > 0.0 && gauge_w > 0 {
        ((stats.pct_of_total / 100.0) * gauge_w as f64).round() as usize
    } else {
        0
    };
    let filled = filled.min(gauge_w);
    let gauge_color = if stats.pct_of_total > 50.0 {
        RED_ACCENT
    } else if stats.pct_of_total > 20.0 {
        AMBER
    } else {
        ACCENT_CYAN
    };
    lines.push(Line::from(vec![
        Span::styled(" ╺", Style::default().fg(BORDER_BRIGHT)),
        Span::styled(
            "━".repeat(filled),
            Style::default().fg(gauge_color),
        ),
        Span::styled(
            "─".repeat(gauge_w.saturating_sub(filled)),
            Style::default().fg(GAUGE_BG),
        ),
        Span::styled("╸ ", Style::default().fg(BORDER_BRIGHT)),
        Span::styled(
            format!("{:.1}%", stats.pct_of_total),
            Style::default()
                .fg(gauge_color)
                .add_modifier(Modifier::BOLD),
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
        Span::styled(" ", Style::default().fg(TEXT_DIM)),
        Span::styled(
            size_str,
            Style::default().fg(AMBER).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" │ ", Style::default().fg(BORDER_DIM)),
        Span::styled(format!("{} files", stats.file_count), Style::default().fg(TEXT_MID)),
        Span::styled(" │ ", Style::default().fg(BORDER_DIM)),
        Span::styled(format!("{} dirs", stats.dir_count), Style::default().fg(TEXT_MID)),
        Span::styled(" │ ", Style::default().fg(BORDER_DIM)),
        Span::styled(
            format!("avg {}", avg_str),
            Style::default().fg(TEXT_DIM),
        ),
    ]));

    // Largest child
    if let Some((ref name, sz)) = stats.largest_child {
        lines.push(Line::from(vec![
            Span::styled(" ◈ ", Style::default().fg(RED_ACCENT)),
            Span::styled(
                pretty_bytes(sz as f64),
                Style::default().fg(RED_ACCENT),
            ),
            Span::styled(
                format!(" {}", truncate_str(name, w.saturating_sub(16))),
                Style::default().fg(TEXT_BRIGHT),
            ),
        ]));
    }

    // Separator
    lines.push(Line::from(Span::styled(
        format!(" {}", "─".repeat(w.saturating_sub(2))),
        Style::default().fg(BORDER_DIM),
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
        Style::default()
            .fg(ACCENT_CYAN)
            .add_modifier(Modifier::BOLD),
    ))];

    if stats.type_distribution.is_empty() {
        lines.push(Line::from(Span::styled(
            "  (empty)",
            Style::default().fg(TEXT_DIM),
        )));
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

        lines.push(Line::from(vec![
            Span::styled(format!(" {:>8}", ext_display), Style::default().fg(ACCENT_CYAN)),
            Span::styled("█".repeat(filled), Style::default().fg(ACCENT_CYAN)),
            Span::styled("░".repeat(bar_w.saturating_sub(filled)), Style::default().fg(BAR_EMPTY)),
            Span::styled(
                format!(" {:>9}", pretty_bytes(*size as f64)),
                Style::default().fg(TEXT_BRIGHT),
            ),
            Span::styled(
                format!("({:>3})", count),
                Style::default().fg(TEXT_DIM),
            ),
        ]));
    }

    let para = Paragraph::new(lines);
    f.render_widget(para, area);
}

fn render_size_histogram(f: &mut Frame, stats: &DetailStats, area: Rect) {
    let max_rows = area.height as usize;
    let mut lines = vec![Line::from(Span::styled(
        " ◈ Size Dist",
        Style::default()
            .fg(PURPLE)
            .add_modifier(Modifier::BOLD),
    ))];

    if stats.size_histogram.is_empty() {
        lines.push(Line::from(Span::styled(
            "  (no files)",
            Style::default().fg(TEXT_DIM),
        )));
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

        lines.push(Line::from(vec![
            Span::styled(format!(" {:>7}", label), Style::default().fg(PURPLE)),
            Span::styled("█".repeat(filled), Style::default().fg(PURPLE)),
            Span::styled("░".repeat(bar_w.saturating_sub(filled)), Style::default().fg(PURPLE_DIM)),
            Span::styled(
                format!(" {:>5}", count),
                Style::default().fg(TEXT_BRIGHT),
            ),
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
        Style::default()
            .fg(AMBER)
            .add_modifier(Modifier::BOLD),
    ))];

    for (i, (name, size, _depth)) in stats.top_largest.iter().enumerate().take(max_rows.saturating_sub(1)) {
        lines.push(Line::from(vec![
            Span::styled(format!(" {:>2}▸", i + 1), Style::default().fg(TEXT_DIM)),
            Span::styled(format!("{:>9}", pretty_bytes(*size as f64)), Style::default().fg(AMBER)),
            Span::styled(
                format!(" {}", truncate_str(name, 20)),
                Style::default().fg(TEXT_BRIGHT),
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
            Span::styled(top_left, Style::default().fg(RED_ACCENT)),
            Span::styled(" DELETE CONFIRM ", Style::default().fg(RED_ACCENT).add_modifier(Modifier::BOLD)),
            Span::styled(h_line.repeat(w - 17), Style::default().fg(RED_ACCENT)),
            Span::styled(top_right, Style::default().fg(RED_ACCENT)),
        ]),
        // Divider
        Line::from(vec![
            Span::styled("║", Style::default().fg(Color::Rgb(120, 30, 30))),
            Span::styled(format!(" {:<width$}", format!("▸ Target: {}", kind), width = w - 2), Style::default().fg(RED_ACCENT)),
            Span::styled("║", Style::default().fg(Color::Rgb(120, 30, 30))),
        ]),
        // Name + size
        Line::from(vec![
            Span::styled("║", Style::default().fg(Color::Rgb(120, 30, 30))),
            Span::styled(format!(" {:<width$}", format!("  {}  [{}]", name, size), width = w - 2),
                Style::default().fg(ACCENT_GLOW).add_modifier(Modifier::BOLD)),
            Span::styled("║", Style::default().fg(Color::Rgb(120, 30, 30))),
        ]),
        // Divider with warning
        Line::from(vec![
            Span::styled("║", Style::default().fg(Color::Rgb(120, 30, 30))),
            Span::styled(format!(" {:<width$}", "⚠  This action cannot be undone.", width = w - 2), Style::default().fg(Color::Yellow)),
            Span::styled("║", Style::default().fg(Color::Rgb(120, 30, 30))),
        ]),
        // Bottom border
        Line::from(vec![
            Span::styled(bot_left, Style::default().fg(RED_ACCENT)),
            Span::styled(h_line.repeat(w - 2), Style::default().fg(RED_ACCENT)),
            Span::styled(bot_right, Style::default().fg(RED_ACCENT)),
        ]),
        // Prompt below the box
        Line::from(vec![
            Span::styled("  [", Style::default().fg(TEXT_DIM)),
            Span::styled("y", Style::default().fg(GREEN_ACCENT).add_modifier(Modifier::BOLD)),
            Span::styled("] confirm   [", Style::default().fg(TEXT_DIM)),
            Span::styled("n/Esc", Style::default().fg(RED_ACCENT).add_modifier(Modifier::BOLD)),
            Span::styled("] abort", Style::default().fg(TEXT_DIM)),
        ]),
    ];

    let dialog_width = (w + 2) as usize;
    let dialog_height = 6usize;
    let area = centered_rect(dialog_width, dialog_height, f.area());

    f.render_widget(Clear, area);
    let para = Paragraph::new(text).style(Style::default().bg(Color::Rgb(20, 4, 8)));
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
    let hidden_color = if state.show_hidden { GREEN_ACCENT } else { TEXT_DIM };
    let status = Line::from(vec![
        Span::styled(" ▌", Style::default().fg(BORDER_BRIGHT)),
        Span::styled(
            format!("{}", state.visible.len()),
            Style::default().fg(ACCENT_GLOW).add_modifier(Modifier::BOLD),
        ),
        Span::styled("▐ ", Style::default().fg(BORDER_BRIGHT)),
        Span::styled("q", Style::default().fg(ACCENT_CYAN).add_modifier(Modifier::BOLD)),
        Span::styled("uit ", Style::default().fg(TEXT_DIM)),
        Span::styled("/", Style::default().fg(ACCENT_CYAN).add_modifier(Modifier::BOLD)),
        Span::styled("find ", Style::default().fg(TEXT_DIM)),
        Span::styled(".", Style::default().fg(hidden_color).add_modifier(Modifier::BOLD)),
        Span::styled("hide ", Style::default().fg(TEXT_DIM)),
        Span::styled("x", Style::default().fg(ACCENT_CYAN).add_modifier(Modifier::BOLD)),
        Span::styled("del ", Style::default().fg(TEXT_DIM)),
        Span::styled("a", Style::default().fg(ACCENT_CYAN).add_modifier(Modifier::BOLD)),
        Span::styled("size ", Style::default().fg(TEXT_DIM)),
        Span::styled("d", Style::default().fg(ACCENT_CYAN).add_modifier(Modifier::BOLD)),
        Span::styled("cd ", Style::default().fg(TEXT_DIM)),
        Span::styled("u", Style::default().fg(ACCENT_CYAN).add_modifier(Modifier::BOLD)),
        Span::styled("up", Style::default().fg(TEXT_DIM)),
    ]);
    let para = Paragraph::new(status).style(Style::default().bg(BG_DEEP));
    f.render_widget(para, area);
}

// ── Loading ────────────────────────────────────────────────

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
