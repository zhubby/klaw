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

## Settings panel
setting-subtitle = Configure application preferences
setting-save-error = Save error: { $error }
setting-section-general = General
setting-section-security = Security & Privacy
setting-section-network = Network
setting-section-sync = Sync

setting-general-title = General Settings
setting-notify-language-updated = Language updated.
setting-notify-language-update-failed = Failed to update language: { $error }
setting-launch-at-startup = Launch at startup:
setting-launch-at-startup-hint = Automatically start Klaw when you log in to your computer.
setting-launch-at-startup-hint-unavailable = Automatically start Klaw when you log in to your computer.
{ $reason } You can still turn the setting off from here.
setting-yes = Yes
setting-no = No
setting-theme-mode-current = Current theme mode: { $mode } (change from the bottom status bar).
setting-light-theme = Light Theme:
setting-dark-theme = Dark Theme:
setting-theme-default-hint = Default keeps the existing egui light/dark visuals.
setting-notify-launch-enabled = Launch at startup enabled.
setting-notify-launch-disabled = Launch at startup disabled.
setting-notify-launch-update-failed = Failed to update launch at startup: { $error }
setting-notify-launch-save-failed = Failed to save launch at startup setting: { $error }
setting-notify-launch-save-and-rollback-failed = Failed to save launch at startup setting and rollback macOS login item: { $message }

setting-security-title = Security & Privacy Settings
setting-location-services = Location Services
setting-system-location-services = System location services:
setting-app-authorization = App authorization:
setting-detail = Detail:
setting-enabled = enabled
setting-disabled = disabled
setting-auth-not-determined = not determined
setting-auth-restricted = restricted
setting-auth-denied = denied
setting-auth-authorized-always = authorized always
setting-auth-authorized-when-in-use = authorized when in use
setting-auth-unsupported-platform = unsupported on this platform
setting-auth-unknown = unknown
setting-auth-detail-not-determined = Authorization has not been granted yet. Open system settings to review Location Services access.
setting-auth-detail-restricted = Location access is restricted by system policy or parental controls.
setting-auth-detail-denied = Location access is currently denied for this app context. Open system settings to allow it.
setting-auth-detail-auth-but-services-off = Authorization exists, but system-wide Location Services are currently disabled.
setting-auth-detail-unsupported-platform = Location Services privacy integration is currently implemented for macOS only.
setting-auth-detail-unknown = The system returned an unknown authorization state.
setting-open-location-settings = Open Location Settings
setting-notify-location-settings-opened = Opened macOS Location Services settings.
setting-notify-location-settings-failed = Failed to open macOS Location Services settings: { $error }
setting-danger-zone = Danger Zone
setting-delete-all-app-data = Delete All App Data
setting-delete-all-app-data-hint = Permanently remove the entire .klaw directory including configs, sessions, skills, memory, databases, and all other application data. This cannot be undone.
setting-delete-all-data-btn = Delete All Data
setting-confirm-delete-title = Confirm Delete All Data
setting-confirm-delete-warning = This will permanently delete all Klaw data!
setting-confirm-delete-description = The entire ~/.klaw directory will be removed, including:
setting-confirm-delete-item-config = Config and settings
setting-confirm-delete-item-sessions = Sessions and archives
setting-confirm-delete-item-skills = Skills and registry
setting-confirm-delete-item-memory = Memory and knowledge
setting-confirm-delete-item-databases = Databases and logs
setting-confirm-delete-irreversible = This action cannot be undone.
setting-cancel = Cancel
setting-delete-everything-btn = Delete Everything
setting-notify-delete-data-failed = Failed to delete data directory: { $error }
setting-notify-data-dir-unavailable = Cannot locate the data directory.

setting-network-title = Network Settings
setting-proxy-configuration = Proxy Configuration:
setting-proxy-no-proxy = No proxy
setting-proxy-system = Use system proxy
setting-proxy-manual = Manual proxy configuration
setting-http-proxy = HTTP Proxy
setting-https-proxy = HTTPS Proxy
setting-socks5-proxy = SOCKS5 Proxy
setting-proxy-host = Host:
setting-proxy-port = Port:

setting-sync-title = Sync Settings
setting-sync-enable-label = Enable manifest sync and S3 storage
setting-notify-sync-enabled = Manifest sync enabled.
setting-notify-sync-disabled = Manifest sync disabled.
setting-sync-general = General
setting-sync-provider = Provider:
setting-sync-provider-s3 = S3
setting-sync-mode = Mode:
setting-sync-mode-versioned = Versioned manifest
setting-sync-device-id = Device ID:
setting-sync-schedule-header = Schedule And Retention
setting-sync-auto-backup = Enable automatic backup
setting-sync-interval = Interval (minutes):
setting-sync-keep-latest = Keep latest manifests:
setting-sync-s3-header = S3 Configuration
setting-s3-endpoint = Endpoint:
setting-s3-region = Region:
setting-s3-bucket = Bucket:
setting-s3-prefix = Prefix:
setting-s3-access-key = Access Key:
setting-s3-secret-key = Secret Key:
setting-s3-session-token = Session Token:
setting-s3-access-key-env = Access Key Env:
setting-s3-secret-key-env = Secret Key Env:
setting-s3-session-token-env = Session Token Env:
setting-s3-force-path-style = Force path style
setting-sync-scope-header = Backup Scope
setting-sync-scope-restore-hint = Restore replays a selected manifest version. Temporary, logs, and observability data are excluded.
setting-sync-actions-header = Manifest Actions
setting-sync-remote-newer = Remote manifest { $id } from { $device } is newer than local.
setting-sync-remote-created = Remote created: { $time }
setting-sync-last-sync = Last sync: { $time }
setting-sync-last-manifest-id = Last manifest ID: { $id }
setting-sync-in-progress = In progress: { $label }
setting-sync-run-now = Run Sync Now
setting-sync-refresh-remote = Refresh Remote Manifests
setting-sync-run-cleanup = Run Retention Cleanup
setting-sync-manual-progress-hint = Manual sync progress is shown below while reconciliation, blob upload, and manifest publish are running.
setting-sync-remote-header = Remote Manifests
setting-sync-no-remote = No remote manifests loaded.
setting-sync-manifest-id = Manifest: { $id }
setting-sync-created = Created: { $time }
setting-sync-device = Device: { $device }
setting-sync-restore-btn = Restore
setting-sync-confirm-restore-title = Confirm Restore
setting-sync-confirm-restore-desc1 = Restore replaces the current local manifest-managed data.
setting-sync-confirm-restore-desc2 = Restore replays the selected manifest version.
setting-sync-confirm-restore-desc3 = Restart Klaw after restore completes.
setting-sync-restore-now-btn = Restore Now
setting-notify-restore-started = Restore started.
setting-notify-sync-backup-done = Manifest { $id } uploaded to S3.
setting-notify-sync-list-done = Remote manifests refreshed.
setting-notify-sync-restore-done = Manifest { $id } restored. Restart Klaw before continuing.
setting-notify-sync-cleanup-done = Remote manifest retention cleanup completed.
setting-sync-task-label-backup = Uploading manifest sync
setting-sync-task-label-refresh = Loading manifests
setting-sync-task-label-restore = Restoring manifest
setting-sync-task-label-cleanup = Cleaning up remote manifests

setting-sync-stage-reconciling = Reconciling remote manifest
setting-sync-stage-preparing = Preparing manifest
setting-sync-stage-uploading-blobs = Uploading blobs
setting-sync-stage-uploading-manifest = Uploading manifest
setting-sync-stage-updating-pointer = Updating latest manifest pointer
setting-sync-stage-cleaning-up = Cleaning up old manifests
setting-sync-stage-completed = Sync completed
setting-sync-stage-connecting = Connecting to remote storage
setting-sync-stage-validating = Validating sync configuration

setting-sync-item-session = Session
setting-sync-item-skills = Skills
setting-sync-item-mcp-excluded = MCP (excluded)
setting-sync-item-skills-registry = Skills Registry
setting-sync-item-gui-settings = GUI Settings
setting-sync-item-archive = Archive
setting-sync-item-user-workspace = User Workspace
setting-sync-item-memory = Memory
setting-sync-item-config = Config

