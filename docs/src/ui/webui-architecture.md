# Klaw WebUI 架构与设计

## 概述

Klaw WebUI 是一个**基于 egui + WASM 的浏览器端聊天客户端**，编译为 WebAssembly 后运行在浏览器中，通过 WebSocket 协议连接到后端 Klaw Gateway，提供多会话、多窗口的流式对话体验。

## 技术栈

| 层 | 技术 | 说明 |
|---|------|------|
| UI 框架 | **egui / eframe** | 立即模式 GUI，编译到 WASM |
| 渲染 | WebGL | 通过 eframe 绑定到浏览器 |
| 网络 | **WebSocket API** | 原生浏览器 WebSocket |
| 序列化 | **serde_json** | JSON 协议帧编解码 |
| 内存 | `Rc<RefCell<T>>` | 跨闭包共享可变状态 |
| 存储 | **localStorage** | 持久化工作区状态 |
| 部署 | 静态嵌入 | WASM 资源由 `klaw-gateway` 嵌入静态服务 |

## 核心设计目标

1. **零安装** - 通过浏览器直接访问，无需下载客户端
2. **多会话并行** - 支持同时打开多个会话窗口，平铺布局
3. **实时流式输出** - 增量接收 LLM 响应，即时渲染
4. **离线持久化** - 会话列表、认证令牌、窗口状态保存在浏览器 localStorage
5. **响应式布局** - 支持窗口拖拽缩放，自动平铺排列

## 模块结构

```rust
klaw-webui/src/
├── lib.rs                      # 入口，纯逻辑单元测试
└── web_chat/
    ├── mod.rs                  # 模块导出，start_chat_ui 入口
    ├── app.rs                 # 顶级应用状态 (ChatApp) 与业务逻辑
    ├── session.rs             # 会话窗口数据结构 (SessionWindow, ChatMessage)
    ├── ui.rs                  # UI 渲染：左侧边栏 + 聊天窗口 + 输入框
    ├── protocol.rs            # WebSocket 帧类型定义 (ClientFrame / ServerFrame)
    ├── transport.rs           # WebSocket 连接管理 + 帧处理
    ├── storage.rs             # localStorage 持久化（工作区状态）
    └── markdown.rs            # Markdown 缓存（egui 文本格式化）
```

## 核心数据结构

### ChatApp (顶级应用状态)

```rust
pub struct ChatApp {
    pub ctx: Context,
    pub gateway_token: Option<String>,   // 认证令牌
    pub gateway_token_input: String,      // 表单输入
    pub ws: Rc<RefCell<Option<WebSocket>>>, // WebSocket 连接
    pub connection_state: Rc<RefCell<ConnectionState>>, // 连接状态
    pub pending_frames: Rc<RefCell<Vec<ServerFrame>>>, // 待处理帧队列
    pub sessions: Vec<SessionWindow>,     // 所有会话窗口
    pub active_session_key: Option<String>, // 当前激活会话
    pub workspace_loaded: bool,           // 是否完成 bootstrap
    pub toasts: Rc<RefCell<NotificationCenter>>, // 通知中心
}
```

### SessionWindow (单个会话窗口)

```rust
pub struct SessionWindow {
    pub session_key: String,      // 服务端会话唯一标识
    pub title: String,            // 会话标题
    pub created_at_ms: i64,       // 创建时间戳
    pub draft: String,            // 用户输入草稿
    pub open: bool,               // 是否打开显示
    pub window_anchor: WindowAnchor, // 窗口位置
    pub buffers: SessionBuffers,  // 消息缓冲
    pub markdown_cache: MarkdownCache, // Markdown 格式化缓存
}

pub struct SessionBuffers {
    pub messages: Rc<RefCell<Vec<ChatMessage>>>,
    pub active_stream_request_id: Rc<RefCell<Option<String>>>,
    pub history_loaded: Rc<RefCell<bool>>,
}
```

### ConnectionState (连接状态机)

```rust
pub enum ConnectionState {
    Disconnected,    // 断开，需要用户点击连接
    Connecting,      // 正在握手
    Connected,       // 就绪，可以发送
    Error(String),   // 错误，携带消息
}
```

状态转换：

```
Disconnected → Connecting → Connected
       ↑↓            ↓↓            ↓↓
         ↑            ↓             ↓
       Error ← ← ← ← ↓ ← ← ← ← ← ← ↙
```

## 页面模式

```rust
pub enum PageMode {
    ConnectionGuide,    // 未连接，显示连接引导
    LoadingWorkspace,   // 已连接，正在 bootstrap
    Workspace,          // 就绪，显示会话列表 + 聊天窗口
}
```

派生逻辑：

```rust
derive_page_mode(connection_state, workspace_loaded) -> PageMode
```

- `Connected + workspace_loaded = true` → `Workspace`
- `Connected + workspace_loaded = false` → `LoadingWorkspace`
- 其他 → `ConnectionGuide`

