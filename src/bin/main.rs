use atty::Stream;
use clap::Parser;
use disk_cleaner::{analyze, DiskItem, FileInfo, ProjectAnalysis, ScanOptions};
use pretty_bytes::converter::convert as pretty_bytes;
use std::collections::BTreeSet;
use std::env;
use std::error::Error;
use std::io;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use std::time::SystemTime;
use termcolor::{Buffer, BufferWriter, Color, ColorChoice, ColorSpec, WriteColor};

const INDENT_COLOR: Option<Color> = Some(Color::Rgb(75, 75, 75));

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn print_logo(buffer: &mut Buffer) -> io::Result<()> {
    buffer.reset()?;
    write!(buffer, "  ")?;
    // Left half of border: bright cyan
    buffer.set_color(ColorSpec::new().set_fg(Some(Color::Cyan)))?;
    write!(buffer, "╺━━━━━━━━━━")?;
    // Right half of border: dark cyan
    buffer.set_color(ColorSpec::new().set_fg(Some(Color::Rgb(0, 139, 139))))?;
    writeln!(buffer, "━━━━━━━━━━━━━━━━━━━━━━━━━━━━╸")?;

    // Empty line with side bars
    buffer.reset()?;
    writeln!(buffer, "  ┃")?;

    // Title line
    buffer.reset()?;
    write!(buffer, "  ┃   ")?;
    buffer.set_color(
        ColorSpec::new()
            .set_fg(Some(Color::Cyan))
            .set_bold(true),
    )?;
    writeln!(buffer, "◈  D I S K  C L E A N E R")?;

    // Subtitle line
    buffer.reset()?;
    write!(buffer, "  ┃      ")?;
    buffer.set_color(ColorSpec::new().set_fg(Some(Color::Rgb(100, 100, 100))))?;
    writeln!(buffer, "disk usage analyzer and cleaner v{}", VERSION)?;

    // Empty line with side bars
    buffer.reset()?;
    writeln!(buffer, "  ┃")?;

    // Bottom border (same gradient)
    buffer.reset()?;
    write!(buffer, "  ")?;
    buffer.set_color(ColorSpec::new().set_fg(Some(Color::Cyan)))?;
    write!(buffer, "╺━━━━━━━━━━")?;
    buffer.set_color(ColorSpec::new().set_fg(Some(Color::Rgb(0, 139, 139))))?;
    writeln!(buffer, "━━━━━━━━━━━━━━━━━━━━━━━━━━━━╸")?;

    Ok(())
}

/// A terminal spinner shown while a long synchronous operation (the directory
/// scan) runs on the main thread. The animation runs on a background thread and
/// is stopped — and its line cleared — by [`Spinner::stop`] or drop, so the
/// spinner never lingers even if the scan returns an error.
struct Spinner {
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl Spinner {
    /// Start a spinner labelled `label`. Only call this when stdout is a TTY:
    /// it writes raw `\r`/ANSI control codes straight to stdout.
    fn start(label: &str) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_flag = stop.clone();
        let label = label.to_string();
        let handle = thread::spawn(move || {
            // Braille spinner frames.
            let frames = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
            let mut out = io::stdout();
            let mut i = 0;
            while !stop_flag.load(Ordering::Relaxed) {
                let _ = write!(out, "\r\x1b[36m{}\x1b[0m {}", frames[i], label);
                let _ = out.flush();
                i = (i + 1) % frames.len();
                thread::sleep(Duration::from_millis(80));
            }
            // Wipe the spinner line so later output starts clean.
            let _ = write!(out, "\r\x1b[2K");
            let _ = out.flush();
        });
        Spinner {
            stop,
            handle: Some(handle),
        }
    }

