# CHANGELOG

## 2026-03-13

### Added

- 新增 `klaw-mcp` crate 级 README 与 CHANGELOG，说明 MCP bootstrap、代理和收尾职责

### Changed

- `stdio` MCP 子进程关闭时不再无条件等待 stderr 读取任务自然结束，避免退出流程被后台收尾阻塞
