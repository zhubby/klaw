# klaw-webui Global WebSocket Workspace Design

## Goal

将 `klaw-webui` 从当前的“每个 agent 自带连接能力、本地先创建 session 再尝试连接”的模型，收敛为：

- `websocket` 是整个 `webapp` 的全局单例连接
- 页面中的 agent 列表与内容完全依赖 websocket 返回的数据
- 未连接时不展示 agent 工作区，也不允许创建新的 agent
- agent 列表始终按会话创建时间倒序排列，且不受激活、置顶、发送消息等 UI 操作影响

本次设计目标不是只改一层 UI 文案，而是统一产品语义、协议模型和前端状态归属，让 Web 端的 agent 工作区成为真正的“服务端驱动工作区”。

## Confirmed Direction

- 连接语义：`websocket` 属于 `webapp`，不属于单个 agent
- 渲染语义：agent 内容必须依赖 websocket 中获取的数据
- 未连接行为：页面只显示连接引导，不显示 agent 内容
- 新建行为：未连接时不允许创建 agent
- 入口设计：顶部 `Connection` 菜单和主区域 `Connect` 按钮同时保留
- 排序规则：始终按 agent 创建会话时间倒序

## Scope

### In Scope

- 将 `klaw-webui` 的 websocket 生命周期改为全局单例
- 移除每个 agent 窗口自身的 `Connect` / `Disconnect` 操作
- 将 agent 列表与内容改为服务端驱动
- 为网关 websocket 协议补充工作区初始化和创建会话能力
- 固定 agent 列表排序为创建时间倒序
- 收紧本地持久化边界，避免本地伪造 agent 工作区
- 为关键前后端行为补充回归测试

### Out of Scope

- 大规模重做聊天视觉风格
- 重新设计消息 markdown 渲染
- 引入分页历史、搜索、标签、归档等高级会话管理功能
- 多标签页同步策略
- 服务端权限模型重构

## Existing Code Anchors

- `klaw-webui/src/web_chat/app.rs`：当前 `ChatApp` 状态、session 创建、持久化入口
- `klaw-webui/src/web_chat/session.rs`：当前 session/window 结构与标题生成
- `klaw-webui/src/web_chat/transport.rs`：当前 websocket 建连、订阅、发送逻辑
- `klaw-webui/src/web_chat/ui.rs`：顶部菜单、agent 列表、agent 窗口渲染
- `klaw-webui/src/web_chat/storage.rs`：当前本地工作区持久化
- `klaw-gateway/src/websocket.rs`：`/ws/chat` 方法与事件处理

## Alternatives Considered

### A. 保留 per-agent websocket，仅把 UI 伪装成全局连接

优点：

- 改动较小
- 可以较快移除窗口级按钮

缺点：

- 语义仍然错误，握手依旧发生在单 agent 级别
- 排序、断线恢复、错误提示仍会被“每个 agent 一条连接”拖累
- 无法满足“agent 内容必须依赖 websocket 拉取”的目标

### B. 全局 websocket，但本地仍预建 agent 壳子

优点：

- 比 A 更接近正确模型
- 前端改动面相对温和

缺点：

- 页面仍存在“本地先有 agent，后面再补数据”的虚假中间态
- 与用户确认的“页面中的 agent 内容必须依赖 websocket 中获取数据”冲突

### C. 全局 websocket + 服务端驱动工作区

优点：

- 语义正确，产品行为与数据来源一致
- 未连接、连接中、已连接的状态边界清晰
- 排序、恢复、错误处理都更稳定
- 为后续多端同步、服务端重命名、历史加载打下正确基础

缺点：

- 需要补充 websocket 协议
- 需要重构前端状态边界

### Recommended Option

采用方案 C。

这是唯一同时满足以下条件的方案：

- 连接属于 `webapp`
- agent 内容依赖 websocket 数据
- 未连接时不显示 agent 工作区
- 排序按创建时间稳定输出

## Design Summary

### 1. Product Model

系统中的“连接”和“会话”将严格分层：

- 全局连接层：由 `ChatApp` 持有，表示当前页面是否已接入 `Klaw Gateway`
- 会话工作区层：由服务端返回的 session 列表和消息内容驱动，表示当前用户可见的 agent 工作区

agent 不再拥有独立连接能力。agent 只是一个被服务端创建、被前端展示和交互的会话容器。

