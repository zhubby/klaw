# klaw-webui

基于 **egui** + **eframe** Web 后端的浏览器聊天壳，连接本仓库 `klaw-gateway` 的 `GET /ws/chat`（按 `session_key` 房间广播纯文本）。

- 会话键：`web:<uuid>`，默认写入浏览器 `localStorage`（`klaw_webui_session_key`）
- 可选鉴权：页面 URL 可带 `?gateway_token=` 或 `?token=`，会附加到 WebSocket 的 `token` query（与 `gateway.auth` 密钥一致时生效）

## 构建并刷新 gateway 内嵌资源

在仓库根目录：

```bash
make webui-wasm
```

或手动：

```bash
rustup target add wasm32-unknown-unknown
cargo build -p klaw-webui --target wasm32-unknown-unknown --release
wasm-bindgen target/wasm32-unknown-unknown/release/klaw_webui.wasm \
  --out-dir klaw-gateway/static/chat/pkg --target web --no-typescript
```

`wasm-bindgen` CLI 的补丁版本须与 crate 依赖一致（当前 workspace 为 `0.2.114`）。输出目录 `klaw-gateway/static/chat/pkg/` 已被 `.gitignore` 忽略；生成后再 `cargo build -p klaw-gateway` 才会通过 `include_*` 打入二进制。
