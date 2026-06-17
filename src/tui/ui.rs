use std::sync::LazyLock;
use std::time::SystemTime;

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph},
};
use ratatui_style::{NodeRef, Stylesheet, css};

use super::app::{AppState, AppMode, DetailStats, ProjectEntry};

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
pub fn render(f: &mut Frame, state: &AppState, detail: &DetailStats, project_detail: &[(String, u64)]) {
    let mode_loading = match state.mode {
        AppMode::Disk => state.loading,
        AppMode::Projects => state.projects_loading,
    };
    if mode_loading {
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
        .constraints(match state.mode {
            AppMode::Disk => [Constraint::Percentage(55), Constraint::Percentage(45)],
            AppMode::Projects => [Constraint::Percentage(58), Constraint::Percentage(42)],
        })
        .split(outer[1]);

    match state.mode {
        AppMode::Disk => {
            render_list(f, state, main[0]);
            render_detail(f, detail, main[1]);
        }
        AppMode::Projects => {
            render_projects_list(f, state, main[0]);
            render_projects_detail(f, state, project_detail, main[1]);
        }
    }
    render_status(f, state, outer[2]);

    // Overlays: confirmation dialogs
    if state.delete_target.is_some() {
        render_delete_dialog(f, state);
    }
    if state.clean_target.is_some() {
        render_clean_dialog(f, state);
    }
    if state.clean_all_pending {
        render_clean_all_dialog(f, state);
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

    if state.mode == AppMode::Projects {
        spans.push(Span::styled(
            format!(
                " ◈ projects:{} · {} reclaimable",
                state.projects.len(),
                pretty_bytes(state.total_reclaimable() as f64)
            ),
            sty("Title", &["apparent"]),
        ));
    }

    // Transient error (e.g. failed clean/delete), shown until next keypress.
    if let Some(err) = &state.error_message {
        let w = area.width as usize;
        spans.push(Span::styled(
            format!(" ! {}", truncate_str(err, w.saturating_sub(40))),
            Style::default().fg(Color::Rgb(255, 90, 90)),
        ));
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

// ── Projects mode: list + detail ───────────────────────────

/// Render the left-hand project list (Kondo-style).
fn render_projects_list(f: &mut Frame, state: &AppState, area: Rect) {
    if state.projects.is_empty() {
        let para = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                " ◈ No projects with reclaimable artifacts",
                sty("Empty", &[]),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "   Build projects (Cargo, Node, Maven, …) found",
                sty("Dim", &[]),
            )),
            Line::from(Span::styled(
                "   beneath this path appear here.",
                sty("Dim", &[]),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "   Press p / Tab to return to the disk view.",
                sty("Dim", &[]),
            )),
        ])
        .style(sty("TreePanel", &[]));
        f.render_widget(para, area);
        return;
    }

    let viewport_height = area.height as usize;
    let max_reclaim = state.projects_max_reclaimable();
    let lines: Vec<Line> = state
        .projects
        .iter()
        .enumerate()
        .skip(state.projects_scroll)
        .take(viewport_height)
        .map(|(i, e)| render_project_row(state, i, e, max_reclaim))
        .collect();
    let para = Paragraph::new(lines).style(sty("TreePanel", &[]));
    f.render_widget(para, area);
}