    /// Signal the background thread to stop and clear its line.
    fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for Spinner {
    fn drop(&mut self) {
        self.stop();
    }
}

mod shape {
    pub const INDENT: &str = "│";
    pub const _LAST_WITH_CHILDREN: &str = "└─┬";
    pub const LAST: &str = "└──";
    pub const ITEM: &str = "├──";
    pub const _ITEM_WITH_CHILDREN: &str = "├─┬";
    pub const SPACING: &str = "──";
}

fn main() -> Result<(), Box<dyn Error>> {
    let config = Config::parse();
    let current_dir = env::current_dir()?;
    let target_dir = config.target_dir.as_ref().unwrap_or(&current_dir);

    if config.tui {
        return disk_cleaner::tui::run_from_path(
            target_dir.to_string_lossy().to_string(),
            config.apparent,
        );
    }

    let file_info = FileInfo::from_path(&target_dir, config.apparent)?;

    let is_tty = atty::is(Stream::Stdout);
    let color_choice = if is_tty {
        ColorChoice::Auto
    } else {
        ColorChoice::Never
    };

    let stdout = BufferWriter::stdout(color_choice);

    // Print the logo + "Analyzing" header up front and flush immediately, so
    // they're visible before the (potentially slow) scan begins.
    {
        let mut intro = stdout.buffer();
        if !config.json {
            if is_tty {
                print_logo(&mut intro)?;
                writeln!(&mut intro)?;
            }
            writeln!(&mut intro, "Analyzing: {}\n", target_dir.display())?;
            stdout.print(&intro)?;
        }
    }

    // Spin on the line below "Analyzing" while the scan runs. Only when stdout
    // is a TTY and we're emitting human output — a piped/redirected stream
    // must not receive the spinner's `\r`/ANSI control codes.
    let mut spinner = if !config.json && is_tty {
        Some(Spinner::start("Scanning directory..."))
    } else {
        None
    };

    let analysed = match file_info {
        FileInfo::Directory { volume_id } => {
            let result = DiskItem::from_analyze(&target_dir, config.apparent, volume_id);
            // Stop the spinner and clear its line whether the scan succeeded
            // or errored, so a failure leaves no stray spinner behind.
            if let Some(s) = spinner.as_mut() {
                s.stop();
            }
            result?
        }
        _ => return Err(format!("{} is not a directory!", target_dir.display()).into()),
    };

    // Fresh buffer for everything after the scan — the intro buffer above was
    // already flushed and still holds its old contents, so don't reuse it.
    let mut buffer = stdout.buffer();

    if config.json {
        let serialized = serde_json::to_string(&analysed)?;
        writeln!(&mut buffer, "{}", serialized)?;
        // JSON mode must stay a clean single-document stream: never append the
        // human-readable project list or prompt.
        stdout.print(&buffer)?;
        return Ok(());
    }

    show(&analysed, &config, &mut DisplayInfo::new(), &mut buffer)?;

    // After the directory tree, scan for build-artifact projects and offer to
    // delete them (a kondo-style reclaim pass). The buffer is flushed inside
    // the helper so the tree is visible before the (second, slower) walk and
    // before any interactive prompt. Interactive only when both stdout and
    // stdin are TTYs; otherwise the list is printed read-only.
    run_projects_flow(target_dir, &stdout, buffer)?;

    Ok(())
}

/// After printing the directory tree, walk the tree once more (via
/// [`analyze`]) to find build-artifact projects, print a reclaimable-size
/// summary, and — when both stdout and stdin are TTYs — prompt the user to
/// choose which to delete. Under non-interactive output (pipe/redirect, or
/// stdin not a TTY) the list is printed read-only and no deletion happens.
///
/// `buffer` (which still holds the unflushed directory tree) is moved in and
/// flushed by this function so the tree is visible before the project walk and
/// before any prompt.
fn run_projects_flow(
    target_dir: &PathBuf,
    stdout: &BufferWriter,
    buffer: Buffer,
) -> Result<(), Box<dyn Error>> {
    let interactive = atty::is(Stream::Stdout) && atty::is(Stream::Stdin);
    let is_tty = atty::is(Stream::Stdout);

    // Flush the directory tree now so it's visible while the second (slower)
    // project scan runs below. Rebuild a fresh buffer afterwards — the old one
    // still holds the tree contents and would be re-emitted if reused.
    stdout.print(&buffer)?;
    let mut buffer = stdout.buffer();

    let opts = ScanOptions {
        follow_symlinks: false,
        same_file_system: false,
    };

    // Spin while walking the tree for build-artifact projects. Only on a TTY —
    // a piped/redirected stream must not receive the spinner's control codes.
    let mut spinner = if is_tty {
        Some(Spinner::start("Scanning for build artifacts..."))
    } else {
        None
    };

    // analyze() already drops projects with zero reclaimable bytes and those
    // whose scan errored, so this list is exactly "projects worth cleaning".
    let mut projects: Vec<ProjectAnalysis> = analyze(target_dir, &opts).collect();

    // Stop the spinner and clear its line before printing results, whether the
    // scan succeeded or errored.
    if let Some(s) = spinner.as_mut() {
        s.stop();
    }

    projects.sort_by(|a, b| b.artifact_size.cmp(&a.artifact_size));

    if projects.is_empty() {
        writeln!(&mut buffer, "No reclaimable build artifacts found.")?;
        stdout.print(&buffer)?;
        return Ok(());
    }

    let total: u64 = projects.iter().map(|p| p.artifact_size).sum();
    writeln!(&mut buffer)?;
    writeln!(
        &mut buffer,
        "Reclaimable build artifacts ({} projects, {} total):",
        projects.len(),
        pretty_bytes(total as f64)
    )?;
    for (i, project) in projects.iter().enumerate() {
        write_project_row(i, project, &mut buffer)?;
    }

    // Flush tree + list now so the prompt (if any) is visible before stdin blocks.
    stdout.print(&buffer)?;

    if !interactive {
        // Read-only: list printed, nothing to prompt. (pipe/redirect, or
        // stdin isn't a TTY — there's nobody to answer a prompt.)
        return Ok(());
    }

    let selection = loop {
        write_and_flush_prompt("Clean which? [1,3,5-7 / all / q to skip]: ")?;
        let mut line = String::new();
        let read = io::stdin().read_line(&mut line)?;
        if read == 0 {
            // EOF (Ctrl-D / closed stdin): treat as skip — nothing deleted.
            eprintln!("(input closed — skipping)");
            return Ok(());
        }
        match parse_selection(&line, projects.len()) {
            ParseResult::Skip => return Ok(()),
            ParseResult::Invalid => {
                eprintln!("Invalid selection, try again.");
                continue;
            }
            ParseResult::Set(indices) => break indices,
        }
    };

    let mut reclaimed: u64 = 0;
    let mut failed: usize = 0;
    for &index in &selection {
        let project = &projects[index];
        match project.project.clean() {
            Ok(()) => {
                reclaimed += project.artifact_size;
                println!(
                    "cleaned {} ({})",
                    project.project.path.display(),
                    pretty_bytes(project.artifact_size as f64)
                );
            }
            Err(e) => {
                failed += 1;
                eprintln!("failed {}: {}", project.project.path.display(), e);
            }
        }
    }

    println!(
        "Done: reclaimed {}, {} failed.",
        pretty_bytes(reclaimed as f64),
        failed
    );
    Ok(())
}

/// Write one numbered project row to the buffer with size-based coloring that
/// mirrors the TUI's thresholds (>1 GB red, >100 MB yellow).
fn write_project_row(i: usize, project: &ProjectAnalysis, buffer: &mut Buffer) -> io::Result<()> {
    let size_color = if project.artifact_size > 1_000_000_000 {
        Some(Color::Red)
    } else if project.artifact_size > 100_000_000 {
        Some(Color::Yellow)
    } else {
        None
    };

    write!(buffer, " {:>2}.  ", i + 1)?;
    buffer.set_color(ColorSpec::new().set_fg(size_color))?;
    write!(buffer, "[{:>10}]", pretty_bytes(project.artifact_size as f64))?;
    buffer.reset()?;
    write!(buffer, "  ")?;
    buffer.set_color(ColorSpec::new().set_fg(Some(Color::Cyan)))?;
    write!(buffer, "{:<14}", project.project.type_name())?;
    buffer.reset()?;
    write!(buffer, "  {}", project.project.path.display())?;
    if let Some(mtime) = project.last_modified {
        write!(buffer, "  {}", relative_time(mtime))?;
    }
    writeln!(buffer)?;
    Ok(())
}

/// Format a `SystemTime` as a coarse relative age (e.g. "3h ago").
/// Mirrors the TUI's `relative_time` (src/tui/ui.rs) — std-only, no extra dep.
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

/// Write `msg` straight to real stdout and flush, bypassing the termcolor
/// buffer so the prompt appears immediately (the buffer isn't flushed until we
/// choose, but a prompt must be visible before stdin blocks).
fn write_and_flush_prompt(msg: &str) -> io::Result<()> {
    let mut out = io::stdout();
    out.write_all(msg.as_bytes())?;
    out.flush()
}

/// Outcome of parsing the user's project-selection input.
enum ParseResult {
    /// Empty / `q` / `quit`: delete nothing and exit the flow.
    Skip,
    /// Any unparseable token: re-prompt the user.
    Invalid,
    /// Deduped, sorted, 0-based, in-range project indices to clean.
    Set(Vec<usize>),
}

/// Parse interactive selection input against `total` projects (1-based input,
/// 0-based result). See `ParseResult` for the skip-vs-invalid distinction and
/// the project-flow docs for the full edge-case table.
fn parse_selection(input: &str, total: usize) -> ParseResult {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return ParseResult::Skip;
    }
    if trimmed.eq_ignore_ascii_case("q") || trimmed.eq_ignore_ascii_case("quit") {
        return ParseResult::Skip;
    }
    if trimmed.eq_ignore_ascii_case("all") || trimmed.eq_ignore_ascii_case("a") {
        return ParseResult::Set((0..total).collect());
    }

