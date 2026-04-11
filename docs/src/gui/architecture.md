# Klaw GUI Architecture

## Overview

Klaw GUI is a desktop application built with [egui](https://www.egui.rs/), a immediate-mode GUI framework in Rust. It provides a visual interface for managing Klaw's configuration, sessions, skills, memory, and other subsystems.

## Design Philosophy

- **Immediate-mode GUI**: Leverages egui's immediate-mode paradigm for declarative UI
- **Tabbed workbench**: Multi-panel workspace with tab-based navigation
- **State persistence**: UI state (layout, theme, window size) persists across sessions
- **Decoupled panels**: Each feature panel is self-contained and independently rendered

## Module Structure

```
klaw-gui/
├── src/
│   ├── lib.rs              # Entry point, eframe integration
│   ├── app/
│   │   └── mod.rs          # Main application (KlawGuiApp)
│   ├── domain/
│   │   └── menu.rs         # Menu domain model
│   ├── ui/
│   │   ├── mod.rs
│   │   ├── shell.rs        # Main shell layout
│   │   ├── sidebar.rs      # Left navigation sidebar
│   │   └── workbench.rs    # Tabbed workbench area
│   ├── panels/
│   │   ├── mod.rs          # Panel registry & trait
│   │   ├── profile.rs      # Profile panel
│   │   ├── session.rs      # Session management
│   │   ├── provider.rs     # Model provider config
│   │   ├── approval.rs     # Approval workflow
│   │   ├── configuration.rs
│   │   ├── channel.rs
│   │   ├── cron.rs
│   │   ├── heartbeat.rs
│   │   ├── mcp.rs
│   │   ├── skill.rs
│   │   ├── skill_manage.rs
│   │   ├── memory.rs
│   │   ├── archive.rs
│   │   ├── tool.rs
│   │   └── system_monitor.rs
│   ├── state/
│   │   ├── mod.rs          # UiState, UiAction, ThemeMode
│   │   ├── workbench.rs    # WorkbenchState, TabId, WorkbenchTab
│   │   └── persistence.rs  # State serialization
│   ├── theme.rs            # Theme & font management
│   ├── notifications.rs    # Toast notification system
│   ├── widgets/
│   │   ├── mod.rs
│   │   └── placeholder.rs  # Placeholder widgets
│   └── runtime_bridge.rs   # Async runtime communication
```

## Core Components

### 1. Application Shell (`KlawGuiApp`)

The main application struct in `app/mod.rs`:

```rust
pub struct KlawGuiApp {
    state: UiState,
    shell: ShellUi,
    state_dirty: bool,
    last_state_save_at: Instant,
}
```

**Responsibilities:**
- Initialize egui/eframe context
- Handle UI actions from child components
- Manage state persistence with debouncing (500ms)
- Sync window size from viewport

### 2. Shell UI (`shell.rs`)

The shell defines the main window layout using egui's panel system:

```
┌─────────────────────────────────────────┐
│         Top Panel (Menu Bar)            │
├──────────┬──────────────────────────────┤
│          │                              │
│ Sidebar  │     Central Panel            │
│          │     (Workbench)              │
│          │                              │
├──────────┴──────────────────────────────┤
│       Bottom Panel (Status Bar)         │
└─────────────────────────────────────────┘
```

**Panel布局:**
- `TopBottomPanel::top("klaw-menu-bar")` - Menu bar (File, View, Windows, Help)
- `TopBottomPanel::bottom("klaw-status-bar")` - Status bar with theme toggle, provider info
- `SidePanel::left("klaw-sidebar")` - Navigation sidebar
- `CentralPanel::default()` - Main workbench area

### 3. State Management

#### UiState (`state/mod.rs`)

```rust
pub struct UiState {
    pub workbench: WorkbenchState,
    pub theme_mode: ThemeMode,
    pub fullscreen: bool,
    pub runtime_provider_override: Option<String>,
    pub window_size: Option<WindowSize>,
    pub show_about: bool,
}
```

#### UiAction - Unidirectional Data Flow

All state changes flow through `UiAction`:

```rust
pub enum UiAction {
    OpenMenu(WorkbenchMenu),
    ActivateTab(TabId),
    CloseTab(TabId),
    SetRuntimeProviderOverride(Option<String>),
    CloseWindow,
    ForcePersistLayout,
    ToggleFullscreen,
    MinimizeWindow,
    ZoomWindow,
    StartWindowDrag,
    ShowAbout,
    HideAbout,
    CycleTheme,
}
```

**Flow:**
```
Panel renders → User clicks → UiAction emitted → KlawGuiApp.handle_action() → State updated → Re-render
```

#### WorkbenchState (`state/workbench.rs`)

Manages tabs in the workbench:

```rust
pub struct WorkbenchState {
    pub tabs: Vec<WorkbenchTab>,
    pub active_tab: Option<TabId>,
}

pub struct WorkbenchTab {
    pub id: TabId,
    pub menu: WorkbenchMenu,
    pub title: String,
    pub closable: bool,
}
```

**Tab operations:**
- `open_or_activate()` - Opens new tab or focuses existing
- `activate()` - Switches to tab by ID
- `close()` - Closes tab, activates previous

### 4. Panel System

#### PanelRenderer Trait

```rust
pub trait PanelRenderer {
    fn render(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &RenderCtx<'_>,
        notifications: &mut NotificationCenter,
    );
}
```

#### PanelRegistry

Maps menu items to panel implementations:

```rust
pub struct PanelRegistry {
    profile: profile::ProfilePanel,
    session: session::SessionPanel,
    provider: provider::ProviderPanel,
    // ... other panels
}
```

**Available Panels:**
| Panel | Menu | Description |
|-------|------|-------------|
| Profile | Profile | User profile summary |
| Session | Session | Session list and management |
| Approval | Approval | Approval workflow status |
| Configuration | Configuration | General configuration |
| Provider | Model Provider | LLM provider management |
| Channel | Channel | Manage inbound channel adapters (WebSocket, Dingtalk, Telegram) |
| Cron | Cron | Scheduled jobs |
| Heartbeat | Heartbeat | Session heartbeat monitoring |
| MCP | MCP | Model Context Protocol |
| Skill | Skills Registry | Skills registry view |
| SkillManage | Skill | Skill installation/management |
| Memory | Memory | Memory system (BM25 + Vector) |
| Archive | Archive | File archive management |
| Tool | Tool | Tool system status |
| SystemMonitor | System Monitor | Resource monitoring |

### 5. Menu System (`domain/menu.rs`)

```rust
pub enum WorkbenchMenu {
    Profile,
    Session,
    Approval,
    Configuration,
    Provider,
    Channel,
    Cron,
    Heartbeat,
    Mcp,
    Skill,
    SkillManage,
    Memory,
    Archive,
    Tool,
    SystemMonitor,
}
```

Each menu item has:
- `id_key()` - Unique identifier for state keys
- `title()` - Display title
- `icon()` - Phosphor icon character
- `default_tab_title()` - Tab title when opened

### 6. Theme System (`theme.rs`)

```rust
pub enum ThemeMode {
    System,
    Light,
    Dark,
}
```

**Features:**
- Cycles: System → Light → Dark → System
- Applies egui theme preferences
- Installs custom font definitions with CJK fallbacks

**Font handling:**
- Adds Phosphor icons as font data
- Scans system fonts for CJK fallbacks (PingFang, Noto Sans CJK, etc.)

### 7. State Persistence (`state/persistence.rs`)

**Storage:** `~/.klaw/gui_state.json`

```rust
#[derive(Serialize, Deserialize)]
struct PersistedUiState {
    schema_version: u32,  // Version 1
    state: UiState,
}
```

**Features:**
- Schema versioning for migration support
- Atomic writes (write to .tmp, then rename)
- Debounced saves (500ms cooldown)
- Saves on close

### 8. Notifications (`notifications.rs`)

Toast notification system using `egui-notify`:

```rust
pub struct NotificationCenter {
    toasts: Toasts,
}

impl NotificationCenter {
    pub fn success(&mut self, message: impl Into<String>);
    pub fn info(&mut self, message: impl Into<String>);
    pub fn warning(&mut self, message: impl Into<String>);
    pub fn error(&mut self, message: impl Into<String>);
}
```

**Configuration:**
- Anchor: TopRight
- Margin: 16px

### 9. Runtime Bridge (`runtime_bridge.rs`)

Communication channel between GUI (main thread) and async runtime:

```rust
pub enum RuntimeCommand {
    ReloadSkillsPrompt,
}

static RUNTIME_COMMAND_SENDER: OnceLock<Mutex<Option<UnboundedSender<RuntimeCommand>>>>;
```

**Usage:**
```rust
// Install sender from async runtime
install_runtime_command_sender(sender);

// Send command from GUI
request_reload_skills_prompt()?;
```

## Panel Implementation Patterns

### Example: ProviderPanel

The Provider panel demonstrates common patterns:

1. **Lazy loading**: `ensure_store_loaded()` loads config on first render
2. **Form state**: Local `ProviderForm` struct for edit/add dialogs
3. **Modal dialogs**: `egui::Window` for forms
4. **Validation**: Form validation before save
5. **Notifications**: User feedback for success/error

```rust
pub struct ProviderPanel {
    store: Option<ConfigStore>,
    config_path: Option<PathBuf>,
    revision: Option<u64>,
    config: AppConfig,
    form: Option<ProviderForm>,
}
```

### Example: SessionPanel

Demonstrates async task execution:

```rust
fn run_session_task<T, F, Fut>(op: F) -> Result<T, String>
where
    F: FnOnce(Box<dyn SessionManager>) -> Fut + Send + 'static,
    Fut: Future<Output = Result<T, SessionError>> + Send + 'static,
{
    thread::spawn(move || {
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()?;
        runtime.block_on(async {
            let manager = SqliteSessionManager::open_default().await?;
            op(manager).await
        })
    })
}
```

## Window Management

### Viewport Commands

KlawGuiApp sends viewport commands for window operations:

```rust
// Close window
ctx.send_viewport_cmd(egui::ViewportCommand::Close);

// Fullscreen
ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));

// Minimize
ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));

// Maximize
ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));

// Start drag (for custom titlebar)
ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
```

### macOS Integration

Native app icon loading via Objective-C:

```rust
#[cfg(target_os = "macos")]
fn load_macos_app_icon() -> Option<egui::IconData> {
    // Load .icns from assets/icons/logo.icns
    // Set as NSApplication icon
}
```

## Key Design Decisions

### 1. Immediate-Mode vs Retained-Mode

**Decision**: Immediate-mode (egui)

**Rationale:**
- Simpler state management
- No manual repaint logic
- Built-in animation support
- Easy to compose panels

### 2. Centralized State

**Decision**: Single `UiState` struct with action-based updates

**Rationale:**
- Predictable state transitions
- Easy to persist/restore
- Testable (pure functions)

### 3. Panel Registry

**Decision**: Static registry with trait-based rendering

**Rationale:**
- Type-safe panel dispatch
- Easy to add new panels
- Consistent interface

### 4. Debounced Persistence

**Decision**: 500ms debounce + immediate on close

**Rationale:**
- Reduces disk writes during rapid changes
- Ensures state is saved on exit

## Testing

### Unit Tests

- `WorkbenchState` - Tab operations
- `ThemeMode` - Theme cycling
- `ProviderPanel` - Form validation
- `persistence` - State roundtrip

### Test Example

```rust
#[test]
fn open_menu_creates_and_activates_new_tab() {
    let mut state = WorkbenchState::new_with_default(WorkbenchMenu::Profile);
    state.apply(UiAction::OpenMenu(WorkbenchMenu::Provider));
    assert_eq!(state.tabs.len(), 2);
    assert_eq!(state.active_tab, Some(TabId::from_menu(WorkbenchMenu::Provider)));
}
```

## Dependencies

| Crate | Purpose |
|-------|---------|
| `eframe` | egui framework integration |
| `egui` | Immediate-mode GUI |
| `egui-phosphor` | Icon font |
| `egui_extras` | Extra widgets |
| `egui-notify` | Toast notifications |
| `fontdb` | System font discovery |
| `tokio` | Async runtime |
| `klaw-*` | Internal Klaw crates |

## Running the GUI

```bash
# From workspace root
cargo run -p klaw-gui

# Or with specific features
cargo run -p klaw-gui --release
```

## Future Directions

- Data binding for real-time session updates
- Drag-and-drop tab reordering
- Command palette (Ctrl+P)
- Keyboard shortcuts
- Custom widget library
