use std::collections::HashMap;
use std::path::Path;
use std::time::SystemTime;

use crate::{dir_size, DiskItem, ScanOptions};

/// Which view the TUI is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppMode {
    /// WinDirStat-style directory tree with sizes.
    Disk,
    /// Kondo-style list of build projects and their reclaimable artifacts.
    Projects,
}

/// One row of the Projects view: a discovered project plus the reclaimable
/// total computed by [`analyze`](crate::analyze). The per-artifact-directory
/// size breakdown is derived lazily (see [`AppState::project_detail_cloned`]).
#[derive(Debug, Clone)]
pub struct ProjectEntry {
    /// Absolute path to the project root.
    pub path: String,
    /// Human-readable ecosystem label, e.g. `"Cargo / Node"`.
    pub type_name: String,
    /// Total reclaimable bytes across the project's artifact directories.
    pub reclaimable: u64,
    /// Most recent modification time across the project tree, if known.
    pub last_modified: Option<SystemTime>,
    /// Artifact-directory names for this project (without sizes); e.g.
    /// `["target", "node_modules"]`. Sizes filled in on demand.
    pub artifact_dir_names: Vec<String>,
}

/// A flattened node from the DiskItem tree, used for efficient rendering.
pub struct FlatItem {
    pub name: String,
    /// Full absolute path from root scan directory.
    pub full_path: String,
    pub disk_size: u64,
    pub parent_size: u64,
    pub depth: usize,
    pub has_children: bool,
    /// File extension (lowercase, without dot). Empty string if none.
    pub extension: String,
    /// Indices of this node's direct children in the flat items list.
    /// Index of this node's parent in the flat items list. None for root.
    pub parent: Option<usize>,
    pub direct_children: Vec<usize>,
}

/// Stats for the detail panel (right side).
#[derive(Default, Clone)]
#[allow(dead_code)]
pub struct DetailStats {
    /// Full path of the selected item.
    pub full_path: String,
    /// Disk size of selected item.
    pub size: u64,
    /// Percentage of parent directory.
    pub pct_of_parent: f64,
    /// Percentage of root total.
    pub pct_of_total: f64,
    /// Number of direct child files.
    pub file_count: usize,
    /// Number of direct child directories.
    pub dir_count: usize,
    /// Total descendant count (files + dirs).
    pub total_descendants: usize,
    /// Name and size of the largest direct child.
    pub largest_child: Option<(String, u64)>,
    /// File type distribution: (extension, size, count).
    pub type_distribution: Vec<(String, u64, usize)>,
    /// Top N largest descendants (name, size, depth).
    pub top_largest: Vec<(String, u64, usize)>,
    /// Breadcrumb: ancestor names from root to selected item.
    pub breadcrumb: Vec<String>,
    /// File size distribution histogram: buckets of (label, count).
    /// Buckets: <1KB, 1-10KB, 10-100KB, 100KB-1MB, 1-10MB, 10-100MB, 100MB-1GB, >1GB
    pub size_histogram: Vec<(String, usize)>,
    /// Average file size.
    pub avg_file_size: u64,
}

use std::collections::HashSet;

/// The main application state for the TUI.
pub struct AppState {
    /// All nodes flattened via DFS from DiskItem.
    pub items: Vec<FlatItem>,
    /// Set of node indices that are currently expanded.
    pub expanded: HashSet<usize>,
    /// Index into `visible` list for the currently selected item.
    pub selected: usize,
    /// Indices into `items` that are currently visible (respecting expand/collapse).
    pub visible: Vec<usize>,
    /// Vertical scroll offset for the visible list.
    pub scroll_offset: usize,
    /// The root path being analyzed.
    pub root_path: String,
    /// Total size of the root directory.
    pub total_size: u64,
    /// Whether showing apparent size.
    pub apparent: bool,
    /// Whether to show hidden files/directories (starting with .).
    pub show_hidden: bool,
    /// Pending delete target: Some(items index) when confirmation dialog is open.
    pub delete_target: Option<usize>,
    /// Error message to display briefly (e.g. after failed delete).
    pub error_message: Option<String>,
    /// Search query filter (empty = no filter).
    pub search_query: String,
    /// Whether we are currently in search input mode.
    pub search_active: bool,
    /// Whether a scan is in progress (blocks all interaction except quit).
    pub loading: bool,
    /// Loading spinner frame counter.
    pub loading_frame: usize,
    /// Loading message to display.
    pub loading_message: String,
    /// Request to rescan a new directory (set by EnterDir/ParentDir).
    pub rescan_path: Option<String>,
    /// Cached detail stats for the selected item.
    cached_detail: Option<(usize, DetailStats)>, // (selected item idx, stats)