fn render_project_row(
    state: &AppState,
    i: usize,
    entry: &ProjectEntry,
    max_reclaim: u64,
) -> Line<'static> {
    let is_selected = i == state.projects_selected;
    let sel_cls: &[&str] = if is_selected { &["selected"] } else { &[] };
    let row_bg = sty("TreeItem", sel_cls).bg.unwrap_or(Color::Reset);

    let filled = if max_reclaim > 0 {
        ((entry.reclaimable as f64 / max_reclaim as f64) * BAR_WIDTH as f64).round() as usize
    } else {
        0
    };
    let filled = filled.min(BAR_WIDTH);

    // Threshold color by absolute reclaimable size (1G / 100M).
    let pct_cls: &[&str] = if entry.reclaimable > 1_000_000_000 {
        &["high"]
    } else if entry.reclaimable > 100_000_000 {
        &["mid"]
    } else {
        &["low"]
    };
    let bar_style = sty("Pct", pct_cls).bg(row_bg);

    // Show the path relative to the directory Projects mode scanned from.
    let base = if state.projects_scan_root.is_empty() {
        state.root_path.as_str()
    } else {
        state.projects_scan_root.as_str()
    };
    let display_path: String = match entry.path.strip_prefix(base) {
        Some(rest) => {
            let r = rest.trim_start_matches('/');
            if r.is_empty() {
                entry.path.clone()
            } else {
                r.to_string()
            }
        }
        None => entry.path.clone(),
    };

    Line::from(vec![
        Span::styled(" ◈ ", sty("Indicator", sel_cls).bg(row_bg)),
        Span::styled("█".repeat(filled), bar_style),
        Span::styled(
            "░".repeat(BAR_WIDTH.saturating_sub(filled)),
            sty("Bar", &["empty"]).bg(row_bg),
        ),
        Span::styled(
            format!(" {:>10}", pretty_bytes(entry.reclaimable as f64)),
            sty("Size", &[]).bg(row_bg),
        ),
        Span::styled(
            format!(" {} ", entry.type_name),
            sty("TypeExt", &[]).bg(row_bg),
        ),
        Span::styled(display_path, sty("Name", &["dir"]).bg(row_bg)),
    ])
}

/// Render the right-hand detail for the selected project.
fn render_projects_detail(
    f: &mut Frame,
    state: &AppState,
    breakdown: &[(String, u64)],
    area: Rect,
) {
    let detail = THEME.compute(&NodeRef::new("DetailPanel"), None);
    let block = detail.to_block();
    let inner = block.inner(area);
    f.render_widget(block, area);

    if state.projects.is_empty() {
        let para = Paragraph::new(vec![
            Line::from(Span::styled(" ◈ Projects", sty("SectionTitle", &[]))),
            Line::from(""),
            Line::from(Span::styled("  Nothing reclaimable found.", sty("Empty", &[]))),
        ]);
        f.render_widget(para, inner);
        return;
    }

    let entry = &state.projects[state.projects_selected];
    let w = inner.width as usize;
    let total = entry.reclaimable;
    let max_dir = breakdown.iter().map(|(_, s)| *s).max().unwrap_or(1).max(1);
    let max_reclaim = state.projects_max_reclaimable();

    let mut lines: Vec<Line> = Vec::new();

    // Type name
    lines.push(Line::from(vec![
        Span::styled(" ◈ ", sty("Largest", &[])),
        Span::styled(entry.type_name.clone(), sty("SectionTitle", &[])),
    ]));

    // Path
    lines.push(Line::from(vec![
        Span::styled(" ", sty("Dim", &[])),
        Span::styled(truncate_str(&entry.path, w.saturating_sub(2)), sty("Crumb", &[])),
    ]));

    // Reclaimable gauge, scaled against the largest project.
    let gauge_w = w.saturating_sub(20);
    let filled = if max_reclaim > 0 && gauge_w > 0 {
        ((total as f64 / max_reclaim as f64) * gauge_w as f64).round() as usize
    } else {
        0
    };
    let filled = filled.min(gauge_w);
    let gauge_cls: &[&str] = if total > 1_000_000_000 {
        &["high"]
    } else if total > 100_000_000 {
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
            pretty_bytes(total as f64),
            sty("Gauge", gauge_cls).add_modifier(Modifier::BOLD),
        ),
    ]));

    // Last modified
    if let Some(mtime) = entry.last_modified {
        lines.push(Line::from(vec![
            Span::styled(" ", sty("Dim", &[])),
            Span::styled(
                format!("modified {}", relative_time(mtime)),
                sty("Dim", &[]),
            ),
        ]));
    }

    // Separator
    lines.push(Line::from(Span::styled(
        format!(" {}", "─".repeat(w.saturating_sub(2))),
        sty("Hr", &[]),
    )));

    // Artifact-directory breakdown
    lines.push(Line::from(Span::styled(
        " ◈ Artifact dirs",
        sty("SectionTitle", &["amber"]),
    )));
    if breakdown.is_empty() {
        lines.push(Line::from(Span::styled("  (none)", sty("Empty", &[]))));
    } else {
        let bar_w = 8;
        for (name, size) in breakdown.iter().take(8) {
            let filled = if max_dir > 0 {
                ((*size as f64 / max_dir as f64) * bar_w as f64).round() as usize
            } else {
                0
            };
            let filled = filled.min(bar_w);
            lines.push(Line::from(vec![
                Span::styled(
                    format!(" {:<16}", truncate_str(name, 16)),
                    sty("TypeExt", &[]),
                ),
                Span::styled("█".repeat(filled), sty("TypeExt", &[])),
                Span::styled(
                    "░".repeat(bar_w.saturating_sub(filled)),
                    sty("Bar", &["empty"]),
                ),
                Span::styled(
                    format!(" {:>9}", pretty_bytes(*size as f64)),
                    sty("TypeSize", &[]),
                ),
                Span::styled(" ⟳", sty("Largest", &[])),
            ]));
        }
    }

    // Clean hint
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled(" [", sty("DialogKey", &[])),
        Span::styled("c", sty("DialogConfirm", &[])),
        Span::styled("] clean ", sty("DialogKey", &[])),
        Span::styled(pretty_bytes(total as f64), sty("StatSize", &[])),
        Span::styled(" reclaimable", sty("Dim", &[])),
    ]));

    f.render_widget(Paragraph::new(lines), inner);
}