    let mut indices: BTreeSet<usize> = BTreeSet::new();
    for token in trimmed.split(',') {
        let token = token.trim();
        match parse_token(token, total) {
            Some(range) => indices.extend(range),
            None => return ParseResult::Invalid,
        }
    }

    if indices.is_empty() {
        // e.g. "," or ","-only input: nothing valid was selected.
        return ParseResult::Invalid;
    }
    ParseResult::Set(indices.into_iter().collect())
}

/// Parse a single comma-separated token into an inclusive 0-based index range.
/// Returns `None` for any malformed or out-of-range token (1-based: `0` is
/// never valid; `start > end` is rejected rather than silently swapped).
fn parse_token(token: &str, total: usize) -> Option<std::ops::RangeInclusive<usize>> {
    if let Some((lhs, rhs)) = token.split_once('-') {
        let start = lhs.trim().parse::<usize>().ok()?;
        let end = rhs.trim().parse::<usize>().ok()?;
        if start == 0 || end == 0 || start > end {
            return None;
        }
        let (s, e) = (start - 1, end - 1);
        if e >= total {
            return None;
        }
        Some(s..=e)
    } else {
        let n = token.parse::<usize>().ok()?;
        if n == 0 || n > total {
            return None;
        }
        Some(n - 1..=n - 1)
    }
}

