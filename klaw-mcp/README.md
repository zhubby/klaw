# klaw-mcp

`klaw-mcp` 负责 MCP server 的连接、bootstrap、远程工具发现，以及把远端工具挂接到本地 `ToolRegistry`。

## Responsibilities

- 按配置启动 `stdio` MCP 子进程或连接 `sse` MCP 服务
- 执行 `initialize` / `tools/list` 并汇总可用工具
- 处理工具名冲突与 bootstrap 失败汇总
- 在 runtime 退出时关闭已连接的 MCP client

## Shutdown

- `stdio` MCP 子进程启用 `kill_on_drop`
- 正常 shutdown 会先关闭子进程 stdin，再等待子进程退出
- stderr 捕获任务在收尾时会显式中止，避免 stderr pipe 收尾拖住整个 CLI 退出