    // ── Projects mode ───────────────────────────────────────────
    /// Which view is active.
    pub mode: AppMode,
    /// True after a clean has deleted files on disk, meaning the cached disk
    /// tree (`items`) no longer reflects reality. The disk view is re-scanned
    /// when the user returns to it, so sizes stay consistent after cleaning.
    pub disk_stale: bool,
    /// The directory the current project list was scanned from (the selected
    /// node's path when Projects mode was entered). Used both to re-scan after
    /// a clean-all and to render project paths relative to it.
    pub projects_scan_root: String,
    /// Projects discovered by `analyze`, sorted by reclaimable size desc.
    pub projects: Vec<ProjectEntry>,
    /// Index of the selected project in `projects`.
    pub projects_selected: usize,
    /// Vertical scroll offset for the project list.
    pub projects_scroll: usize,
    /// Whether a project scan is running (blocks interaction except quit).
    pub projects_loading: bool,
    /// Pending clean target: Some(index into `projects`) when the clean
    /// confirmation dialog is open.
    pub clean_target: Option<usize>,
    /// Snapshot of the clean target's per-artifact-directory breakdown,
    /// captured when the confirmation dialog opens so the (immutable) render
    /// path can display it without re-walking the tree.
    pub clean_breakdown: Vec<(String, u64)>,
    /// Whether the "clean ALL projects" confirmation dialog is open.
    pub clean_all_pending: bool,
    /// Cached per-artifact-directory breakdown for the selected project:
    /// `(projects_selected, Vec<(name, size)>)`.
    cached_project_detail: Option<(usize, Vec<(String, u64)>)>,
}

/// Default `ScanOptions` used by the Projects view (no symlink following,
/// same-filesystem off — matching kondo's defaults).
fn scan_opts() -> ScanOptions {
    ScanOptions {
        follow_symlinks: false,
        same_file_system: false,
    }
}

impl AppState {
    /// Build AppState from a DiskItem tree.
    #[allow(dead_code)]
    pub fn from_disk_item(root: DiskItem, root_path: String, total_size: u64) -> Self {
        Self::from_disk_item_with_apparent(root, root_path, total_size, false)
    }

    /// Build AppState with explicit apparent flag.
    pub fn from_disk_item_with_apparent(
        root: DiskItem,
        root_path: String,
        total_size: u64,
        apparent: bool,
    ) -> Self {
        let mut state = Self {
            items: Vec::new(),
            expanded: HashSet::new(),
            selected: 0,
            visible: Vec::new(),
            scroll_offset: 0,
            root_path,
            total_size,
            apparent,
            show_hidden: true,
            search_query: String::new(),
            search_active: false,
            loading: false,
            loading_frame: 0,
            loading_message: String::new(),
            rescan_path: None,
            cached_detail: None,
            delete_target: None,
            error_message: None,
            mode: AppMode::Disk,
            disk_stale: false,
            projects_scan_root: String::new(),
            projects: Vec::new(),
            projects_selected: 0,
            projects_scroll: 0,
            projects_loading: false,
            clean_target: None,
            clean_breakdown: Vec::new(),
            clean_all_pending: false,
            cached_project_detail: None,
        };
        state.rebuild_from(root);
        state
    }

    /// Create an empty state in loading mode (for initial splash).
    pub fn new_empty(root_path: String, apparent: bool) -> Self {
        Self {
            items: Vec::new(),
            expanded: HashSet::new(),
            selected: 0,
            visible: Vec::new(),
            scroll_offset: 0,
            root_path,
            total_size: 0,
            apparent,
            show_hidden: true,
            search_query: String::new(),
            search_active: false,
            loading: true,
            loading_frame: 0,
            loading_message: String::new(),
            rescan_path: None,
            cached_detail: None,
            delete_target: None,
            error_message: None,
            mode: AppMode::Disk,
            disk_stale: false,
            projects_scan_root: String::new(),
            projects: Vec::new(),
            projects_selected: 0,
            projects_scroll: 0,
            projects_loading: false,
            clean_target: None,
            clean_breakdown: Vec::new(),
            clean_all_pending: false,
            cached_project_detail: None,
        }
    }