fn show(item: &DiskItem, conf: &Config, info: &mut DisplayInfo, buffer: &mut Buffer) -> io::Result<()> {
    // Show self
    show_item(item, info, buffer)?;
    // Recursively show children
    if info.level < conf.max_depth {
        if let Some(children) = &item.children {
            let children = children
                .iter()
                .map(|child| (child, size_fraction(child, item)))
                .filter(|&(_, fraction)| fraction > conf.min_percent)
                .collect::<Vec<_>>();

            if let Some((last_child, rest)) = children.split_last() {
                for &(child, fraction) in rest.iter() {
                    info.push(child, fraction, false);
                    show(child, conf, info, buffer)?;
                    info.pop();
                }
                let &(child, fraction) = last_child;
                info.push(child, fraction, true);
                show(child, conf, info, buffer)?;
                info.pop();
            }
        }
    }
    Ok(())
}

fn show_item(item: &DiskItem, info: &DisplayInfo, buffer: &mut Buffer) -> io::Result<()> {
    // Indentation
    buffer.set_color(ColorSpec::new().set_fg(INDENT_COLOR))?;
    write!(buffer, "{}{}", info.indents, info.prefix())?;
    // Percentage
    buffer.set_color(ColorSpec::new().set_fg(info.color()))?;
    write!(buffer, " {} ", format!("{:.2}%", info.fraction))?;
    // Disk size
    buffer.reset()?;
    write!(buffer, "[{}]", pretty_bytes(item.disk_size as f64),)?;
    // Arrow
    buffer.set_color(ColorSpec::new().set_fg(INDENT_COLOR))?;
    write!(buffer, " {} ", shape::SPACING)?;
    // Name
    buffer.reset()?;
    writeln!(buffer, "{}", item.name)?;
    Ok(())
}