## LLM panel
llm-error-config-load = Failed to load config: { $error }
llm-error-config-reload = Failed to reload config: { $error }
llm-error-rows-load = Failed to load LLM audit rows: { $error }
llm-error-loader-disconnected = LLM audit loader closed unexpectedly
llm-error-detail-load = Failed to load LLM audit detail: { $error }
llm-error-detail-loader-disconnected = LLM audit detail loader closed unexpectedly

llm-btn-refresh = Refresh
llm-label-total = Total: { $count }
llm-status-loading = Loading...
llm-status-loading-rows = Loading LLM audit rows...
llm-status-no-rows = No LLM audit rows found.

llm-filter-session = Session
llm-filter-provider = Provider
llm-filter-all = All
llm-filter-start-date = Start Date
llm-filter-end-date = End Date
llm-label-page = Page
llm-label-size = Size

llm-col-session = Session
llm-col-provider = Provider
llm-col-model = Model
llm-col-wire-api = Wire API
llm-col-turn = Turn
llm-col-seq = Seq
llm-col-status = Status

llm-ctx-view-details = { $icon } View Details
llm-ctx-copy-session-key = { $icon } Copy Session Key
llm-ctx-copy-request-id = { $icon } Copy Request ID

llm-title-detail = LLM Audit Detail
llm-detail-session = Session: { $session }
llm-detail-time = Time: { $time }
llm-detail-provider = Provider: { $provider }
llm-detail-model = Model: { $model }
llm-detail-wire-api = Wire API: { $wire_api }
llm-detail-status = { $icon } { $text }
llm-detail-error-code = Error Code: { $error_code }
llm-detail-error-message = Error Message: { $error_message }

llm-tab-request = Request
llm-tab-response = Response
llm-detail-loading-request = Loading request payload...
llm-detail-loading-response = Loading response payload...
llm-detail-empty-response = empty

llm-sort-time-asc = Time ↑
llm-sort-time-desc = Time ↓

llm-status-success = success
llm-status-failed = failed

## MCP panel
mcp-notify-config-loaded = MCP config loaded from disk
mcp-notify-load-config-failed = Failed to load config: { $error }
mcp-notify-store-unavailable = Configuration store is not available
mcp-notify-save-failed = Save failed: { $error }
mcp-notify-server-saved = MCP server saved
mcp-notify-server-deleted = MCP server '{ $id }' deleted
mcp-notify-config-reloaded = Configuration reloaded from disk
mcp-notify-reload-failed = Reload failed: { $error }
mcp-notify-status-refreshed = MCP status refreshed
mcp-notify-status-refresh-failed = Failed to refresh MCP status: { $error }
mcp-notify-status-refresh-disconnected = Failed to refresh MCP status: background task disconnected
mcp-notify-sync-success = MCP runtime synchronized
mcp-notify-sync-failed = Failed to sync MCP runtime: { $error }
mcp-notify-sync-disconnected = Failed to sync MCP runtime: background task disconnected
mcp-notify-server-restarted = Restarted MCP server { $target }
mcp-notify-restart-failed = Failed to restart { $target }: { $error }
mcp-notify-restart-already-in-progress = An MCP server restart is already in progress
mcp-notify-settings-saved = MCP settings saved

mcp-label-servers-count = Servers: { $count }
mcp-status-applying-changes = Applying MCP changes...
mcp-status-refreshing = Refreshing runtime status...
mcp-status-restarting = Restarting MCP server...
mcp-label-no-servers = No MCP servers configured.

mcp-col-id = ID
mcp-col-on = On
mcp-col-status = Status
mcp-col-mode = Mode
mcp-col-command-url = Command/URL
mcp-col-args = Args
mcp-col-tools = Tools

mcp-mode-stdio = stdio
mcp-mode-sse = sse
mcp-label-enabled-yes = yes
mcp-label-enabled-no = no

