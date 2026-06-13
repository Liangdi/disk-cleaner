use atty::Stream;
use clap::Parser;
use disk_cleaner::{DiskItem, FileInfo};
use pretty_bytes::converter::convert as pretty_bytes;
use std::env;
use std::error::Error;
use std::io;
use std::io::Write;
use std::path::PathBuf;
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

    let color_choice = if atty::is(Stream::Stdout) {
        ColorChoice::Auto
    } else {
        ColorChoice::Never
    };

    let stdout = BufferWriter::stdout(color_choice);
    let mut buffer = stdout.buffer();

    if !config.json {
        if atty::is(Stream::Stdout) {
            print_logo(&mut buffer)?;
            writeln!(&mut buffer)?;
        }
        writeln!(&mut buffer, "Analyzing: {}\n", target_dir.display())?;
    };

    let analysed = match file_info {
        FileInfo::Directory { volume_id } => {
            DiskItem::from_analyze(&target_dir, config.apparent, volume_id)?
        }
        _ => return Err(format!("{} is not a directory!", target_dir.display()).into()),
    };

    if config.json {
        let serialized = serde_json::to_string(&analysed)?;
        writeln!(&mut buffer, "{}", serialized)?;
    } else {
        show(&analysed, &config, &mut DisplayInfo::new(), &mut buffer)?;
    }

    stdout.print(&buffer)?;
    Ok(())
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
        // Remove "  " + indent char (3 chars)
        self.indents.truncate(self.indents.len().saturating_sub(3));
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