    /// Flatten a DiskItem tree into items, expand root, compute visible.
    pub fn rebuild_from(&mut self, root: DiskItem) {
        self.items.clear();
        self.expanded.clear();
        self.selected = 0;
        self.scroll_offset = 0;
        self.total_size = root.disk_size;
        self.cached_detail = None;

        flatten_disk_item(&root, self.root_path.clone(), root.disk_size, 0, None, &mut self.items);

        self.expanded.insert(0);
        self.compute_visible();
    }

    /// Rebuild from pre-flattened items (received from background scan thread).
    pub fn rebuild_from_items(&mut self, items: Vec<FlatItem>) {
        self.items = items;
        self.expanded.clear();
        self.selected = 0;
        self.scroll_offset = 0;
        self.cached_detail = None;

        self.expanded.insert(0);
        self.compute_visible();
    }

    /// Build AppState with explicit apparent flag.

    /// Get detail stats (cached). Returns a clone for borrow safety.
    pub fn detail_stats_cloned(&mut self) -> DetailStats {
        if self.visible.is_empty() {
            return DetailStats::default();
        }

        let idx = self.visible[self.selected];

        // Return cached clone if selection hasn't changed
        if let Some((cached_idx, ref stats)) = self.cached_detail {
            if cached_idx == idx {
                return stats.clone();
            }
        }

        // Recompute
        let stats = self.compute_detail_stats(idx);
        self.cached_detail = Some((idx, stats.clone()));
        stats
    }

    fn compute_detail_stats(&self, idx: usize) -> DetailStats {
        let item = &self.items[idx];
        let root_total = self.total_size;

        let pct_of_parent = if item.parent_size > 0 {
            100.0 * (item.disk_size as f64 / item.parent_size as f64)
        } else {
            100.0
        };

        let pct_of_total = if root_total > 0 {
            100.0 * (item.disk_size as f64 / root_total as f64)
        } else {
            0.0
        };

        // Stats from direct children only (no recursion)
        let mut file_count = 0usize;
        let mut dir_count = 0usize;
        let mut largest_child: Option<(String, u64)> = None;
        let mut ext_map: HashMap<String, (u64, usize)> = HashMap::new();
        let mut file_sizes: Vec<u64> = Vec::new();
        let mut top_children: Vec<(String, u64, usize)> = Vec::new();

        for &child_idx in &item.direct_children {
            let child = &self.items[child_idx];
            if child.has_children {
                dir_count += 1;
            } else {
                file_count += 1;
                // Extension stats from direct child files
                let ext = if child.extension.is_empty() {
                    "(no ext)".to_string()
                } else {
                    format!(".{}", child.extension)
                };
                let entry = ext_map.entry(ext).or_insert((0, 0));
                entry.0 += child.disk_size;
                entry.1 += 1;
                file_sizes.push(child.disk_size);
            }
            // Track largest child
            top_children.push((child.name.clone(), child.disk_size, child.depth));
            match &largest_child {
                None => largest_child = Some((child.name.clone(), child.disk_size)),
                Some((_, max_size)) if child.disk_size > *max_size => {
                    largest_child = Some((child.name.clone(), child.disk_size));
                }
                _ => {}
            }
        }

        let total_descendants = item.direct_children.len();

        // Type distribution: top 8 by size
        let mut type_distribution: Vec<(String, u64, usize)> = ext_map
            .into_iter()
            .map(|(ext, (size, count))| (ext, size, count))
            .collect();
        type_distribution.sort_by(|a, b| b.1.cmp(&a.1));
        type_distribution.truncate(8);

        // Top largest children (sorted by size desc)
        top_children.sort_by(|a, b| b.1.cmp(&a.1));
        let top_largest = top_children.into_iter().take(10).collect();

        // File size distribution histogram
        let size_histogram = build_size_histogram(&file_sizes);

        // Average file size
        let avg_file_size = if file_sizes.is_empty() {
            0
        } else {
            file_sizes.iter().sum::<u64>() / file_sizes.len() as u64
        };

        // Breadcrumb: walk up from selected to root
        let breadcrumb = self.build_breadcrumb(idx);

        let full_path = item.full_path.clone();

        DetailStats {
            full_path,
            size: item.disk_size,
            pct_of_parent,
            pct_of_total,
            file_count,
            dir_count,
            total_descendants,
            largest_child,
            type_distribution,
            top_largest,
            breadcrumb,
            size_histogram,
            avg_file_size,
        }
    }

