language = 语言
menu-file = 文件
menu-view = 视图
menu-windows = 窗口
menu-help = 帮助
menu-force-persist-layout = 强制保存布局
menu-hide-window = 隐藏窗口
menu-toggle-full-windows = 切换全窗口
menu-exit-full-windows = 退出全窗口
menu-minimize = 最小化
menu-zoom = 缩放
menu-about = 关于

## 侧边栏菜单分组
menu-group-workspace = 工作区
menu-group-ai-and-capability = AI 与能力
menu-group-runtime-and-access = 运行与接入
menu-group-automation-and-operations = 自动化与运维
menu-group-data-and-history = 数据与历史
menu-group-observability = 可观测性

## 侧边栏菜单项
menu-profile = 角色提示词
menu-system = 系统
menu-setting = 设置
menu-terminal = 终端
menu-session = 会话
menu-approval = 审批
menu-configuration = 配置
menu-provider = 模型提供商
menu-local-models = 模型
menu-llm = LLM
menu-channel = 通道
menu-voice = 语音
menu-cron = 定时任务
menu-heartbeat = 心跳
menu-gateway = 网关
menu-webhook = Webhook
menu-mcp = MCP
menu-acp = ACP
menu-skill-registry = 技能仓库
menu-skills-manager = 技能管理
menu-memory = 记忆
menu-knowledge = 知识库
menu-archive = 归档
menu-tool = 工具
menu-monitor = 监控
menu-logs = 日志
menu-analyze-dashboard = 分析仪表盘
menu-observability = 可观测性

## 底部状态栏
status-theme-mode = 主题模式：
status-model-provider = 模型提供商：
status-model-provider-na = 无
status-default-model = 默认模型：{ $model }
status-update-available = { $icon } 更新 v{ $version }
status-update-hover =
    发现新版本：v{ $current } -> v{ $latest }
    { $name }
    点击打开 Release 页面
status-hide-window = 隐藏窗口
status-zoom-window = 缩放窗口
status-minimize-window = 最小化窗口

## 关于对话框
about-title = 关于 Klaw
about-close = 关闭
about-version = 版本 { $version }
about-git-commit = Git Commit { $sha }

## 配置面板
config-save = 保存
config-validate = 验证
config-reset = 重置
config-migrate = 迁移
config-reload = 重载
config-unsaved = ● 未保存
config-saved = ● 已保存
config-find = 查找
config-search-hint = 搜索 TOML
config-search-type-to-search = 输入以搜索
config-search-no-matches = 0 个匹配
config-prev = 上一个
config-next = 下一个
config-subtitle = 编辑 klaw 运行时的 TOML 配置
config-path-hint = 配置文件：{ $path }
config-path-not-loaded = 配置文件：（未加载）
config-notify-loaded = 配置已从磁盘加载
config-notify-load-failed = 加载配置失败：{ $error }
config-notify-store-unavailable = 配置存储不可用
config-notify-saved = 配置已保存
config-notify-save-failed = 保存失败：{ $error }
config-notify-valid = 配置有效
config-notify-validation-failed = 验证失败：{ $error }
config-notify-reset = 配置已重置为默认值
config-notify-migrated = 配置已使用默认值迁移
config-notify-operation-failed = 操作失败：{ $error }
config-notify-reloaded = 配置已从磁盘重新加载
config-notify-reload-failed = 重载失败：{ $error }
config-confirm-title = 未保存的更改
config-confirm-message = 当前编辑尚未保存。是否继续并覆盖编辑器内容？
config-confirm-continue = 继续
config-confirm-cancel = 取消

## 角色提示词面板
profile-subtitle = 管理工作区提示词文档并预览组合后的系统提示词
profile-path-hint = 工作区：{ $path }
profile-path-not-loaded = 工作区：（未加载）
profile-markdown-files-count = Markdown 文件：{ $count }
profile-reload = 重载
profile-create-file = 创建文件
profile-workspace-markdown-files = 工作区 Markdown 文件
profile-no-markdown-files = 在工作区目录中未找到 Markdown 文件。
profile-name = 名称
profile-summary = 摘要
profile-size = 大小
profile-modified = 修改时间
profile-path = 路径
profile-preview = 预览
profile-edit = 编辑
profile-reset = 重置
profile-delete = 删除
profile-system-prompt-preview = 系统提示词预览
profile-loading = 加载中...
profile-system-prompt-desc = 由当前工作区提示词文档和已安装技能渲染而成。
profile-system-prompt-unavailable-title = 系统提示词预览不可用
profile-system-prompt-unavailable-body = 后台预览任务已断开连接。
profile-edit-title = 编辑 { $name }
profile-preview-title = 预览 { $name }
profile-path-label = 路径：{ $path }
profile-dirty-yes = 已修改：是
profile-dirty-no = 已修改：否
profile-workspace-editor = 工作区 Markdown 编辑器
profile-save = 保存
profile-cancel = 取消
profile-reset-btn = 重置
profile-default = 默认
profile-reset-default-title = 重置为默认模板
profile-reset-default-editor-desc = 将 { $name } 重置为内置默认模板？这将替换当前编辑器内容。
profile-reset-default-file-desc = 将 { $name } 重置为内置默认模板？确认后将覆盖 { $path }。
profile-reset-default-btn = 重置为默认
profile-create-file-title = 创建工作区文件
profile-create-workspace-path = 工作区路径：{ $path }
profile-create-file-hint = 文件将直接在工作区目录下创建。
profile-file-name-label = 文件名
profile-body-label = 内容
profile-create-btn = 创建
profile-delete-file-title = 删除工作区文件
profile-delete-file-desc = 删除 { $name }？
profile-delete-file-path = 路径：{ $path }
profile-notify-load-failed = 加载工作区 Markdown 文件失败：{ $error }
profile-notify-preview-disconnected = 系统提示词预览加载器已断开
profile-notify-saved = 已保存 { $name }
profile-notify-save-failed = 保存 { $path } 失败：{ $error }
profile-notify-reset-to-default = 已将 { $name } 重置为默认模板
profile-notify-reset-failed = 重置 { $path } 失败：{ $error }
profile-notify-created = 已创建 { $path }
profile-notify-create-failed = { $error }
profile-notify-deleted = 已删除 { $name }
profile-notify-delete-failed = 删除 { $path } 失败：{ $error }
profile-notify-workspace-unavailable = 工作区路径不可用。

## 设置面板
setting-subtitle = 配置应用偏好
setting-save-error = 保存错误：{ $error }
setting-section-general = 通用
setting-section-security = 安全与隐私
setting-section-network = 网络
setting-section-sync = 同步

setting-general-title = 通用设置
setting-notify-language-updated = 语言已更新。
setting-notify-language-update-failed = 更新语言失败：{ $error }
setting-launch-at-startup = 登录时启动：
setting-launch-at-startup-hint = 登录计算机时自动启动 Klaw。
setting-launch-at-startup-hint-unavailable = 登录计算机时自动启动 Klaw。
{ $reason } 您仍可在此处关闭该设置。
setting-yes = 是
setting-no = 否
setting-theme-mode-current = 当前主题模式：{ $mode }（可在底部状态栏更改）。
setting-light-theme = 浅色主题：
setting-dark-theme = 深色主题：
setting-theme-default-hint = 默认保留 egui 原有的浅色/深色外观。
setting-notify-launch-enabled = 已启用登录时启动。
setting-notify-launch-disabled = 已禁用登录时启动。
setting-notify-launch-update-failed = 更新登录时启动失败：{ $error }
setting-notify-launch-save-failed = 保存登录时启动设置失败：{ $error }
setting-notify-launch-save-and-rollback-failed = 保存登录时启动设置失败，并回滚 macOS 登录项：{ $message }

setting-security-title = 安全与隐私设置
setting-location-services = 定位服务
setting-system-location-services = 系统定位服务：
setting-app-authorization = 应用授权：
setting-detail = 详情：
setting-enabled = 已启用
setting-disabled = 已禁用
setting-auth-not-determined = 未决定
setting-auth-restricted = 受限制
setting-auth-denied = 已拒绝
setting-auth-authorized-always = 始终授权
setting-auth-authorized-when-in-use = 使用时授权
setting-auth-unsupported-platform = 此平台不支持
setting-auth-unknown = 未知
setting-auth-detail-not-determined = 尚未授予授权。请打开系统设置查看定位服务访问权限。
setting-auth-detail-restricted = 定位访问受系统策略或家长控制限制。
setting-auth-detail-denied = 此应用当前定位访问已被拒绝。请打开系统设置允许访问。
setting-auth-detail-auth-but-services-off = 已存在授权，但系统级定位服务当前已禁用。
setting-auth-detail-unsupported-platform = 定位服务隐私集成目前仅适用于 macOS。
setting-auth-detail-unknown = 系统返回了未知的授权状态。
setting-open-location-settings = 打开定位设置
setting-notify-location-settings-opened = 已打开 macOS 定位服务设置。
setting-notify-location-settings-failed = 打开 macOS 定位服务设置失败：{ $error }
setting-danger-zone = 危险区域
setting-delete-all-app-data = 删除所有应用数据
setting-delete-all-app-data-hint = 永久移除整个 .klaw 目录，包括配置、会话、技能、记忆、数据库及所有其他应用数据。此操作不可撤销。
setting-delete-all-data-btn = 删除所有数据
setting-confirm-delete-title = 确认删除所有数据
setting-confirm-delete-warning = 此操作将永久删除所有 Klaw 数据！
setting-confirm-delete-description = 整个 ~/.klaw 目录将被移除，包括：
setting-confirm-delete-item-config = 配置与设置
setting-confirm-delete-item-sessions = 会话与归档
setting-confirm-delete-item-skills = 技能与注册表
setting-confirm-delete-item-memory = 记忆与知识
setting-confirm-delete-item-databases = 数据库与日志
setting-confirm-delete-irreversible = 此操作不可撤销。
setting-cancel = 取消
setting-delete-everything-btn = 删除所有内容
setting-notify-delete-data-failed = 删除数据目录失败：{ $error }
setting-notify-data-dir-unavailable = 无法定位数据目录。

