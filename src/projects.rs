//! Locate build-tool artifact directories across many project types and tell
//! you how much disk space they waste — optionally deleting them.
//!
//! A *project* is any directory containing a recognized marker file
//! (`Cargo.toml`, `package.json`, `pom.xml`, …). Each project has one or more
//! [`ProjectType`]s, and each type maps to a set of *artifact directories*
//! (`target`, `node_modules`, `build`, …) that are safe to delete because the
//! toolchain will regenerate them.
//!
//! A single directory may match several project types at once: a Rust + web
//! project with both `Cargo.toml` and `package.json` is `{Cargo, Node}`, and
//! [`Project::artifact_dirs`] yields the **union** of their artifact
//! directories (`target` + `node_modules`).
//!
//! This module performs no destructive actions on its own — you must call
//! [`Project::clean`] explicitly to delete anything.
//!
//! # Example
//!
//! ```no_run
//! use disk_cleaner::projects::{scan, ScanOptions};
//!
//! let opts = ScanOptions { follow_symlinks: false, same_file_system: false, apparent: true };
//! for project in scan(&".", &opts).filter_map(Result::ok) {
//!     println!("{} [{}]", project.path.display(), project.type_name());
//! }
//! ```
//!
//! [`ProjectType`]: ProjectType
//! [`Project::artifact_dirs`]: Project::artifact_dirs
//! [`Project::clean`]: Project::clean

use std::{
    borrow::Cow,
    error,
    fs,
    path::{self, Path},
    time::SystemTime,
};

const FILE_CARGO_TOML: &str = "Cargo.toml";
const FILE_PACKAGE_JSON: &str = "package.json";
const FILE_ASSEMBLY_CSHARP: &str = "Assembly-CSharp.csproj";
const FILE_STACK_HASKELL: &str = "stack.yaml";
const FILE_CABAL_HASKELL: &str = "cabal.project";
const FILE_SBT_BUILD: &str = "build.sbt";
const FILE_MVN_BUILD: &str = "pom.xml";
const FILE_BUILD_GRADLE: &str = "build.gradle";
const FILE_BUILD_GRADLE_KTS: &str = "build.gradle.kts";
const FILE_CMAKE_BUILD: &str = "CMakeLists.txt";
const FILE_UNREAL_SUFFIX: &str = ".uproject";
const FILE_JUPYTER_SUFFIX: &str = ".ipynb";
const FILE_PYTHON_SUFFIX: &str = ".py";
const FILE_PIXI_PACKAGE: &str = "pixi.toml";
const FILE_COMPOSER_JSON: &str = "composer.json";
const FILE_PUBSPEC_YAML: &str = "pubspec.yaml";
const FILE_ELIXIR_MIX: &str = "mix.exs";
const FILE_SWIFT_PACKAGE: &str = "Package.swift";
const FILE_BUILD_ZIG: &str = "build.zig";
const FILE_GODOT_4_PROJECT: &str = "project.godot";
const FILE_CSPROJ_SUFFIX: &str = ".csproj";
const FILE_FSPROJ_SUFFIX: &str = ".fsproj";
const FILE_TERRAFORM_HCL: &str = ".terraform.lock.hcl";
const FILE_PROJECT_TURBOREPO: &str = "turbo.json";
const FILE_PODFILE: &str = "Podfile";

const PROJECT_CARGO_DIRS: [&str; 2] = ["target", ".xwin-cache"];
const PROJECT_NODE_DIRS: [&str; 2] = ["node_modules", ".angular"];
const PROJECT_REACT_NATIVE_DIRS: [&str; 8] = [
    "node_modules",
    "android/build",
    "android/.gradle",
    "ios/build",
    "ios/DerivedData",
    "ios/Pods",
    ".expo",
    ".metro",
];
const PROJECT_UNITY_DIRS: [&str; 7] = [
    "Library",
    "Temp",
    "Obj",
    "Logs",
    "MemoryCaptures",
    "Build",
    "Builds",
];
const PROJECT_STACK_DIRS: [&str; 1] = [".stack-work"];
const PROJECT_CABAL_DIRS: [&str; 1] = ["dist-newstyle"];
const PROJECT_SBT_DIRS: [&str; 2] = ["target", "project/target"];
const PROJECT_MVN_DIRS: [&str; 1] = ["target"];
const PROJECT_GRADLE_DIRS: [&str; 2] = ["build", ".gradle"];
const PROJECT_CMAKE_DIRS: [&str; 3] = ["build", "cmake-build-debug", "cmake-build-release"];
const PROJECT_UNREAL_DIRS: [&str; 5] = [
    "Binaries",
    "Build",
    "Saved",
    "DerivedDataCache",
    "Intermediate",
];
const PROJECT_JUPYTER_DIRS: [&str; 1] = [".ipynb_checkpoints"];
const PROJECT_PYTHON_DIRS: [&str; 7] = [
    ".mypy_cache",
    ".nox",
    ".pytest_cache",
    ".ruff_cache",
    ".tox",
    "__pycache__",
    "__pypackages__",
];
const PROJECT_PIXI_DIRS: [&str; 1] = [".pixi"];
const PROJECT_COMPOSER_DIRS: [&str; 1] = ["vendor"];
const PROJECT_PUB_DIRS: [&str; 4] = [
    "build",
    ".dart_tool",
    "linux/flutter/ephemeral",
    "windows/flutter/ephemeral",
];
const PROJECT_ELIXIR_DIRS: [&str; 4] = ["_build", ".elixir-tools", ".elixir_ls", ".lexical"];
const PROJECT_SWIFT_DIRS: [&str; 2] = [".build", ".swiftpm"];
const PROJECT_ZIG_DIRS: [&str; 3] = ["zig-cache", ".zig-cache", "zig-out"];
const PROJECT_GODOT_4_DIRS: [&str; 1] = [".godot"];
const PROJECT_DOTNET_DIRS: [&str; 2] = ["bin", "obj"];
const PROJECT_TURBOREPO_DIRS: [&str; 1] = [".turbo"];
const PROJECT_TERRAFORM_DIRS: [&str; 1] = [".terraform"];
const PROJECT_COCOAPODS_DIRS: [&str; 1] = ["Pods"];

