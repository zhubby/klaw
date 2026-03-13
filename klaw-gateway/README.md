# klaw-gateway

`klaw-gateway` 提供基于 `axum` 的 WebSocket 网关服务，负责：

- 绑定配置中的监听地址和端口
- 暴露 `/ws/chat` WebSocket 入口
- 按 `session_key` 维护房间广播通道
- 在启动成功后打印可连接的 WebSocket 地址

## Runtime Notes

- 当前仅支持非 TLS 监听
- 启动成功后会输出 `ws://<listen_addr>/ws/chat`