setting-network-title = 网络设置
setting-proxy-configuration = 代理配置：
setting-proxy-no-proxy = 不使用代理
setting-proxy-system = 使用系统代理
setting-proxy-manual = 手动代理配置
setting-http-proxy = HTTP 代理
setting-https-proxy = HTTPS 代理
setting-socks5-proxy = SOCKS5 代理
setting-proxy-host = 主机：
setting-proxy-port = 端口：

setting-sync-title = 同步设置
setting-sync-enable-label = 启用清单同步和 S3 存储
setting-notify-sync-enabled = 已启用清单同步。
setting-notify-sync-disabled = 已禁用清单同步。
setting-sync-general = 通用
setting-sync-provider = 提供商：
setting-sync-provider-s3 = S3
setting-sync-mode = 模式：
setting-sync-mode-versioned = 版本化清单
setting-sync-device-id = 设备 ID：
setting-sync-schedule-header = 计划与保留
setting-sync-auto-backup = 启用自动备份
setting-sync-interval = 间隔（分钟）：
setting-sync-keep-latest = 保留最新清单数：
setting-sync-s3-header = S3 配置
setting-s3-endpoint = 端点：
setting-s3-region = 区域：
setting-s3-bucket = 存储桶：
setting-s3-prefix = 前缀：
setting-s3-access-key = 访问密钥：
setting-s3-secret-key = 秘密密钥：
setting-s3-session-token = 会话令牌：
setting-s3-access-key-env = 访问密钥环境变量：
setting-s3-secret-key-env = 秘密密钥环境变量：
setting-s3-session-token-env = 会话令牌环境变量：
setting-s3-force-path-style = 强制路径风格
setting-sync-scope-header = 备份范围
setting-sync-scope-restore-hint = 恢复将回放所选清单版本。临时数据、日志和可观测性数据不包含在内。
setting-sync-actions-header = 清单操作
setting-sync-remote-newer = 远程清单 { $id }（来自 { $device }）比本地更新。
setting-sync-remote-created = 远程创建时间：{ $time }
setting-sync-last-sync = 上次同步：{ $time }
setting-sync-last-manifest-id = 上次清单 ID：{ $id }
setting-sync-in-progress = 进行中：{ $label }
setting-sync-run-now = 立即同步
setting-sync-refresh-remote = 刷新远程清单
setting-sync-run-cleanup = 运行保留清理
setting-sync-manual-progress-hint = 手动同步进度如下显示，涵盖清单协调、Blob 上传和清单发布。
setting-sync-remote-header = 远程清单
setting-sync-no-remote = 未加载远程清单。
setting-sync-manifest-id = 清单：{ $id }
setting-sync-created = 创建时间：{ $time }
setting-sync-device = 设备：{ $device }
setting-sync-restore-btn = 恢复
setting-sync-confirm-restore-title = 确认恢复
setting-sync-confirm-restore-desc1 = 恢复将替换当前本地清单管理的数据。
setting-sync-confirm-restore-desc2 = 恢复将回放所选清单版本。
setting-sync-confirm-restore-desc3 = 恢复完成后请重启 Klaw。
setting-sync-restore-now-btn = 立即恢复
setting-notify-restore-started = 恢复已开始。
setting-notify-sync-backup-done = 清单 { $id } 已上传至 S3。
setting-notify-sync-list-done = 远程清单已刷新。
setting-notify-sync-restore-done = 清单 { $id } 已恢复。请重启 Klaw 后继续。
setting-notify-sync-cleanup-done = 远程清单保留清理已完成。
setting-sync-task-label-backup = 上传清单同步
setting-sync-task-label-refresh = 加载清单
setting-sync-task-label-restore = 恢复清单
setting-sync-task-label-cleanup = 清理远程清单

setting-sync-stage-reconciling = 协调远程清单
setting-sync-stage-preparing = 准备清单
setting-sync-stage-uploading-blobs = 上传 Blob
setting-sync-stage-uploading-manifest = 上传清单
setting-sync-stage-updating-pointer = 更新最新清单指针
setting-sync-stage-cleaning-up = 清理旧清单
setting-sync-stage-completed = 同步完成
setting-sync-stage-connecting = 连接远程存储
setting-sync-stage-validating = 验证同步配置

setting-sync-item-session = 会话
setting-sync-item-skills = 技能
setting-sync-item-mcp-excluded = MCP（排除）
setting-sync-item-skills-registry = 技能仓库
setting-sync-item-gui-settings = GUI 设置
setting-sync-item-archive = 归档
setting-sync-item-user-workspace = 用户工作区
setting-sync-item-memory = 记忆
setting-sync-item-config = 配置

## LLM 面板
llm-error-config-load = 加载配置失败: { $error }
llm-error-config-reload = 重载配置失败: { $error }
llm-error-rows-load = 加载 LLM 审计行失败: { $error }
llm-error-loader-disconnected = LLM 审计加载器意外关闭
llm-error-detail-load = 加载 LLM 审计详情失败: { $error }
llm-error-detail-loader-disconnected = LLM 审计详情加载器意外关闭

llm-btn-refresh = 刷新
llm-label-total = 总计: { $count }
llm-status-loading = 加载中...
llm-status-loading-rows = 正在加载 LLM 审计行...
llm-status-no-rows = 未找到 LLM 审计行。

llm-filter-session = 会话
llm-filter-provider = 提供商
llm-filter-all = 全部
llm-filter-start-date = 开始日期
llm-filter-end-date = 结束日期
llm-label-page = 页码
llm-label-size = 每页数量

llm-col-session = 会话
llm-col-provider = 提供商
llm-col-model = 模型
llm-col-wire-api = 传输协议
llm-col-turn = 轮次
llm-col-seq = 序号
llm-col-status = 状态

llm-ctx-view-details = { $icon } 查看详情
llm-ctx-copy-session-key = { $icon } 复制会话密钥
llm-ctx-copy-request-id = { $icon } 复制请求 ID

llm-title-detail = LLM 审计详情
llm-detail-session = 会话: { $session }
llm-detail-time = 时间: { $time }
llm-detail-provider = 提供商: { $provider }
llm-detail-model = 模型: { $model }
llm-detail-wire-api = 传输协议: { $wire_api }
llm-detail-status = { $icon } { $text }
llm-detail-error-code = 错误码: { $error_code }
llm-detail-error-message = 错误消息: { $error_message }

llm-tab-request = 请求
llm-tab-response = 响应
llm-detail-loading-request = 正在加载请求载荷...
llm-detail-loading-response = 正在加载响应载荷...
llm-detail-empty-response = 空

llm-sort-time-asc = 时间 ↑
llm-sort-time-desc = 时间 ↓

llm-status-success = 成功
llm-status-failed = 失败

## MCP 面板
mcp-notify-config-loaded = MCP 配置已从磁盘加载
mcp-notify-load-config-failed = 加载配置失败: { $error }
mcp-notify-store-unavailable = 配置存储不可用
mcp-notify-save-failed = 保存失败: { $error }
mcp-notify-server-saved = MCP 服务器已保存
mcp-notify-server-deleted = 已删除 MCP 服务器 '{ $id }'
mcp-notify-config-reloaded = 配置已从磁盘重新加载
mcp-notify-reload-failed = 重载失败: { $error }
mcp-notify-status-refreshed = MCP 状态已刷新
mcp-notify-status-refresh-failed = 刷新 MCP 状态失败: { $error }
mcp-notify-status-refresh-disconnected = 刷新 MCP 状态失败: 后台任务已断开连接
mcp-notify-sync-success = MCP 运行时已同步
mcp-notify-sync-failed = 同步 MCP 运行时失败: { $error }
mcp-notify-sync-disconnected = 同步 MCP 运行时失败: 后台任务已断开连接
mcp-notify-server-restarted = 已重启 MCP 服务器 { $target }
mcp-notify-restart-failed = 重启 { $target } 失败: { $error }
mcp-notify-restart-already-in-progress = MCP 服务器重启正在进行中
mcp-notify-settings-saved = MCP 设置已保存

mcp-label-servers-count = 服务器: { $count }
mcp-status-applying-changes = 正在应用 MCP 变更...
mcp-status-refreshing = 正在刷新运行时状态...
mcp-status-restarting = 正在重启 MCP 服务器...
mcp-label-no-servers = 未配置 MCP 服务器。

mcp-col-id = ID
mcp-col-on = 启用
mcp-col-status = 状态
mcp-col-mode = 模式
mcp-col-command-url = 命令/URL
mcp-col-args = 参数
mcp-col-tools = 工具

mcp-mode-stdio = stdio
mcp-mode-sse = sse
mcp-label-enabled-yes = 是
mcp-label-enabled-no = 否

mcp-form-title-edit = 编辑 MCP 服务器
mcp-form-title-add = 添加 MCP 服务器
mcp-form-id = ID
mcp-form-enabled = 启用
mcp-form-mode = 模式
mcp-form-tool-timeout-seconds = 工具超时秒数
mcp-form-command = 命令
mcp-form-cwd = 工作目录
mcp-form-url = URL
mcp-form-args = 参数
mcp-form-env = 环境变量
mcp-form-headers = 请求头
mcp-btn-save = 保存
mcp-btn-cancel = 取消

mcp-error-server-id-empty = MCP 服务器 ID 不能为空
mcp-error-server-id-duplicate = MCP 服务器 ID '{ $id }' 已存在，请选择其他 ID
mcp-error-tool-timeout-invalid = tool_timeout_seconds 必须为正整数
mcp-error-startup-timeout-invalid = startup_timeout_seconds 必须为正整数

mcp-btn-config = { $icon } 配置
mcp-btn-add = 添加
mcp-btn-reload = 重载
mcp-btn-refresh-status = { $icon } 刷新状态
mcp-btn-detail = { $icon } 详情
mcp-btn-edit = { $icon } 编辑
mcp-btn-restart = { $icon } 重启
mcp-btn-delete = { $icon } 删除