mcp-form-title-edit = Edit MCP Server
mcp-form-title-add = Add MCP Server
mcp-form-id = ID
mcp-form-enabled = Enabled
mcp-form-mode = Mode
mcp-form-tool-timeout-seconds = Tool Timeout Seconds
mcp-form-command = Command
mcp-form-cwd = CWD
mcp-form-url = URL
mcp-form-args = Args
mcp-form-env = Env
mcp-form-headers = Headers
mcp-btn-save = Save
mcp-btn-cancel = Cancel

mcp-error-server-id-empty = MCP server ID cannot be empty
mcp-error-server-id-duplicate = MCP server ID '{ $id }' already exists, choose another ID
mcp-error-tool-timeout-invalid = tool_timeout_seconds must be a positive integer
mcp-error-startup-timeout-invalid = startup_timeout_seconds must be a positive integer

mcp-btn-config = { $icon } Config
mcp-btn-add = Add
mcp-btn-reload = Reload
mcp-btn-refresh-status = { $icon } Refresh Status
mcp-btn-detail = { $icon } Detail
mcp-btn-edit = { $icon } Edit
mcp-btn-restart = { $icon } Restart
mcp-btn-delete = { $icon } Delete

mcp-window-global-settings = MCP Settings
mcp-form-startup-timeout-seconds = startup_timeout_seconds:

mcp-window-detail-title = MCP Detail: { $server_id }

mcp-detail-heading = MCP Server Detail
mcp-detail-server = Server: { $server_id }
mcp-detail-state = State: { $state }
mcp-detail-tools = Tools: { $tool_count }
mcp-detail-last-error = Last Error: { $last_error }
mcp-detail-tools-list-heading = tools/list response
mcp-detail-tools-list-null = null
mcp-detail-json-render-error = { "error" : "failed to render json: { $error }" }

mcp-state-starting = starting
mcp-state-running = running
mcp-state-stopped = stopped
mcp-state-failed = failed

mcp-placeholder-none = -

## ACP panel
acp-panel-description = ACP lets klaw call external ACP-compatible coding agents through adapter commands.
acp-panel-default-templates-hint = Default templates use `npx -y @zed-industries/claude-agent-acp` and `npx -y @zed-industries/codex-acp`; runtime cwd comes from `working_directory`.

acp-notify-config-loaded = ACP config loaded from disk
acp-notify-load-config-failed = Failed to load config: { $error }
acp-notify-store-unavailable = Configuration store is not available
acp-notify-save-failed = Save failed: { $error }
acp-notify-config-reloaded = Configuration reloaded from disk
acp-notify-reload-failed = Reload failed: { $error }
acp-notify-status-refreshed = ACP status refreshed
acp-notify-status-refresh-failed = Failed to refresh ACP status: { $error }
acp-notify-status-refresh-disconnected = Failed to refresh ACP status: background task disconnected
acp-notify-sync-success = ACP runtime synchronized
acp-notify-sync-failed = Failed to sync ACP runtime: { $error }
acp-notify-sync-disconnected = Failed to sync ACP runtime: background task disconnected
acp-notify-server-restarted = Restarted ACP agent { $target }
acp-notify-restart-failed = Failed to restart { $target }: { $error }
acp-notify-server-deleted = ACP agent '{ $id }' deleted
acp-notify-server-saved = ACP agent saved
acp-notify-agent-started = ACP agent { $agent_id } session started
acp-notify-agent-start-failed = Failed to start ACP agent { $agent_id }: { $error }
acp-notify-agent-stopped = ACP agent session stopped
acp-notify-agent-stop-failed = Failed to stop ACP agent: { $error }
acp-notify-agent-stopped-with-error = ACP agent session stopped with error: { $error }
acp-notify-agent-stop-disconnected = Failed to stop ACP agent: background task disconnected
acp-notify-permission-resolved = Permission response sent for request { $request_id }
acp-notify-permission-resolve-failed = Failed to send permission response for request { $request_id }: { $error }
acp-notify-prompt-opened = ACP test prompt opened
acp-notify-prompt-failed = Failed to open test prompt: { $error }
acp-notify-settings-saved = ACP settings saved
acp-notify-restart-already-in-progress = An ACP agent restart is already in progress

acp-stats-enabled = Enabled
acp-stats-running = Running
acp-stats-failed = Failed
acp-stats-tools = Tools