const PROJECT_CARGO_NAME: &str = "Cargo";
const PROJECT_NODE_NAME: &str = "Node";
const PROJECT_NODE_REACT_NATIVE_NAME: &str = "Node (React Native)";
const PROJECT_UNITY_NAME: &str = "Unity";
const PROJECT_STACK_NAME: &str = "Stack";
const PROJECT_CABAL_NAME: &str = "Cabal";
const PROJECT_SBT_NAME: &str = "SBT";
const PROJECT_MVN_NAME: &str = "Maven";
const PROJECT_GRADLE_NAME: &str = "Gradle";
const PROJECT_CMAKE_NAME: &str = "CMake";
const PROJECT_UNREAL_NAME: &str = "Unreal";
const PROJECT_JUPYTER_NAME: &str = "Jupyter";
const PROJECT_PYTHON_NAME: &str = "Python";
const PROJECT_PIXI_NAME: &str = "Pixi";
const PROJECT_COMPOSER_NAME: &str = "Composer";
const PROJECT_PUB_NAME: &str = "Pub";
const PROJECT_ELIXIR_NAME: &str = "Elixir";
const PROJECT_SWIFT_NAME: &str = "Swift";
const PROJECT_ZIG_NAME: &str = "Zig";
const PROJECT_GODOT_4_NAME: &str = "Godot 4.x";
const PROJECT_DOTNET_NAME: &str = ".NET";
const PROJECT_TURBOREPO_NAME: &str = "Turborepo";
const PROJECT_TERRAFORM_NAME: &str = "Terraform";
const PROJECT_COCOAPODS_NAME: &str = "CocoaPods";

/// A recognized development-project ecosystem (Cargo, Node, Unity, …).
///
/// A single directory may be classified as several `ProjectType`s at once —
/// for example a directory with both `Cargo.toml` and `package.json` is
/// `{Cargo, Node}`. Variant ordering follows declaration order and is used to
/// keep `type_name()` and similar output stable and deterministic rather than
/// dependent on filesystem `readdir` order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProjectType {
    Cargo,
    Node,
    Unity,
    Stack,
    Cabal,
    #[allow(clippy::upper_case_acronyms)]
    SBT,
    Maven,
    Gradle,
    CMake,
    Unreal,
    Jupyter,
    Python,
    Pixi,
    Composer,
    Pub,
    Elixir,
    Swift,
    Zig,
    Godot4,
    Dotnet,
    Turborepo,
    Terraform,
    Cocoapods,
}

/// A discovered project: a directory on disk plus the set of project types
/// detected within it.
///
/// Obtain one via [`scan`](scan), or construct it directly. The struct is
/// otherwise inert — call [`clean`](Project::clean) to delete the project's
/// artifact directories.
#[derive(Debug, Clone)]
pub struct Project {
    /// Every project type detected in `path` (at least one). Ordered by
    /// `ProjectType` declaration order, de-duplicated.
    pub project_types: Vec<ProjectType>,
    /// The project's root directory.
    pub path: path::PathBuf,
}

/// A breakdown of a project directory's disk usage, produced by
/// [`Project::size_dirs`].
#[derive(Debug, Clone)]
pub struct ProjectSize {
    pub artifact_size: u64,
    pub non_artifact_size: u64,
    pub dirs: Vec<(String, u64, bool)>,
}

fn artifact_dirs_for(pt: ProjectType, path: &Path) -> &'static [&'static str] {
    match pt {
        ProjectType::Cargo => &PROJECT_CARGO_DIRS,
        ProjectType::Node => {
            if is_react_native_project(path) {
                &PROJECT_REACT_NATIVE_DIRS
            } else {
                &PROJECT_NODE_DIRS
            }
        }
        ProjectType::Unity => &PROJECT_UNITY_DIRS,
        ProjectType::Stack => &PROJECT_STACK_DIRS,
        ProjectType::Cabal => &PROJECT_CABAL_DIRS,
        ProjectType::SBT => &PROJECT_SBT_DIRS,
        ProjectType::Maven => &PROJECT_MVN_DIRS,
        ProjectType::Unreal => &PROJECT_UNREAL_DIRS,
        ProjectType::Jupyter => &PROJECT_JUPYTER_DIRS,
        ProjectType::Python => &PROJECT_PYTHON_DIRS,
        ProjectType::Pixi => &PROJECT_PIXI_DIRS,
        ProjectType::CMake => &PROJECT_CMAKE_DIRS,
        ProjectType::Composer => &PROJECT_COMPOSER_DIRS,
        ProjectType::Pub => &PROJECT_PUB_DIRS,
        ProjectType::Elixir => &PROJECT_ELIXIR_DIRS,
        ProjectType::Swift => &PROJECT_SWIFT_DIRS,
        ProjectType::Gradle => &PROJECT_GRADLE_DIRS,
        ProjectType::Zig => &PROJECT_ZIG_DIRS,
        ProjectType::Godot4 => &PROJECT_GODOT_4_DIRS,
        ProjectType::Dotnet => &PROJECT_DOTNET_DIRS,
        ProjectType::Turborepo => &PROJECT_TURBOREPO_DIRS,
        ProjectType::Terraform => &PROJECT_TERRAFORM_DIRS,
        ProjectType::Cocoapods => &PROJECT_COCOAPODS_DIRS,
    }
}