mcp-window-global-settings = MCP 设置
mcp-form-startup-timeout-seconds = startup_timeout_seconds:

mcp-window-detail-title = MCP 详情: { $server_id }

mcp-detail-heading = MCP 服务器详情
mcp-detail-server = 服务器: { $server_id }
mcp-detail-state = 状态: { $state }
mcp-detail-tools = 工具: { $tool_count }
mcp-detail-last-error = 最近错误: { $last_error }
mcp-detail-tools-list-heading = tools/list 响应
mcp-detail-tools-list-null = 无
mcp-detail-json-render-error = 错误: 渲染 JSON 失败: { $error }

mcp-state-starting = 启动中
mcp-state-running = 运行中
mcp-state-stopped = 已停止
mcp-state-failed = 已失败

mcp-placeholder-none = -

## ACP 面板
acp-panel-description = ACP 让 klaw 通过适配器命令调用外部 ACP 兼容的编码代理。
acp-panel-default-templates-hint = 默认模板使用 `npx -y @zed-industries/claude-agent-acp` 和 `npx -y @zed-industries/codex-acp`; 运行时工作目录来自 `working_directory`。

acp-notify-config-loaded = ACP 配置已从磁盘加载
acp-notify-load-config-failed = 加载配置失败: { $error }
acp-notify-store-unavailable = 配置存储不可用
acp-notify-save-failed = 保存失败: { $error }
acp-notify-config-reloaded = 配置已从磁盘重新加载
acp-notify-reload-failed = 重载失败: { $error }
acp-notify-status-refreshed = ACP 状态已刷新
acp-notify-status-refresh-failed = 刷新 ACP 状态失败: { $error }
acp-notify-status-refresh-disconnected = 刷新 ACP 状态失败: 后台任务已断开连接
acp-notify-sync-success = ACP 运行时已同步
acp-notify-sync-failed = 同步 ACP 运行时失败: { $error }
acp-notify-sync-disconnected = 同步 ACP 运行时失败: 后台任务已断开连接
acp-notify-server-restarted = 已重启 ACP 代理 { $target }
acp-notify-restart-failed = 重启 { $target } 失败: { $error }
acp-notify-server-deleted = 已删除 ACP 代理 '{ $id }'
acp-notify-server-saved = ACP 代理已保存
acp-notify-agent-started = ACP 代理 { $agent_id } 会话已启动
acp-notify-agent-start-failed = 启动 ACP 代理 { $agent_id } 失败: { $error }
acp-notify-agent-stopped = ACP 代理会话已停止
acp-notify-agent-stop-failed = 停止 ACP 代理失败: { $error }
acp-notify-agent-stopped-with-error = ACP 代理会话停止时出错: { $error }
acp-notify-agent-stop-disconnected = 停止 ACP 代理失败: 后台任务已断开连接
acp-notify-permission-resolved = 已发送请求 { $request_id } 的权限响应
acp-notify-permission-resolve-failed = 发送请求 { $request_id } 的权限响应失败: { $error }
acp-notify-prompt-opened = ACP 测试提示已打开
acp-notify-prompt-failed = 打开测试提示失败: { $error }
acp-notify-settings-saved = ACP 设置已保存
acp-notify-restart-already-in-progress = ACP 代理重启正在进行中

acp-stats-enabled = 已启用
acp-stats-running = 运行中
acp-stats-failed = 已失败
acp-stats-tools = 工具

acp-col-id = ID
acp-col-on = 启用
acp-col-status = 状态
acp-col-command = 命令
acp-col-tools = 工具

acp-enabled-status-yes = 是
acp-enabled-status-no = 否

acp-value-not-set = (未设置)
acp-value-unknown = (未知)
acp-value-none = (无)

acp-button-config = { $icon } 配置
acp-button-add-agent = 添加代理
acp-button-reload = 重载
acp-button-sync-runtime = { $icon } 同步运行时
acp-button-refresh-status = { $icon } 刷新状态
acp-button-test = { $icon } 测试

acp-form-title-edit = 编辑 ACP 代理
acp-form-title-add = 添加 ACP 代理
acp-form-config-persisted-info = ACP 代理配置保存在 config.toml 中。
acp-form-label-id = ID
acp-form-label-enabled = 启用
acp-form-label-command = 命令
acp-form-working-directory-info = 运行时工作目录来自工具/测试提示的 `working_directory` 输入。
acp-form-label-description = 描述
acp-form-button-save = 保存
acp-form-button-cancel = 取消

acp-settings-window-title = ACP 设置
acp-settings-description = ACP 通过 stdio 调用外部 ACP 兼容的编码代理。
acp-settings-startup-timeout-label = startup_timeout_seconds:
acp-settings-button-save = 保存
acp-settings-button-cancel = 取消
acp-settings-startup-timeout-invalid = startup_timeout_seconds 必须为正整数

acp-delete-dialog-title = 删除 ACP 代理
acp-delete-dialog-message = 确定要删除 ACP 代理 '{ $agent_id }' 吗？
acp-delete-dialog-info = 此操作将从 config.toml 中移除该 ACP 代理。
acp-delete-dialog-button-delete = { $icon } 删除
acp-delete-dialog-button-cancel = 取消

acp-detail-window-title = ACP 详情: { $agent_id }
acp-detail-label-id = ID
acp-detail-label-enabled = 启用
acp-detail-label-tool-name = 工具名称
acp-detail-label-command = 命令
acp-detail-label-env-vars = 环境变量
acp-detail-label-description = 描述
acp-detail-label-last-error = 最近错误
acp-detail-latest-prompt-snapshot = 最近提示快照
acp-detail-snapshot-mode = 模式: { $mode }
acp-detail-snapshot-title = 标题: { $title }
acp-detail-snapshot-updated-at = 更新时间: { $updated_at }
acp-detail-snapshot-available-commands = 可用命令: { $commands }
acp-detail-snapshot-config-options = 配置选项: { $options }

acp-test-prompt-title = ACP 测试提示
acp-test-prompt-working-directory-info = 工作目录: { $working_directory }
acp-test-prompt-input-hint = 输入消息并按 Enter 发送给 ACP 代理。
acp-test-prompt-input-placeholder = 输入消息...
acp-test-prompt-stop-button = { $icon } 停止
acp-test-prompt-output-section = 输出
acp-test-prompt-last-error = 最近错误
acp-test-prompt-session-snapshot = 会话快照
acp-test-prompt-snapshot-title = 标题
acp-test-prompt-snapshot-mode = 模式
acp-test-prompt-snapshot-updated-at = 更新时间
acp-test-prompt-snapshot-commands = 命令
acp-test-prompt-config-options = 配置选项
acp-test-prompt-pending-permissions = 待处理权限
acp-test-prompt-permission-timeline = 权限时间线
acp-test-prompt-structured-events = 结构化事件
acp-test-prompt-raw-stream = 原始流
acp-waiting-for-session-updates = 等待 ACP 会话更新...

acp-permission-label = #{ $request_id } { $title }
acp-permission-sending-response = 正在发送响应...
acp-permission-tool-kind = 工具类型: { $kind }
acp-permission-tool-status = 工具状态: { $status }
acp-permission-raw-input = 原始输入: { $raw_input }
acp-permission-option-button = { $label } ({ $kind })
acp-permission-cancel = 取消

acp-content-block-image-with-uri = [图片 { $mime_type } { $data_len } 字节 { $uri }]
acp-content-block-image = [图片 { $mime_type } { $data_len } 字节]
acp-content-block-audio = [音频 { $mime_type } { $data_len } 字节]
acp-content-block-resource-with-title = [资源 { $name } { $title } { $uri }]
acp-content-block-resource = [资源 { $name } { $uri }]
acp-content-block-embedded-text-with-mime = [嵌入文本 { $uri } { $mime_type }] { $text }
acp-content-block-embedded-text = [嵌入文本 { $uri }] { $text }
acp-content-block-embedded-blob-with-mime = [嵌入数据块 { $uri } { $mime_type } { $byte_len } 字节]
acp-content-block-embedded-blob = [嵌入数据块 { $uri } { $byte_len } 字节]
acp-content-block-unsupported = [不支持的内容 { $description }]

## 系统面板
system-view-host-information = 主机信息
system-view-program-disk-usage = 程序磁盘使用
system-view-environment = 环境

system-dir-tmp = 临时文件
system-dir-workspace = 工作区
system-dir-sessions = 会话
system-dir-archives = 归档
system-dir-logs = 日志
system-dir-skills = 技能
system-dir-skills-registry = 技能仓库
system-dir-models = 模型

system-cpu-usage = CPU 使用率
system-memory-usage = 内存使用率
system-system-information = 系统信息

system-host-app-uptime = 应用运行时间
system-host-name = 主机名
system-host-os-name = 操作系统名称
system-host-os-version = 操作系统版本
system-host-long-os-version = 详细操作系统版本
system-host-kernel-version = 内核版本
system-host-cpu-architecture = CPU 架构
system-host-logical-cpu-count = 逻辑 CPU 核心数
system-host-physical-core-count = 物理 CPU 核心数
system-host-primary-cpu-brand = 主 CPU 品牌
system-host-primary-cpu-frequency = 主 CPU 频率
system-host-total-memory = 总内存
system-host-used-memory = 已用内存
system-host-free-memory = 可用内存
system-host-total-swap = 总交换空间
system-host-used-swap = 已用交换空间
system-host-system-uptime = 系统运行时间
system-host-system-boot-time = 系统启动时间
system-host-load-average = 平均负载
system-host-data-directory = 数据目录

system-host-data-dir-size = 数据目录大小
system-host-data-dir-file-count = 数据目录文件数
system-host-data-dir-mount-point = 数据目录挂载点
system-host-data-dir-disk-capacity = 数据目录磁盘容量
system-host-data-dir-disk-available = 数据目录磁盘可用空间

system-cpu-cores-info = { $logical } 逻辑 / { $physical } 物理 核心数
system-memory-free = 可用: { $free }
system-cpu-frequency-mhz = { $freq } MHz
system-host-na = 无
system-host-loading = 加载中...

