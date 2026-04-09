# klaw-webui

基于 **egui** + **eframe** Web 后端的浏览器聊天壳，连接本仓库 `klaw-gateway` 的 `GET /ws/chat`（按 `session_key` 房间广播纯文本）。

- 会话键：`web:<uuid>`，默认写入浏览器 `localStorage`（`klaw_webui_session_key`）
- 可选鉴权：页面 URL 可带 `?gateway_token=` 或 `?token=`，会附加到 WebSocket 的 `token` query（与 `gateway.auth` 密钥一致时生效）

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
