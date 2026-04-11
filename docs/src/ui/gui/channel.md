# 渠道面板

## 功能说明

管理所有输入渠道（Channel）配置，包括 WebSocket、DingTalk、Telegram、Webhook、Terminal 等。

## 核心功能

- 列出所有已配置渠道实例
- 查看每个渠道的运行状态
- 添加新渠道
- 编辑渠道配置
- 删除渠道
- 启停渠道

## 支持的渠道类型

| 类型 | 说明 |
|------|------|
| `websocket` | WebSocket 网关渠道 |
| `dingtalk` | 钉钉机器人 |
| `telegram` | Telegram Bot |
| `terminal` | 本地终端 |
| `webhook` | HTTP Webhook 回调 |
| `cron` | 定时任务触发器 |
| `heartbeat` | 会话心跳监控 |

## 相关文档

- [WebSocket Channel](../websocket-channel.md) - WebSocket 协议完整文档