system-disk-usage-description = 检查和清理 Klaw 数据目录下的数据。
system-dir-path = 路径: { $path }
system-dir-path-unavailable = 路径不可用。
system-dir-clearing-hint = 清除操作将删除 `{ $dir }` 内的文件；目录本身将被保留。

system-usage-calculating = 计算中...
system-usage = 使用: { $usage }
system-usage-unavailable-error = 使用: 不可用 ({ $error })
system-usage-unavailable = 使用: 不可用

system-refresh = 刷新
system-open-dir-hint = 在文件管理器中打开 { $title } 目录
system-clear-dir-hint = 清除 { $title } 目录
system-clear = 清除
system-cancel = 取消

system-confirm-clear-title = 清除 { $title } 目录
system-confirm-clear-message = 确定要清除 { $title } 目录吗？

system-env-dependencies = 环境依赖
system-env-loading = 加载中...
system-env-not-found = 未找到
system-env-required = 必需
system-env-preferred = 推荐
system-env-optional = 可选
system-env-project = 项目:
system-env-all-available = 所有依赖可用
system-env-tm-missing = 注意：终端复用器 (zellij/tmux) 不可用
system-env-preferred-missing = 注意：部分推荐依赖缺失
system-env-required-missing = 警告：部分必需依赖缺失

system-notify-failed-collect-usage = 收集 { $title } 使用量失败: { $error }
system-notify-dir-cleared = { $title } 目录已清除
system-notify-failed-clear-dir = 清除 { $title } 目录失败: { $error }
system-notify-failed-resolve = 解析数据目录失败: { $error }
system-notify-failed-open-dir = 打开 { $title } 目录失败: { $error }

## 本地模型面板
local-model-subtitle = 浏览、安装和管理存储在设备上的本地 LLM 模型。
local-model-btn-refresh = { $icon } 刷新
local-model-btn-install = { $icon } 安装模型
local-model-btn-open-dir = { $icon } 打开模型目录
local-model-btn-set-default-gguf = 设置默认 GGUF 文件
local-model-btn-clear-default-gguf = 清除默认 GGUF
local-model-installed-label = 已安装模型
local-model-no-models = 尚未安装本地模型。
local-model-col-name = 名称
local-model-col-size = 大小
local-model-col-created = 创建时间
local-model-col-default-file = 默认模型文件
local-model-default-set = ✓
local-model-default-none = —
local-model-ctx-upgrade = { $icon } 升级
local-model-ctx-delete = { $icon } 删除
local-model-window-install = 安装模型
local-model-window-downloading = 正在下载模型
local-model-window-delete = 删除本地模型
local-model-install-desc = 下载完整的 Hugging Face 仓库快照。
local-model-install-repo = 仓库
local-model-install-revision = 分支 / 版本
local-model-install-download = 下载
local-model-install-cancel = 取消
local-model-download-file-label = 文件 { $index } / { $total }: { $name }
local-model-download-preparing = 正在准备仓库文件列表...
local-model-download-overall = 总体进度
local-model-download-cancel = 取消下载
local-model-delete-confirm-message = 删除模型 '{ $model_id }'？
local-model-delete-confirm-info = 此操作将删除本地快照文件和清单。当前绑定在配置中的模型无法删除。
local-model-delete-confirm-delete = { $icon } 删除
local-model-delete-confirm-cancel = 取消
local-model-notify-config-loaded = 本地模型配置已从磁盘加载
local-model-notify-load-failed = 加载配置失败: { $error }
local-model-notify-refresh-failed = 刷新失败: { $error }
local-model-notify-no-selected-model = 未选择模型，无法设置默认 GGUF 文件
local-model-notify-gguf-extension-required = 默认模型文件必须具有 .gguf 扩展名
local-model-notify-download-running = 另一个模型下载任务正在运行
local-model-notify-upgrading = 正在升级模型 '{ $model_id }'，版本 '{ $revision }'
local-model-notify-up-to-date = 模型 '{ $model_id }' 已是最新版本
local-model-notify-installed = 已安装模型 '{ $model_id }'
local-model-notify-install-cancelled = 模型安装已取消
local-model-notify-install-failed = 安装失败: { $error }
local-model-notify-removed = 已删除模型 '{ $model_id }'
local-model-notify-remove-failed = 删除失败: { $error }
local-model-notify-gguf-saved = 已保存模型 '{ $model_id }' 的默认 GGUF 文件
local-model-notify-gguf-save-failed = 保存默认 GGUF 文件失败: { $error }
local-model-notify-starting-download = 开始下载模型
local-model-notify-cancelling-download = 正在取消模型下载
local-model-notify-open-dir-failed = 打开模型目录失败: { $error }

## 模型提供商面板
provider-subtitle = 配置模型提供商并设置运行时的默认提供商。
provider-label-config-default = 配置默认: { $provider }
provider-label-runtime-active = 运行时活跃: { $provider }
provider-btn-add = { $icon } 添加提供商
provider-btn-reload = { $icon } 重载
provider-no-providers = 未配置提供商。
provider-col-id = ID
provider-col-name = 名称
provider-col-base-url = 基础 URL
provider-col-wire-api = 传输协议
provider-col-default-model = 默认模型
provider-col-stream = 流式
provider-col-tokenizer = 分词器
provider-col-auth = 认证
provider-badge-config = 配置
provider-badge-runtime = 运行时
provider-auth-api-key = API 密钥
provider-auth-env = 环境变量: { $key }
provider-auth-none = 无
provider-stream-yes = 是
provider-stream-no = 否
provider-ctx-edit = { $icon } 编辑
provider-ctx-set-default = { $icon } 设为配置默认
provider-ctx-delete = { $icon } 删除
provider-ctx-copy-id = { $icon } 复制 ID
provider-form-title-add = 添加提供商
provider-form-title-edit = 编辑提供商
provider-form-persisted-info = 提供商配置保存在 config.toml 中。
provider-form-id = 提供商 ID
provider-form-name = 显示名称
provider-form-base-url = 基础 URL
provider-form-wire-api = 传输协议
provider-form-default-model = 默认模型
provider-form-tokenizer = 分词器路径
provider-form-proxy = 使用系统代理
provider-form-stream = 启用流式传输
provider-form-api-key = API 密钥
provider-form-set-active = 设为活跃模型提供商
provider-form-save = 保存
provider-form-cancel = 取消
provider-delete-title = 删除提供商
provider-delete-message = 确定要删除提供商 '{ $provider_id }' 吗？
provider-delete-info = 此操作将从 config.toml 中移除该提供商。活跃或正在使用的提供商无法删除。
provider-delete-btn = { $icon } 删除
provider-delete-cancel = 取消

## Tool panel

tool-subtitle = 管理工具启停与各项设置。
tool-status-sync-pending = 运行时同步等待中...
tool-btn-reload = { $icon } 刷新

## Tool table
tool-col-tool = 工具
tool-col-status = 状态
tool-col-description = 描述
tool-status-enabled = 已启用
tool-status-disabled = 已禁用

## Tool context menu
tool-ctx-edit = { $icon } 编辑
tool-ctx-inspect = { $icon } 查看详情
tool-ctx-logs = { $icon } 日志

## Tool form
tool-form-title = 编辑工具: { $name }
tool-toggle-title = 编辑工具: { $kind }
tool-form-enabled = 启用
tool-form-workspace = 工作目录
tool-form-allow-absolute-paths = 允许绝对路径
tool-form-allow-login-shell = 允许登录 Shell
tool-form-max-timeout-ms = 最大超时(ms)
tool-form-max-output-bytes = 最大输出字节数
tool-form-max-bytes = 最大字节数
tool-form-search-limit = 搜索上限
tool-form-fts-limit = 全文检索上限
tool-form-vector-limit = 向量检索上限
tool-form-use-vector = 使用向量检索
tool-form-context-limit = 上下文上限
tool-form-include-explain = 包含解释
tool-form-max-chars = 最大字符数
tool-form-timeout-seconds = 超时(秒)
tool-form-cache-ttl-minutes = 缓存 TTL(分钟)
tool-form-max-redirects = 最大重定向数
tool-form-readability = 可读性提取
tool-form-provider = 提供商
tool-form-base-url = 基础 URL
tool-form-api-key = API 密钥
tool-form-env-key = 环境变量键
tool-form-search-depth = 搜索深度
tool-form-topic = 主题
tool-form-include-answer = 包含答案
tool-form-include-raw-content = 包含原始内容
tool-form-include-images = 包含图片
tool-form-project-id = 项目 ID
tool-form-country = 国家
tool-form-search-lang = 搜索语言
tool-form-ui-lang = 界面语言
tool-form-safesearch = 安全搜索
tool-form-freshness = 时效性
tool-form-max-iterations = 最大迭代数
tool-form-max-tool-calls = 最大工具调用数
tool-form-inherit-parent-tools = 继承父级工具

## Tool web search sections
tool-section-tavily = Tavily
tool-section-brave = Brave

## Tool form buttons
tool-form-save = 保存
tool-form-cancel = 取消

## Tool inspect window
tool-inspect-title = 查看工具详情: { $name }
tool-inspect-description = 描述
tool-inspect-schema = 模式
tool-inspect-metadata-unavailable = 该工具无运行时元数据。

## Tool logs window
tool-log-window-title = 工具日志: { $name }
tool-log-btn-refresh = { $icon } 刷新
tool-log-rows = { $count } 行
tool-log-hint-summary = 双击行或右键查看摘要。
tool-log-filter-session = 会话
tool-log-filter-start = 开始时间
tool-log-filter-end = 结束时间
tool-log-filter-status = 状态
tool-log-filter-all = 全部
tool-log-filter-failed-only = 仅失败
tool-log-sort-time-asc = 时间升序
tool-log-sort-time-desc = 时间降序
tool-log-col-tool-call-id = 工具调用 ID
tool-log-col-status = 状态
tool-log-col-seq = 序号
tool-log-col-session = 会话
tool-log-status-success = 成功
tool-log-status-failed = 失败
tool-log-ctx-summary = { $icon } 摘要
tool-log-no-rows = 未找到工具审计记录。

