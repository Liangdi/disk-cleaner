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

use crate::{analyze, DiskItem, FileInfo, ScanOptions};

type Tui = Terminal<CrosstermBackend<io::Stdout>>;

/// Message sent from scan thread to main thread.
struct ScanResult {
    /// Pre-flattened items (already computed in background).
    items: Option<Vec<app::FlatItem>>,
    path: String,
    total_size: u64,
    // Populated on scan failure but not currently surfaced (the user stays on
    // the last view); retained for future error reporting.
    #[allow(dead_code)]
    error: Option<String>,
}

/// Message sent from the projects-scan thread to the main thread.
struct ProjectScanResult {
    entries: Vec<app::ProjectEntry>,
    /// Non-empty after a clean-all: per-project failures to surface to the user.
    errors: Vec<String>,
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
    let (project_tx, project_rx) = mpsc::channel::<ProjectScanResult>();

    // Kick off initial shallow scan
    if state.loading {
        start_shallow_scan(&scan_tx, state.root_path.clone(), state.apparent, state);
    }

    loop {
        // Check if a disk scan completed
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

        // Check if a projects scan completed
        if state.projects_loading {
            if let Ok(result) = project_rx.try_recv() {
                state.projects_loading = false;
                state.set_projects(result.entries);
                if !result.errors.is_empty() {
                    state.error_message = Some(format!(
                        "{} project(s) failed to clean: {}",
                        result.errors.len(),
                        result.errors.join("; ")
                    ));
                }
            } else {
                state.loading_frame = state.loading_frame.wrapping_add(1);
            }
        }

        let viewport_height = terminal.size()?.height as usize;
        let mode_loading = match state.mode {
            app::AppMode::Disk => state.loading,
            app::AppMode::Projects => state.projects_loading,
        };
        if !mode_loading {
            let list_height = viewport_height.saturating_sub(2);
            match state.mode {
                app::AppMode::Disk => state.adjust_scroll(list_height),
                app::AppMode::Projects => state.projects_adjust_scroll(list_height),
            }
        }

        let detail = state.detail_stats_cloned();
        let project_detail = state.project_detail_cloned();
        terminal.draw(|f| ui::render(f, state, &detail, &project_detail))?;

        if crossterm_event::poll(std::time::Duration::from_millis(100))? {
            match crossterm_event::read()? {
                crossterm_event::Event::Key(key) => {
                    if mode_loading {
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

                    // Delete confirmation dialog mode (disk)
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

                    // Clean confirmation dialog mode (projects)
                    if state.clean_target.is_some() {
                        match event::handle_delete_confirm_key(key) {
                            event::DeleteConfirmAction::Confirm => {
                                let path = state
                                    .clean_target
                                    .and_then(|i| state.projects.get(i).map(|e| e.path.clone()));
                                match path {
                                    Some(path) => {
                                        match crate::clean(std::path::Path::new(&path)) {
                                            Ok(()) => {
                                                state.remove_cleaned_project();
                                                // Files were deleted: the cached disk
                                                // tree is now stale.
                                                state.disk_stale = true;
                                            }
                                            Err(e) => {
                                                state.error_message =
                                                    Some(format!("Clean failed: {}", e));
                                                state.cancel_clean();
                                            }
                                        }
                                    }
                                    None => state.cancel_clean(),
                                }
                            }
                            event::DeleteConfirmAction::Cancel => state.cancel_clean(),
                            event::DeleteConfirmAction::Ignore => {}
                        }
                        continue;
                    }

                    // Clean-ALL confirmation dialog mode (projects)
                    if state.clean_all_pending {
                        match event::handle_delete_confirm_key(key) {
                            event::DeleteConfirmAction::Confirm => {
                                start_clean_all(&project_tx, state);
                            }
                            event::DeleteConfirmAction::Cancel => state.cancel_clean_all(),
                            event::DeleteConfirmAction::Ignore => {}
                        }
                        continue;
                    }

                    if let Some(action) = event::handle_key(key) {
                        // Any normal action dismisses a prior error message.
                        state.error_message = None;

                        // Mode toggle works in both views.
                        if let event::AppAction::ToggleMode = action {
                            state.mode = match state.mode {
                                app::AppMode::Disk => app::AppMode::Projects,
                                app::AppMode::Projects => app::AppMode::Disk,
                            };
                            match state.mode {
                                // Entering Projects mode: scan the directory
                                // under the cursor (selected node), falling back
                                // to the scan root when a file or nothing is
                                // selected.
                                app::AppMode::Projects if !state.projects_loading => {
                                    let scan_target = state
                                        .visible
                                        .get(state.selected)
                                        .filter(|&&idx| state.items[idx].has_children)
                                        .map(|&idx| state.items[idx].full_path.clone())
                                        .unwrap_or_else(|| state.root_path.clone());
                                    state.projects_scan_root = scan_target.clone();
                                    start_projects_scan(&project_tx, scan_target, state);
                                }
                                // Returning to Disk mode after a clean: the
                                // cached tree is stale, so re-scan it.
                                app::AppMode::Disk if state.disk_stale && !state.loading => {
                                    state.disk_stale = false;
                                    start_shallow_scan(
                                        &scan_tx,
                                        state.root_path.clone(),
                                        state.apparent,
                                        state,
                                    );
                                }
                                _ => {}
                            }
                            continue;
                        }

                        match state.mode {
                            app::AppMode::Disk => match action {
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
                                event::AppAction::Refresh => {
                                    start_shallow_scan(
                                        &scan_tx,
                                        state.root_path.clone(),
                                        state.apparent,
                                        state,
                                    );
                                }
                                _ => {}
                            },
                            app::AppMode::Projects => match action {
                                event::AppAction::Quit => break,
                                event::AppAction::Up => state.projects_up(),
                                event::AppAction::Down => state.projects_down(),
                                event::AppAction::JumpTop => state.projects_jump_top(),
                                event::AppAction::JumpBottom => state.projects_jump_bottom(),
                                event::AppAction::Enter | event::AppAction::CleanProject => {
                                    state.request_clean();
                                }
                                event::AppAction::CleanAllProjects => state.request_clean_all(),
                                event::AppAction::Refresh => {
                                    let target = if state.projects_scan_root.is_empty() {
                                        state.root_path.clone()
                                    } else {
                                        state.projects_scan_root.clone()
                                    };
                                    start_projects_scan(&project_tx, target, state);
                                }
                                _ => {}
                            },
                        }
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}

/// Start a background project scan with [`analyze`], collecting every project
/// (with reclaimable size + type) found beneath `path`.
fn start_projects_scan(
    tx: &mpsc::Sender<ProjectScanResult>,
    path: String,
    state: &mut app::AppState,
) {
    state.projects_loading = true;
    state.loading_frame = 0;
    state.loading_message = format!("projects in {}", path);

    let tx = tx.clone();
    let path_owned = path;
    let apparent = state.apparent;
    std::thread::spawn(move || {
        let opts = ScanOptions {
            follow_symlinks: false,
            same_file_system: false,
            apparent,
        };
        let entries: Vec<app::ProjectEntry> = analyze(&path_owned, &opts)
            .map(|pa| app::ProjectEntry {
                path: pa.project.path.to_string_lossy().into_owned(),
                type_name: pa.project.type_name(),
                reclaimable: pa.artifact_size,
                last_modified: pa.last_modified,
                artifact_dir_names: pa
                    .project
                    .artifact_dirs()
                    .into_iter()
                    .map(String::from)
                    .collect(),
            })
            .collect();
        let _ = tx.send(ProjectScanResult {
            entries,
            errors: Vec::new(),
        });
    });
}

/// Clean every project in the current list, then re-scan the same root so the
/// list reflects reality (partially-cleaned or permission-denied projects stay
/// listed). Runs in the background; failures are surfaced via `error_message`.
fn start_clean_all(tx: &mpsc::Sender<ProjectScanResult>, state: &mut app::AppState) {
    let paths: Vec<String> = state.projects.iter().map(|e| e.path.clone()).collect();
    let root_path = if state.projects_scan_root.is_empty() {
        state.root_path.clone()
    } else {
        state.projects_scan_root.clone()
    };
    let count = paths.len();
    state.projects_loading = true;
    state.loading_frame = 0;
    state.loading_message = format!("cleaning {} projects", count);
    state.clean_all_pending = false;
    state.disk_stale = true;

    let tx = tx.clone();
    let apparent = state.apparent;
    std::thread::spawn(move || {
        let opts = ScanOptions {
            follow_symlinks: false,
            same_file_system: false,
            apparent,
        };
        let mut errors: Vec<String> = Vec::new();
        for path in &paths {
            if let Err(e) = crate::clean(std::path::Path::new(path)) {
                errors.push(format!("{} ({})", path, e));
            }
        }
        // Re-analyze the same root: successfully cleaned projects now have zero
        // artifacts and drop out; failed ones remain with their sizes updated.
        let entries: Vec<app::ProjectEntry> = analyze(&root_path, &opts)
            .map(|pa| app::ProjectEntry {
                path: pa.project.path.to_string_lossy().into_owned(),
                type_name: pa.project.type_name(),
                reclaimable: pa.artifact_size,
                last_modified: pa.last_modified,
                artifact_dir_names: pa
                    .project
                    .artifact_dirs()
                    .into_iter()
                    .map(String::from)
                    .collect(),
            })
            .collect();
        let _ = tx.send(ProjectScanResult { entries, errors });
    });
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