## 数据流

### 启动流程

```
1. start_chat_ui (wasm-bindgen)
   ↓
2. ChatApp::new
   ↓
3. 从 localStorage 恢复：
   - gateway_token
   - sessions 列表
   - active_session_key
   ↓
4. 如果有预填充 token → 自动连接
   ↓
5. egui 进入渲染循环 → ChatApp::update
```

### 连接建立

```
用户点击 "Connect"
   ↓
connect_workspace()
   ↓
创建 WebSocket → ws://host/ws/chat?token=...
   ↓
onopen → 发送 workspace.bootstrap 请求
   ↓
服务端返回 sessions 列表
   ↓
sync_sessions_from_workspace()
   ↓
workspace_loaded = true → 进入 Workspace 模式
  ↓
subscribe_sessions_needing_history() → 仅为当前已打开且尚未初始化的窗口异步拉取服务端历史
```

### 发送消息

```
用户输入 → 点击 Send
   ↓
send_session_draft()
   ↓
添加 User 消息到 SessionBuffers.messages
   ↓
生成 request_id → 保存到 active_stream_request_id
   ↓
发送 session.submit 请求 (stream = true)
   ↓
清空 draft
```

### 接收流式响应

```
WebSocket 消息到达 → onmessage
   ↓
推入 pending_frames → request_repaint()
   ↓
process_pending_frames() 在主线程处理
   ↓
session.message 事件 → classify_stream_message_action()
   ↓
空内容 → IgnoreEmpty
已有同 request_id 的 Assistant 消息 → ReplaceLastAssistant
不存在 → PushAssistant
   ↓
egui 重绘 → 增量显示
   ↓
session.stream.done → 清空 active_stream_request_id
```

### 创建新会话

```
用户点击 "+ New Chat"
   ↓
create_session()
   ↓
发送 session.create 请求
   ↓
服务端返回新会话信息 → process_result_frame
   ↓
加入 sessions 列表 → 打开窗口 → 持久化
```

### 重命名会话

```
用户在侧边栏重命名
   ↓
rename_session()
   ↓
发送 session.update 请求
   ↓
服务端确认 → 更新本地标题 → 持久化
```

### 删除会话

```
用户确认删除
   ↓
delete_session()
   ↓
发送 session.delete 请求
   ↓
服务端确认 → 移除本地会话 → 持久化
```

## 布局设计

### 整体布局

```
┌─────────────────┬───────────────────────────────────┐
│  Session List   │  Window 1  │  Window 2  ...        │
│                 │                           ┌───────┐│
│                 │                           │Chat   ││
│  - Session 1    │                           │History ││
│  - Session 2    │                           │       ││
│  + New Chat     │                           │[Input] ││
│                 │                           └───────┘│
└─────────────────┴───────────────────────────────────┘
```

- **左侧边栏** (固定宽度 `240px`)：会话列表、新建按钮、主题切换、连接状态
- **主区域**：浮动会话窗口，支持拖动缩放，自动错开排列

###  stagger 排列策略

新窗口按 4 列交错排列：

```rust
const WINDOW_STAGGER_COLUMNS: u32 = 4;
let column = slot % WINDOW_STAGGER_COLUMNS;
let row = slot / WINDOW_STAGGER_COLUMNS;
x = START_X + column * OFFSET_X;
y = START_Y + row * OFFSET_Y;
```

保证每个新窗口不会完全遮挡前面的窗口，用户可以看到所有打开的会话。

### 会话窗口内布局

```
┌────────────────────────────────────┐
│  Title  ──── ✍  ✕                  │  (标题栏)
├────────────────────────────────────┤
│                                    │
│  Message 1 (User)                  │
│  Message 2 (Assistant)             │  (消息气泡区域，自动滚动)
│  ...                               │
│                                    │
├────────────────────────────────────┤
│  [Enter message...]  [Send]        │  (输入框 + 按钮)
└────────────────────────────────────┘
```

## 持久化设计

### localStorage 存储结构

```javascript
// 键: klaw-workspace-state
{
  "legacy_theme_mode": "system",
  "sessions": [
    { "session_key": "...", "open": true }, ...
  ],
  "active_session_key": "...",
  "gateway_token": "..."
}
```

### 持久化时机

| 操作 | 是否持久化 |
|------|------------|
| 连接成功拉取会话列表 | ✅ |
| 创建会话 | ✅ |
| 重命名会话 | ✅ |
| 删除会话 | ✅ |
| 打开/关闭会话窗口 | ✅ |
| 修改网关令牌 | ✅ |
| 滚动、拖动窗口 | ✅ (egui 内建) |
| 用户输入草稿 | ❌ (内存中) |

### 查询参数优先级

URL 查询参数 `?gateway_token=...` 优先于 localStorage：

```
resolve_gateway_token(query_token, persisted_token) -> Option<String>
1. query_token 非空 → 使用 query_token
2. 否则使用 persisted_token
```