    /// Build breadcrumb path from root to the given item index.
    /// This is now O(depth) instead of O(n*depth) by following parent pointers.
    fn build_breadcrumb(&self, idx: usize) -> Vec<String> {
        let mut path = Vec::new();
        let mut current = Some(idx);
        
        // Walk up the parent chain until we reach the root (parent == None)
        while let Some(current_idx) = current {
            path.push(self.items[current_idx].name.clone());
            current = self.items[current_idx].parent;
        }
        
        // Reverse to get path from root to selected item
        path.reverse();
        path
    }

    /// Recompute the visible items list based on current expansion and search state.
    pub fn compute_visible(&mut self) {
        self.visible.clear();
        if self.items.is_empty() {
            return;
        }

        if self.search_query.is_empty() {
            self.build_visible(0);
        } else {
            let query = self.search_query.to_lowercase();
            self.build_visible_search(0, &query);
        }

        if !self.visible.is_empty() && self.selected >= self.visible.len() {
            self.selected = self.visible.len() - 1;
        }
    }

    fn build_visible(&mut self, idx: usize) {
        self.visible.push(idx);
        if self.expanded.contains(&idx) {
            let children: Vec<usize> = self.items[idx]
                .direct_children
                .iter()
                .copied()
                .filter(|&ci| self.show_hidden || !self.items[ci].name.starts_with('.'))
                .collect();
            for child_idx in children {
                self.build_visible(child_idx);
            }
        }
    }

    fn build_visible_search(&mut self, idx: usize, query: &str) -> bool {
        let item = &self.items[idx];
        let name_matches = item.name.to_lowercase().contains(query);

        let mut child_matches = false;
        if self.expanded.contains(&idx) {
            let children: Vec<usize> = self.items[idx]
                .direct_children
                .iter()
                .copied()
                .filter(|&ci| self.show_hidden || !self.items[ci].name.starts_with('.'))
                .collect();
            for child_idx in children {
                if self.build_visible_search(child_idx, query) {
                    child_matches = true;
                }
            }
        } else {
            child_matches = self.has_matching_descendant(idx, query);
        }

        if name_matches || child_matches {
            self.visible.push(idx);
        }
        name_matches || child_matches
    }

    fn has_matching_descendant(&self, idx: usize, query: &str) -> bool {
        for &child_idx in &self.items[idx].direct_children {
            if !self.show_hidden && self.items[child_idx].name.starts_with('.') {
                continue;
            }
            if self.items[child_idx].name.to_lowercase().contains(query) {
                return true;
            }
            if self.has_matching_descendant(child_idx, query) {
                return true;
            }
        }
        false
    }

    pub fn apply_search(&mut self) {
        self.compute_visible();
    }

    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn move_down(&mut self) {
        if self.selected + 1 < self.visible.len() {
            self.selected += 1;
        }
    }

    pub fn jump_top(&mut self) {
        self.selected = 0;
    }

    pub fn jump_bottom(&mut self) {
        if !self.visible.is_empty() {
            self.selected = self.visible.len() - 1;
        }
    }

    pub fn toggle_expand(&mut self) {
        let idx = self.visible[self.selected];
        if !self.items[idx].has_children {
            return;
        }
        if self.expanded.contains(&idx) {
            self.expanded.remove(&idx);
        } else {
            self.expanded.insert(idx);
        }
        self.compute_visible();
    }