fn size_fraction(child: &DiskItem, parent: &DiskItem) -> f64 {
    100.0 * (child.disk_size as f64 / parent.disk_size as f64)
}

#[derive(Debug)]
struct DisplayInfo {
    fraction: f64,
    level: usize,
    last: bool,
    indents: String,
}

impl DisplayInfo {
    fn new() -> Self {
        Self {
            fraction: 100.0,
            level: 0,
            last: true,
            indents: String::new(),
        }
    }

    /// Descend into a child item (mutate in place).
    fn push(&mut self, _child: &DiskItem, fraction: f64, is_last: bool) {
        let indent_char = if self.last { " " } else { shape::INDENT };
        self.indents.push_str(indent_char);
        self.indents.push_str("  ");
        self.level += 1;
        self.fraction = fraction;
        self.last = is_last;
    }

    /// Ascend back to parent (undo push).
    fn pop(&mut self) {
        self.level -= 1;
        // Undo one push: indent char + "  " = 3 chars. Drop by *character* count,
        // not bytes — shape::INDENT ("│") is multibyte (3 bytes), so a byte-based
        // `truncate(len - 3)` can land mid-character and panic on `is_char_boundary`.
        let mut new_len = self.indents.len();
        for _ in 0..3 {
            new_len = self.indents[..new_len]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
        }
        self.indents.truncate(new_len);
    }

    fn prefix(&self) -> &'static str {
        if self.last {
            shape::LAST
        } else {
            shape::ITEM
        }
    }

    fn color(&self) -> Option<Color> {
        if self.level == 0 {
            Some(Color::Green)
        } else if self.fraction > 20.0 {
            Some(Color::Red)
        } else {
            Some(Color::Cyan)
        }
    }
}

#[derive(Parser)]
struct Config {
    #[arg(short = 'd', default_value = "1")]
    /// Maximum recursion depth in directory.
    max_depth: usize,

    #[arg(
        short = 'm',
        default_value = "0.1",
        value_parser = parse_percent
    )]
    /// Threshold that determines if entry is worth
    /// being shown. Between 0-100 % of dir size.
    min_percent: f64,

    target_dir: Option<PathBuf>,

    #[arg(short = 'a')]
    /// Show apparent file size.
    ///
    /// This reports logical file length instead of allocated size on disk.
    apparent: bool,

    #[arg(short = 'j')]
    /// Output sorted json.
    json: bool,

    #[arg(short = 't', long = "tui")]
    /// Launch interactive TUI mode.
    tui: bool,
}