acp-col-id = ID
acp-col-on = On
acp-col-status = Status
acp-col-command = Command
acp-col-tools = Tools

acp-enabled-status-yes = yes
acp-enabled-status-no = no

acp-value-not-set = (not set)
acp-value-unknown = (unknown)
acp-value-none = (none)

acp-button-config = { $icon } Config
acp-button-add-agent = Add Agent
acp-button-reload = Reload
acp-button-sync-runtime = { $icon } Sync Runtime
acp-button-refresh-status = { $icon } Refresh Status
acp-button-test = { $icon } Test

acp-form-title-edit = Edit ACP Agent
acp-form-title-add = Add ACP Agent
acp-form-config-persisted-info = ACP agent configuration is persisted to config.toml.
acp-form-label-id = ID
acp-form-label-enabled = Enabled
acp-form-label-command = Command
acp-form-working-directory-info = Runtime working directory comes from the tool/test prompt `working_directory` input.
acp-form-label-description = Description
acp-form-button-save = Save
acp-form-button-cancel = Cancel

acp-settings-window-title = ACP Settings
acp-settings-description = ACP calls external ACP-compatible coding agents over stdio.
acp-settings-startup-timeout-label = startup_timeout_seconds:
acp-settings-button-save = Save
acp-settings-button-cancel = Cancel
acp-settings-startup-timeout-invalid = startup_timeout_seconds must be a positive integer

acp-delete-dialog-title = Delete ACP Agent
acp-delete-dialog-message = Are you sure you want to delete ACP agent '{ $agent_id }'?
acp-delete-dialog-info = This removes the ACP agent from config.toml.
acp-delete-dialog-button-delete = { $icon } Delete
acp-delete-dialog-button-cancel = Cancel

acp-detail-window-title = ACP Detail: { $agent_id }
acp-detail-label-id = ID
acp-detail-label-enabled = Enabled
acp-detail-label-tool-name = Tool Name
acp-detail-label-command = Command
acp-detail-label-env-vars = Env Vars
acp-detail-label-description = Description
acp-detail-label-last-error = Last Error
acp-detail-latest-prompt-snapshot = Latest Prompt Snapshot
acp-detail-snapshot-mode = mode: { $mode }
acp-detail-snapshot-title = title: { $title }
acp-detail-snapshot-updated-at = updated_at: { $updated_at }
acp-detail-snapshot-available-commands = available commands: { $commands }
acp-detail-snapshot-config-options = config options: { $options }

acp-test-prompt-title = ACP Test Prompt
acp-test-prompt-working-directory-info = working_directory: { $working_directory }
acp-test-prompt-input-hint = Type a message and press Enter to send to the ACP agent.
acp-test-prompt-input-placeholder = Type a message...
acp-test-prompt-stop-button = { $icon } Stop
acp-test-prompt-output-section = Output
acp-test-prompt-last-error = Last Error
acp-test-prompt-session-snapshot = Session Snapshot
acp-test-prompt-snapshot-title = Title
acp-test-prompt-snapshot-mode = Mode
acp-test-prompt-snapshot-updated-at = Updated At
acp-test-prompt-snapshot-commands = Commands
acp-test-prompt-config-options = Config Options
acp-test-prompt-pending-permissions = Pending Permissions
acp-test-prompt-permission-timeline = Permission Timeline
acp-test-prompt-structured-events = Structured Events
acp-test-prompt-raw-stream = Raw Stream
acp-waiting-for-session-updates = Waiting for ACP session updates...

acp-permission-label = #{ $request_id } { $title }
acp-permission-sending-response = sending response...
acp-permission-tool-kind = tool kind: { $kind }
acp-permission-tool-status = tool status: { $status }
acp-permission-raw-input = raw input: { $raw_input }
acp-permission-option-button = { $label } ({ $kind })
acp-permission-cancel = Cancel

