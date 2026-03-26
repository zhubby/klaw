# CHANGELOG

## 2026-03-26

### Fixed

- stdio MCP server launches now inherit the shared augmented PATH so GUI/macOS sessions can resolve configured server commands from Homebrew/MacPorts install locations

## 2026-03-25

### Added

- 为 `McpProxyTool` 增加回归测试，覆盖代理工具在创建后读取最新 hub 状态的调用路径

### Fixed

- 修复 MCP 代理工具持有 hub 快照导致的 `mcp server '<id>' not found` 问题
- 调整 MCP client 插入与代理工具注册顺序，避免启动和热重载期间暴露过期路由状态

## 2026-03-23

### Added

- 新增 `McpRuntimeSnapshot` 与 `McpServerDetail`，支持读取运行中 MCP server 的状态快照和缓存的 `tools/list` 响应

### Changed

- `McpManager` 现在会在工具发现成功后缓存每个 server 的 `tools/list` 结果，供 GUI 详情弹窗读取
- 运行时状态查询现在可直接读取 manager 快照，而不必触发一次完整 `sync`

## 2026-03-22

### Added

- 新增 MCP 热重载支持：`McpManager` 支持动态启动/停止/重启 MCP 服务器
- 新增 `McpServerKey`、`McpLifecycleState`、`McpServerStatus` 数据结构用于状态管理
- 新增 `McpConfigSnapshot` 用于配置快照和差异比较
- 新增 `McpSyncResult` 返回同步操作结果
- 新增 `RuntimeCommand::SyncMcp` 命令支持 GUI 触发 MCP 配置同步
- 新增 `ToolRegistry::unregister` 和 `unregister_many` 方法支持工具动态注销

### Changed

- `McpManager::spawn_init` 现在接收 `ToolRegistry` 和 `McpConfigSnapshot` 参数，返回 `McpInitHandle`
- `McpInitHandle` 包含 `manager()` 方法获取 `Arc<Mutex<McpManager>>` 用于后续 sync 操作
- `McpClientHub::remove` 返回类型改为 `()`
- `McpServerConfig` 新增 `Default` 和 `PartialEq` 实现

### Fixed

- Stdio 类型的 MCP 服务器在停止时正确杀子进程
- SSE 类型的 MCP 服务器在停止时仅更新内存数据

## 2026-03-13

### Added

- 新增 `klaw-mcp` crate 级 README 与 CHANGELOG，说明 MCP bootstrap、代理和收尾职责

### Changed

- `stdio` MCP 子进程关闭时不再无条件等待 stderr 读取任务自然结束，避免退出流程被后台收尾阻塞
