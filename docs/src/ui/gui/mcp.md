# MCP 面板

## 功能说明

管理 Model Context Protocol (MCP) 服务器配置。

## 核心功能

- 列出所有已配置 MCP 服务器
- 添加新服务器配置
- 编辑服务器配置（命令、参数、环境变量）
- 删除服务器
- 测试连接
- 查看服务器可用工具列表

## 配置示例

```toml
[mcp.servers."my-server"]
command = "npx"
args = ["my-mcp-server"]
env = { API_KEY = "xxx" }
```