### 2. UI States

页面只存在以下四种主状态：

1. `Disconnected`
2. `Connecting`
3. `Bootstrapping`
4. `Ready`

以及一个错误分支：

- `Error(String)`：表示连接或初始化失败

状态行为如下：

#### `Disconnected`

- 只显示连接引导页
- 顶部保留 `Connection` 菜单
- 主区显示 `Connect` 按钮
- 不显示 agent 列表
- 不显示 agent 窗口
- `New Agent` 禁用

#### `Connecting`

- 仍显示连接引导页
- 明确提示 websocket 正在连接
- 禁止所有 agent 相关操作

#### `Bootstrapping`

- websocket 握手已成功
- 页面仍不显示 agent 工作区
- 等待从服务端加载工作区初始数据
- 若初始化失败，留在全局错误态并允许重试

#### `Ready`

- 渲染 agent 列表
- 渲染 agent 工作区
- 允许创建新 agent
- 允许发送消息

#### `Error(String)`

- 使用连接引导页承载错误展示
- 明确区分：
  - 握手失败
  - token 校验失败
  - workspace bootstrap 失败
- 保持 agent 工作区不可见

### 3. Top-Level Layout

页面布局仍可保持 `egui` 现有的：

1. `TopBottomPanel::top`：工具栏
2. `SidePanel::left`：agent 列表，仅在 `Ready` 时显示
3. `CentralPanel`：未连接引导页或工作区
4. `TopBottomPanel::bottom`：状态栏

关键改变不是布局容器本身，而是“哪些区域在什么状态下可见”：

- `Disconnected` / `Connecting` / `Bootstrapping` / `Error`：只显示工具栏、状态栏和中央引导页
- `Ready`：才显示 agent 列表与 agent 窗口

### 4. Agent Window Behavior

每个 agent 窗口应移除所有连接级操作：

- 删除窗口顶部 `Connect`
- 删除窗口顶部 `Disconnect`
- 删除窗口自己的连接状态文本和状态灯

窗口顶部仅保留会话级信息，例如：

- `title`
- `session_key`
- 可选的轻量状态提示，如该会话正在加载历史

消息发送能力依赖于全局连接状态：

- 只有 `Ready` 且全局 websocket 可用时，输入框和发送按钮才可用
- 如果全局连接断开，整个页面退回连接引导，不保留旧工作区内容展示

### 5. Fixed Session Ordering

agent 列表顺序必须由服务端会话元数据决定，而不是由前端数组“谁最后被点击/置顶”决定。

排序规则：

- 主排序：`created_at_ms desc`
- 次排序：`session_key desc`，仅作为同时间戳的稳定兜底

必须明确禁止以下行为改变列表顺序：

- `focus_session()`
- `bring_session_to_front()`
- 打开发送窗口
- 发送消息
- 窗口显示/隐藏
- 当前 active session 变化

列表顺序是业务顺序。
窗口前后层级是展示顺序。
这两个概念必须解耦。

## Protocol Changes

当前 websocket 协议只覆盖：

- `session.subscribe`
- `session.unsubscribe`
- `session.submit`

这不足以支撑“服务端驱动工作区”。本次需要新增两个必需方法，以及一个推荐方法。

### 1. `workspace.bootstrap`

用途：

- 在 websocket 握手成功后，初始化当前页面所需的工作区元数据

建议返回：

```json
{
  "sessions": [
    {
      "session_key": "web:123",
      "title": "Agent 2",
      "created_at_ms": 1712726400000
    }
  ],
  "active_session_key": "web:123"
}
```

要求：

- 返回结果已按 `created_at_ms desc` 排序，前端可再做防御性排序
- 不要求在 bootstrap 中返回完整历史消息
- 如果没有会话，返回空数组而不是本地补默认 agent

### 2. `session.create`

用途：

- 在全局连接建立后，由前端显式请求创建一个新 agent 会话

建议参数：

```json
{
  "title": null
}
```

建议返回：

```json
{
  "session_key": "web:456",
  "title": "Agent 3",
  "created_at_ms": 1712726500000
}
```

要求：

- 服务端负责决定最终标题与创建时间
- 服务端返回的数据必须足够让前端立即插入列表
- 创建成功后前端自动对该 session 执行 `session.subscribe`