acp-content-block-image-with-uri = [image { $mime_type } { $data_len } bytes { $uri }]
acp-content-block-image = [image { $mime_type } { $data_len } bytes]
acp-content-block-audio = [audio { $mime_type } { $data_len } bytes]
acp-content-block-resource-with-title = [resource { $name } { $title } { $uri }]
acp-content-block-resource = [resource { $name } { $uri }]
acp-content-block-embedded-text-with-mime = [embedded text { $uri } { $mime_type }] { $text }
acp-content-block-embedded-text = [embedded text { $uri }] { $text }
acp-content-block-embedded-blob-with-mime = [embedded blob { $uri } { $mime_type } { $byte_len } bytes]
acp-content-block-embedded-blob = [embedded blob { $uri } { $byte_len } bytes]
acp-content-block-unsupported = [unsupported content { $description }]

## System panel
system-view-host-information = Host Information
system-view-program-disk-usage = Program Disk Usage
system-view-environment = Environment

system-dir-tmp = Temporary
system-dir-workspace = Workspace
system-dir-sessions = Sessions
system-dir-archives = Archives
system-dir-logs = Logs
system-dir-skills = Skills
system-dir-skills-registry = Skills Registry
system-dir-models = Models

system-cpu-usage = CPU Usage
system-memory-usage = Memory Usage
system-system-information = System Information

system-host-app-uptime = App Uptime
system-host-name = Host Name
system-host-os-name = OS Name
system-host-os-version = OS Version
system-host-long-os-version = Long OS Version
system-host-kernel-version = Kernel Version
system-host-cpu-architecture = CPU Architecture
system-host-logical-cpu-count = Logical CPU Count
system-host-physical-core-count = Physical Core Count
system-host-primary-cpu-brand = Primary CPU Brand
system-host-primary-cpu-frequency = Primary CPU Frequency
system-host-total-memory = Total Memory
system-host-used-memory = Used Memory
system-host-free-memory = Free Memory
system-host-total-swap = Total Swap
system-host-used-swap = Used Swap
system-host-system-uptime = System Uptime
system-host-system-boot-time = System Boot Time
system-host-load-average = Load Average
system-host-data-directory = Data Directory

system-host-data-dir-size = Data Directory Size
system-host-data-dir-file-count = Data Directory File Count
system-host-data-dir-mount-point = Data Directory Mount Point
system-host-data-dir-disk-capacity = Data Directory Disk Capacity
system-host-data-dir-disk-available = Data Directory Disk Available

system-cpu-cores-info = { $logical } logical / { $physical } physical cores
system-memory-free = Free: { $free }
system-cpu-frequency-mhz = { $freq } MHz
system-host-na = N/A
system-host-loading = Loading...

system-disk-usage-description = Inspect and clear data under the Klaw data directory.
system-dir-path = Path: { $path }
system-dir-path-unavailable = Path unavailable.
system-dir-clearing-hint = Clearing removes files inside `{ $dir }`; the directory itself is kept.

system-usage-calculating = Calculating...
system-usage = Usage: { $usage }
system-usage-unavailable-error = Usage: unavailable ({ $error })
system-usage-unavailable = Usage: unavailable

system-refresh = Refresh
system-open-dir-hint = Open { $title } directory in Finder
system-clear-dir-hint = Clear { $title } directory
system-clear = Clear
system-cancel = Cancel

system-confirm-clear-title = Clear { $title } directory
system-confirm-clear-message = Are you sure you want to clear the { $title } directory?

system-env-dependencies = Environment Dependencies
system-env-loading = Loading...
system-env-not-found = not found
system-env-required = Required
system-env-preferred = Preferred
system-env-optional = Optional
system-env-project = Project:
system-env-all-available = All dependencies available
system-env-tm-missing = Note: Terminal multiplexer (zellij/tmux) not available
system-env-preferred-missing = Note: Some preferred dependencies are missing
system-env-required-missing = Warning: Some required dependencies are missing

system-notify-failed-collect-usage = Failed to collect { $title } usage: { $error }
system-notify-dir-cleared = { $title } directory cleared
system-notify-failed-clear-dir = Failed to clear { $title } directory: { $error }
system-notify-failed-resolve = Failed to resolve data directories: { $error }
system-notify-failed-open-dir = Failed to open { $title } directory: { $error }

test-english-only = English only
