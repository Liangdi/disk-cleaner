# disk-cleaner

[English](README.en.md) | **中文**

一个快速、跨平台的终端磁盘占用分析与清理工具 —— 把 WinDirStat 风格的目录树浏览与 Kondo 风格的构建产物清理合并在一个 `disk` 命令里。

![Language](https://img.shields.io/badge/language-rust-orange)
![Platforms](https://img.shields.io/badge/platforms-Windows%20%7C%20macOS%20%7C%20Linux-blue)
![License](https://img.shields.io/github/license/Liangdi/disk-cleaner)

- **一个工具，两种视图。** 既可浏览按大小加权的实时目录树，也可切换到 *Projects*（项目）模式，从陈旧的构建产物中回收磁盘空间。
- **真实的磁盘占用。** 在 Unix 上统计已分配的块大小，在 Windows 上统计 NTFS 压缩后大小；加 `-a` 则显示逻辑文件长度。
- **并行扫描。** 使用 `rayon` 遍历目录树，面对巨型文件系统依然流畅。扫描还会遵守文件系统边界（不会跨挂载点）。
- **交互式 TUI。** 科幻 HUD 风格界面，包含目录树、详情面板、进度条、直方图、搜索与即时删除 —— 样式由可编辑的 CSS 主题驱动。
- **清理 23 种生态。** Projects 模式可识别 Cargo、Node、Unity、Maven、CMake、Unreal、Python 等等，并支持一键清理。

## 安装

### 从源码构建

```sh
git clone https://github.com/Liangdi/disk-cleaner
cd disk-cleaner
cargo build --release
# 可执行文件位于 target/release/disk
```

随后把它放到 `PATH` 中，例如：

```sh
sudo cp target/release/disk /usr/local/bin/
```

### 直接从 git 安装

```sh
cargo install --git https://github.com/Liangdi/disk-cleaner
```

## 用法

```sh
disk [选项] [路径]
```

省略 `路径` 时分析当前目录。

### 选项

| 参数             | 说明                                                  |
|------------------|-------------------------------------------------------|
| `-d <深度>`      | 目录树的最大递归深度（默认 `1`）                       |
| `-m <百分比>`    | 显示某条目所需的最小父目录占比，`0`–`100`（默认 `0.1`）|
| `-a`             | 显示表观（逻辑）大小，而非实际占用的磁盘空间            |
| `-j`             | 输出排序后的 JSON，而非目录树                          |
| `-t`, `--tui`    | 启动交互式 TUI                                        |

### 示例

```sh
disk                      # 当前目录，深度 1
disk -d 3                 # 更深的目录树
disk -a PATH              # 表观大小
disk -m 1 PATH            # 仅显示占父目录 ≥ 1% 的条目
disk -j PATH              # JSON 输出
disk --tui                # 在当前目录启动交互式 TUI
disk --tui /path/to/dir   # 在指定目录启动交互式 TUI
```

### CLI 输出

每一行都会打印按颜色编码的「占父目录百分比」与占用大小，目录名以 `/`
结尾。颜色代表量级 —— 根目录为绿色，体积大户为红色，其余为青色。

```
$ disk -d 2 ~/projects
  ╺━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━╸
  ┃
  ┃   ◈  D I S K  C L E A N E R
  ┃      disk usage analyzer and cleaner v0.1.0
  ┃
  ╺━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━╸

Analyzing: /home/you/projects

 82.34% [12.4 GB] ─── rust/
 ├ 11.82% [1.8 GB]  ─── node/
 ├  3.05% [480 MB]  ─── python/
 └  0.61% [96 MB]   ─── go/
```

![CLI 输出](screenshot/cli.png)

## TUI 模式

用 `--tui` 启动。TUI 把所有扫描都放到后台线程执行，并在扫描期间显示带
动画的加载界面，因此界面永远不会卡住。

### Disk 视图 —— 按大小加权的目录树

- **左侧面板** —— 可导航的目录树。任意节点都能展开/折叠，下钻时大小实时
  更新。
- **右侧面板** —— 所选项的详情统计：面包屑路径、占比进度条、快速计数、
  文件类型分布、文件大小直方图，以及最大的若干后代。

![Disk 视图](screenshot/tui.png)

### Projects 视图 —— 构建产物清理

用 `p` 或 `Tab` 切换到 Projects 模式。它扫描光标所在目录的子树，列出每个
检测到的构建项目及其可回收大小，按从大到小排序。右侧面板显示所属生态、
可回收进度条、按产物目录的细分（如 `target`、`node_modules`）以及项目
的年龄。

可清理单个选中项目，也可一次清理全部 —— 两者都由确认对话框把关。标题栏会
跟踪整个列表的累计可回收大小。

![Projects 视图](screenshot/tui-project.png)

### 快捷键

通用：

| 按键       | 动作                              |
|------------|-----------------------------------|
| `p` / `Tab`| 在 Disk 与 Projects 视图间切换     |
| `r`        | 重新扫描当前视图                  |
| `q` / `Ctrl-C` | 退出                          |

Disk 视图：

| 按键                     | 动作                          |
|--------------------------|-------------------------------|
| `j` / `↓`                | 下移                          |
| `k` / `↑`                | 上移                          |
| `Enter` / `l` / `→`      | 展开目录                      |
| `Backspace` / `h` / `←`  | 折叠目录                      |
| `Space`                  | 切换展开 / 折叠               |
| `d`                      | 进入所选目录（重新扫描）       |
| `u`                      | 返回父目录（重新扫描）         |
| `/`                      | 搜索 / 过滤                   |
| `a`                      | 切换表观大小 / 磁盘占用        |
| `.`                      | 切换隐藏条目的显示            |
| `x`                      | 删除所选条目                  |
| `g`                      | 跳到顶部                      |
| `G`                      | 跳到底部                      |

Projects 视图：

| 按键         | 动作                          |
|--------------|-------------------------------|
| `j` / `↓`    | 下移                          |
| `k` / `↑`    | 上移                          |
| `c` / `Enter`| 清理所选项目                  |
| `C`          | 清理列表中的**全部**项目      |
| `g`          | 跳到顶部                      |
| `G`          | 跳到底部                      |

确认对话框用 `y`/`Y` 确认，用 `n`/`N`/`Esc` 取消。

## 支持的项目生态

Projects 模式会识别下列构建系统及其产物目录：

| 生态                     | 生态                 | 生态            |
|--------------------------|----------------------|-----------------|
| Cargo（Rust）            | Node（含 React Native） | Unreal       |
| Unity                    | Stack / Cabal（Haskell） | Gradle      |
| SBT / Maven（JVM）       | CMake                | Jupyter         |
| Python                   | Pixi                 | Composer（PHP） |
| Pub（Dart/Flutter）      | Elixir               | Swift           |
| Zig                      | Godot 4.x            | .NET            |
| Turborepo                | Terraform            | CocoaPods       |

当一个目录同时匹配多种生态时，会全部上报 —— 例如同时含 `Cargo.toml` 与
`package.json` 的项目会被识别为 `Cargo / Node`，清理时回收 `target` 与
`node_modules` 的并集。扫描器在进入项目内部时会对产物目录剪枝，因此能找到
嵌套项目（如 Cargo workspace 的子 crate），而不会一头扎进构建产物里。

## 主题

交互式 UI 的样式来自 [src/tui/theme.css](src/tui/theme.css)，在编译期通过
`ratatui-style` 加载。直接修改其中的 CSS 变量与选择器即可为应用换肤 —— 无需
改动 Rust 代码。（加载界面是过程式动画，不走 CSS。）

## 许可证

MIT