fn type_name_for(pt: ProjectType, path: &Path) -> &'static str {
    match pt {
        ProjectType::Cargo => PROJECT_CARGO_NAME,
        ProjectType::Node => {
            if is_react_native_project(path) {
                PROJECT_NODE_REACT_NATIVE_NAME
            } else {
                PROJECT_NODE_NAME
            }
        }
        ProjectType::Unity => PROJECT_UNITY_NAME,
        ProjectType::Stack => PROJECT_STACK_NAME,
        ProjectType::Cabal => PROJECT_CABAL_NAME,
        ProjectType::SBT => PROJECT_SBT_NAME,
        ProjectType::Maven => PROJECT_MVN_NAME,
        ProjectType::Unreal => PROJECT_UNREAL_NAME,
        ProjectType::Jupyter => PROJECT_JUPYTER_NAME,
        ProjectType::Python => PROJECT_PYTHON_NAME,
        ProjectType::Pixi => PROJECT_PIXI_NAME,
        ProjectType::CMake => PROJECT_CMAKE_NAME,
        ProjectType::Composer => PROJECT_COMPOSER_NAME,
        ProjectType::Pub => PROJECT_PUB_NAME,
        ProjectType::Elixir => PROJECT_ELIXIR_NAME,
        ProjectType::Swift => PROJECT_SWIFT_NAME,
        ProjectType::Gradle => PROJECT_GRADLE_NAME,
        ProjectType::Zig => PROJECT_ZIG_NAME,
        ProjectType::Godot4 => PROJECT_GODOT_4_NAME,
        ProjectType::Dotnet => PROJECT_DOTNET_NAME,
        ProjectType::Turborepo => PROJECT_TURBOREPO_NAME,
        ProjectType::Terraform => PROJECT_TERRAFORM_NAME,
        ProjectType::Cocoapods => PROJECT_COCOAPODS_NAME,
    }
}

impl Project {
    /// The de-duplicated union of artifact directories across all detected
    /// project types in this directory (e.g. a Cargo+Node project yields both
    /// `target` and `node_modules`).
    pub fn artifact_dirs(&self) -> Vec<&'static str> {
        let mut dirs: Vec<&'static str> = Vec::new();
        for pt in &self.project_types {
            for d in artifact_dirs_for(*pt, &self.path) {
                if !dirs.contains(d) {
                    dirs.push(*d);
                }
            }
        }
        dirs
    }

    /// The project's path as a lossy UTF-8 string (for display).
    pub fn name(&self) -> Cow<'_, str> {
        self.path.to_string_lossy()
    }

    /// Total size in bytes of this project's artifact directories — the amount
    /// [`clean`](Project::clean) would reclaim.
    pub fn size(&self, options: &ScanOptions) -> u64 {
        self.artifact_dirs()
            .iter()
            .copied()
            .map(|p| dir_size(&self.path.join(p), options))
            .sum()
    }

    /// The most recent modification time across all entries in the project tree.
    pub fn last_modified(&self, options: &ScanOptions) -> Result<SystemTime, std::io::Error> {
        let top_level_modified = fs::metadata(&self.path)?.modified()?;
        let most_recent_modified = walkdir::WalkDir::new(&self.path)
            .follow_links(options.follow_symlinks)
            .same_file_system(options.same_file_system)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter_map(|e| e.metadata().ok())
            .filter_map(|m| m.modified().ok())
            .fold(top_level_modified, |acc, m| if m > acc { m } else { acc });
        Ok(most_recent_modified)
    }

    /// Per-top-level-entry disk usage for the project: total artifact bytes,
    /// total non-artifact bytes, and a list of `(name, size, is_artifact)`.
    pub fn size_dirs(&self, options: &ScanOptions) -> ProjectSize {
        let mut artifact_size = 0;
        let mut non_artifact_size = 0;
        let mut dirs = Vec::new();

        let project_root = match fs::read_dir(&self.path) {
            Err(_) => {
                return ProjectSize {
                    artifact_size,
                    non_artifact_size,
                    dirs,
                }
            }
            Ok(rd) => rd,
        };

        for entry in project_root.filter_map(|rd| rd.ok()) {
            let file_type = match entry.file_type() {
                Err(_) => continue,
                Ok(file_type) => file_type,
            };

            if file_type.is_file() {
                if let Ok(metadata) = entry.metadata() {
                    non_artifact_size += metadata.len();
                }
                continue;
            }

            if file_type.is_dir() {
                let file_name = match entry.file_name().into_string() {
                    Err(_) => continue,
                    Ok(file_name) => file_name,
                };
                let size = dir_size(&entry.path(), options);
                let artifact_dir = self.artifact_dirs().contains(&file_name.as_str());
                if artifact_dir {
                    artifact_size += size;
                } else {
                    non_artifact_size += size;
                }
                dirs.push((file_name, size, artifact_dir));
            }
        }

        ProjectSize {
            artifact_size,
            non_artifact_size,
            dirs,
        }
    }

    /// Human-readable project-type label(s), joined by `" / "` — e.g.
    /// `"Cargo"` or `"Cargo / Node"`.
    pub fn type_name(&self) -> String {
        self.project_types
            .iter()
            .map(|pt| type_name_for(*pt, &self.path))
            .collect::<Vec<&str>>()
            .join(" / ")
    }

    /// Deletes the project's artifact directories and their contents.
    ///
    /// Every artifact directory is attempted even if an earlier one fails; the
    /// first error (if any) is returned so callers — e.g. a TUI — can surface
    /// it. Returns `Ok(())` when all present artifact directories were removed.
    pub fn clean(&self) -> Result<(), Box<dyn error::Error>> {
        let mut failures: Vec<(path::PathBuf, std::io::Error)> = Vec::new();
        for artifact_dir in self
            .artifact_dirs()
            .iter()
            .copied()
            .map(|ad| self.path.join(ad))
            .filter(|ad| ad.exists())
        {
            if let Err(e) = fs::remove_dir_all(&artifact_dir) {
                failures.push((artifact_dir, e));
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            let detail = failures
                .iter()
                .map(|(p, e)| format!("{} ({})", p.display(), e))
                .collect::<Vec<_>>()
                .join("; ");
            Err(format!("failed to remove some artifact directories: {detail}").into())
        }
    }
}

