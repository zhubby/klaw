# CHANGELOG

## 2026-03-13

### Added

- 新增 `klaw daemon` 子命令，支持 `install`、`status`、`uninstall`、`start`、`stop`、`restart`
- 新增 `systemd --user` 与 `launchd` 用户级服务文件渲染与管理逻辑
- 新增 daemon 相关单元测试和计划文档
- 新增 `klaw stdio` 启动 ASCII `KLAW` 标记与版本、skills、tools、MCP 加载摘要输出
- 新增 `klaw gateway` 启动成功后的监听地址 stdout 输出

### Changed

- `klaw gateway` 增加终止信号处理，并在退出时执行 runtime shutdown
- `klaw stdio` 在进入交互前等待 MCP bootstrap 完成，避免启动后首条消息才触发就绪校验