fn parse_percent(src: &str) -> Result<f64, Box<dyn Error + Send + Sync>> {
    let num = src.parse::<f64>()?;
    if num >= 0.0 && num <= 100.0 {
        Ok(num)
    } else {
        Err("Percentage must be in range [0, 100].".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item() -> DiskItem {
        DiskItem {
            name: "x".into(),
            disk_size: 1,
            children: None,
        }
    }

    /// Regression: `pop()` must remove whole characters, not bytes. With the
    /// multibyte `shape::INDENT` ("│"), the old byte-based `truncate(len - 3)`
    /// landed mid-character and panicked on `is_char_boundary`.
    #[test]
    fn push_pop_roundtrip_with_multibyte_indent() {
        let mut info = DisplayInfo::new();

        // From the root (last=true), descend through non-last siblings.
        // push #1 adds "   " (spaces); #2 and #3 add "│  " (multibyte).
        info.push(&item(), 10.0, false);
        info.push(&item(), 20.0, false);
        info.push(&item(), 30.0, false);
        assert_eq!(info.level, 3);
        assert_eq!(info.indents, "   │  │  ");
        assert_eq!(info.indents.chars().count(), 9); // 3 levels × 3 chars

        // Partial pop keeps the lower levels intact (no mid-char truncation).
        info.pop();
        assert_eq!(info.indents, "   │  ");
        assert_eq!(info.level, 2);

        // Pop back to root must not panic and must clear indents fully.
        info.pop();
        info.pop();
        assert_eq!(info.level, 0);
        assert!(info.indents.is_empty(), "indents not fully popped: {:?}", info.indents);
    }

    // ------------------------------------------------------------------
    // parse_selection
    // ------------------------------------------------------------------

    /// Unwrap a `ParseResult::Set` into its sorted index vec, panicking on
    /// Skip/Invalid so a test failure points squarely at the wrong branch.
    fn as_set(result: ParseResult) -> Vec<usize> {
        match result {
            ParseResult::Set(v) => v,
            other => panic!("expected Set, got {:?}", match other {
                ParseResult::Skip => "Skip",
                ParseResult::Invalid => "Invalid",
                ParseResult::Set(_) => unreachable!(),
            }),
        }
    }

    #[test]
    fn parse_selection_skip_on_empty_or_quit() {
        assert!(matches!(parse_selection("", 5), ParseResult::Skip));
        assert!(matches!(parse_selection("   ", 5), ParseResult::Skip));
        assert!(matches!(parse_selection("q", 5), ParseResult::Skip));
        assert!(matches!(parse_selection("QUIT", 5), ParseResult::Skip));
    }

    #[test]
    fn parse_selection_all_selects_everything() {
        assert_eq!(as_set(parse_selection("all", 5)), vec![0, 1, 2, 3, 4]);
        assert_eq!(as_set(parse_selection("a", 5)), vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn parse_selection_single_and_list() {
        assert_eq!(as_set(parse_selection("1", 5)), vec![0]);
        assert_eq!(as_set(parse_selection("1,3,5", 5)), vec![0, 2, 4]);
        assert_eq!(as_set(parse_selection("1, 3", 5)), vec![0, 2]); // spaces ok
    }

    #[test]
    fn parse_selection_range_inclusive() {
        assert_eq!(as_set(parse_selection("1-3", 5)), vec![0, 1, 2]);
        assert_eq!(as_set(parse_selection("5-5", 5)), vec![4]); // degenerate range
    }

    #[test]
    fn parse_selection_dedups_and_sorts() {
        assert_eq!(as_set(parse_selection("3,1,2", 5)), vec![0, 1, 2]);
        assert_eq!(as_set(parse_selection("1,1,2", 5)), vec![0, 1]);
        assert_eq!(as_set(parse_selection("1-3,2", 5)), vec![0, 1, 2]);
    }

    #[test]
    fn parse_selection_rejects_invalid_tokens() {
        // 1-based: 0 is never valid.
        assert!(matches!(parse_selection("0", 5), ParseResult::Invalid));
        // Out of range.
        assert!(matches!(parse_selection("6", 5), ParseResult::Invalid));
        assert!(matches!(parse_selection("1-6", 5), ParseResult::Invalid));
        // Reversed range is an error, not a silent swap.
        assert!(matches!(parse_selection("3-1", 5), ParseResult::Invalid));
        // Empty tokens (trailing/double comma).
        assert!(matches!(parse_selection("1,2,", 5), ParseResult::Invalid));
        assert!(matches!(parse_selection("1,,2", 5), ParseResult::Invalid));
        assert!(matches!(parse_selection(",", 5), ParseResult::Invalid));
        // `all` mixed with numbers is ambiguous → reject.
        assert!(matches!(parse_selection("all,3", 5), ParseResult::Invalid));
        // Malformed.
        assert!(matches!(parse_selection("abc", 5), ParseResult::Invalid));
        assert!(matches!(parse_selection("1-", 5), ParseResult::Invalid));
        assert!(matches!(parse_selection("-3", 5), ParseResult::Invalid));
        assert!(matches!(parse_selection("1.5", 5), ParseResult::Invalid));
        // Numeric overflow.
        assert!(matches!(
            parse_selection("999999999999999999999", 5),
            ParseResult::Invalid
        ));
    }
}