fn is_hidden(entry: &walkdir::DirEntry) -> bool {
    entry.file_name().to_string_lossy().starts_with('.')
}

/// Union of every artifact-directory name across all [`ProjectType`]s.
///
/// [`scan`] prunes any directory whose name matches, so it can descend into a
/// project's real subdirectories — finding **nested** projects such as a Cargo
/// workspace's sub-crates, each of which may carry its own reclaimable
/// artifacts — without descending into heavy build-output trees (`target`,
/// `node_modules`, …). Descending into `node_modules` in particular would
/// otherwise report every vendored package as a project.
///
/// Names starting with `.` are also pruned by [`is_hidden`], but are listed
/// here too so pruning does not silently depend on that coincidence. Kept as a
/// flat const (rather than derived from the per-type arrays) for readability;
/// the project-type list is stable.
const ALL_ARTIFACT_DIR_NAMES: &[&str] = &[
    // Cargo
    "target",
    ".xwin-cache",
    // Node (incl. React Native leaf names)
    "node_modules",
    ".angular",
    ".expo",
    ".metro",
    // Unity
    "Library",
    "Temp",
    "Obj",
    "Logs",
    "MemoryCaptures",
    "Build",
    "Builds",
    // Haskell
    ".stack-work",
    "dist-newstyle",
    // SBT / Maven
    // Gradle / CMake
    "build",
    ".gradle",
    "cmake-build-debug",
    "cmake-build-release",
    // Unreal
    "Binaries",
    "Saved",
    "DerivedDataCache",
    "Intermediate",
    // Jupyter / Python
    ".ipynb_checkpoints",
    ".mypy_cache",
    ".nox",
    ".pytest_cache",
    ".ruff_cache",
    ".tox",
    "__pycache__",
    "__pypackages__",
    // Misc ecosystems
    ".pixi",
    "vendor",
    ".dart_tool",
    "_build",
    ".elixir-tools",
    ".elixir_ls",
    ".lexical",
    ".build",
    ".swiftpm",
    "zig-cache",
    ".zig-cache",
    "zig-out",
    ".godot",
    ".turbo",
    ".terraform",
    "Pods",
    "bin",
    "obj",
];

/// Whether `name` is a known artifact (build-output) directory name — i.e. a
/// subtree [`scan`] should never descend into when hunting for nested projects.
fn is_artifact_dir_name(name: &str) -> bool {
    ALL_ARTIFACT_DIR_NAMES.contains(&name)
}

struct ProjectIter {
    it: walkdir::IntoIter,
}

/// Errors that can occur while scanning a directory tree for projects.
#[derive(Debug)]
pub enum ScanError {
    /// A plain [`std::io::Error`], e.g. lacking permission to read a directory.
    IOError(::std::io::Error),
    /// An error from the underlying `walkdir` traversal.
    WalkdirError(walkdir::Error),
}

impl std::fmt::Display for ScanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScanError::IOError(e) => write!(f, "io error: {e}"),
            ScanError::WalkdirError(e) => write!(f, "directory traversal error: {e}"),
        }
    }
}

impl std::error::Error for ScanError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ScanError::IOError(e) => Some(e),
            ScanError::WalkdirError(e) => Some(e),
        }
    }
}

impl Iterator for ProjectIter {
    type Item = Result<Project, ScanError>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let entry: walkdir::DirEntry = match self.it.next() {
                None => return None,
                Some(Err(e)) => return Some(Err(ScanError::WalkdirError(e))),
                Some(Ok(entry)) => entry,
            };
            if !entry.file_type().is_dir() {
                continue;
            }
            // Prune hidden dirs and artifact (build-output) dirs: never descend
            // into them when searching for nested projects. Artifact dirs are
            // build output (target/, node_modules/, …) — descending into
            // node_modules would report every vendored package as a project.
            if is_hidden(&entry) || is_artifact_dir_name(&entry.file_name().to_string_lossy()) {
                self.it.skip_current_dir();
                continue;
            }
            let project_types = match detect_project_types(entry.path()) {
                Err(e) => return Some(Err(ScanError::IOError(e))),
                Ok(project_types) if project_types.is_empty() => continue,
                Ok(project_types) => project_types,
            };
            // Emit the project but KEEP descending into its subtree: a project
            // may contain nested projects (e.g. a Cargo workspace's sub-crates)
            // that carry their own reclaimable artifacts. Artifact subdirs were
            // already pruned above, so this never descends into build output.
            return Some(Ok(Project {
                project_types,
                path: entry.path().to_path_buf(),
            }));
        }
    }
}