/// Format a `SystemTime` as a coarse relative age (e.g. "3h ago").
fn relative_time(t: SystemTime) -> String {
    match SystemTime::now().duration_since(t) {
        Ok(d) => {
            let s = d.as_secs();
            if s < 60 {
                format!("{}s ago", s)
            } else if s < 3600 {
                format!("{}m ago", s / 60)
            } else if s < 86400 {
                format!("{}h ago", s / 3600)
            } else {
                format!("{}d ago", s / 86400)
            }
        }
        Err(_) => "just now".to_string(),
    }
}

// ── Delete confirmation dialog (HUD style) ──────────────────

/// Outer width (in cells) of the delete-confirm dialog box.
const DELETE_DIALOG_W: usize = 54;

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
    let inner_w = DELETE_DIALOG_W.saturating_sub(2);

    // HUD frame — box-drawing via boxed_line keeps top/mid/bottom equally wide.
    let title = " DELETE CONFIRM ";

    // Truncate the name so the "  <name>  [<size>]" line never overflows inner_w.
    // Fixed overhead: 2 leading spaces + "  " before size + "[" + "]" = 5 chars,
    // plus the size itself.
    let overhead = 5 + size.chars().count();
    let name_max = inner_w.saturating_sub(overhead);
    let name_disp = truncate_str(name, name_max);
    let name_line = format!("  {}  [{}]", name_disp, size);

    let mut text: Vec<Line> = Vec::new();

    // Top border + title
    text.push(Line::from(vec![
        Span::styled("╔", sty("DialogBorder", &[])),
        Span::styled(title, sty("DialogTitle", &[])),
        Span::styled(
            "═".repeat(inner_w.saturating_sub(title.chars().count())),
            sty("DialogBorder", &[]),
        ),
        Span::styled("╗", sty("DialogBorder", &[])),
    ]));

    text.push(boxed_line(
        &format!("▸ Target: {}", kind),
        inner_w,
        sty("DialogTarget", &[]),
    ));
    text.push(boxed_line(&name_line, inner_w, sty("DialogName", &[])));
    text.push(boxed_line(
        "⚠  This action cannot be undone.",
        inner_w,
        sty("DialogWarn", &[]),
    ));

    // Bottom border
    text.push(Line::from(vec![
        Span::styled("╚", sty("DialogBorder", &[])),
        Span::styled("═".repeat(inner_w), sty("DialogBorder", &[])),
        Span::styled("╝", sty("DialogBorder", &[])),
    ]));

    // Prompt below the box
    text.push(Line::from(vec![
        Span::styled("  [", sty("DialogKey", &[])),
        Span::styled("y", sty("DialogConfirm", &[])),
        Span::styled("] confirm   [", sty("DialogKey", &[])),
        Span::styled("n/Esc", sty("DialogAbort", &[])),
        Span::styled("] abort", sty("DialogKey", &[])),
    ]));

    let area = centered_rect(DELETE_DIALOG_W, text.len(), f.area());
    f.render_widget(Clear, area);
    let para = Paragraph::new(text).style(sty("Dialog", &[]));
    f.render_widget(para, area);
}

