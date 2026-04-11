# Webhook 面板

## 功能说明

管理 Webhook 配置，查看 Webhook 请求历史。

## 核心功能

- 列出所有已配置 Webhook
- 添加新 Webhook
- 编辑 Webhook URL
- 删除 Webhook
- 查看最近请求历史
- 手动触发测试请求

## 工作流程

1. 外部事件触发 → Klaw Gateway 接收 Webhook 请求
2. 请求进入对应 Channel
3. Agent 处理后调用回调 URL 发送结果

## 配置示例

```toml
[channels.webhook.my-webhook]
enabled = true
url = "https://example.com/callback"
```