fn dir_contains_subdir(path: &Path, subdir: &str) -> bool {
    path.read_dir()
        .map(|rd| {
            rd.filter_map(|rd| rd.ok()).any(|de| {
                de.file_type().is_ok_and(|t| t.is_dir()) && de.file_name().to_str() == Some(subdir)
            })
        })
        .unwrap_or(false)
}

fn is_react_native_project(path: &Path) -> bool {
    dir_contains_subdir(path, "ios") || dir_contains_subdir(path, "android")
}

/// Detect every project type whose marker file is present in `path`.
///
/// A directory may legitimately contain markers for more than one ecosystem
/// (e.g. `Cargo.toml` + `package.json`); all of them are returned so the
/// caller can clean the union of their artifact directories.
///
/// The directory is read exactly once: every file name is collected into a
/// `Vec` first (rather than classified on the fly) because a `.csproj`/`.fsproj`
/// may be visited before `project.godot` or `Assembly-CSharp.csproj`. Collecting
/// everything first lets the disambiguation between Godot4 / Unity / .NET be
/// done with precomputed flags after the full picture of the directory is known.
fn detect_project_types(path: &Path) -> Result<Vec<ProjectType>, std::io::Error> {
    // Single read_dir: collect every file name and precompute the two flags the
    // csproj/fsproj branch needs. This avoids re-reading the directory once per
    // csproj file (a Unity project commonly has 20+ of them).
    let mut file_names: Vec<String> = Vec::new();
    let mut has_godot = false;
    let mut has_assembly = false;
    for dir_entry in path.read_dir()?.filter_map(|rd| rd.ok()) {
        if !dir_entry.file_type().is_ok_and(|ft| ft.is_file()) {
            continue;
        }
        let file_name = match dir_entry.file_name().into_string() {
            Ok(file_name) => file_name,
            Err(_) => continue,
        };
        if file_name == FILE_GODOT_4_PROJECT {
            has_godot = true;
        } else if file_name == FILE_ASSEMBLY_CSHARP {
            has_assembly = true;
        }
        file_names.push(file_name);
    }

    let mut types: Vec<ProjectType> = Vec::new();
    for file_name in &file_names {
        let p_type = match file_name.as_str() {
            FILE_CARGO_TOML => Some(ProjectType::Cargo),
            FILE_PACKAGE_JSON => Some(ProjectType::Node),
            FILE_ASSEMBLY_CSHARP => Some(ProjectType::Unity),
            FILE_STACK_HASKELL => Some(ProjectType::Stack),
            FILE_CABAL_HASKELL => Some(ProjectType::Cabal),
            FILE_SBT_BUILD => Some(ProjectType::SBT),
            FILE_MVN_BUILD => Some(ProjectType::Maven),
            FILE_CMAKE_BUILD => Some(ProjectType::CMake),
            FILE_COMPOSER_JSON => Some(ProjectType::Composer),
            FILE_PUBSPEC_YAML => Some(ProjectType::Pub),
            FILE_PIXI_PACKAGE => Some(ProjectType::Pixi),
            FILE_ELIXIR_MIX => Some(ProjectType::Elixir),
            FILE_SWIFT_PACKAGE => Some(ProjectType::Swift),
            FILE_BUILD_GRADLE => Some(ProjectType::Gradle),
            FILE_BUILD_GRADLE_KTS => Some(ProjectType::Gradle),
            FILE_BUILD_ZIG => Some(ProjectType::Zig),
            FILE_GODOT_4_PROJECT => Some(ProjectType::Godot4),
            FILE_PROJECT_TURBOREPO => Some(ProjectType::Turborepo),
            FILE_TERRAFORM_HCL => Some(ProjectType::Terraform),
            FILE_PODFILE => Some(ProjectType::Cocoapods),
            file_name if file_name.ends_with(FILE_UNREAL_SUFFIX) => Some(ProjectType::Unreal),
            file_name if file_name.ends_with(FILE_JUPYTER_SUFFIX) => Some(ProjectType::Jupyter),
            file_name if file_name.ends_with(FILE_PYTHON_SUFFIX) => Some(ProjectType::Python),
            file_name
                if file_name.ends_with(FILE_CSPROJ_SUFFIX)
                    || file_name.ends_with(FILE_FSPROJ_SUFFIX) =>
            {
                if has_godot {
                    Some(ProjectType::Godot4)
                } else if has_assembly {
                    Some(ProjectType::Unity)
                } else {
                    Some(ProjectType::Dotnet)
                }
            }
            _ => None,
        };
        if let Some(pt) = p_type {
            if !types.contains(&pt) {
                types.push(pt);
            }
        }
    }
    types.sort();
    types.dedup();
    Ok(types)
}

/// Options controlling directory traversal, passed to [`scan`](scan) and
/// [`dir_size`](dir_size).
#[derive(Clone, Debug)]
pub struct ScanOptions {
    /// Whether to follow symbolic links during traversal.
    pub follow_symlinks: bool,
    /// Whether to restrict traversal to the same filesystem as the root.
    pub same_file_system: bool,
    /// When `true`, count each file's logical length (`metadata.len()`); when
    /// `false`, count its on-disk allocation (Unix `blocks()*512`, Windows
    /// compressed size).
    pub apparent: bool,
}

