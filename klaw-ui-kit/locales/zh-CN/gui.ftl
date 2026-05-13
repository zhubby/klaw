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
