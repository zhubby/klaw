# klaw-gateway

`klaw-gateway` 提供基于 `axum` 的 WebSocket 网关服务，负责：

- 绑定配置中的监听地址和端口
- 支持 `listen_port = 0` 时由系统分配随机可用端口
- 暴露 `/ws/chat` WebSocket 入口
- 暴露 `/chat` 嵌入式 Web 聊天页（`klaw-webui` WASM + egui），静态资源为 `/chat/dist/klaw_webui.js` 与 `/chat/dist/klaw_webui_bg.wasm`
- 暴露 `/` 默认落地页与内置 logo 静态资源
- 可选暴露固定路径的 `POST /webhook/events` 与 `POST /webhook/agents`，并支持 Bearer、GitHub、GitLab 多种 header/signature 校验
- 可选暴露 archive 文件上传下载接口：
  - `POST /archive/upload`: 文件上传（multipart/form-data）
  - `GET /archive/download/:id`: 文件下载
  - `GET /archive/list`: 查询文件列表
  - `GET /archive/:id`: 获取文件元数据
- 可选暴露 model providers 列表接口：
  - `GET /providers/list`: 获取所有配置的 model providers
- webhook 请求会进入独立的 `webhook:*` 执行 session；若提供 `base_session_key`，最终回复会路由回目标 IM 会话当前 active session
- 按 `session_key` 维护房间广播通道
- 在启动成功后打印实际可连接的 WebSocket 地址
- 提供可管理的 `GatewayHandle` / `GatewayRuntimeInfo`，以及可注入业务逻辑的 `GatewayWebhookHandler`

## Module Layout

- `lib.rs`: 仅保留模块声明与公开 API re-export
- `runtime.rs`: gateway 启动、监听、路由装配与生命周期入口
- `state.rs`: 运行态共享状态、`GatewayHandle` 与 `GatewayRuntimeInfo`
- `websocket.rs`: `/ws/chat` WebSocket 连接与房间广播逻辑
- `chat_page.rs`: `/chat` 与 `/chat/dist/*` WASM/JS 内嵌资源响应
- `webhook.rs`: webhook 鉴权、`events` / `agents` payload 归一化与 handler 集成
- `archive.rs`: archive 文件上传下载接口实现
- `providers.rs`: model providers 列表接口实现
- `handlers.rs`: health / metrics HTTP handlers
- `error.rs`: `GatewayError`

## Runtime Notes

- 当前仅支持非 TLS 监听
- 启动成功后会输出实际监听地址对应的 `http://<listen_addr>/ws/chat`
- 根路径 `/` 会返回单页品牌首页，logo 资源位于 `/logo.webp`；浏览器聊天 UI 位于 `/chat`（会话 `session_key` 形如 `websocket:<uuid>`，存于 `localStorage`）
- 当 `gateway.auth.enabled = true` 时，浏览器无法为 WebSocket 设置 `Authorization` 头，因此 `/ws/chat` 同时接受 query 参数 `token` 或 `access_token`（值与配置的 Bearer secret 相同）。**Token 会出现在 URL 与访问日志中**，公网请优先使用 WSS 并知晓风险
- webhook 路由是否注册由 `gateway.webhook.enabled` 决定；`events` / `agents` 仅可分别启停并配置独立 body limit，路径固定不再开放配置
- archive 路由在 `GatewayOptions` 中提供 `archive_service` 时自动注册，所有 archive 接口均需要 Bearer 鉴权
- 仅 `/ws/chat` 和 `/archive/*` 会走 gateway Bearer 鉴权中间件（含 query token 回退）；`/webhook/events` 与 `/webhook/agents` 继续复用 `gateway.auth` 的 token/env secret 做 webhook 专用多模式校验；首页、`/chat` 及其静态资源、health、metrics 不做鉴权
- `TailscaleManager::inspect_host()` 可独立读取本机 Tailscale 状态，供 GUI 在 gateway 未运行时展示主机连接信息；当本机 daemon 无响应时会在短超时后回落为 host 侧不可用状态，避免拖慢整个 gateway 状态刷新
- Tailscale Serve/Funnel 会在 gateway 绑定完成后使用实际监听端口做反向代理，并在 setup 后回读 `tailscale serve status --json` / `tailscale funnel status --json` 确认配置是否生效；Funnel 未配置 auth 时允许启动，但应视为公网裸露入口

## Web UI（WASM）构建

更新内嵌聊天资源前，在仓库根目录执行：

```bash
make webui-wasm
```