// ── Clean confirmation dialog (projects mode) ──────────────

/// Outer width (in cells) of the clean-project dialog box.
const CLEAN_DIALOG_W: usize = 58;

/// Render the HUD-style confirmation for cleaning a project's artifacts.
fn render_clean_dialog(f: &mut Frame, state: &AppState) {
    let idx = match state.clean_target {
        Some(i) => i,
        None => return,
    };
    let entry = match state.projects.get(idx) {
        Some(e) => e,
        None => return,
    };
    let total = pretty_bytes(entry.reclaimable as f64);
    let inner_w = CLEAN_DIALOG_W.saturating_sub(2);

    // Show up to 5 artifact dirs, summarize the rest.
    let shown: Vec<&(String, u64)> = state.clean_breakdown.iter().take(5).collect();
    let extra = state.clean_breakdown.len().saturating_sub(shown.len());

    let title = " CLEAN PROJECT ";
    let mut text: Vec<Line> = Vec::new();

    // Top border + title
    text.push(Line::from(vec![
        Span::styled("╔", sty("DialogBorder", &[])),
        Span::styled(title, sty("DialogTitle", &[])),
        Span::styled(
            "═".repeat(CLEAN_DIALOG_W.saturating_sub(2).saturating_sub(title.chars().count())),
            sty("DialogBorder", &[]),
        ),
        Span::styled("╗", sty("DialogBorder", &[])),
    ]));

    text.push(boxed_line(
        &format!("▸ {}", entry.type_name),
        inner_w,
        sty("DialogTarget", &[]),
    ));
    text.push(boxed_line(
        &format!("  {}", truncate_str(&entry.path, inner_w.saturating_sub(2))),
        inner_w,
        sty("DialogName", &[]),
    ));
    text.push(boxed_line("", inner_w, sty("DialogDivider", &[])));

    for (name, size) in &shown {
        text.push(boxed_line(
            &format!(
                "  {:<14} {:>9}",
                truncate_str(name, 14),
                pretty_bytes(*size as f64)
            ),
            inner_w,
            sty("DialogName", &[]),
        ));
    }
    if extra > 0 {
        text.push(boxed_line(
            &format!("  +{} more", extra),
            inner_w,
            sty("Dim", &[]),
        ));
    }

    text.push(boxed_line(
        &format!(
            "⚠  Remove {} dir(s) — {}. Cannot be undone.",
            state.clean_breakdown.len(),
            total
        ),
        inner_w,
        sty("DialogWarn", &[]),
    ));

    // Bottom border
    text.push(Line::from(vec![
        Span::styled("╚", sty("DialogBorder", &[])),
        Span::styled("═".repeat(inner_w), sty("DialogBorder", &[])),
        Span::styled("╝", sty("DialogBorder", &[])),
    ]));

    // Prompt below the box
    text.push(Line::from(vec![
        Span::styled("  [", sty("DialogKey", &[])),
        Span::styled("y", sty("DialogConfirm", &[])),
        Span::styled("] confirm   [", sty("DialogKey", &[])),
        Span::styled("n/Esc", sty("DialogAbort", &[])),
        Span::styled("] abort", sty("DialogKey", &[])),
    ]));

    let area = centered_rect(CLEAN_DIALOG_W, text.len(), f.area());
    f.render_widget(Clear, area);
    let para = Paragraph::new(text).style(sty("Dialog", &[]));
    f.render_widget(para, area);
}