    pub fn enter(&mut self) {
        let idx = self.visible[self.selected];
        let has_children = self.items[idx].has_children;
        if !has_children {
            return;
        }
        let children_start = self.items[idx].direct_children.first().copied();
        if !self.expanded.contains(&idx) {
            self.expanded.insert(idx);
            self.compute_visible();
        }
        if let Some(first_child) = children_start {
            if self.expanded.contains(&idx) {
                if let Some(new_sel) = self.visible.iter().position(|&v| v == first_child) {
                    self.selected = new_sel;
                }
            }
        }
    }

    pub fn back(&mut self) {
        let idx = self.visible[self.selected];
        let depth = self.items[idx].depth;
        if depth == 0 {
            return;
        }
        let target_depth = depth - 1;
        for i in (0..self.selected).rev() {
            let vidx = self.visible[i];
            if self.items[vidx].depth == target_depth {
                self.selected = i;
                if self.expanded.contains(&vidx) {
                    self.expanded.remove(&vidx);
                    self.compute_visible();
                    for (j, &v) in self.visible.iter().enumerate() {
                        if v == vidx {
                            self.selected = j;
                            break;
                        }
                    }
                }
                return;
            }
        }
    }

    /// Request to enter the selected directory (rescan).
    pub fn enter_dir(&mut self) {
        let idx = self.visible[self.selected];
        if !self.items[idx].has_children {
            return;
        }
        self.rescan_path = Some(self.items[idx].full_path.clone());
    }

    /// Request to go to parent directory (rescan).
    pub fn parent_dir(&mut self) {
        let parent = std::path::Path::new(&self.root_path)
            .parent()
            .map(|p| p.to_string_lossy().to_string());
        if let Some(p) = parent {
            if !p.is_empty() {
                self.rescan_path = Some(p);
            }
        }
    }

    /// Adjust scroll offset so the selected item is visible in the viewport.
    pub fn adjust_scroll(&mut self, viewport_height: usize) {
        if viewport_height == 0 {
            return;
        }
        if self.selected < self.scroll_offset {
            self.scroll_offset = self.selected;
        } else if self.selected >= self.scroll_offset + viewport_height {
            self.scroll_offset = self.selected - viewport_height + 1;
        }
    }

    /// Open delete confirmation for the currently selected item.
    pub fn request_delete(&mut self) {
        if self.visible.is_empty() {
            return;
        }
        // Don't allow deleting the root item
        let idx = self.visible[self.selected];
        if self.items[idx].depth == 0 {
            return;
        }
        self.delete_target = Some(idx);
    }

    /// Cancel the pending delete confirmation.
    pub fn cancel_delete(&mut self) {
        self.delete_target = None;
    }

    /// Get the full path and whether it's a directory for the delete target.
    pub fn delete_target_info(&self) -> Option<(String, bool)> {
        self.delete_target.map(|idx| {
            let item = &self.items[idx];
            (item.full_path.clone(), item.has_children)
        })
    }

    // ── Projects mode ────────────────────────────────────────────

    /// Per-artifact-directory `(name, size)` breakdown for the selected
    /// project, computed lazily and cached until the selection changes.
    pub fn project_detail_cloned(&mut self) -> Vec<(String, u64)> {
        if self.projects.is_empty() {
            return Vec::new();
        }
        let idx = self.projects_selected;
        if let Some((cached_idx, ref br)) = self.cached_project_detail {
            if cached_idx == idx {
                return br.clone();
            }
        }
        let breakdown = compute_breakdown(&self.projects[idx]);
        self.cached_project_detail = Some((idx, breakdown.clone()));
        breakdown
    }

    /// Replace the project list (from a background scan), sorting by
    /// reclaimable size descending and resetting selection/scroll/cache.
    pub fn set_projects(&mut self, mut entries: Vec<ProjectEntry>) {
        entries.sort_by(|a, b| b.reclaimable.cmp(&a.reclaimable));
        self.projects = entries;
        self.projects_selected = 0;
        self.projects_scroll = 0;
        self.cached_project_detail = None;
    }

    /// Largest reclaimable size among all projects — the scale for the list's
    /// size bars. Returns 1 to avoid division by zero when the list is empty.
    pub fn projects_max_reclaimable(&self) -> u64 {
        self.projects
            .iter()
            .map(|e| e.reclaimable)
            .max()
            .unwrap_or(1)
            .max(1)
    }

