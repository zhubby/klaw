language = 语言
menu-file = 文件
menu-window = 窗口
menu-connection = 连接
menu-help = 帮助
menu-settings = 设置
menu-tile-windows = 平铺窗口
menu-reset-layout = 重置布局
menu-gateway-token = 网关令牌
menu-reconnect = 重新连接
menu-connect = 连接
menu-disconnect = 断开连接
menu-about = 关于
status-connected = 已连接
status-connecting = 连接中...
status-disconnected = 未连接
status-error = 错误

## 设置对话框
settings-title = 设置
settings-general = 常规设置
settings-current-theme-mode = 当前主题模式：{$mode}
settings-theme-mode = 主题模式
settings-light-theme = 亮色主题
settings-dark-theme = 暗色主题
settings-theme-default-hint = 默认使用 egui 自带的亮色/暗色外观。

## 关于对话框
about-title = 关于 Klaw
about-version = 版本 {$version}
about-close = 关闭

## 资源预览
archive-preview-title = 资源预览
archive-preview-close = 关闭
archive-preview-download = 下载

## 会话侧边栏
session-list-heading = 代理
session-list-empty = 暂无代理。
session-visible = 窗口可见
session-hidden = 窗口隐藏
session-rename = 重命名
session-copy-id = 复制 ID
session-delete = 删除
session-id-copied = 代理 ID 已复制

## 重命名对话框
rename-title = 重命名代理
rename-hint = 代理名称
rename-save = 保存
rename-cancel = 取消

## 网关对话框
gateway-title = 网关令牌
gateway-hint = 如果网关认证已启用，请在此输入令牌。
gateway-blank-hint = 认证未启用时留空即可。
gateway-token-hint = 网关令牌
gateway-save-reconnect = 保存并重新连接
gateway-clear = 清除

## 删除对话框
delete-title = 删除代理
delete-confirmation = 确定永久删除代理「{$agent_name}」？此操作不可撤销。
delete-confirm = 删除
delete-cancel = 取消

## 输入区
composer-slash-hint = 输入 / 打开命令补全。
composer-connected-hint = 给 Klaw 发消息…
composer-connecting-hint = 正在连接 Klaw…
composer-disconnected-hint = 请重新连接后再给 Klaw 发消息…
composer-error-hint = 请修复连接后继续对话…
upload = 上传
upload-hover = 上传并附加文件
file-count = 文件 ({ $count })
file-count-hover = 显示已上传文件
send = 发送
model-hint = 模型
provider-hint = 提供商
selecting-file = 选择文件…
uploading = 上传中…
send-card-failed = 发送卡片操作失败。

## 思考占位符
thinking = 思考中…
assistant-label = Klaw

## History loading states
history-loading-title = 正在加载对话历史…
history-loading-body = 正在从 Klaw 网关获取消息。
history-page-loading = 正在加载更早的消息…

## 工作区 / 连接引导
workbench-connect-heading = 连接到 Klaw 网关
workbench-connect-body = 请先成功连接后再加载代理。
workbench-connect-button = 连接
workbench-loading = 正在从 Klaw 网关加载代理…
workbench-no-agents = 暂无代理。点击新建代理开始。
workbench-heading = 代理工作区
workbench-subheading = 每个代理以独立窗口打开。

## 状态栏
statusbar-theme-mode = 主题模式
statusbar-agents = 代理：{$total}/{ $open }
statusbar-agents-hover = 代理总数 / 当前打开的窗口数。
statusbar-stream = 流式
statusbar-stream-on-hover = 开启：实时流式回复。关闭：等待完整回复后淡入显示。
statusbar-fps-hover = 基于最新 egui 帧间隔的近似实时帧率。
statusbar-activity-hover = 当前活跃代理的活动状态。
statusbar-messages-hover = 活跃代理窗口中已加载的消息数。
statusbar-no-active-agent = 无活跃代理

## 空状态（连接状态）
empty-connected-title = 开始与 Klaw 对话
empty-connected-body = 在下方发送消息开始此对话。
empty-connecting-title = 正在连接 Klaw
empty-connecting-body = 正在等待聊天服务上线。
empty-disconnected-title = 重新连接 Klaw
empty-disconnected-body = 请从工具栏重新连接，然后发送您的下一条消息。
empty-error-title = 连接错误
empty-error-body = Klaw 无法保持聊天连接：{$error}

## 卡片消息
card-approval-badge = 审批
card-question-badge = 问题
card-approval-title = 需要审批
card-question-title = 问题
card-command-label = 命令
card-approval-id = 审批 ID：{$id}
card-selected-answer = 已选择：{$answer}

## 文件对话框
file-dialog-title = 已上传文件
file-dialog-hint = 右键点击行可预览或从当前页面移除。
file-dialog-empty = 无已上传文件。
file-dialog-col-name = 文件名
file-dialog-col-archive-id = 存档 ID
file-dialog-col-size = 大小

## 附件上下文菜单
attachment-preview = 预览
attachment-download = 下载
attachment-delete = 删除

## Role labels
role-you = 你
role-system = 系统

## Card interaction completed labels
card-approved = 已审批
card-rejected = 已拒绝

## Archive hover texts
archive-hover-preview = 预览存档资源
archive-hover-download = 下载存档资源

## Archive preview loading/unavailable
archive-preview-loading = 正在加载预览...
archive-preview-unavailable = 此文件类型不支持预览。

## Session route labels
route-default = 路由：默认
route-provider = 路由：{$provider}
route-model = 路由：{$model}
route-provider-model = 路由：{$provider}/{ $model }

## Session activity labels
activity-history = 历史
activity-uploading = 上传中
activity-picking-file = 选择文件
activity-streaming = 流式传输中
activity-files-ready = 文件就绪

## Status bar message count
statusbar-messages = {$count} 条消息