/// Confirmation HUD for cleaning ALL projects at once.
fn render_clean_all_dialog(f: &mut Frame, state: &AppState) {
    if state.projects.is_empty() {
        return;
    }
    let n = state.projects.len();
    let total = pretty_bytes(state.total_reclaimable() as f64);
    let inner_w = CLEAN_DIALOG_W.saturating_sub(2);

    // Distinct artifact-dir names across all projects, for a preview.
    let mut dirs: Vec<&str> = Vec::new();
    for e in &state.projects {
        for d in &e.artifact_dir_names {
            if !dirs.contains(&d.as_str()) {
                dirs.push(d.as_str());
            }
        }
    }
    let extra_dirs = dirs.len().saturating_sub(5);
    let dir_preview = dirs.iter().take(5).copied().collect::<Vec<_>>().join(", ");

    let title = " CLEAN ALL PROJECTS ";
    let mut text: Vec<Line> = Vec::new();

    text.push(Line::from(vec![
        Span::styled("╔", sty("DialogBorder", &[])),
        Span::styled(title, sty("DialogTitle", &[])),
        Span::styled(
            "═".repeat(
                CLEAN_DIALOG_W
                    .saturating_sub(2)
                    .saturating_sub(title.chars().count()),
            ),
            sty("DialogBorder", &[]),
        ),
        Span::styled("╗", sty("DialogBorder", &[])),
    ]));

    text.push(boxed_line(
        &format!("▸ {} projects", n),
        inner_w,
        sty("DialogTarget", &[]),
    ));
    text.push(boxed_line(
        &format!("  Reclaims {} total", total),
        inner_w,
        sty("StatSize", &[]),
    ));
    text.push(boxed_line("", inner_w, sty("DialogDivider", &[])));
    let preview = if extra_dirs > 0 {
        format!("  removes: {}… (+{} more)", dir_preview, extra_dirs)
    } else {
        format!("  removes: {}", dir_preview)
    };
    text.push(boxed_line(&preview, inner_w, sty("DialogName", &[])));
    text.push(boxed_line(
        "⚠  Cannot be undone.",
        inner_w,
        sty("DialogWarn", &[]),
    ));

    text.push(Line::from(vec![
        Span::styled("╚", sty("DialogBorder", &[])),
        Span::styled("═".repeat(inner_w), sty("DialogBorder", &[])),
        Span::styled("╝", sty("DialogBorder", &[])),
    ]));
    text.push(Line::from(vec![
        Span::styled("  [", sty("DialogKey", &[])),
        Span::styled("y", sty("DialogConfirm", &[])),
        Span::styled("] confirm   [", sty("DialogKey", &[])),
        Span::styled("n/Esc", sty("DialogAbort", &[])),
        Span::styled("] abort", sty("DialogKey", &[])),
    ]));

    let area = centered_rect(CLEAN_DIALOG_W, text.len(), f.area());
    f.render_widget(Clear, area);
    let para = Paragraph::new(text).style(sty("Dialog", &[]));
    f.render_widget(para, area);
}

/// One row of the clean dialog: `║<content padded to inner_w>║`.
fn boxed_line(content: &str, inner_w: usize, style: Style) -> Line<'static> {
    let content = truncate_str(content, inner_w);
    Line::from(vec![
        Span::styled("║", sty("DialogDivider", &[])),
        Span::styled(format!("{:<width$}", content, width = inner_w), style),
        Span::styled("║", sty("DialogDivider", &[])),
    ])
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
    let count = match state.mode {
        AppMode::Disk => state.visible.len(),
        AppMode::Projects => state.projects.len(),
    };

    // Each hint is a bright key followed by a dim label, pushed into one line.
    let mut spans: Vec<Span> = vec![
        Span::styled(" ▌", sty("StatusGlyph", &[])),
        Span::styled(format!("{}", count), sty("StatusCount", &[])),
        Span::styled("▐ ", sty("StatusGlyph", &[])),
    ];

    let hints: &[(&str, &str)] = match state.mode {
        AppMode::Disk => &[
            ("p", "roj "),
            ("/", "find "),
            ("x", "del "),
            ("a", "size "),
            ("d", "cd "),
            ("u", "up "),
            ("r", "efresh "),
            ("q", "uit"),
        ],
        AppMode::Projects => &[
            ("p", "disk "),
            ("c", "lean "),
            ("C", "lean-all "),
            ("j/k", "move "),
            ("g/G", "top/bot "),
            ("r", "efresh "),
            ("q", "uit"),
        ],
    };
    for (key, label) in hints {
        spans.push(Span::styled(*key, sty("StatusKey", &[])));
        spans.push(Span::styled(*label, sty("StatusHint", &[])));
    }

    let para = Paragraph::new(Line::from(spans)).style(sty("Status", &[]));
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
        terminal
            .draw(|f| render(f, &state, &detail, &[]))
            .unwrap();
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