## Tool log summary window
tool-log-summary-title = 工具日志摘要: { $name }
tool-log-summary-hint = 参数 / 结果 / 元数据支持标签页切换。
tool-log-summary-label-summary = 摘要
tool-log-summary-label-tool = 工具
tool-log-summary-label-session = 会话
tool-log-summary-label-chat = 对话
tool-log-summary-label-status = 状态
tool-log-summary-label-seq = 序号
tool-log-summary-label-started = 开始时间
tool-log-summary-label-duration = 持续时间
tool-log-summary-label-error-code = 错误码
tool-log-summary-tab-arguments = 参数
tool-log-summary-tab-result = 结果
tool-log-summary-tab-metadata = 元数据
tool-log-summary-section-arguments = 参数
tool-log-summary-section-result = 结果
tool-log-summary-section-metadata = 元数据
tool-log-summary-section-error = 错误
tool-log-summary-section-error-details = 错误详情
tool-log-summary-section-signals = 信号

## Tool notifications
tool-notify-config-loaded = 工具配置已从磁盘加载
tool-notify-load-failed = 加载配置失败: { $error }
tool-notify-store-unavailable = 配置存储不可用
tool-notify-save-failed = 保存失败: { $error }
tool-notify-config-reloaded = 配置已从磁盘重新加载
tool-notify-reload-failed = 重新加载失败: { $error }
tool-notify-runtime-metadata-failed = 加载运行时工具元数据失败: { $error }
tool-notify-synced = 工具配置已保存并同步运行时（{ $count } 个工具活跃）
tool-notify-sync-failed = 工具配置已保存，但同步运行时失败: { $error }
tool-notify-syncing = 正在同步工具配置与运行时...
tool-notify-saved = 工具配置已保存

## 技能仓库面板
skills-reg-subtitle = 管理技能仓库并从远程仓库同步技能。
skills-reg-label-registries-count = 注册源: { $count }
skills-reg-btn-config = { $icon } 配置
skills-reg-btn-reload = { $icon } 刷新
skills-reg-btn-add = { $icon } 添加技能注册源
skills-reg-no-registries = 未配置技能注册源。

## Skills Registry table
skills-reg-col-name = 名称
skills-reg-col-address = 地址
skills-reg-col-synced = 已同步
skills-reg-col-commit = 提交
skills-reg-col-installed = 已安装
skills-reg-status-outdated = { $icon } 过期
skills-reg-status-synced = { $icon } 已同步

## Skills Registry context menu
skills-reg-ctx-sync = { $icon } 同步
skills-reg-ctx-edit = { $icon } 编辑
skills-reg-ctx-copy-name = { $icon } 复制名称
skills-reg-ctx-delete = { $icon } 删除

## Skills Registry config window
skills-reg-config-title = 技能注册源配置
skills-reg-config-sync-timeout = 同步超时 (秒)
skills-reg-config-save = 保存超时
skills-reg-config-cancel = 取消

## Skills Registry form window
skills-reg-form-title-edit = 编辑技能注册源
skills-reg-form-title-add = 添加技能注册源
skills-reg-form-label-name = 名称
skills-reg-form-label-address = 地址
skills-reg-form-btn-save = 保存
skills-reg-form-btn-cancel = 取消

## Skills Registry delete dialog
skills-reg-delete-title = 删除技能注册源
skills-reg-delete-message = 确定要删除注册源 '{ $registry_name }' 吗？
skills-reg-delete-description = 这将从配置中移除注册源，并清理清单中已安装的技能。
skills-reg-delete-btn = { $icon } 删除
skills-reg-delete-cancel = 取消

## Skills Registry notifications
skills-reg-notify-config-loaded = 技能注册源配置已从磁盘加载
skills-reg-notify-load-failed = 加载配置失败: { $error }
skills-reg-notify-store-unavailable = 配置存储不可用
skills-reg-notify-save-failed = 保存失败: { $error }
skills-reg-notify-config-reloaded = 配置已从磁盘重新加载
skills-reg-notify-reload-failed = 重新加载失败: { $error }
skills-reg-notify-sync-already-running = 技能注册源同步已在运行
skills-reg-notify-registry-not-found = 未找到技能注册源 `{ $registry_name }`
skills-reg-error-sync-timeout-invalid = skills.sync_timeout 必须为正整数
skills-reg-notify-sync-timeout-saved = skills.sync_timeout 已保存
skills-reg-notify-sync-started = 开始同步注册源 `{ $registry_name }`
skills-reg-notify-sync-success = 注册源 `{ $registry_name }` 已同步: 新增 { $added }, 移除 { $removed }
skills-reg-notify-sync-failed = 同步注册源 `{ $registry_name }` 失败: { $error }
skills-reg-notify-sync-disconnected = 技能注册源同步工作器已断开
skills-reg-notify-registry-deleted = 技能注册源 `{ $registry_name }` 已删除
skills-reg-notify-cleaned-skills = 已清理 { $count } 个已安装技能
skills-reg-notify-cleanup-failed = 清理注册源清单失败: { $error }
skills-reg-notify-registry-saved = 技能注册源已保存
skills-reg-notify-reload-not-sent = 运行时技能提示刷新未发送: { $error }
skills-reg-error-name-empty = 技能注册源名称不能为空
skills-reg-error-address-empty = 技能注册源地址不能为空
skills-reg-error-name-duplicate = 技能注册源 '{ $name }' 已存在，请使用其他名称

## 技能管理面板
skills-mgr-subtitle = 从注册源或本地来源安装、查看和管理技能。
skills-mgr-label-installed-count = 已安装: { $count }
skills-mgr-label-registries-count = 注册源: { $count }
skills-mgr-btn-refresh = { $icon } 刷新
skills-mgr-btn-install = { $icon } 安装
skills-mgr-btn-install-local = { $icon } 安装本地
skills-mgr-no-skills = 未找到已安装技能。

## Skills Manager table
skills-mgr-col-name = 名称
skills-mgr-col-source = 来源
skills-mgr-col-registry = 注册源
skills-mgr-col-state = 状态
skills-mgr-col-updated = 更新时间
skills-mgr-col-path = 路径
skills-mgr-source-local = 本地
skills-mgr-source-registry = 注册源
skills-mgr-state-stale = 过期
skills-mgr-state-fresh = 最新
skills-mgr-state-none = -

## Skills Manager context menu
skills-mgr-menu-view = { $icon } 查看
skills-mgr-menu-remove = { $icon } 移除
skills-mgr-menu-copy-name = { $icon } 复制名称

## Skills Manager detail window
skills-mgr-detail-title = 技能详情: { $name }
skills-mgr-detail-name = 名称: { $name }
skills-mgr-detail-source = 来源: { $source }
skills-mgr-detail-registry = 注册源: { $registry }
skills-mgr-detail-state = 状态: { $icon } { $state }
skills-mgr-detail-path = 路径: { $path }
skills-mgr-detail-updated = 更新时间: { $time }

## Skills Manager install window
skills-mgr-install-title = 安装技能
skills-mgr-install-registry = 注册源
skills-mgr-install-select-registry = (选择注册源)
skills-mgr-install-select-registry-error = 请先选择注册源
skills-mgr-install-col-action = 操作
skills-mgr-install-col-skill = 技能
skills-mgr-install-col-id = ID
skills-mgr-install-col-path = 路径
skills-mgr-install-btn-install = 安装
skills-mgr-install-btn-uninstall = 卸载

## Skills Manager delete dialog
skills-mgr-delete-title = 确认移除
skills-mgr-delete-message = 确定要移除技能 `{ $name }` 吗？
skills-mgr-delete-registry = 注册源: { $registry }
skills-mgr-delete-btn-remove = { $icon } 移除
skills-mgr-delete-cancel = 取消

## Skills Manager notifications
skills-mgr-notify-load-failed = 加载配置失败: { $error }
skills-mgr-notify-reload-failed = 重新加载配置失败: { $error }
skills-mgr-notify-reload-not-sent = 运行时技能提示刷新未发送: { $error }
skills-mgr-notify-load-skills-failed = 加载已安装技能失败: { $error }
skills-mgr-notify-load-detail-failed = 加载技能 `{ $skill_name }` 详情失败: { $error }
skills-mgr-notify-no-registry = 未配置技能注册源
skills-mgr-notify-local-install-success = 已从 { $source_dir } 安装本地技能 `{ $skill_name }` 至 { $target_dir }
skills-mgr-notify-local-install-failed = 安装本地技能失败: { $error }
skills-mgr-notify-install-config-save-failed = 保存已安装技能 `{ $skill_id }` 至配置失败: { $error }
skills-mgr-notify-already-installed = `{ $skill_id }` 已安装
skills-mgr-notify-install-success = 已从注册源 `{ $registry_name }` 安装 `{ $skill_id }`
skills-mgr-notify-install-partial = `{ $skill_id }` 已保存至配置，但安装未能立即完成: { $error }
skills-mgr-notify-uninstall-config-update-failed = 卸载 `{ $skill_id }` 时更新配置失败: { $error }
skills-mgr-notify-not-installed = `{ $skill_id }` 未安装
skills-mgr-notify-uninstall-success = 已从注册源 `{ $registry_name }` 卸载 `{ $skill_id }`
skills-mgr-notify-uninstall-partial = `{ $skill_id }` 已从配置移除，但注册源卸载未能立即完成: { $error }
skills-mgr-notify-remove-config-failed = 从配置移除 `{ $skill_name }` 失败: { $error }
skills-mgr-notify-uninstall-local-cleanup-failed = `{ $skill_name }` 已从配置移除，但本地清理失败: { $error }
skills-mgr-notify-uninstall-failed = 卸载 `{ $skill_name }` 失败: { $error }
skills-mgr-notify-uninstall-scope-registry = 注册源 `{ $registry_name }` 中的 `{ $skill_name }`
skills-mgr-notify-uninstall-scope-local = 本地 `{ $skill_name }`
skills-mgr-notify-uninstall-result = 已卸载 { $scope }: { $managed } 个管理文件, { $local } 个本地文件已移除

