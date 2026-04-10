# klaw-webui Chat Layout Design

## Goal

将 `klaw-webui` 从当前的“顶部状态 + 中部列表 + 底部单行输入”的基础调试界面，调整为更接近现代聊天产品的三段式布局：

- 顶部为紧凑工具栏
- 中间为居中的会话主列
- 底部为独立的输入区

本次设计目标是优先在 `egui` 的能力边界内实现更好的聊天体验，而不是像素级复刻 ChatGPT。

## Confirmed Direction

- 视觉方向：借鉴 ChatGPT 的布局骨架，但整体风格保持 `Klaw` 自己的气质
- 顶部信息密度：保留紧凑工具栏
- 会话区布局：采用方案 A，即经典居中的对话列
- 实现原则：按 `egui` 最合适的方法做，不强追复杂网页视觉效果

## Scope

### In Scope

- 重新组织 `klaw-webui` 的整体页面布局
- 调整消息列表区域的宽度、对齐方式和空白分布
- 将输入框升级为独立的底部 composer 区域
- 精简顶部状态栏信息展示
- 为空状态、连接错误状态、未连接状态定义更合理的呈现

### Out of Scope

- Markdown 富文本渲染
- 头像系统
- 左侧会话列表 / 历史记录导航
- 文件上传、语音输入、工具按钮组
- runtime / agent 会话桥接
- WebSocket 协议扩展或消息结构变更

## Existing Code Anchors

- `klaw-webui/src/web_chat.rs`：WASM chat app、WebSocket 生命周期、当前 egui 布局
- `klaw-webui/src/lib.rs`：WASM 入口导出
- `klaw-gateway/static/chat/index.html`：承载 canvas 的页面入口

## Design Summary

### 1. Page Structure

`ChatApp::update()` 将维持三段式结构，但重新组织为：

1. `TopBottomPanel::top`：轻量工具栏
2. `CentralPanel`：聊天主区，内部使用一个居中的窄列容器承载消息流
3. `TopBottomPanel::bottom`：独立 composer 区域

这样做的原因：

- 符合 `egui` 最稳定的布局模型
- 不需要依赖复杂浮层定位
- 可以自然适配浏览器窗口高宽变化
- 与用户确认的“中间会话面板 + 底部输入框”目标一致

### 2. Top Toolbar

顶部工具栏应保持低视觉权重，仅保留必要上下文：

- 左侧：`Klaw Web Chat`
- 右侧：连接状态文本、必要时 `Reconnect`
- 次级信息：`session_key` 不再大面积常驻展示，可降级为小号次级文本，或只在断线 / 调试态展示

设计原则：

- 减少顶部占高，避免把聊天页做成“控制台”
- 顶栏更多承担状态反馈，而不是信息面板职责

### 3. Conversation Column

中间主体是页面最重要的区域，应形成明显的“阅读中轴”：

- 整个消息流限制最大宽度，居中显示
- 消息之间保留稳定垂直间距
- 左右两类消息分别靠左 / 靠右排列
- 不做复杂外层边框卡片，主要依靠留白、宽度和气泡差异建立层级

这会让页面更像聊天产品，而不是日志窗口。

### 4. Message Presentation

消息采用 `egui::Frame` 风格的轻量气泡：

- assistant：左对齐，使用偏 `Klaw` 的深色/品牌色背景
- user：右对齐，使用中性背景
- 单条消息最大宽度应小于会话列宽度，避免整行铺满

推荐气泡设计：

- 明确圆角
- 适中的内边距
- 消息内容优先可读性，不依赖复杂阴影

如需进一步增强品牌感，应通过配色和空状态文案来体现，而不是依赖复杂装饰。

### 5. Empty State

空状态出现在首次进入或没有任何消息时：

- 位于中间会话列中央区域
- 一行主文案，例如 “Start a conversation with Klaw”
- 一行次级说明，例如提示发送消息或等待连接

要求：

- 保持安静、简洁，不做 landing page 式大块品牌展示
- 不干扰“这是聊天界面”的核心认知

### 6. Bottom Composer

底部输入区应从当前普通单行输入，调整为独立的 composer 容器：

- 整体位于 `TopBottomPanel::bottom`
- 内部采用居中窄宽度布局，与中间消息列宽度协调
- 使用多行输入框
- 发送按钮固定在右侧

推荐交互：

- 输入区有独立圆角边框容器
- 输入框支持更自然的聊天式输入，不再显得像命令行文本框
- 若当前未连接，composer 可禁用或保留可见但给出明确状态提示

### 7. Visual Style

视觉风格不追求模仿 ChatGPT 皮肤，而是遵循以下原则：

- 深色背景
- 中央内容更亮，形成阅读焦点
- 颜色克制，避免过多装饰
- 用留白和宽度约束建立层级

品牌表达方式优先级：

1. 色彩语气
2. 文案语气
3. 顶栏命名

不优先通过复杂 logo、插画或大面积品牌块来表达。

## State Design

当前已有连接状态：

- `Disconnected`
- `Connecting`
- `Connected`
- `Error(String)`

这些状态在新版 UI 中的呈现方式：

- `Connected`：顶部显示简洁正向状态
- `Connecting`：顶部显示进行中状态，空状态/消息区可给轻提示
- `Disconnected`：顶部弱提示，composer 可保留但发送不可用，或提示需要重连
- `Error(String)`：顶部显示错误摘要，必要时在空状态区补一行可读错误说明

## Data Flow

本次不改现有消息模型，仍沿用：

- WebSocket 收到文本 → push 到 `lines`
- 发送时从 `draft` 发送纯文本
- 页面初次渲染时自动建立连接

本次只调整“展示层组织”，不扩大协议与状态机范围。

## Error Handling

错误处理保持简单清晰：

- WebSocket 初始化失败：顶部显示错误
- 发送失败：顶部切换为错误态
- 关闭连接：恢复为 `Disconnected`

本次不新增自动重试策略，只保留显式 `Reconnect`。

## Testing Strategy

本次改动主要是表现层，但仍应保持基础验证：

- 保留已有 `klaw-webui` 测试
- 如拆出纯函数（例如布局辅助、状态文案格式化），为这些纯函数增加单测
- 继续使用：
  - `make webui-wasm`
  - `cargo test -p klaw-webui`
  - `cargo test -p klaw-gateway --lib`

由于 `egui` 布局本身不容易做高价值像素测试，本次以行为验证和人工预览为主。

## Recommended Implementation Approach

推荐的实现拆分：

1. 在 `web_chat.rs` 中新增小型视图辅助函数：
   - 顶栏渲染
   - 空状态渲染
   - 消息列表渲染
   - composer 渲染
2. 保持 `ChatApp` 为主状态持有者，不额外引入复杂组件体系
3. 优先完成稳定布局与基本视觉层级，再决定是否补充更细节的配色和排版调整

这个拆分与当前 crate 规模相匹配，也更适合 `egui` 的使用方式。

## Risks

- `egui` 在 Web 上的输入框体验与原生网页仍有差距，不能过度承诺“完全像 ChatGPT”
- 若消息量变大，当前 `Vec<String>` 模型会限制后续表现力，但不属于本次范围
- 若后续需要 markdown、工具消息或多种消息类型，当前 `String` 模型将需要升级

## Acceptance Criteria

- 页面不再表现为“调试面板”，而是明显的聊天产品布局
- 顶部仅保留紧凑工具栏
- 中间消息区为居中窄列
- 底部为独立 composer
- 空状态、连接态、错误态都具备清晰可读的 UI 呈现
- `egui` 实现保持简单稳定，不为追求网页效果引入脆弱结构