    /// Total reclaimable bytes across every project — the summary figure shown
    /// in the title bar and the clean-all dialog.
    pub fn total_reclaimable(&self) -> u64 {
        self.projects.iter().map(|e| e.reclaimable).sum()
    }

    /// Open the "clean all projects" confirmation dialog.
    pub fn request_clean_all(&mut self) {
        if !self.projects.is_empty() {
            self.clean_all_pending = true;
        }
    }

    pub fn cancel_clean_all(&mut self) {
        self.clean_all_pending = false;
    }

    pub fn projects_up(&mut self) {
        if self.projects_selected > 0 {
            self.projects_selected -= 1;
        }
    }

    pub fn projects_down(&mut self) {
        if self.projects_selected + 1 < self.projects.len() {
            self.projects_selected += 1;
        }
    }

    pub fn projects_jump_top(&mut self) {
        self.projects_selected = 0;
    }

    pub fn projects_jump_bottom(&mut self) {
        if !self.projects.is_empty() {
            self.projects_selected = self.projects.len() - 1;
        }
    }

    /// Keep the selected project row within the viewport (mirrors
    /// `adjust_scroll` for the disk tree).
    pub fn projects_adjust_scroll(&mut self, viewport_height: usize) {
        if viewport_height == 0 {
            return;
        }
        if self.projects_selected < self.projects_scroll {
            self.projects_scroll = self.projects_selected;
        } else if self.projects_selected >= self.projects_scroll + viewport_height {
            self.projects_scroll = self.projects_selected - viewport_height + 1;
        }
    }

    /// Open the clean-confirmation dialog for the selected project, capturing
    /// its artifact-directory breakdown for display.
    pub fn request_clean(&mut self) {
        if self.projects.is_empty() {
            return;
        }
        let idx = self.projects_selected;
        self.clean_breakdown = compute_breakdown(&self.projects[idx]);
        self.clean_target = Some(idx);
    }

    pub fn cancel_clean(&mut self) {
        self.clean_target = None;
        self.clean_breakdown.clear();
    }

    /// Remove the cleaned project from the list (Kondo drops zero-artifact
    /// projects) and reset dialog state. Called after a successful clean.
    pub fn remove_cleaned_project(&mut self) {
        let idx = self.clean_target.unwrap_or(self.projects_selected);
        if idx < self.projects.len() {
            self.projects.remove(idx);
        }
        self.cached_project_detail = None;
        self.clean_target = None;
        self.clean_breakdown.clear();
        if !self.projects.is_empty() && self.projects_selected >= self.projects.len() {
            self.projects_selected = self.projects.len() - 1;
        }
    }
}