这个设计支持：
- 分享带令牌的链接
- 刷新页面后保持登录

## 协议对齐

客户端 WebSocket 协议完全对齐 [Klaw Gateway WebSocket 协议](../websocket-channel.md)：

| 客户端方法 | 对应服务端事件 |
|------------|----------------|
| `workspace.bootstrap` | `session.connected` → `Result` |
| `session.create` → `Result` | |
| `session.update` → `Result` | |
| `session.delete` → `Result` | |
| `session.subscribe` | `session.subscribed` → 多个 `session.message` |
| `session.submit` (`stream=true`) | `session.message` (增量) → `session.stream.done` |

完整协议文档参见 [WebSocket Channel](./websocket-channel.md)。

## 流式消息分类算法

```rust
classify_stream_message_action(
    last_role: Option<MessageRole>,
    active_stream_request_id: Option<&str>,
    request_id: Option<&str>,
    content: &str,
) -> StreamMessageAction
```

规则：

1. **内容为空** → `IgnoreEmpty`
2. **最后一条是 Assistant** **且** `request_id == active_stream_request_id` → `ReplaceLastAssistant` (增量更新)
3. **其他情况** → `PushAssistant` (新建消息)

这个算法支持：
- 流式增量更新最后一条响应
- 多并发请求隔离
- 空包过滤

## 错误处理

| 场景 | 处理 |
|------|------|
| JSON 解析失败 | 弹出 Error toast，继续运行 |
| WebSocket 断开 | 状态设为 `Disconnected`，等待用户重连 |
| 服务端返回 Error 帧 | 弹出 toast，更新连接状态 |
| 发送失败 | 更新连接状态为 Error，通知用户 |

## 主题支持

支持三种主题模式：

- **System** - 跟随系统深色/浅色
- **Light** - 强制浅色
- **Dark** - 强制深色

主题选择持久化到 localStorage，由 egui 原生支持。

## 编译与部署

### 编译流程

```bash
# 编译 WASM 并优化
wasm-pack build --target web --release
# 压缩生成 .wasm
gzip -9 klaw_webui_bg.wasm
# 结果被拷贝到 klaw-gateway/assets/
# 由 Rust include_bytes! 嵌入二进制
```

### 部署方式

`klaw-gateway` 静态服务：

- `/` →  serving `index.html`
- `/klaw_webui.js` →  serving wasm-bindgen 生成的 JS  glue
- `/klaw_webui_bg.wasm` →  serving 压缩后的 WASM
- `/ws/chat` → WebSocket 升级

整个应用**单二进制部署**，无需额外静态文件服务。

## 设计决策

### Q: 为什么用 egui/WASM 而不是 React/Vue 传统前端？

A:

1. **技术栈统一** - 整个 Klaw 项目全 Rust，前端开发者不需要额外学习 JavaScript/TypeScript
2. **共享类型** - 协议帧和核心算法可以在前端后端之间共享类型定义
3. **体积小** - 最终优化后 WASM 约 1.5~2.5 MB，比现代前端框架包更小
4. **开发速度** - 对于多窗口浮动布局这种需求，egui 开箱即用

### Q: 为什么用多窗口浮动布局而不是单会话 Tab 布局？

A: 多窗口允许：
- 同时对比多个会话回答
- 参考旧会话编写新会话提问
- 拖拽排列工作区符合用户自由组织习惯

### Q: 为什么会话状态服务端保存，客户端只保存打开/关闭？

A:

- 服务端是真实来源，保证和桌面客户端一致
- 客户端不保存消息内容，减少 localStorage 占用
- 重新连接自动拉取最新状态

## 限制与未来改进

### 当前限制

- 不支持富文本编辑（Markdown 渲染只读）
- 不支持复制粘贴图片（媒体资源通过链接处理）
- 移动端触摸优化有限

### 可能改进方向

- 支持移动端自适应布局
- 支持键盘快捷键（Ctrl+Enter 发送，Ctrl+N 新建）
- 支持搜索历史消息
- 支持 markdown 代码块语法高亮

## 源码索引

| 文件 | 职责 |
|------|------|
| `klaw-webui/src/lib.rs` | 入口、纯逻辑单元测试 |
| `klaw-webui/src/web_chat/app.rs` | `ChatApp` 顶级状态与业务逻辑 |
| `klaw-webui/src/web_chat/ui.rs` | UI 渲染代码 |
| `klaw-webui/src/web_chat/transport.rs` | WebSocket 连接管理 |
| `klaw-webui/src/web_chat/protocol.rs` | 协议帧定义 |
| `klaw-webui/src/web_chat/session.rs` | 会话窗口数据结构 |
| `klaw-webui/src/web_chat/storage.rs` | localStorage 持久化 |
| `klaw-webui/src/web_chat/markdown.rs` | Markdown 缓存 |
