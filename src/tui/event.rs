use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Actions that can be triggered by keyboard input.
pub enum AppAction {
    Up,
    Down,
    Enter,
    Back,
    Toggle,
    Quit,
    /// Toggle apparent size display (requires re-scan)
    ToggleApparent,
    /// Enter search/filter mode
    StartSearch,
    /// Enter selected directory (rescan)
    EnterDir,
    /// Go up to parent directory (rescan)
    ParentDir,
    /// Jump to top (g)
    JumpTop,
    /// Jump to bottom (G)
    JumpBottom,
    /// Toggle hidden files visibility (.)
    ToggleHidden,
    /// Request delete of selected item (x)
    DeleteItem,
    /// Switch between Disk and Projects views (p / Tab)
    ToggleMode,
    /// Request clean of the selected project's artifacts (c)
    CleanProject,
    /// Request clean of ALL projects' artifacts (C)
    CleanAllProjects,
    /// Re-scan the current view (r)
    Refresh,
}

/// Result of a key press during the delete confirmation dialog.
pub enum DeleteConfirmAction {
    Confirm,
    Cancel,
    Ignore,
}

/// Map a crossterm key event to an AppAction (normal mode).
pub fn handle_key(key: KeyEvent) -> Option<AppAction> {
    match key.code {
        KeyCode::Char('q') => Some(AppAction::Quit),
        KeyCode::Char('j') | KeyCode::Down => Some(AppAction::Down),
        KeyCode::Char('k') | KeyCode::Up => Some(AppAction::Up),
        KeyCode::Char('l') | KeyCode::Right | KeyCode::Enter => Some(AppAction::Enter),
        KeyCode::Char('h') | KeyCode::Left | KeyCode::Backspace => Some(AppAction::Back),
        KeyCode::Char(' ') => Some(AppAction::Toggle),
        KeyCode::Char('a') => Some(AppAction::ToggleApparent),
        KeyCode::Char('/') => Some(AppAction::StartSearch),
        KeyCode::Char('d') => Some(AppAction::EnterDir),
        KeyCode::Char('u') => Some(AppAction::ParentDir),
        KeyCode::Char('g') => Some(AppAction::JumpTop),
        KeyCode::Char('G') => Some(AppAction::JumpBottom),
        KeyCode::Char('.') => Some(AppAction::ToggleHidden),
        KeyCode::Char('x') => Some(AppAction::DeleteItem),
        KeyCode::Tab | KeyCode::Char('p') => Some(AppAction::ToggleMode),
        // Plain `c` cleans a project; Ctrl-C still quits (handled by the
        // guarded arm below — this must not match when Control is held).
        KeyCode::Char('c') if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            Some(AppAction::CleanProject)
        }
        // `C` (shift-c) cleans every project at once.
        KeyCode::Char('C') => Some(AppAction::CleanAllProjects),
        KeyCode::Char('r') => Some(AppAction::Refresh),
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            Some(AppAction::Quit)
        }
        _ => None,
    }
}

/// Map a crossterm key event during search input mode.
pub enum SearchAction {
    Char(char),
    Backspace,
    Finish,
    Ignore,
}

pub fn handle_search_key(key: KeyEvent) -> SearchAction {
    match key.code {
        KeyCode::Esc | KeyCode::Enter => SearchAction::Finish,
        KeyCode::Backspace => SearchAction::Backspace,
        KeyCode::Char(c) => SearchAction::Char(c),
        _ => SearchAction::Ignore,
    }
}

/// Map a crossterm key event during the delete confirmation dialog.
pub fn handle_delete_confirm_key(key: KeyEvent) -> DeleteConfirmAction {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => DeleteConfirmAction::Confirm,
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => DeleteConfirmAction::Cancel,
        _ => DeleteConfirmAction::Ignore,
    }
}