### 3. `session.history` 或在 `session.subscribe` 中附带历史

用途：

- 加载单个会话的初始消息内容

推荐做法：

- 保持 `workspace.bootstrap` 只拉会话元数据
- 在 `session.subscribe` 成功后推送该会话历史，或单独新增 `session.history`

推荐方向：

- 若当前网关代码想保持最小协议面，优先让 `session.subscribe` 成功后附带历史
- 若希望长期接口边界更清晰，则新增 `session.history`

本设计接受两者任一实现，但要求：

- agent 窗口的内容必须来自 websocket 协议返回
- 前端不得用本地伪造历史填充会话

## Frontend State Design

### 1. `ChatApp` Global State

`ChatApp` 应成为唯一连接状态持有者，新增或重组为以下状态：

- `connection_state`
- `workspace_state`
- `ws`
- `gateway_token`
- `sessions`
- `active_session_key`
- `window_layout_by_session_key`
- `drafts_by_session_key`

建议新增两个枚举：

```text
GlobalConnectionState = Disconnected | Connecting | Connected | Error(String)
WorkspaceLoadState = NotLoaded | Loading | Ready | Error(String)
```

也可以合并为一个更高层的 UI 状态，但必须避免把“已连接但未 bootstrap 完成”和“已完全就绪”混为一谈。

### 2. `SessionWindow` Responsibility

`SessionWindow` 应只保留会话展示与编辑所需的数据：

- `session_key`
- `title`
- `created_at_ms`
- `open`
- `window_anchor`
- `messages`
- `draft`
- `markdown_cache`

应删除的 per-agent 连接状态：

- `buffers.ws`
- `buffers.state`
- `buffers.auth_verified`
- `buffers.suppress_next_close_notice`
- `buffers.active_stream_request_id` 中与连接强绑定的部分

若流式消息仍需要按会话追踪进行中的请求，可保留“按 `session_key` 记录的流式请求 id”，但它必须由全局 websocket 事件分发驱动，而不是由 session 自己持有连接。

### 3. Persistence Boundary

为了满足“agent 内容必须依赖 websocket 获取”，本地持久化必须收紧。

建议本地只保留：

- `gateway_token`
- 主题
- 可选的 `active_session_key`
- 可选的窗口布局信息

建议移除本地持久化：

- `sessions`
- `next_session_number`
- 本地生成的默认 agent

刷新页面后的正确流程应是：

1. 读取 token / 主题 / 可选布局
2. 建立全局 websocket
3. 调用 `workspace.bootstrap`
4. 根据服务端返回渲染 agent 工作区

如果 bootstrap 返回空会话，页面应显示“暂无 agent，可创建”的已连接空态，而不是本地自动生成 `Agent 1`。

## Data Flow

### 1. Page Startup

页面启动后：

1. 读取本地 `gateway_token`
2. 若有 token，可尝试自动建立 websocket
3. 建连成功后发送 `workspace.bootstrap`
4. bootstrap 成功后进入 `Ready`
5. 根据返回的会话列表决定默认激活项

### 2. Create Agent

点击 `New Agent` 时：

1. 先验证当前状态为 `Ready`
2. 发送 `session.create`
3. 收到会话元数据后插入本地 `sessions`
4. 按 `created_at_ms desc` 重新排序
5. 自动 `session.subscribe`
6. 打开对应窗口并设为 active

### 3. Open Existing Agent

点击左侧 agent：

1. 若会话未打开，则打开窗口
2. 设为 active
3. 如尚未加载该会话历史，则触发订阅或历史加载

注意：

- 打开会话不会改变列表顺序
- 激活会话不会改变列表顺序

### 4. Send Message

发送消息时：

1. 依赖全局 websocket 发送 `session.submit`
2. 参数中显式包含 `session_key`
3. 流式回包按 `session_key` 路由到对应 session 消息列表

### 5. Connection Loss

若全局 websocket 关闭：

1. 清理全局 websocket 句柄
2. 将 UI 切回连接引导页
3. 不再展示旧的 agent 内容
4. 保留必要的本地轻量状态，如 token / 主题 / 布局

## UI Details

### 1. Toolbar

`Agent` 菜单：

- `New Agent`
  - 仅在 `Ready` 时可用
  - 在非 `Ready` 时禁用，并提供 hover 提示，例如“Connect to gateway first”