fn build_walkdir_iter<P: AsRef<path::Path>>(path: &P, options: &ScanOptions) -> ProjectIter {
    ProjectIter {
        it: walkdir::WalkDir::new(path)
            .follow_links(options.follow_symlinks)
            .same_file_system(options.same_file_system)
            .into_iter(),
    }
}

/// Recursively scan `path` for projects, yielding each wrapped in a [`Result`].
///
/// Hidden directories (name starting with `.`) and artifact directories
/// (`target`, `node_modules`, `build`, … — see [`ALL_ARTIFACT_DIR_NAMES`]) are
/// never descended into. Otherwise the traversal keeps descending even after
/// finding a project, so **nested** projects are reported too — a Cargo
/// workspace and each of its sub-crates, for example, are emitted separately
/// (sub-crates with no artifacts of their own are dropped by [`analyze`]).
///
/// Traversal errors are reported per-entry via [`ScanError`] but do not stop
/// iteration: use `filter_map(Result::ok)` to ignore them, or handle them
/// explicitly.
///
/// [`ScanError`]: ScanError
pub fn scan<P: AsRef<path::Path>>(
    path: &P,
    options: &ScanOptions,
) -> impl Iterator<Item = Result<Project, ScanError>> {
    build_walkdir_iter(path, options)
}

/// Single file's counted size: its logical length when `apparent`, otherwise
/// its on-disk allocation. Mirrors the size semantics of `FileInfo::from_path`
/// but operates on borrowed metadata already obtained by walkdir (no re-stat).
fn file_size(md: &fs::Metadata, path: &Path, apparent: bool) -> u64 {
    if apparent {
        md.len()
    } else {
        allocated_size(md, path)
    }
}

#[cfg(unix)]
fn allocated_size(md: &fs::Metadata, _path: &Path) -> u64 {
    use std::os::unix::fs::MetadataExt;
    md.blocks() * 512
}

#[cfg(windows)]
fn allocated_size(md: &fs::Metadata, path: &Path) -> u64 {
    crate::ffi::compressed_size(path).unwrap_or_else(|_| md.len())
}

#[cfg(not(any(unix, windows)))]
fn allocated_size(md: &fs::Metadata, _path: &Path) -> u64 {
    md.len()
}

/// Total size in bytes of all regular files beneath `path` (recursive),
/// traversed with the same options as [`scan`].
///
/// [`scan`]: scan
pub fn dir_size<P: AsRef<path::Path>>(path: &P, options: &ScanOptions) -> u64 {
    build_walkdir_iter(path, options)
        .it
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| {
            let md = e.metadata().ok()?;
            Some(file_size(&md, e.path(), options.apparent))
        })
        .sum()
}

/// A project plus the results of analyzing it, produced by [`analyze`].
#[derive(Debug, Clone)]
pub struct ProjectAnalysis {
    /// The discovered project.
    pub project: Project,
    /// Total bytes across the project's artifact directories — what
    /// [`Project::clean`] would reclaim.
    pub artifact_size: u64,
    /// Most recent modification time across the project tree, if obtainable.
    pub last_modified: Option<SystemTime>,
}

/// Compute a project's total artifact-directory size and most recent
/// modification time in a **single** tree walk.
///
/// `artifact_size` sums the sizes of every regular file that lives beneath one
/// of the project's [`artifact_dirs`](Project::artifact_dirs). `last_modified`
/// is the maximum `mtime` across *all* files in the project tree (not just
/// artifacts) — or `None` if no file's mtime could be read.
///
/// This replaces the older `analyze` path which walked the tree three times
/// (`Project::size` + `Project::last_modified`, plus the scan walk). The
/// artifact test is O(artifact_dirs) per file, which is small in practice.
fn project_size_and_mtime(project: &Project, options: &ScanOptions) -> (u64, Option<SystemTime>) {
    // Precompute the absolute artifact-directory prefixes once so the per-file
    // `starts_with` check doesn't re-join on every entry.
    let artifact_prefixes: Vec<path::PathBuf> = project
        .artifact_dirs()
        .into_iter()
        .map(|d| project.path.join(d))
        .collect();

    let mut total_artifact_size: u64 = 0;
    let mut latest_mtime: Option<SystemTime> = None;

    for entry in walkdir::WalkDir::new(&project.path)
        .follow_links(options.follow_symlinks)
        .same_file_system(options.same_file_system)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let metadata = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if !metadata.is_file() {
            continue;
        }

        // Artifact accounting: only count files beneath an artifact directory.
        if artifact_prefixes
            .iter()
            .any(|prefix| entry.path().starts_with(prefix))
        {
            total_artifact_size += file_size(&metadata, entry.path(), options.apparent);
        }

        // mtime accounting: every file, not just artifacts.
        if let Ok(modified) = metadata.modified() {
            latest_mtime = Some(latest_mtime.map_or(modified, |prev| prev.max(modified)));
        }
    }

    (total_artifact_size, latest_mtime)
}

/// Scan `path` for projects and compute each one's reclaimable size and last
/// modification time — a streaming convenience over [`scan`] +
/// [`project_size_and_mtime`].
///
/// Each project is yielded as soon as it is found and analyzed in a single
/// merged tree walk (size + mtime together, rather than the older separate
/// `Project::size` + `Project::last_modified` calls). Projects that error or
/// have zero reclaimable bytes are omitted, matching the kondo CLI's behavior.
/// For error-aware use, drive [`scan`] directly.
///
/// [`scan`]: scan
pub fn analyze<'a, P: AsRef<Path>>(
    path: &'a P,
    options: &'a ScanOptions,
) -> impl Iterator<Item = ProjectAnalysis> + use<'a, P> {
    scan(path, options).filter_map(|project| {
        let project = project.ok()?;
        let (artifact_size, last_modified) = project_size_and_mtime(&project, options);
        if artifact_size == 0 {
            return None;
        }
        Some(ProjectAnalysis {
            project,
            artifact_size,
            last_modified,
        })
    })
}

