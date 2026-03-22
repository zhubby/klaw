# Tailscale 集成

`klaw-gateway` 支持 Tailscale Serve 和 Funnel，可将 Gateway 暴露到 Tailscale 私有网络或公网。

## 前置条件

1. 安装 Tailscale CLI 并登录：
   ```bash
   tailscale login
   ```

2. 对于 Funnel 模式，需要在 [Tailscale Admin Console](https://login.tailscale.com/admin/dns) 启用 HTTPS。

## 配置

```toml
[gateway]
enabled = true
listen_ip = "127.0.0.1"
listen_port = 18789

[gateway.auth]
enabled = true
token = "your-secret-token"
# 或使用环境变量
# env_key = "KLAW_GATEWAY_TOKEN"

[gateway.tailscale]
mode = "funnel"        # off | serve | funnel
reset_on_exit = true
```

### 配置说明

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `mode` | `off` / `serve` / `funnel` | `off` | Tailscale 暴露模式 |
| `reset_on_exit` | `bool` | `true` | 进程退出时是否撤销 Tailscale 配置 |

### 模式对比

| 模式 | 访问范围 | 要求 |
|------|----------|------|
| `off` | 仅本地 | 无 |
| `serve` | Tailscale 私有网络 | Tailscale 已登录 |
| `funnel` | 公网 | Tailscale 已登录 + HTTPS 已启用 + 认证已配置 |

## 认证配置

Funnel 模式将 Gateway 暴露到公网，**必须配置认证**：

```toml
[gateway.auth]
enabled = true
token = "your-secure-token"
```

未配置认证时启用 Funnel 模式会返回错误：

```
funnel mode requires authentication. Configure gateway.auth first.
```

## 使用方式

### Serve 模式（私有网络）

仅在 Tailscale 私有网络内可访问：

```bash
# 连接（使用 MagicDNS 名称）
wscat -c "wss://your-machine.tailnet-name.ts.net/ws/chat?session_key=room1" \
  -H "Authorization: Bearer your-secret-token"
```

### Funnel 模式（公网）

任何人都可以通过公网访问：

```bash
# 连接
wscat -c "wss://your-machine.tailnet-name.ts.net/ws/chat?session_key=room1" \
  -H "Authorization: Bearer your-secret-token"
```

## GUI 管理

在 GUI Gateway 面板中可以：

1. 查看 Tailscale 状态（Connected / Disconnected / Error）
2. 切换 Tailscale 模式（Off / Serve / Funnel）
3. 查看公网 URL

切换模式会自动重启 Gateway。

## 实现原理

1. Gateway 始终绑定 `127.0.0.1`
2. Tailscale 作为反向代理：
   - `serve`: `tailscale serve --bg=127.0.0.1:PORT`
   - `funnel`: `tailscale funnel 443 --bg=127.0.0.1:PORT`
3. 进程退出时执行 `tailscale serve --reset` 或 `tailscale funnel --reset`

## 错误处理

| 错误 | 原因 | 解决方案 |
|------|------|----------|
| `tailscale CLI not found` | 未安装 Tailscale | 安装 Tailscale |
| `tailscale not logged in` | 未登录 Tailscale | 执行 `tailscale login` |
| `tailscale HTTPS not enabled` | Tailnet 未启用 HTTPS | 在 Admin Console 启用 HTTPS |
| `funnel requires auth` | Funnel 模式未配置认证 | 配置 `gateway.auth` |

## 安全建议

1. 使用强随机 token（至少 32 字符）
2. 定期轮换 token
3. 仅在需要公网访问时使用 Funnel 模式
4. 生产环境建议配合 HTTPS 使用