## 通道面板
channel-subtitle = 管理与外部消息服务（钉钉、Telegram、WebSocket）的通道连接。
channel-restarting = 正在重启通道...
channel-synchronizing = 正在同步通道...
channel-btn-disabled = { $icon } 设置禁用通道
channel-disabled-label = 设置禁用通道
channel-btn-add-dingtalk = { $icon } 添加钉钉
channel-btn-add-telegram = { $icon } 添加 Telegram
channel-btn-add-websocket = { $icon } 添加 WebSocket
channel-btn-reload = { $icon } 重载
channel-btn-refresh-status = { $icon } 刷新状态
channel-no-channels = 未配置通道。

channel-col-type = 类型
channel-col-id = ID
channel-col-enabled = 启用
channel-col-status = 状态
channel-col-last-activity = 最后活动
channel-col-reconnect = 重连
channel-col-title = 标题
channel-col-reasoning = 推理
channel-col-stream = 流式
channel-col-proxy = 代理

channel-status-running = 运行中
channel-status-degraded = 降级
channel-status-reconnecting = 重连中
channel-status-starting = 启动中
channel-status-stopped = 已停止
channel-status-failed = 失败
channel-status-unknown = 未知
channel-yes = 是
channel-no = 否
channel-dash = -

channel-hover-last-event = 最后事件: { $event }
channel-hover-last-error = 最后错误: { $error }

channel-ctx-edit = { $icon } 编辑
channel-ctx-restart = { $icon } 重启
channel-ctx-disable = { $icon } 禁用
channel-ctx-enable = { $icon } 启用
channel-ctx-delete = { $icon } 删除
channel-ctx-copy-id = { $icon } 复制 ID

channel-form-title-add-dingtalk = 添加钉钉通道
channel-form-title-edit-dingtalk = 编辑钉钉通道
channel-form-title-add-telegram = 添加 Telegram 通道
channel-form-title-edit-telegram = 编辑 Telegram 通道
channel-form-title-add-websocket = 添加 WebSocket 通道
channel-form-title-edit-websocket = 编辑 WebSocket 通道

channel-form-id = ID
channel-form-id-hint-dingtalk = 钉钉通道实例的唯一标识符（如 "ops"、"devops"）。
channel-form-id-hint-telegram = Telegram 通道实例的唯一标识符（如 "ops-bot"）
channel-form-id-hint-websocket = WebSocket 通道实例的唯一标识符（如 "browser"）
channel-form-enabled = 启用
channel-form-enabled-hint-dingtalk = 启用或禁用此钉钉通道实例。
channel-form-enabled-hint-telegram = 启用或禁用此 Telegram 通道实例。
channel-form-enabled-hint-websocket = 启用或禁用此 WebSocket 通道实例。
channel-form-client-id = 客户端 ID
channel-form-client-id-hint = 钉钉开发者控制台中的应用客户端 ID。
channel-form-client-secret = 客户端密钥
channel-form-client-secret-hint = 钉钉开发者控制台中的应用客户端密钥。
channel-form-bot-title = 机器人名称
channel-form-bot-title-hint = 钉钉对话中显示的机器人名称。
channel-form-show-reasoning = 显示推理
channel-form-show-reasoning-hint-dingtalk = 在钉钉响应中包含推理/思考过程。
channel-form-show-reasoning-hint-telegram = 在 Telegram 响应中包含推理/思考过程。
channel-form-show-reasoning-hint-websocket = 在 WebSocket 响应中包含推理/思考过程。
channel-form-stream-output = 流式输出
channel-form-stream-output-hint = 逐步流式输出代理响应，而非等待完成后再输出。
channel-form-stream-template-id = 流式模板 ID
channel-form-stream-template-id-hint = 用于格式化流式输出块的钉钉消息模板 ID。
channel-form-stream-content-key = 流式内容键
channel-form-stream-content-key-hint = 流式模板中存放内容文本的 JSON 键名。
channel-form-proxy-enabled = 启用代理
channel-form-proxy-enabled-hint-dingtalk = 通过 HTTP 代理路由钉钉 API 请求。
channel-form-proxy-enabled-hint-telegram = 通过 HTTP 代理路由 Telegram API 请求。
channel-form-proxy-url = 代理 URL
channel-form-proxy-url-hint-dingtalk = 钉钉 API 连接的 HTTP 代理 URL（如 "http://proxy:8080"）
channel-form-proxy-url-hint-telegram = Telegram API 连接的 HTTP 代理 URL（如 "http://proxy:8080"）
channel-form-bot-token = 机器人令牌
channel-form-bot-token-hint = 从 BotFather 获取的 Telegram 机器人令牌（如 "123456:ABC-DEF"）
channel-form-allowlist = 白名单
channel-form-save = 保存
channel-form-cancel = 取消

channel-disabled-title = 设置禁用通道
channel-disabled-save = 保存
channel-disabled-cancel = 取消

channel-delete-title = 删除 { $kind } 通道
channel-delete-message = 确定要删除通道 '{ $id }' 吗？
channel-delete-info = 此操作无法撤销。
channel-delete-btn = { $icon } 删除
channel-delete-cancel = 取消

## Webhook 面板
webhook-subtitle = 管理 Webhook 端点以接收传入事件和代理提示词。
webhook-btn-refresh = { $icon } 刷新
webhook-btn-config = { $icon } 配置
webhook-btn-create-prompt = { $icon } 创建提示词
webhook-btn-inspect-prompt = { $icon } 检查提示词
webhook-label-rows = 行数: { $count }

webhook-filter-type = 类型
webhook-filter-events = 事件
webhook-filter-agents = 代理
webhook-filter-source = 来源
webhook-filter-event-type = 事件类型
webhook-filter-hook-id = Hook ID
webhook-filter-session = 会话
webhook-filter-status = 状态
webhook-filter-all = 全部
webhook-filter-start-date = 开始日期
webhook-filter-end-date = 结束日期
webhook-filter-page = 页码
webhook-filter-size = 每页数量

webhook-status-accepted = 已接受
webhook-status-processed = 已处理
webhook-status-failed = 失败

webhook-no-rows = 未找到 Webhook 行数据。

webhook-col-source = 来源
webhook-col-hook-id = Hook ID
webhook-col-event-type = 事件类型
webhook-col-session = 会话
webhook-col-status = 状态
webhook-col-sender = 发送者

webhook-sort-time-asc = 时间 ↑
webhook-sort-time-desc = 时间 ↓

webhook-ctx-view-reply = { $icon } 查看回复
webhook-ctx-raw-json = { $icon } Raw JSON
webhook-ctx-copy-id = { $icon } 复制 ID

webhook-raw-title = Raw JSON: { $id }
webhook-raw-payload = 载荷
webhook-raw-metadata = 元数据

webhook-summary-title = 回复摘要: { $id }

webhook-config-title = Webhook 配置
webhook-config-enabled = 启用
webhook-config-enabled-hint = 启用或禁用整个 Webhook 子系统。
webhook-config-events-header = 事件端点
webhook-config-events-enabled = 启用
webhook-config-events-enabled-hint = 启用事件端点以接收传入的 Webhook 事件。
webhook-config-events-path = 路径
webhook-config-events-path-hint = 事件端点的 URL 路径（只读，自动分配）。
webhook-config-events-max-body = 最大请求体字节数
webhook-config-events-max-body-hint = 事件端点接受的最大请求体字节数。
webhook-config-agents-header = 代理端点
webhook-config-agents-enabled = 启用
webhook-config-agents-enabled-hint = 启用代理端点以接收传入的代理提示词。
webhook-config-agents-path = 路径
webhook-config-agents-path-hint = 代理端点的 URL 路径（只读，自动分配）。
webhook-config-agents-max-body = 最大请求体字节数
webhook-config-agents-max-body-hint = 代理端点接受的最大请求体字节数。
webhook-config-reload = 重载
webhook-config-save = 保存

webhook-prompt-create-title = 创建提示词
webhook-prompt-edit-title = 编辑提示词
webhook-prompt-hook-id = Hook ID
webhook-prompt-save-to = 保存至: { $path }
webhook-prompt-markdown = Markdown
webhook-prompt-save = 保存
webhook-prompt-save-changes = 保存更改

webhook-inspect-title = 检查提示词
webhook-inspect-reload = 重载
webhook-inspect-templates = 提示词模板: { $count }
webhook-inspect-directory = 目录: { $path }
webhook-inspect-no-templates = 未找到提示词模板。
webhook-inspect-col-hook-id = Hook ID
webhook-inspect-col-path = 路径

webhook-inspect-ctx-edit = { $icon } 编辑
webhook-inspect-ctx-view = { $icon } 查看
webhook-inspect-ctx-trick = { $icon } Trick
webhook-inspect-ctx-delete = { $icon } 删除

webhook-view-title = 查看提示词: { $hook_id }
webhook-view-preview = 预览

webhook-delete-title = 删除提示词
webhook-delete-message = 确定要删除提示词模板 '{ $hook_id }' 吗？
webhook-delete-btn = { $icon } 删除
webhook-delete-cancel = 取消

webhook-trick-title = Trick 提示词: { $hook_id }
webhook-trick-hook-id = Hook ID
webhook-trick-base-session = 基础会话
webhook-trick-select-session = 选择基础会话
webhook-trick-provider = 提供商
webhook-trick-select-provider = 选择提供商
webhook-trick-model = 模型
webhook-trick-no-sessions = 未找到可交付的基础会话。请先从支持的 IM 聊天中发送一条新消息。
webhook-trick-generate = 生成
webhook-trick-url-label = Webhook URL
webhook-trick-copy-url = { $icon } 复制 URL
webhook-trick-url-copied = Webhook URL 已复制到剪贴板