/// Recursively delete every artifact directory of the project at `project_path`.
///
/// Does nothing if `project_path` is not a recognized project. Convenience
/// wrapper around [`Project::clean`]; for finer control, use [`scan`] and call
/// [`Project::clean`] on the resulting [`Project`]s directly.
pub fn clean(project_path: &Path) -> Result<(), Box<dyn error::Error>> {
    let project_types = detect_project_types(project_path)?;
    if project_types.is_empty() {
        return Ok(());
    }
    let project = Project {
        project_types,
        path: project_path.to_path_buf(),
    };
    project.clean()?;

    Ok(())
}
#[cfg(test)]
mod tests {
    use super::{
        analyze, detect_project_types, dir_size, scan, Project, ProjectType, ScanOptions,
    };
    use std::fs;
    use std::path::{Path, PathBuf};

    /// Shared default options used across the scan/dir_size tests.
    fn opts() -> ScanOptions {
        ScanOptions {
            follow_symlinks: false,
            same_file_system: false,
            apparent: true,
        }
    }

    /// Create a non-hidden scratch directory under the system temp dir.
    ///
    /// `tempfile::tempdir()` itself names its directory `.tmpXXXX`, which
    /// `kondo` treats as hidden (name starts with `.`), so `scan` would skip
    /// it entirely. For scan/analyze tests we need a root whose own name does
    /// not start with a dot.
    fn fresh_root() -> (tempfile::TempDir, PathBuf) {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("kondo-test-root");
        fs::create_dir_all(&root).unwrap();
        (parent, root)
    }

    /// Write `contents` to `dir/<name>`, creating parent dirs as needed.
    fn touch<P: AsRef<Path>>(dir: P, name: &str, contents: &str) {
        let path = dir.as_ref().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    // ------------------------------------------------------------------
    // detect_project_types
    // ------------------------------------------------------------------

    #[test]
    fn detect_cargo_only() {
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "Cargo.toml", "");
        let types = detect_project_types(dir.path()).unwrap();
        assert_eq!(types, vec![ProjectType::Cargo]);
    }

