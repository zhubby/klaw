# klaw-webui

基于 **egui** + **eframe** Web 后端的浏览器聊天壳，连接本仓库 `klaw-gateway` 的 `GET /ws/chat`（按 `session_key` 房间广播纯文本）。

- 会话键：`web:<uuid>`，工作区状态默认写入浏览器 `localStorage`（`klaw_webui_workspace_state`）
- 可选鉴权：页面 URL 可带 `?gateway_token=` 或 `?token=`，会附加到 WebSocket 的 `token` query（与 `gateway.auth` 密钥一致时生效）

## 模块布局

`klaw-webui` 现在按职责拆成以下模块：

- `src/lib.rs`: crate 入口与可测试的轻量逻辑
- `src/web_chat/mod.rs`: wasm 启动入口
- `src/web_chat/app.rs`: `ChatApp` 状态与顶层编排
- `src/web_chat/session.rs`: 会话模型、窗口定位和消息元数据
- `src/web_chat/protocol.rs`: WebSocket 帧编码与解码
- `src/web_chat/storage.rs`: 浏览器 `localStorage` 持久化
- `src/web_chat/transport.rs`: WebSocket 生命周期与消息收发
- `src/web_chat/ui.rs`: `egui` 渲染辅助

## 与 klaw-ui-kit 的关系

跨前端复用的基础 UI 能力已上移到 `klaw-ui-kit`，当前包括：

- `ThemeMode`
- `theme_preference()`
- `NotificationCenter`

浏览器专属逻辑仍保留在 `klaw-webui`，例如 `web_sys`、WASM 启动入口、WebSocket 回调和 `localStorage` 细节。

## 构建并刷新 gateway 内嵌资源

唯一推荐入口是在仓库根目录执行：

```bash
make webui-wasm
```

该目标会自动：

- 确保 `wasm32-unknown-unknown` target 已安装
- 编译 `klaw-webui`
- 运行 `wasm-bindgen` 并把产物写入 `klaw-gateway/static/chat/pkg/`

如果本机缺少 `wasm-bindgen` CLI，`make` 会直接提示按 workspace 当前版本安装。

`klaw-gateway/static/chat/pkg/` 已被 `.gitignore` 忽略；生成后再执行 `cargo build -p klaw-gateway`，`include_*` 才能把这些资源打入二进制。