webhook-notify-config-loaded = Webhook 配置已加载
webhook-notify-config-save-restart = Webhook 配置已保存。重启网关以应用运行时变更。
webhook-notify-config-saved = Webhook 配置已保存
webhook-notify-config-reloaded = Webhook 配置已从磁盘重新加载
webhook-notify-rows-failed = 加载 Webhook 行数据失败: { $error }
webhook-notify-rows-disconnected = Webhook 行数据加载器意外关闭
webhook-notify-status-failed = 加载网关状态失败: { $error }
webhook-notify-status-disconnected = 网关状态加载器意外关闭
webhook-notify-store-unavailable = 配置存储不可用
webhook-notify-save-failed = 保存失败: { $error }
webhook-notify-reload-failed = 重载失败: { $error }
webhook-notify-prompt-dir-unavailable = 提示词目录不可用，因为无法解析数据根目录。
webhook-notify-prompt-dir-not-exist = 提示词目录尚未创建: { $path }
webhook-notify-prompt-dir-create-failed = 创建提示词目录 { $path } 失败: { $error }
webhook-notify-prompt-save-failed = 保存 { $path } 失败: { $error }
webhook-notify-prompt-updated = 已更新提示词模板 `{ $hook_id }`。
webhook-notify-prompt-saved = 已保存提示词模板 `{ $hook_id }`。
webhook-notify-prompt-deleted = 已删除提示词模板 `{ $hook_id }`。
webhook-notify-prompt-delete-failed = 删除 { $path } 失败: { $error }
webhook-notify-trick-webhook-disabled = 配置中 Webhook 已禁用。
webhook-notify-trick-agents-disabled = 配置中代理 Webhook 端点已禁用。
webhook-notify-trick-gateway-not-running = 网关未运行。
webhook-notify-trick-info-unavailable = 网关运行时信息不可用。

## Gateway panel main view
gw-subtitle = 管理 GUI 运行时使用的嵌入式网关服务。
gw-loading = 加载中...
gw-status-unavailable = 网关状态不可用: { $error }
gw-btn-retry = { $icon } 重试
gw-btn-refresh = { $icon } 刷新
gw-btn-config = { $icon } 配置
gw-btn-start = { $icon } 启动
gw-btn-restart = { $icon } 重启
gw-status-refreshed = 网关状态已刷新
gw-tailscale-status-refreshed = Tailscale 状态已刷新
gw-notify-started = 网关已启动
gw-notify-started-at = 网关已启动于 { $url }
gw-notify-restarted = 网关已重启
gw-notify-restarted-at = 网关已重启于 { $url }
gw-notify-tailscale-mode-set = Tailscale 模式已设置为 { $mode }
gw-notify-load-failed = 加载网关状态失败: { $error }
gw-notify-tailscale-refresh-failed = 刷新 Tailscale 状态失败: { $error }
gw-notify-start-failed = 启动网关失败: { $error }
gw-notify-restart-failed = 重启网关失败: { $error }
gw-notify-tailscale-mode-failed = 设置 Tailscale 模式失败: { $error }
gw-notify-worker-closed = 网关请求处理线程意外关闭
gw-notify-config-store-unavailable = 配置存储不可用
gw-notify-config-saved = 网关配置已保存
gw-notify-config-saved-restart = 网关配置已保存。请重启网关以应用变更。
gw-notify-config-reloaded = 配置已从磁盘重新加载
gw-notify-save-failed = 保存失败: { $error }
gw-notify-reload-failed = 重载失败: { $error }

## Gateway status labels
gw-status-configured = 已配置
gw-status-enabled = 已启用
gw-status-disabled = 已禁用
gw-status-runtime = 运行状态
gw-status-running = 运行中
gw-status-stopped = 已停止
gw-status-transition = 过渡状态
gw-status-busy = 处理中
gw-status-idle = 空闲
gw-status-auth = 认证
gw-status-auth-configured = 已配置
gw-status-auth-not-configured = 未配置
gw-status-listen-ip = 监听 IP
gw-status-configured-port = 配置端口
gw-status-actual-port = 实际端口
gw-status-address = 地址
gw-status-started-at = 启动时间

## Gateway Tailscale section
gw-ts-heading = Tailscale
gw-ts-subtitle = 通过 Tailscale Serve（仅 tailnet）或 Funnel（公共互联网）暴露网关。
gw-ts-mode = 模式
gw-ts-mode-off = 关闭
gw-ts-mode-serve = Serve（仅 tailnet）
gw-ts-mode-funnel = Funnel（公共）
gw-ts-mode-apply-disabled = 已禁用
gw-ts-mode-apply-serve = serve（仅 tailnet）
gw-ts-mode-apply-funnel = funnel（公共）
gw-btn-refresh-ts = { $icon } 刷新 Tailscale
gw-btn-apply = 应用
gw-ts-host-status = 主机状态
gw-ts-host-status-label = 状态
gw-ts-host-connected = 已连接
gw-ts-host-disconnected = 已断开
gw-ts-host-version = 版本
gw-ts-host-backend-state = 后端状态
gw-ts-host-dns-name = DNS 名称
gw-ts-host-tailnet-url = Tailnet URL
gw-ts-host-message = 主机消息
gw-ts-gateway-exposure = 网关暴露
gw-ts-gateway-url = 网关 URL
gw-ts-message = 消息
gw-ts-funnel-no-auth-warning = ⚠️ Funnel 会将网关暴露在公网。请配置 gateway.auth 以保护网关。

## Gateway Config window
gw-cfg-title = 网关配置
gw-cfg-basic = 基本
gw-cfg-enabled = 已启用
gw-cfg-enabled-hint = 启用或禁用网关服务。
gw-cfg-listen-ip = 监听 IP
gw-cfg-listen-ip-hint = 网关绑定的 IP 地址。使用 0.0.0.0 监听所有接口。
gw-cfg-listen-port = 监听端口
gw-cfg-listen-port-hint = 网关的端口号。0 表示自动选择。
gw-cfg-port-auto = (0 = 自动)
gw-cfg-auth = 认证
gw-cfg-auth-enabled = 已启用
gw-cfg-auth-enabled-hint = 要求网关连接使用认证令牌。
gw-cfg-auth-token = 令牌
gw-cfg-auth-token-hint = 用于认证网关客户端的密钥令牌。
gw-btn-generate = 生成
gw-notify-auth-token-empty = 网关认证令牌为空
gw-notify-auth-token-copied = 网关认证令牌已复制
gw-btn-reload = 重载
gw-btn-save = 保存

## 心跳面板
hb-btn-refresh = { $icon } 刷新
hb-btn-add = { $icon } 添加心跳任务
hb-btn-config = { $icon } 配置
hb-label-jobs = 任务: { $count }
hb-label-running = 正在运行心跳...
hb-filter-start-date = 开始日期
hb-filter-end-date = 结束日期
hb-filter-page = 页码
hb-filter-size = 每页数量
hb-col-id = ID
hb-col-session = 会话
hb-col-channel = 通道
hb-col-enabled = 启用
hb-col-every = 间隔
hb-col-recent-msgs = 近期消息
hb-col-next-run = 下次运行时间
hb-col-last-run = 上次运行时间
hb-col-updated-at = 更新时间
hb-enabled-yes = 是
hb-enabled-no = 否
hb-no-rows = 数据库中未找到心跳任务。
hb-ctx-runs = { $icon } 运行记录
hb-ctx-run-now = { $icon } 立即运行
hb-ctx-edit = { $icon } 编辑
hb-ctx-disable = { $icon } 禁用
hb-ctx-enable = { $icon } 启用
hb-ctx-delete = { $icon } 删除
hb-ctx-copy-id = { $icon } 复制 ID
hb-delete-title = 删除心跳任务
hb-delete-prompt = 确定删除心跳任务 '{ $id }' 吗？
hb-delete-btn = 删除
hb-delete-cancel = 取消
hb-form-title-edit = 编辑心跳任务
hb-form-title-add = 添加心跳任务
hb-form-id = ID
hb-form-session-key = 会话密钥
hb-form-session-select = 选择一个会话
hb-form-channel = 通道
hb-form-chat-id = 聊天 ID
hb-form-enabled = 启用
hb-form-enabled-hint = 启用或禁用此心跳任务。
hb-form-every = 间隔
hb-form-every-hint = 心跳执行的间隔时间（例如 30m、1h、2h）。
hb-form-timezone = 时区
hb-form-silent-ack-token = 静默确认令牌
hb-form-silent-ack-token-hint = 用于识别静默确认的令牌。
hb-form-recent-messages = 近期消息数
hb-form-recent-messages-hint = 心跳上下文中包含的近期消息数量。
hb-form-no-sessions = 未找到已索引的会话。心跳任务必须指向一个现有会话。
hb-form-save = 保存
hb-form-cancel = 取消
hb-runs-title = 心跳运行记录: { $id }
hb-runs-refresh = 刷新运行记录
hb-runs-run-now = 立即运行
hb-runs-no-rows = 未找到心跳运行记录。
hb-runs-col-id = 运行 ID
hb-runs-col-status = 状态
hb-runs-col-scheduled = 计划时间
hb-runs-col-started = 开始时间
hb-runs-col-finished = 完成时间
hb-runs-col-error = 错误
hb-status-pending = 待执行
hb-status-running = 运行中
hb-status-success = 成功
hb-status-failed = 失败
hb-config-title = 心跳配置
hb-config-form-defaults = 表单默认值
hb-config-enabled-default = 默认启用
hb-config-enabled-default-hint = 新建的心跳任务将默认启用。
hb-config-recent-messages = 近期消息数
hb-config-info = 仅默认启用状态和近期消息窗口保存在 GUI 本地。\n其他心跳字段使用内置默认值。
hb-notify-sessions-failed = 加载会话列表失败: { $error }
hb-notify-jobs-failed = 加载心跳任务列表失败: { $error }
hb-notify-runs-failed = 加载心跳运行记录失败: { $error }
hb-notify-form-unavailable = 心跳表单不可用
hb-notify-id-empty = 心跳 ID 不能为空
hb-notify-session-empty = 会话密钥不能为空
hb-notify-channel-empty = 通道不能为空
hb-notify-chat-id-empty = 聊天 ID 不能为空
hb-notify-every-empty = 间隔不能为空
hb-notify-ack-token-empty = 静默确认令牌不能为空
hb-notify-recent-msgs-zero = 近期消息数必须大于零
hb-notify-timezone-empty = 时区不能为空
hb-notify-updated = 心跳任务已更新
hb-notify-update-failed = 更新心跳任务失败: { $error }
hb-notify-created = 心跳任务已创建
hb-notify-create-failed = 创建心跳任务失败: { $error }
hb-notify-enabled = 心跳已启用
hb-notify-disabled = 心跳已禁用
hb-notify-set-enabled-failed = 设置启用状态失败: { $error }
hb-notify-deleted = 心跳任务已删除
hb-notify-delete-failed = 删除心跳任务失败: { $error }
hb-notify-already-running = 心跳运行已在进行中
hb-notify-running-bg = 正在后台运行心跳 '{ $id }'...
hb-notify-executed = 心跳已执行: { $id }
hb-notify-run-failed = 立即运行心跳失败: { $error }