`Connection` 菜单：

- `Gateway Token`
- `Connect` 或 `Reconnect`
- 可选 `Disconnect`

推荐命名：

- 已断开：显示 `Connect`
- 已连接：显示 `Reconnect`

### 2. Connection Guide Screen

中央引导页建议包含：

- 标题：`Connect to Klaw Gateway`
- 次级说明：连接成功后才会加载 agents
- 主按钮：`Connect`
- 次级入口：打开 token 配置
- 如有错误，展示简洁明确的失败原因

### 3. Ready Empty State

若 bootstrap 成功但当前没有任何 session：

- 显示“还没有 agents”
- 显示“Create your first agent”按钮

这是唯一允许在工作区中没有 agent 的正常情况。

## Backend Design Notes

网关层需要补充以下责任：

- 为 websocket 会话提供工作区 bootstrap 能力
- 为前端提供会话创建能力
- 在 session 元数据中包含可靠的 `created_at_ms`
- 对会话列表做稳定排序或返回足够字段供前端排序

数据来源应尽量使用现有 session/storage 层，而不是在 websocket handler 中临时拼装非持久元数据。

若当前 `session` 模块尚未显式暴露 `created_at`，本次应补齐该字段的持久化与查询链路。

## Testing Strategy

本次是行为与状态模型升级，必须覆盖前后端两侧。

### Frontend Tests

应至少覆盖：

- 未连接时不显示 agent 列表
- 未连接时 `New Agent` 不可用
- 连接成功但 bootstrap 未完成时仍显示加载页
- bootstrap 返回会话后按 `created_at_ms desc` 排序
- 聚焦/打开/关闭 agent 不改变列表顺序
- 全局 websocket 断开后页面回退到连接引导页

优先给纯函数和状态转换逻辑加测试，例如：

- `sort_sessions_by_created_at`
- 由连接状态和加载状态推导页面模式的函数

### Gateway Tests

应至少覆盖：

- `workspace.bootstrap` 返回预期结构
- `workspace.bootstrap` 返回空会话时不报错
- `session.create` 创建成功并返回 `session_key/title/created_at_ms`
- 会话列表按创建时间倒序
- 未认证或连接未就绪时不能创建 session
- `session.subscribe` 可返回或触发会话历史加载

### Regression Tests

应补一组回归验证，证明：

- 页面刷新后不会从 local storage 恢复伪造 agent 列表
- 只有 websocket bootstrap 成功后才会渲染 agent 工作区

## Recommended Implementation Plan

推荐实现顺序如下：

1. 网关补充 `workspace.bootstrap`
2. 网关补充 `session.create`
3. 补齐 session 元数据中的 `created_at_ms`
4. 前端将 websocket 收敛为 `ChatApp` 全局单例
5. 删除 per-agent 连接状态与窗口级连接按钮
6. 收紧 `storage.rs` 的本地持久化字段
7. 将 agent 列表排序切换为 `created_at_ms desc`
8. 补齐前后端回归测试

这样拆分的原因：

- 先有协议，前端才能摆脱本地伪造状态
- 先有 `created_at_ms`，排序规则才有可靠依据
- 最后再收紧本地持久化，可避免中间阶段把页面做成不可用状态

## Risks

- 当前网关 websocket 协议较轻，新增 `workspace.bootstrap` 和 `session.create` 会触及前后端边界，必须保持参数与返回结构清晰稳定
- 如果现有 session 持久层没有可靠创建时间字段，本次会扩展到存储模型，风险高于纯 UI 改动
- 如果前端一次性移除本地持久化但服务端 bootstrap 尚未补齐，页面会在刷新后失去工作区数据来源，因此必须按阶段实施

## Acceptance Criteria

- websocket 连接语义提升为 `webapp` 级别，而不是 agent 级别
- 未连接时页面只显示连接引导，不显示 agent 列表和 agent 内容
- 未连接时不能创建新的 agent
- agent 窗口中不再出现 `Connect` / `Disconnect`
- agent 列表与会话内容来自 websocket 返回数据，而不是本地预构建
- agent 列表始终按创建时间倒序
- 激活、聚焦、置顶、发送消息不改变列表顺序
- 页面刷新后必须重新通过 websocket bootstrap 才能恢复工作区
