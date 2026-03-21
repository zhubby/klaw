# klaw-gateway

`klaw-gateway` 提供基于 `axum` 的 WebSocket 网关服务，负责：

- 绑定配置中的监听地址和端口
- 支持 `listen_port = 0` 时由系统分配随机可用端口
- 暴露 `/ws/chat` WebSocket 入口
- 按 `session_key` 维护房间广播通道
- 在启动成功后打印实际可连接的 WebSocket 地址
- 提供可管理的 `GatewayHandle` / `GatewayRuntimeInfo`，供 GUI runtime 启停与展示状态

## Runtime Notes

- 当前仅支持非 TLS 监听
- 启动成功后会输出实际监听地址对应的 `http://<listen_addr>/ws/chat`
