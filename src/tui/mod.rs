mod app;
mod event;
mod ui;

use std::error::Error;
use std::io;
use std::path::Path;
use std::sync::mpsc;

use crossterm::{
    event as crossterm_event,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use crate::{DiskItem, FileInfo};

type Tui = Terminal<CrosstermBackend<io::Stdout>>;

/// Message sent from scan thread to main thread.
struct ScanResult {
    /// Pre-flattened items (already computed in background).
    items: Option<Vec<app::FlatItem>>,
    path: String,
    total_size: u64,
    error: Option<String>,
}

/// Run the interactive TUI with an explicit apparent flag.
#[allow(dead_code)]
pub fn run_with_apparent(
    root: DiskItem,
    root_path: String,
    total_size: u64,
    apparent: bool,
) -> Result<(), Box<dyn Error>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    crossterm::execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut state =
        app::AppState::from_disk_item_with_apparent(root, root_path, total_size, apparent);

    let res = run_loop(&mut terminal, &mut state);

    disable_raw_mode()?;
    crossterm::execute!(io::stdout(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    res
}

/// Run the interactive TUI starting with a path (shows loading splash while scanning).
pub fn run_from_path(path: String, apparent: bool) -> Result<(), Box<dyn Error>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    crossterm::execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut state = app::AppState::new_empty(path.clone(), apparent);

    let res = run_loop(&mut terminal, &mut state);

    disable_raw_mode()?;
    crossterm::execute!(io::stdout(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    res
}

fn run_loop(terminal: &mut Tui, state: &mut app::AppState) -> Result<(), Box<dyn Error>> {
    let (scan_tx, scan_rx) = mpsc::channel::<ScanResult>();

    // Kick off initial shallow scan
    if state.loading {
        start_shallow_scan(&scan_tx, state.root_path.clone(), state.apparent, state);
    }

    loop {
        // Check if a scan completed
        if state.loading {
            if let Ok(result) = scan_rx.try_recv() {
                state.loading = false;
                if let Some(items) = result.items {
                    state.root_path = result.path;
                    state.total_size = result.total_size;
                    state.rebuild_from_items(items);
                }
                // If error, loading is cancelled, user stays on current view
            } else {
                state.loading_frame = state.loading_frame.wrapping_add(1);
            }
        }

        let viewport_height = terminal.size()?.height as usize;
        if !state.loading {
            let list_height = viewport_height.saturating_sub(2);
            state.adjust_scroll(list_height);
        }

        let detail = state.detail_stats_cloned();
        terminal.draw(|f| ui::render(f, state, &detail))?;

        if crossterm_event::poll(std::time::Duration::from_millis(100))? {
            match crossterm_event::read()? {
                crossterm_event::Event::Key(key) => {
                    if state.loading {
                        if let Some(action) = event::handle_key(key) {
                            if let event::AppAction::Quit = action {
                                break;
                            }
                        }
                        continue;
                    }

                    if state.search_active {
                        match event::handle_search_key(key) {
                            event::SearchAction::Char(c) => {
                                state.search_query.push(c);
                                state.apply_search();
                            }
                            event::SearchAction::Backspace => {
                                state.search_query.pop();
                                state.apply_search();
                            }
                            event::SearchAction::Finish => {
                                state.search_active = false;
                                state.apply_search();
                            }
                            event::SearchAction::Ignore => {}
                        }
                        continue;
                    }

                    // Delete confirmation dialog mode
                    if state.delete_target.is_some() {
                        match event::handle_delete_confirm_key(key) {
                            event::DeleteConfirmAction::Confirm => {
                                if let Some((path, is_dir)) = state.delete_target_info() {
                                    let result = if is_dir {
                                        std::fs::remove_dir_all(&path)
                                    } else {
                                        std::fs::remove_file(&path)
                                    };
                                    match result {
                                        Ok(()) => {
                                            state.cancel_delete();
                                            // Rescan to refresh the view
                                            start_shallow_scan(
                                                &scan_tx,
                                                state.root_path.clone(),
                                                state.apparent,
                                                state,
                                            );
                                        }
                                        Err(e) => {
                                            state.error_message = Some(format!("Delete failed: {}", e));
                                            state.cancel_delete();
                                        }
                                    }
                                } else {
                                    state.cancel_delete();
                                }
                            }
                            event::DeleteConfirmAction::Cancel => {
                                state.cancel_delete();
                            }
                            event::DeleteConfirmAction::Ignore => {}
                        }
                        continue;
                    }

                    if let Some(action) = event::handle_key(key) {
                        match action {
                            event::AppAction::Quit => break,
                            event::AppAction::Up => state.move_up(),
                            event::AppAction::Down => state.move_down(),
                            event::AppAction::Enter => state.enter(),
                            event::AppAction::Back => state.back(),
                            event::AppAction::Toggle => state.toggle_expand(),
                            event::AppAction::JumpTop => state.jump_top(),
                            event::AppAction::JumpBottom => state.jump_bottom(),
                            event::AppAction::ToggleHidden => {
                                state.show_hidden = !state.show_hidden;
                                state.compute_visible();
                            }
                            event::AppAction::DeleteItem => {
                                state.request_delete();
                            }
                            event::AppAction::ToggleApparent => {
                                state.apparent = !state.apparent;
                                start_shallow_scan(
                                    &scan_tx,
                                    state.root_path.clone(),
                                    state.apparent,
                                    state,
                                );
                            }
                            event::AppAction::StartSearch => {
                                state.search_active = true;
                                state.search_query.clear();
                            }
                            event::AppAction::EnterDir => {
                                state.enter_dir();
                                if let Some(new_path) = state.rescan_path.take() {
                                    start_shallow_scan(
                                        &scan_tx,
                                        new_path,
                                        state.apparent,
                                        state,
                                    );
                                }
                            }
                            event::AppAction::ParentDir => {
                                state.parent_dir();
                                if let Some(new_path) = state.rescan_path.take() {
                                    start_shallow_scan(
                                        &scan_tx,
                                        new_path,
                                        state.apparent,
                                        state,
                                    );
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}

/// Start a background shallow scan (one level only).
fn start_shallow_scan(
    tx: &mpsc::Sender<ScanResult>,
    path: String,
    apparent: bool,
    state: &mut app::AppState,
) {
    state.loading = true;
    state.loading_frame = 0;
    state.loading_message = format!("{}", path);

    let tx = tx.clone();
    let path_owned = path;
    std::thread::spawn(move || {
        let result = do_shallow_scan(&path_owned, apparent);
        let msg = match result {
            Ok(root) => {
                let total_size = root.disk_size;
                // Flatten in the background thread — no main thread freeze
                let mut items = Vec::new();
                app::flatten_disk_item(&root, path_owned.clone(), total_size, 0, None, &mut items);
                ScanResult {
                    items: Some(items),
                    path: path_owned,
                    total_size,
                    error: None,
                }
            }
            Err(e) => ScanResult {
                items: None,
                path: path_owned,
                total_size: 0,
                error: Some(format!("{}", e)),
            },
        };
        let _ = tx.send(msg);
    });
}

/// Perform a one-level shallow scan of a directory.
fn do_shallow_scan(path: &str, apparent: bool) -> Result<DiskItem, Box<dyn Error>> {
    let target = Path::new(path);
    let file_info = FileInfo::from_path(target, apparent)?;
    match file_info {
        FileInfo::Directory { volume_id } => {
            DiskItem::from_shallow_scan(target, apparent, volume_id)
        }
        _ => Err(format!("{} is not a directory!", path).into()),
    }
}
