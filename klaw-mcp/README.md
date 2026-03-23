# klaw-mcp

`klaw-mcp` 负责 MCP server 的连接、bootstrap、远程工具发现，以及把远端工具挂接到本地 `ToolRegistry`，并支持 MCP 服务器的热重载。

## Responsibilities

- 按配置启动 `stdio` MCP 子进程或连接 `sse` MCP 服务
- 执行 `initialize` / `tools/list` 并汇总可用工具
- 缓存每个 MCP server 最近一次成功 `tools/list` 的响应，供 GUI/调试面板查看
- 处理工具名冲突与 bootstrap 失败汇总
- 支持动态启动/停止/重启 MCP 服务器（热重载）
- 支持只读运行时快照查询，不触发额外 MCP 同步
- 在 runtime 退出时关闭已连接的 MCP client

## Hot Reload

`McpManager` 支持通过配置变更动态管理 MCP 服务器：

```rust
// 初始化
let manager = McpManager::spawn_init(tools, config_snapshot);
let mcp_manager = manager.manager();

// 热重载配置
let new_snapshot = McpConfigSnapshot::from_mcp_config(&new_config);
let result = mcp_manager.lock().await.sync(new_snapshot).await;
```

### Lifecycle States

| State | Description |
|-------|-------------|
| `Starting` | Server is being initialized |
| `Running` | Server is operational |
| `Stopped` | Server is stopped |
| `Failed` | Server failed to start |

### Stdio vs SSE Handling

| Operation | Stdio Mode | SSE Mode |
|-----------|------------|----------|
| Start | Spawn subprocess + init handshake | Create HTTP client + init handshake |
| Stop | Kill subprocess + cleanup | Remove client from hub (memory only) |
| Restart | Kill → Respawn | Update client config |

## Shutdown

- `stdio` MCP 子进程启用 `kill_on_drop`
- 正常 shutdown 会先关闭子进程 stdin，再等待子进程退出
- stderr 捕获任务在收尾时会显式中止，避免 stderr pipe 收尾拖住整个 CLI 退出