    #[test]
    fn detect_mixed_cargo_node() {
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "Cargo.toml", "");
        touch(dir.path(), "package.json", "");
        let types = detect_project_types(dir.path()).unwrap();
        // Declaration order has Cargo before Node, and that survives sort.
        assert_eq!(types, vec![ProjectType::Cargo, ProjectType::Node]);
    }

    #[test]
    fn detect_node_and_turborepo() {
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "package.json", "");
        touch(dir.path(), "turbo.json", "");
        let types = detect_project_types(dir.path()).unwrap();
        // Node is declared before Turborepo, so sort leaves them in this order.
        assert_eq!(types, vec![ProjectType::Node, ProjectType::Turborepo]);
    }

    #[test]
    fn detect_csproj_with_godot_is_godot4() {
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "something.csproj", "");
        touch(dir.path(), "project.godot", "");
        let types = detect_project_types(dir.path()).unwrap();
        // A .csproj with a project.godot present classifies as Godot4.
        assert_eq!(types, vec![ProjectType::Godot4]);
    }

    #[test]
    fn detect_assembly_csharp_is_unity() {
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "Assembly-CSharp.csproj", "");
        let types = detect_project_types(dir.path()).unwrap();
        assert_eq!(types, vec![ProjectType::Unity]);
    }

    #[test]
    fn detect_plain_csproj_is_dotnet() {
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "app.csproj", "");
        let types = detect_project_types(dir.path()).unwrap();
        assert_eq!(types, vec![ProjectType::Dotnet]);
    }

    #[test]
    fn detect_empty_directory_is_empty_vec() {
        let dir = tempfile::tempdir().unwrap();
        let types = detect_project_types(dir.path()).unwrap();
        assert!(types.is_empty());
    }

    // ------------------------------------------------------------------
    // Project::artifact_dirs
    // ------------------------------------------------------------------

    fn project_with(types: Vec<ProjectType>, path: &Path) -> Project {
        Project {
            project_types: types,
            path: path.to_path_buf(),
        }
    }

    #[test]
    fn artifact_dirs_cargo_includes_target_and_xwin() {
        let dir = tempfile::tempdir().unwrap();
        let p = project_with(vec![ProjectType::Cargo], dir.path());
        let dirs = p.artifact_dirs();
        assert!(dirs.contains(&"target"));
        assert!(dirs.contains(&".xwin-cache"));
    }

    #[test]
    fn artifact_dirs_cargo_node_union() {
        let dir = tempfile::tempdir().unwrap();
        let p = project_with(vec![ProjectType::Cargo, ProjectType::Node], dir.path());
        let dirs = p.artifact_dirs();
        assert!(dirs.contains(&"target"));
        assert!(dirs.contains(&"node_modules"));
        // No duplicates of shared dirs.
        assert_eq!(dirs.iter().filter(|&&d| d == "target").count(), 1);
    }

    #[test]
    fn artifact_dirs_sbt_maven_target_deduped() {
        let dir = tempfile::tempdir().unwrap();
        let p = project_with(vec![ProjectType::SBT, ProjectType::Maven], dir.path());
        let dirs = p.artifact_dirs();
        // Both SBT and Maven use `target`; it must appear exactly once.
        assert_eq!(dirs.iter().filter(|&&d| d == "target").count(), 1);
    }

    // ------------------------------------------------------------------
    // scan
    // ------------------------------------------------------------------

    #[test]
    fn scan_finds_nested_subcrate_projects() {
        let (_keep, root) = fresh_root();
        // A workspace root with its shared target/ ...
        touch(&root, "Cargo.toml", "");
        touch(&root, "target/keep.o", "x");
        // ... and a sub-crate that has its OWN target/ (built independently).
        let sub = root.join("crates").join("sub");
        touch(&sub, "Cargo.toml", "");
        touch(&sub, "target/keep.o", "y");

        let projects: Vec<Project> = scan(&root, &opts()).filter_map(Result::ok).collect();
        // Both the root and the nested sub-crate are reported (the old code
        // skipped the whole root subtree and missed the sub-crate).
        assert_eq!(projects.len(), 2);
        assert!(projects.iter().any(|p| p.path == root));
        assert!(projects.iter().any(|p| p.path == sub));
    }

    #[test]
    fn scan_prunes_artifact_directories() {
        let (_keep, root) = fresh_root();
        // A Cargo.toml placed INSIDE a target/ dir must never be reported:
        // target/ is build output and pruned from the scan descent.
        touch(root.join("target"), "Cargo.toml", "");

        let projects: Vec<Project> = scan(&root, &opts()).filter_map(Result::ok).collect();
        assert!(projects.is_empty());
    }

    #[test]
    fn scan_skips_hidden_directories() {
        let (_keep, root) = fresh_root();
        // Root itself has no marker file, so we only check that the hidden
        // subtree is never descended into.
        let hidden = root.join(".hidden");
        fs::create_dir_all(&hidden).unwrap();
        touch(&hidden, "Cargo.toml", "");

        let projects: Vec<Project> = scan(&root, &opts()).filter_map(Result::ok).collect();
        assert!(projects.is_empty());
    }

    // ------------------------------------------------------------------
    // Project::clean
    // ------------------------------------------------------------------

    #[test]
    fn clean_removes_target() {
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "Cargo.toml", "");
        // Populate a target/ artifact directory with a real file.
        touch(dir.path(), "target/keep.o", "x");
        assert!(dir.path().join("target").exists());

        let p = project_with(vec![ProjectType::Cargo], dir.path());
        p.clean().unwrap();

        assert!(!dir.path().join("target").exists());
    }

    // ------------------------------------------------------------------
    // dir_size
    // ------------------------------------------------------------------

    #[test]
    fn dir_size_counts_file_contents() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();
        touch(path, "a.txt", "hello world");
        let size = dir_size(&path, &opts());
        assert!(size > 0);
    }

    // ------------------------------------------------------------------
    // analyze
    // ------------------------------------------------------------------

    #[test]
    fn analyze_skips_zero_artifact_projects() {
        let (_keep, root) = fresh_root();
        // Project A: has a non-empty target/ artifact directory.
        let a = root.join("a");
        fs::create_dir_all(&a).unwrap();
        touch(&a, "Cargo.toml", "");
        touch(&a, "target/build.o", "data");
        // Project B: only a marker, no artifact directory => 0 reclaimable.
        let b = root.join("b");
        fs::create_dir_all(&b).unwrap();
        touch(&b, "Cargo.toml", "");

        let analyses: Vec<_> = analyze(&root, &opts()).collect();
        // Only the project with reclaimable bytes should be reported.
        assert_eq!(analyses.len(), 1);
        assert!(analyses[0].artifact_size > 0);
        assert!(analyses[0].project.path.ends_with(a.file_name().unwrap()));
    }

    #[test]
    fn analyze_reports_size_and_mtime_for_populated_project() {
        let (_keep, root) = fresh_root();
        let a = root.join("a");
        fs::create_dir_all(&a).unwrap();
        touch(&a, "Cargo.toml", "");
        // A real artifact file whose size we can reason about.
        touch(&a, "target/build.o", "12345");

        let analyses: Vec<_> = analyze(&root, &opts()).collect();
        assert_eq!(analyses.len(), 1);
        let analysis = &analyses[0];
        // The single 5-byte artifact file must be counted.
        assert_eq!(analysis.artifact_size, 5);
        // mtime must be obtainable for a freshly written file.
        assert!(analysis.last_modified.is_some());
    }

    #[test]
    fn analyze_reports_nested_subcrate_separately() {
        let (_keep, root) = fresh_root();
        // Workspace root with a shared target/.
        touch(&root, "Cargo.toml", "");
        touch(&root, "target/root.o", "RRRR"); // 4 bytes
        // A sub-crate with its OWN independent target/.
        let sub = root.join("crates").join("sub");
        touch(&sub, "Cargo.toml", "");
        touch(&sub, "target/sub.o", "SSSSSS"); // 6 bytes

        let analyses: Vec<_> = analyze(&root, &opts()).collect();
        // Both are reported, each with its OWN artifact size — the root must
        // not absorb the sub-crate's target into its own total.
        assert_eq!(analyses.len(), 2);
        let size_of = |p: &Path| {
            analyses
                .iter()
                .find(|a| a.project.path == p)
                .map(|a| a.artifact_size)
        };
        assert_eq!(size_of(&root), Some(4));
        assert_eq!(size_of(&sub), Some(6));
    }
}
