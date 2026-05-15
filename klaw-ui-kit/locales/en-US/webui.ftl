language = Language
menu-file = File
menu-window = Window
menu-connection = Connection
menu-help = Help
menu-settings = Settings
menu-tile-windows = Tile Windows
menu-reset-layout = Reset Layout
menu-gateway-token = Gateway Token
menu-reconnect = Reconnect
menu-connect = Connect
menu-disconnect = Disconnect
menu-about = About
status-connected = Connected
status-connecting = Connecting...
status-disconnected = Disconnected
status-error = Error

## Settings dialog
settings-title = Settings
settings-general = General Settings
settings-current-theme-mode = Current theme mode: {$mode}
settings-theme-mode = Theme Mode
settings-light-theme = Light Theme
settings-dark-theme = Dark Theme
settings-theme-default-hint = Default keeps the existing egui light/dark visuals.

## About dialog
about-title = About Klaw
about-version = Version {$version}
about-close = Close

## Archive preview
archive-preview-title = Resource Preview
archive-preview-close = Close
archive-preview-download = Download

## Session sidebar
session-list-heading = Agents
session-list-empty = No agents yet.
session-visible = Window visible
session-hidden = Window hidden
session-rename = Rename
session-copy-id = Copy ID
session-delete = Delete
session-id-copied = Agent ID copied

## Rename dialog
rename-title = Rename Agent
rename-hint = Agent name
rename-save = Save
rename-cancel = Cancel

## Gateway dialog
gateway-title = Gateway Token
gateway-hint = If gateway auth is enabled, enter the token here.
gateway-blank-hint = Leave it blank when auth is disabled.
gateway-token-hint = Gateway token
gateway-save-reconnect = Save & Reconnect
gateway-clear = Clear

## Delete dialog
delete-title = Delete Agent
delete-confirmation = Delete agent '{$agent_name}' permanently? This cannot be undone.
delete-confirm = Delete
delete-cancel = Cancel

## Composer / input area
composer-slash-hint = Type / to open command completion.
composer-connected-hint = Message Klaw…
composer-connecting-hint = Connecting to Klaw…
composer-disconnected-hint = Reconnect to message Klaw…
composer-error-hint = Fix the connection to keep chatting…
upload = Upload
upload-hover = Upload and attach files
file-count = File ({ $count })
file-count-hover = Show uploaded files
send = Send
model-hint = Model
provider-hint = Provider
selecting-file = Selecting file…
uploading = Uploading…
send-card-failed = Failed to send card action.

## Thinking placeholder
thinking = Thinking…
assistant-label = Klaw

## History loading states
history-loading-title = Loading conversation history…
history-loading-body = Fetching messages from Klaw gateway.
history-page-loading = Loading older messages…

## Workbench / connection guide
workbench-connect-heading = Connect to Klaw Gateway
workbench-connect-body = Connect successfully before loading agents.
workbench-connect-button = Connect
workbench-loading = Loading agents from Klaw gateway…
workbench-no-agents = No agents yet. Click New Agent to start.
workbench-heading = Agent Workspace
workbench-subheading = Each agent opens as its own egui window.

## Status bar
statusbar-theme-mode = Theme Mode
statusbar-agents = Agents: {$total}/{ $open }
statusbar-agents-hover = Total agent windows / currently open windows.
statusbar-stream = Stream
statusbar-stream-on-hover = On: stream replies live. Off: wait for a full reply and play fade-in.
statusbar-fps-hover = Approximate live frame rate from the latest egui frame delta.
statusbar-activity-hover = Current activity for the active agent.
statusbar-messages-hover = Messages currently loaded in the active agent window.
statusbar-no-active-agent = No active agent

## Empty state (connection states)
empty-connected-title = Start a conversation with Klaw
empty-connected-body = Send a message below to begin this chat.
empty-connecting-title = Connecting to Klaw
empty-connecting-body = Waiting for the chat room to come online.
empty-disconnected-title = Reconnect to Klaw
empty-disconnected-body = Reconnect from the toolbar, then send your next message.
empty-error-title = Connection error
empty-error-body = Klaw could not keep the chat connection alive: {$error}

## Card messages
card-approval-badge = Approval
card-question-badge = Question
card-approval-title = Approval Required
card-question-title = Question
card-command-label = Command
card-approval-id = Approval ID: {$id}
card-selected-answer = Selected: {$answer}

## File dialog
file-dialog-title = Uploaded Files
file-dialog-hint = Right-click a row to preview or remove it from this page.
file-dialog-empty = No uploaded files.
file-dialog-col-name = File Name
file-dialog-col-archive-id = Archive ID
file-dialog-col-size = Size

## Attachment context menu
attachment-preview = Preview
attachment-download = Download
attachment-delete = Delete

## Role labels
role-you = You
role-system = System

## Card interaction completed labels
card-approved = Approved
card-rejected = Rejected

## Archive hover texts
archive-hover-preview = Preview archive resource
archive-hover-download = Download archive resource

## Archive preview loading/unavailable
archive-preview-loading = Loading preview...
archive-preview-unavailable = Preview is not available for this file type.

## Session route labels
route-default = Route: default
route-provider = Route: {$provider}
route-model = Route: {$model}
route-provider-model = Route: {$provider}/{ $model }

## Session activity labels
activity-history = History
activity-uploading = Uploading
activity-picking-file = Picking File
activity-streaming = Streaming
activity-files-ready = Files Ready

## Status bar message count
statusbar-messages = {$count} msgs