/// Compute the non-empty `(name, size)` artifact-directory breakdown for a
/// project, sorted largest first. Each directory is walked once via
/// [`dir_size`](crate::dir_size); callers cache the result.
fn compute_breakdown(entry: &ProjectEntry) -> Vec<(String, u64)> {
    let opts = scan_opts();
    let mut breakdown: Vec<(String, u64)> = entry
        .artifact_dir_names
        .iter()
        .map(|d| {
            let size = dir_size(&Path::new(&entry.path).join(d), &opts);
            (d.clone(), size)
        })
        .filter(|(_, s)| *s > 0)
        .collect();
    breakdown.sort_by(|a, b| b.1.cmp(&a.1));
    breakdown
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;

    fn entry(path: &str, reclaimable: u64) -> ProjectEntry {
        ProjectEntry {
            path: path.to_string(),
            type_name: "Cargo".to_string(),
            reclaimable,
            last_modified: Some(SystemTime::UNIX_EPOCH),
            artifact_dir_names: vec!["target".to_string()],
        }
    }

    #[test]
    fn set_projects_sorts_by_reclaimable_desc() {
        let mut s = AppState::new_empty("/".into(), false);
        s.set_projects(vec![entry("a", 100), entry("b", 500), entry("c", 300)]);
        assert_eq!(s.projects.len(), 3);
        assert_eq!(s.projects[0].path, "b"); // 500
        assert_eq!(s.projects[1].path, "c"); // 300
        assert_eq!(s.projects[2].path, "a"); // 100
        assert_eq!(s.projects_selected, 0);
    }

    #[test]
    fn remove_cleaned_project_removes_target_and_clamps_selection() {
        let mut s = AppState::new_empty("/".into(), false);
        s.set_projects(vec![entry("a", 100), entry("b", 500), entry("c", 300)]);
        // After sort-desc the order is [b(500), c(300), a(100)].
        assert_eq!(s.projects[1].path, "c");
        s.projects_selected = 1; // select "c"
        s.request_clean(); // clean_target = Some(1)
        assert_eq!(s.clean_target, Some(1));
        s.remove_cleaned_project();
        assert!(!s.projects.iter().any(|e| e.path == "c"));
        assert_eq!(s.projects.len(), 2);
        assert_eq!(s.clean_target, None);
        assert!(s.projects_selected < s.projects.len());
    }

    #[test]
    fn projects_navigation_clamps_at_ends() {
        let mut s = AppState::new_empty("/".into(), false);
        s.set_projects(vec![entry("a", 1), entry("b", 2)]);
        s.projects_jump_bottom();
        assert_eq!(s.projects_selected, 1);
        s.projects_down(); // already last
        assert_eq!(s.projects_selected, 1);
        s.projects_jump_top();
        assert_eq!(s.projects_selected, 0);
        s.projects_up(); // already first
        assert_eq!(s.projects_selected, 0);
    }

    #[test]
    fn projects_max_reclaimable_is_at_least_one() {
        let s = AppState::new_empty("/".into(), false);
        assert_eq!(s.projects_max_reclaimable(), 1); // empty list → guard
    }

    #[test]
    fn total_reclaimable_sums_all_projects() {
        let mut s = AppState::new_empty("/".into(), false);
        assert_eq!(s.total_reclaimable(), 0); // empty
        s.set_projects(vec![entry("a", 100), entry("b", 500), entry("c", 300)]);
        assert_eq!(s.total_reclaimable(), 900);
    }
}

/// Extract file extension (lowercase, without the dot).
fn get_extension(name: &str) -> String {
    if let Some(pos) = name.rfind('.') {
        if pos > 0 && pos < name.len() - 1 {
            return name[pos + 1..].to_lowercase();
        }
    }
    String::new()
}

/// Flatten a DiskItem tree into a Vec<FlatItem>. Can run on a background thread.
pub fn flatten_disk_item(
    item: &DiskItem,
    parent_path: String,
    parent_size: u64,
    depth: usize,
    parent_idx: Option<usize>,
    out: &mut Vec<FlatItem>,
) {
    let idx = out.len();
    let extension = get_extension(&item.name);

    let full_path = if depth == 0 {
        parent_path.clone()
    } else {
        format!("{}/{}", parent_path, item.name)
    };

    out.push(FlatItem {
        name: item.name.clone(),
        full_path: full_path.clone(),
        parent: parent_idx,
        disk_size: item.disk_size,
        parent_size,
        depth,
        has_children: item.children.is_some(),
        extension,
        direct_children: Vec::new(),
    });

    if let Some(children) = &item.children {
        for child in children {
            let child_idx = out.len();
            out[idx].direct_children.push(child_idx);
            flatten_disk_item(child, full_path.clone(), item.disk_size, depth + 1, Some(idx), out);
        }
    }
}

/// Build a file size distribution histogram.
fn build_size_histogram(sizes: &[u64]) -> Vec<(String, usize)> {
    const BUCKETS: [(u64, u64, &str); 8] = [
        (0,        1024,          "<1KB"),
        (1024,     10*1024,       "1-10K"),
        (10*1024,  100*1024,      "10-100K"),
        (100*1024, 1024*1024,     "100K-1M"),
        (1024*1024, 10*1024*1024,  "1-10M"),
        (10*1024*1024, 100*1024*1024, "10-100M"),
        (100*1024*1024, 1024*1024*1024, "100M-1G"),
        (1024*1024*1024, u64::MAX, ">1G"),
    ];

    let mut counts = vec![0usize; BUCKETS.len()];
    for &size in sizes {
        for (i, &(lo, hi, _)) in BUCKETS.iter().enumerate() {
            if size >= lo && size < hi {
                counts[i] += 1;
                break;
            }
        }
    }

    BUCKETS
        .iter()
        .zip(counts.iter())
        .filter(|(_, &c)| c > 0)
        .map(|((_, _, label), &c)| (label.to_string(), c))
        .collect()
}