这是唯一推荐入口；它会负责 target 检查、`klaw-webui` 编译，以及把 wasm-bindgen 产物写入 `klaw-gateway/static/chat/dist/`。如果本机缺少 `wasm-bindgen` CLI，`make` 会按 workspace 当前版本给出安装提示。

清理本地生成的 `dist/`（可选）：`make clean-webui-wasm`

然后重新编译 `klaw-gateway`；`rust-embed` 会从 `static/` 与 `assets/` 目录打包首页、聊天页以及 `dist/` 下的 `.js` / `.wasm`。

`klaw-gateway/static/chat/dist/` 已列入仓库根目录 `.gitignore`，wasm-bindgen 产物不提交。若需要刷新浏览器聊天资源，请先执行上文命令，再启动或编译 gateway。

## Examples

- `examples/webhook_request.rs`: 使用 Rust 和 `reqwest` 向 `events` webhook 端点发送一条测试事件
- `examples/webhook_agents_request.rs`: 使用 Rust 和 `reqwest` 向 `agents` webhook 端点发送 query + raw JSON body 请求
- `examples/archive_upload.rs`: 使用 Rust 和 `reqwest` 向 archive 端点上传文件

```bash
cargo run -p klaw-gateway --example webhook_request
WEBHOOK_TOKEN=replace-me BASE_URL=http://127.0.0.1:18080 cargo run -p klaw-gateway --example webhook_request

cargo run -p klaw-gateway --example webhook_agents_request
WEBHOOK_TOKEN=replace-me BASE_URL=http://127.0.0.1:18080 cargo run -p klaw-gateway --example webhook_agents_request

cargo run -p klaw-gateway --example archive_upload
GATEWAY_TOKEN=your-token BASE_URL=http://127.0.0.1:18080 cargo run -p klaw-gateway --example archive_upload
```

## Archive API Usage

### Upload File

```bash
curl -X POST http://127.0.0.1:18080/archive/upload \
  -H "Authorization: Bearer your-token" \
  -F "file=@/path/to/file.pdf" \
  -F "session_key=terminal:test" \
  -F "channel=terminal" \
  -F "chat_id=test-chat"
```

Response:
```json
{
  "success": true,
  "record": {
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "source_kind": "user_upload",
    "media_kind": "pdf",
    "mime_type": "application/pdf",
    "extension": "pdf",
    "original_filename": "file.pdf",
    "content_sha256": "abc123...",
    "size_bytes": 12345,
    "storage_rel_path": "archives/2024-01-15/550e8400-e29b-41d4-a716-446655440000.pdf",
    "session_key": "terminal:test",
    "channel": "terminal",
    "chat_id": "test-chat",
    "message_id": null,
    "metadata_json": "{}",
    "created_at_ms": 1705334400000
  },
  "error": null
}
```

### Download File

```bash
curl -X GET http://127.0.0.1:18080/archive/download/550e8400-e29b-41d4-a716-446655440000 \
  -H "Authorization: Bearer your-token" \
  -o downloaded-file.pdf
```

### List Files

```bash
curl -X GET "http://127.0.0.1:18080/archive/list?session_key=terminal:test&limit=10&offset=0" \
  -H "Authorization: Bearer your-token"
```

Response:
```json
{
  "success": true,
  "records": [
    {
      "id": "550e8400-e29b-41d4-a716-446655440000",
      "source_kind": "user_upload",
      "media_kind": "pdf",
      ...
    }
  ],
  "error": null
}
```

### Get File Metadata

```bash
curl -X GET http://127.0.0.1:18080/archive/550e8400-e29b-41d4-a716-446655440000 \
  -H "Authorization: Bearer your-token"
```

Response:
```json
{
  "success": true,
  "record": {
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "source_kind": "user_upload",
    "media_kind": "pdf",
    ...
  },
  "error": null
}
```

## Providers API Usage

### List Model Providers

```bash
curl -X GET http://127.0.0.1:18080/providers/list \
  -H "Authorization: Bearer your-token"
```

Response:
```json
{
  "success": true,
  "providers": [
    {
      "id": "openai",
      "name": "OpenAI",
      "base_url": "https://api.openai.com/v1",
      "wire_api": "chat_completions",
      "default_model": "gpt-4o-mini",
      "stream": false,
      "has_api_key": true
    },
    {
      "id": "anthropic",
      "name": "Anthropic",
      "base_url": "https://api.anthropic.com",
      "wire_api": "messages",
      "default_model": "claude-3-5-sonnet-20241022",
      "stream": false,
      "has_api_key": true
    }
  ],
  "default_provider": "openai",
  "error": null
}
```