## 审批面板
approval-notify-filters-failed = 加载筛选器失败: { $error }
approval-notify-list-failed = 加载审批列表失败: { $error }
approval-notify-resolved = 审批 { $id } 已更新
approval-notify-resolve-failed = 更新审批失败: { $error }
approval-notify-consumed = 审批 { $id } 已消费
approval-notify-consume-failed = 审批 { $id } 未被消费
approval-notify-consume-op-failed = 消费审批失败: { $error }
approval-btn-refresh = { $icon } 刷新
approval-label-count = 审批: { $count }
approval-filter-session-key = 会话密钥
approval-filter-session-key-all = 全部
approval-filter-tool-name = 工具名称
approval-filter-tool-name-all = 全部
approval-filter-status = 状态
approval-filter-status-all = 全部
approval-filter-preview = 预览
approval-filter-page = 页码
approval-filter-size = 每页数量
approval-col-id = ID
approval-col-session = 会话
approval-col-tool = 工具
approval-col-risk = 风险
approval-col-status = 状态
approval-col-requested-by = 请求人
approval-col-approved-by = 审批人
approval-col-expires-at = 过期时间
approval-col-preview = 预览
approval-status-pending = 待审批
approval-status-approved = 已批准
approval-status-rejected = 已拒绝
approval-status-expired = 已过期
approval-status-consumed = 已消费
approval-no-rows = 未找到审批记录。
approval-ctx-view = { $icon } 查看
approval-ctx-approve = { $icon } 批准
approval-ctx-reject = { $icon } 拒绝
approval-ctx-consume = { $icon } 消费
approval-ctx-copy-id = { $icon } 复制 ID
approval-detail-title = 审批: { $id }
approval-detail-id = ID:
approval-detail-session = 会话:
approval-detail-tool = 工具:
approval-detail-risk-level = 风险等级:
approval-detail-status = 状态:
approval-detail-requested-by = 请求人:
approval-detail-approved-by = 审批人:
approval-detail-justification = 理由:
approval-detail-expires-at = 过期时间:
approval-detail-created-at = 创建时间:
approval-detail-updated-at = 更新时间:
approval-detail-consumed-at = 消费时间:
approval-detail-command-preview = 命令预览:
approval-detail-command-text = 命令文本:
approval-detail-na = -

## Session panel
sess-btn-refresh = { $icon } 刷新
sess-btn-clean = { $icon } 清理
sess-label-count = 会话: { $count }
sess-filter-start-date = 开始日期
sess-filter-end-date = 结束日期
sess-filter-channel = 通道
sess-filter-channel-all = 全部
sess-filter-page = 页码
sess-filter-size = 每页数量
sess-col-session-key = 会话密钥
sess-col-chat-id = 聊天 ID
sess-col-channel = 通道
sess-col-active-session = 活动会话
sess-col-provider = 提供商
sess-col-model = 模型
sess-col-turns = 对话轮次
sess-col-input = 输入
sess-col-output = 输出
sess-col-total = 总计
sess-col-jsonl-path = JSONL 路径
sess-sort-updated-asc = 更新时间 ↑
sess-sort-updated-desc = 更新时间 ↓
sess-sort-created-desc = 创建时间 ↓
sess-no-rows = 未找到会话。
sess-ctx-view-chat = { $icon } 查看聊天
sess-ctx-copy-key = { $icon } 复制会话密钥
sess-chat-title = 聊天: { $key }
sess-clean-title = 清理会话
sess-clean-desc = 删除指定日期之前更新的 cron/webhook 会话。
sess-clean-updated-before = 更新时间早于
sess-clean-session-types = 会话类型
sess-clean-type-cron = cron
sess-clean-type-webhook = webhook
sess-clean-hint = 请选择日期和至少一种会话类型以继续。
sess-clean-btn = 清理
sess-clean-cancel = 取消
sess-clean-progress-title = 正在清理会话
sess-clean-progress-label = 正在清理过期的 cron/webhook 会话...
sess-clean-progress-total = 总计: { $count }
sess-clean-progress-deleted = 已删除: { $count }
sess-clean-progress-bar = { $deleted } / { $total }
sess-clean-progress-footer = 清理完成后此对话框将自动关闭。
sess-clean-already-running = 会话清理已在进行中。
sess-clean-validation-error = 请选择更新时间日期和至少一种会话类型。
sess-notify-list-failed = 加载会话列表失败: { $error }
sess-notify-chat-failed = 加载聊天记录失败: { $error }
sess-notify-clean-success = 已清理 { $sessions } 个会话，删除 { $files } 个 JSONL 文件（{ $missing } 个已不存在）。
sess-notify-clean-failed = 清理会话失败: { $error }
sess-notify-clean-disconnected = 清理会话失败：清理任务意外停止

## Cron panel
cron-btn-refresh = { $icon } 刷新
cron-btn-add = { $icon } 添加定时任务
cron-label-total = 总计: { $count }
cron-label-running = 运行中: { $id }
cron-filter-name = 名称
cron-filter-kind = 类型
cron-filter-kind-all = 全部
cron-filter-kind-cron = cron
cron-filter-kind-every = every
cron-filter-created-from = 创建起始
cron-filter-created-to = 创建截止
cron-filter-page = 页码
cron-filter-size = 每页数量
cron-sort-updated-desc = 更新时间 ↓
cron-sort-created-desc = 创建时间 ↓
cron-sort-updated-asc = 更新时间 ↑
cron-sort-created-asc = 创建时间 ↑
cron-col-id = ID
cron-col-name = 名称
cron-col-kind = 类型
cron-col-expr = 表达式
cron-col-enabled = 启用
cron-col-next-run = 下次运行时间
cron-col-last-run = 上次运行时间
cron-col-updated-at = 更新时间
cron-no-rows = 数据库中未找到定时任务。
cron-ctx-runs = { $icon } 运行记录
cron-ctx-run-now = { $icon } 立即运行
cron-ctx-edit = { $icon } 编辑
cron-ctx-disable = { $icon } 禁用
cron-ctx-enable = { $icon } 启用
cron-ctx-delete = { $icon } 删除
cron-ctx-copy-id = { $icon } 复制 ID
cron-delete-title = 删除定时任务
cron-delete-prompt = 确定删除定时任务 '{ $id }' 吗？
cron-delete-btn = 删除
cron-delete-cancel = 取消
cron-form-title-edit = 编辑定时任务
cron-form-title-add = 添加定时任务
cron-form-id = ID
cron-form-name = 名称
cron-form-schedule-kind = 调度类型
cron-form-schedule-expr = 调度表达式
cron-form-schedule-expr-hint = 调度表达式（如 5m 表示 every 间隔，或标准 cron 格式）。
cron-form-timezone = 时区
cron-form-enabled = 启用
cron-form-enabled-hint = 启用或禁用此定时任务。
cron-form-payload = Payload JSON
cron-form-payload-hint = 符合 InboundMessage 格式的 JSON 载荷。
cron-form-save = 保存
cron-form-cancel = 取消
cron-runs-title = 任务运行记录: { $id }
cron-runs-refresh = 刷新运行记录
cron-runs-run-now = 立即运行
cron-runs-no-rows = 未找到任务运行记录。
cron-runs-col-id = 运行 ID
cron-runs-col-status = 状态
cron-runs-col-scheduled = 计划时间
cron-runs-col-started = 开始时间
cron-runs-col-finished = 完成时间
cron-runs-col-error = 错误
cron-status-pending = 待执行
cron-status-running = 运行中
cron-status-success = 成功
cron-status-failed = 失败
cron-kind-cron = cron
cron-kind-every = every
cron-enabled-yes = 是
cron-enabled-no = 否
cron-notify-executed = 定时任务已执行: { $id }
cron-notify-run-failed = 立即运行定时任务失败: { $error }
cron-notify-list-failed = 加载定时任务列表失败: { $error }
cron-notify-runs-failed = 加载任务运行记录失败: { $error }
cron-notify-form-unavailable = 定时任务表单不可用
cron-notify-id-empty = 定时任务 ID 不能为空
cron-notify-name-empty = 定时任务名称不能为空
cron-notify-expr-empty = 调度表达式不能为空
cron-notify-payload-empty = Payload JSON 不能为空
cron-notify-payload-invalid = Payload JSON 格式无效: { $error }
cron-notify-payload-invalid-schema = Payload JSON 必须是有效的 InboundMessage 对象: { $error }
cron-notify-timezone-empty = 时区不能为空
cron-notify-schedule-invalid = 调度表达式无效: { $error }
cron-notify-updated = 定时任务已更新
cron-notify-update-failed = 更新定时任务失败: { $error }
cron-notify-created = 定时任务已创建
cron-notify-create-failed = 创建定时任务失败: { $error }
cron-notify-enabled = 定时任务已启用
cron-notify-disabled = 定时任务已禁用
cron-notify-set-enabled-failed = 设置启用状态失败: { $error }
cron-notify-deleted = 定时任务已删除
cron-notify-delete-failed = 删除定时任务失败: { $error }
cron-notify-already-running = 定时任务运行已在进行中
cron-notify-running-bg = 正在后台运行定时任务 '{ $id }'...
