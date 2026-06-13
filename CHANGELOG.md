## [unreleased]

### 🚀 Features

- **TUI: Projects mode** — a Kondo-style second view (`p` / `Tab`) that scans the
  selected directory's subtree and lists every build project (Cargo, Node, Maven,
  Unity, …) with its reclaimable size, sorted descending. Right panel shows the
  project type, a reclaimable gauge, per-artifact-directory breakdown, and age.
- **Clean projects** — `c` / `Enter` cleans the selected project; `C` cleans all
  projects at once. Both use HUD confirmation dialogs. Clean-all runs in a
  background thread and re-scans afterward so the list stays accurate.
- **Total reclaimable summary** — title bar shows the combined reclaimable size
  across all listed projects.
- **Refresh** — `r` re-scans the current view; the disk tree auto-refreshes when
  returning from Projects mode after a clean (no stale sizes).

### 🐛 Bug Fixes

- **Nested sub-crates** — project scan now descends into projects (pruning
  artifact directories like `target` / `node_modules`) instead of skipping the
  whole subtree, so a Cargo workspace's sub-crates with their own artifacts are
  found. Parents no longer double-count a child's artifact directory.

### ⚙️ Miscellaneous Tasks

- Port `projects` module (`scan` / `analyze` / `clean` over 23 ecosystems).
- `Project::clean()` now returns `Result` so failures surface in the TUI.
