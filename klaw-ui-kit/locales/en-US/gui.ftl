language = Language
menu-file = File
menu-view = View
menu-windows = Windows
menu-help = Help
menu-force-persist-layout = Force Persist Layout
menu-hide-window = Hide Window
menu-toggle-full-windows = Toggle Full Windows
menu-exit-full-windows = Exit Full Windows
menu-minimize = Minimize
menu-zoom = Zoom
menu-about = About

## Sidebar menu groups
menu-group-workspace = WORKSPACE
menu-group-ai-and-capability = AI & CAPABILITY
menu-group-runtime-and-access = RUNTIME & ACCESS
menu-group-automation-and-operations = AUTOMATION & OPERATIONS
menu-group-data-and-history = DATA & HISTORY
menu-group-observability = OBSERVABILITY

## Sidebar menu items
menu-profile = Profile Prompt
menu-system = System
menu-setting = Settings
menu-terminal = Terminal
menu-session = Session
menu-approval = Approval
menu-configuration = Configuration
menu-provider = Model Provider
menu-local-models = Model
menu-llm = LLM
menu-channel = Channel
menu-voice = Voice
menu-cron = Cron
menu-heartbeat = Heartbeat
menu-gateway = Gateway
menu-webhook = Webhook
menu-mcp = MCP
menu-acp = ACP
menu-skill-registry = Skills Registry
menu-skills-manager = Skills Manager
menu-memory = Memory
menu-knowledge = Knowledge
menu-archive = Archive
menu-tool = Tool
menu-monitor = Monitor
menu-logs = Logs
menu-analyze-dashboard = Analyze Dashboard
menu-observability = Observability

## Status bar
status-theme-mode = Theme Mode:
status-model-provider = Model Provider:
status-model-provider-na = N/A
status-default-model = Default Model: { $model }
status-update-available = { $icon } Update v{ $version }
status-update-hover =
    New version available: v{ $current } -> v{ $latest }
    { $name }
    Click to open the release page
status-hide-window = Hide Window
status-zoom-window = Zoom Window
status-minimize-window = Minimize Window

## About dialog
about-title = About Klaw
about-close = Close
about-version = Version { $version }
about-git-commit = Git Commit { $sha }

## Configuration panel
config-save = Save
config-validate = Validate
config-reset = Reset
config-migrate = Migrate
config-reload = Reload
config-unsaved = ● Unsaved
config-saved = ● Saved
config-find = Find
config-search-hint = Search TOML
config-search-type-to-search = Type to search
config-search-no-matches = 0 matches
config-prev = Prev
config-next = Next
config-subtitle = Edit the TOML configuration for the klaw runtime
config-path-hint = Config file: { $path }
config-path-not-loaded = Config file: (not loaded)
config-notify-loaded = Configuration loaded from disk
config-notify-load-failed = Failed to load config: { $error }
config-notify-store-unavailable = Configuration store is not available
config-notify-saved = Configuration saved
config-notify-save-failed = Save failed: { $error }
config-notify-valid = Configuration is valid
config-notify-validation-failed = Validation failed: { $error }
config-notify-reset = Configuration reset to defaults
config-notify-migrated = Configuration migrated with defaults
config-notify-operation-failed = Operation failed: { $error }
config-notify-reloaded = Configuration reloaded from disk
config-notify-reload-failed = Reload failed: { $error }
config-confirm-title = Unsaved changes
config-confirm-message = Current edits are not saved. Continue and overwrite editor content?
config-confirm-continue = Continue
config-confirm-cancel = Cancel

## Profile prompt panel
profile-subtitle = Manage workspace prompt documents and preview the assembled system prompt
profile-path-hint = Workspace: { $path }
profile-path-not-loaded = Workspace: (not loaded)
profile-markdown-files-count = Markdown Files: { $count }
profile-reload = Reload
profile-create-file = Create File
profile-workspace-markdown-files = Workspace Markdown Files
profile-no-markdown-files = No markdown files found in the workspace directory.
profile-name = Name
profile-summary = Summary
profile-size = Size
profile-modified = Modified
profile-path = Path
profile-preview = Preview
profile-edit = Edit
profile-reset = Reset
profile-delete = Delete
profile-system-prompt-preview = System Prompt Preview
profile-loading = Loading...
profile-system-prompt-desc = Rendered from current workspace prompt docs and installed skills.
profile-system-prompt-unavailable-title = System Prompt Preview Unavailable
profile-system-prompt-unavailable-body = Background preview task disconnected.
profile-edit-title = Edit { $name }
profile-preview-title = Preview { $name }
profile-path-label = Path: { $path }
profile-dirty-yes = Dirty: yes
profile-dirty-no = Dirty: no
profile-workspace-editor = Workspace markdown editor
profile-save = Save
profile-cancel = Cancel
profile-reset-btn = Reset
profile-default = Default
profile-reset-default-title = Reset to default template
profile-reset-default-editor-desc = Reset { $name } to the built-in default template? This will replace the current editor content.
profile-reset-default-file-desc = Reset { $name } to the built-in default template? This will overwrite { $path } after confirmation.
profile-reset-default-btn = Reset to default
profile-create-file-title = Create workspace file
profile-create-workspace-path = Workspace Path: { $path }
profile-create-file-hint = The file will be created directly under the workspace directory.
profile-file-name-label = File Name
profile-body-label = Body
profile-create-btn = Create
profile-delete-file-title = Delete workspace file
profile-delete-file-desc = Delete { $name }?
profile-delete-file-path = Path: { $path }
profile-notify-load-failed = Failed to load workspace markdown files: { $error }
profile-notify-preview-disconnected = System prompt preview loader disconnected
profile-notify-saved = Saved { $name }
profile-notify-save-failed = Failed to save { $path }: { $error }
profile-notify-reset-to-default = Reset { $name } to default template
profile-notify-reset-failed = Failed to reset { $path }: { $error }
profile-notify-created = Created { $path }
profile-notify-create-failed = { $error }
profile-notify-deleted = Deleted { $name }
profile-notify-delete-failed = Failed to delete { $path }: { $error }
profile-notify-workspace-unavailable = Workspace path is unavailable.

test-english-only = English only
