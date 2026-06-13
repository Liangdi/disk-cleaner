# dirstat-rs

Fast, cross-platform disk usage CLI with an interactive TUI mode.

[![Crates.io](https://img.shields.io/crates/v/dirstat-rs.svg)](https://crates.io/crates/dirstat-rs)
[![Docs.rs](https://docs.rs/dirstat-rs/badge.svg)](https://docs.rs/dirstat-rs/)
![Language](https://img.shields.io/badge/language-rust-orange)
![Platforms](https://img.shields.io/badge/platforms-Windows%2C%20macOS%20and%20Linux-blue)
![License](https://img.shields.io/github/license/scullionw/dirstat-rs)

![](demo/ds_demo.gif)

2X faster than du

4X faster than ncdu, dutree, dua, du-dust

6X faster than windirstat

(On 4-core hyperthreaded cpu)

# Installation

## Homebrew (macOS only)

    brew tap scullionw/tap
    brew install dirstat-rs

## Or if you prefer compiling yourself

### from crates.io:

        cargo install dirstat-rs
        
### or latest from git:

        cargo install --git "https://github.com/scullionw/dirstat-rs"
        
### or from source:

        cargo build --release
        sudo chmod +x /target/release/ds
        sudo cp /target/release/ds /usr/local/bin/

# Usage

### Current directory

        $ ds

### Specific path

        $ ds PATH

### Options

| Flag | Description |
|------|-------------|
| `-d <depth>` | Max recursion depth (default 1) |
| `-m <percent>` | Minimum percentage threshold |
| `-a` | Show apparent size instead of disk usage |
| `-j` | JSON output (flat first-level data only) |
| `--tui` | Launch interactive TUI mode |

### Examples

        $ ds -d 3
        $ ds -a PATH
        $ ds -m 0.2 PATH
        $ ds -j PATH
        $ ds --tui
        $ ds --tui /path/to/dir

### CLI output

Each entry shows a visual percentage bar and directories are suffixed with `/` for quick identification. A bottom summary line displays total size, file count, and directory count.

```
$ ds -d 2 ~/projects
╭─────────────────────────────────────╮
│         dirstat-rs 0.x.x           │
╰─────────────────────────────────────╯
  12.4 GiB  ████████████████████░░░  82%  rust/
   1.8 GiB  ███░░░░░░░░░░░░░░░░░░░  12%  node/
 480.0 MiB  █░░░░░░░░░░░░░░░░░░░░░   3%  python/
  96.0 MiB  ░░░░░░░░░░░░░░░░░░░░░░   1%  go/
   14.0 GiB total · 42,317 files · 128 dirs
```

### TUI mode

Launch an interactive terminal UI with `--tui`:

        $ ds --tui
        $ ds --tui /path/to/dir

The TUI has two panels:

- **Left panel** -- navigable directory tree with expand/collapse
- **Right panel** -- detail stats: breadcrumb path, proportion gauge, file type distribution, size histogram, and top 10 largest files

An animated loading splash is shown while scanning.

#### Key bindings

| Key | Action |
|-----|--------|
| `j` / `k` | Navigate down / up |
| `Enter` / `l` | Expand directory |
| `Backspace` / `h` | Collapse directory |
| `Space` | Toggle expand / collapse |
| `/` | Search |
| `a` | Toggle apparent size |
| `d` | Enter selected directory |
| `u` | Go up to parent directory |
| `g` | Jump to top |
| `G` | Jump to bottom |
| `q` | Quit |
