# 网关面板

## 功能说明

监控 klaw-gateway HTTP/WebSocket 网关服务状态。

## 核心功能

- 显示网关监听地址和端口
- 显示当前连接数
- 显示网关运行状态
- 快速打开网关 WebUI
- 启停网关服务

## 相关配置

```toml
[gateway]
enabled = true
bind_addr = "0.0.0.0:3000"
token = "your-auth-token"
```
