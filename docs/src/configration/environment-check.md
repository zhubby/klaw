# 环境依赖检查

Klaw 在启动时会自动检查外部二进制依赖的可用性，并在日志中输出检查结果。检查结果同时保存在运行时状态中，可在 GUI 的 System 面板中查看。

## 检查的依赖项

| 依赖 | 描述 | 必需性 | 用途 |
|------|------|--------|------|
| `git` | Skills registry 同步 | 必需 | 用于从远程仓库同步 Skills |
| `rg` (ripgrep) | 本地文件内容搜索（首选后端） | 首选 | `local_search` 的首选后端；缺失时会降级到 `grep` fallback |
| `tailscale` | Gateway 公网暴露（serve/funnel） | 可选 | 用于本机 gateway 的公网暴露能力 |
| `zellij` | 终端复用器 (首选) | 可选 | `terminal_multiplexer` 工具的首选后端 |
| `tmux` | 终端复用器 (备选) | 可选 | `terminal_multiplexer` 工具的备选后端 |
| `docker` | 容器 CLI 与镜像工具链 | 可选 | 用于发现本机 Docker CLI 是否可用 |
| `container` | Apple container CLI | 可选 | 用于发现 macOS 原生 Apple container 工具是否可用 |

### 依赖说明

- **必需依赖**: 如果缺失，会在日志中输出 `WARN` 级别警告，相关功能将不可用
- **首选依赖**: 缺失不会阻塞启动，但会提示仍在使用降级路径
- **可选依赖**: `zellij` 和 `tmux` 是互斥备选关系，只需其中一个可用即可；`tailscale`、`docker`、`container` 仅做可用性报告

## 启动日志示例

```
INFO klaw::runtime::env_check: Checking environment dependencies...
INFO klaw::runtime::env_check: git: available (2.43.0)
INFO klaw::runtime::env_check: rg: available (14.1.0)
INFO klaw::runtime::env_check: zellij: available (0.40.0)
INFO klaw::runtime::env_check: docker: available (28.0.1)
INFO klaw::runtime::env_check: container: not found (optional)
INFO klaw::runtime::env_check: Environment check completed: all dependencies available
```

如果缺少必需依赖:

```
WARN klaw::runtime::env_check: git: NOT FOUND (required)
WARN klaw::runtime::env_check: Environment check completed: some required dependencies missing
```

## 安装依赖

### macOS

```bash
# 使用 Homebrew 安装
brew install git
brew install ripgrep
brew install zellij
brew install docker
# Apple container CLI 需按 Apple 官方安装说明单独安装
# 或
brew install tmux
```

### Linux

```bash
# Debian/Ubuntu
sudo apt install git ripgrep
sudo apt install zellij
sudo apt install docker.io
# 或
sudo apt install tmux

# Arch Linux
sudo pacman -S git ripgrep zellij
sudo pacman -S docker
# 或
sudo pacman -S git ripgrep tmux

# Fedora
sudo dnf install git ripgrep zellij
sudo dnf install docker
# 或
sudo dnf install git ripgrep tmux
```

### Windows

```powershell
# 使用 winget 安装
winget install Git.Git
winget install BurntSushi.ripgrep.MSVC

# zellij 和 tmux 在 Windows 上不可用
# 可使用 Windows Terminal 的内置分屏功能
```

## GUI 查看

在 GUI 中，打开 **System** 面板，顶部会显示 **Environment Dependencies** 区域：

```
Environment Dependencies
────────────────────────────
✓ git      2.43.0    Required
  Skills registry synchronization

✓ rg       14.1.0    Preferred
  Local file content search (ripgrep)

✓ zellij   0.40.0    Optional
  Terminal multiplexer (preferred)

✓ docker   28.0.1    Optional
  Container CLI and image tooling

✗ tmux     not found Optional
  Terminal multiplexer (fallback)

✗ container not found Optional
  Apple container CLI for macOS-native containers

All dependencies available
```

### 状态说明

| 图标 | 含义 |
|------|------|
| ✓ (绿色) | 依赖可用，显示版本号 |
| ✗ (红色) | 必需依赖缺失 |
| ✗ (黄色) | 可选依赖缺失 |

## 程序化访问

### 通过 RuntimeBridge (GUI)

```rust
use klaw_gui::request_env_check;

let report = request_env_check()?;
for check in &report.checks {
    println!("{}: available={}", check.name, check.available);
}

// 检查所有必需依赖是否可用
if report.all_required_available() {
    println!("All required dependencies are available");
}

// 检查终端复用器是否可用
if report.terminal_multiplexer_available() {
    println!("Terminal multiplexer is available");
}
```

### 从 RuntimeBundle (CLI)

```rust
let env_check = runtime.env_check.clone();
for status in &env_check.checks {
    tracing::info!(
        name = %status.name,
        available = status.available,
        version = ?status.version,
        "dependency status"
    );
}
```

## 数据结构

```rust
pub struct EnvironmentCheckReport {
    pub checks: Vec<DependencyStatus>,
    pub checked_at: time::OffsetDateTime,
}

pub struct DependencyStatus {
    pub name: String,
    pub description: String,
    pub available: bool,
    pub version: Option<String>,
    pub required: bool,
    pub category: DependencyCategory,
}

pub enum DependencyCategory {
    Required,
    Preferred,
    OptionalWithFallback,
}
```

## 故障排查

### git 未找到

```
WARN klaw::runtime::env_check: git: NOT FOUND (required)
```

**解决**: 安装 git 并确保在 PATH 中:
```bash
# 检查 git 是否可用
which git
git --version
```

### ripgrep 未找到

```
WARN klaw::runtime::env_check: rg: not found (preferred)
```

`local_search` 仍可使用系统 `grep` 继续工作，但会失去 `rg` 的首选执行路径，环境检查也会继续提示缺少首选依赖。

**解决**: 安装 ripgrep:
```bash
# 检查 rg 是否可用
which rg
rg --version
```

### 终端复用器不可用

```
INFO klaw::runtime::env_check: Note: Terminal multiplexer (zellij/tmux) not available
```

**解决**: 安装 zellij 或 tmux 其中之一:
```bash
# 安装 zellij (推荐)
brew install zellij  # macOS
# 或安装 tmux
brew install tmux    # macOS
```

### 容器 CLI 不可用

```
INFO klaw::runtime::env_check: docker: not found (optional)
INFO klaw::runtime::env_check: container: not found (optional)
```

**说明**: 这两项仅作可选环境报告，不会阻止 runtime 或 GUI 启动。

## 相关文档

- [工具配置](../tools/README.md)
- [Skills Registry](../tools/advanced/skills.md)
- [本地搜索](../tools/built-in/local_search.md)
