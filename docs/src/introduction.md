# Klaw 项目简介

Klaw 是一个基于 Rust 的 AI Agent 框架，采用 MQ 风格的消息传递架构和内置可靠性控制。

## 核心特性

- **会话隔离**：基于 `session_key` 的串行执行保证，同会话消息顺序处理
- **可靠性保障**：重试策略（指数退避）、幂等存储、熔断器、死信队列
- **工具系统**：可扩展的工具抽象，内置 shell、web search、memory、sub-agent 等能力
- **Skills 支持**：兼容 Anthropic/Vercel skills，支持从 Git 仓库同步
- **多后端存储**：支持 Turso/libSQL 和 SQLx 后端，统一 trait 对外
- **消息总线**：Channel 与 Agent 通过 MessageBus 解耦，支持多平台接入

## 架构概览

```
User Input → InboundMessage (agent.inbound)
                    → AgentLoop.run_once_reliable()
                    → OutboundMessage (agent.outbound) → Response
                                               ↘ DeadLetterMessage (agent.dlq)
```

## 工作空间结构

| Crate | 职责 |
|-------|------|
| `klaw-config` | TOML 配置加载（`~/.klaw/config.toml`） |
| `klaw-tool` | 工具 trait 定义和内置工具（shell、fs、web 等） |
| `klaw-core` | Agent 运行时：消息协议、调度器、可靠性控制 |
| `klaw-cli` | CLI 入口（binary: `klaw`） |
| `klaw-storage` | 存储抽象层（session/cron 持久化） |
| `klaw-archive` | 媒体归档服务（文件落盘与索引） |
| `klaw-gateway` | WebSocket 网关服务 |
| `klaw-skill` | Skills 生命周期管理 |
| `klaw-memory` | 长期记忆服务（BM25 + Vector） |
| `klaw-mcp` | Model Context Protocol 支持 |
| `klaw-cron` | 定时任务管理 |
| `klaw-heartbeat` | 会话心跳监控 |
| `klaw-channel` | 多平台 Channel 适配器 |

## 快速链接

- [快速开始](./quickstart.md)
- [Agent Core](./agent-core/README.md)
- [工具文档](./tools/README.md)
- [存储文档](./storage/README.md)
- [设计计划](./plans/README.md)

---

# 关于本文档

本文档使用 [mdbook](https://rust-lang.github.io/mdBook/) 构建，是 Klaw 项目的官方文档。

## 安装 mdbook

```bash
# 使用 cargo 安装 mdbook (推荐使用 v0.4.x 版本)
# 注意：v0.5.x 版本存在字体渲染问题，请使用 v0.4.40
cargo install mdbook --version 0.4.40

# 可选：安装预处理器和主题支持
cargo install mdbook-mermaid    # Mermaid 图表支持
cargo install mdbook-katex      # LaTeX 数学公式支持
```

## 构建文档

```bash
# 进入 docs 目录
cd docs

# 构建静态站点（输出到 docs/book/）
mdbook build

# 清理并重新构建
mdbook clean && mdbook build
```

## 开发模式（实时预览）

```bash
# 启动本地服务器，默认访问 http://localhost:3000
mdbook serve

# 指定端口
mdbook serve -p 8000

# 监听所有网络接口
mdbook serve -n 0.0.0.0
```

## 目录结构

```
docs/
├── book.toml          # mdbook 配置文件
├── book/              # 构建输出目录（自动生成）
└── src/
    ├── SUMMARY.md     # 文档目录和导航结构
    ├── introduction.md # 项目简介（本文件）
    ├── quickstart.md  # 快速开始指南
    ├── agent-core/    # Agent 核心文档
    ├── tools/         # 工具文档
    ├── storage/       # 存储文档
    ├── gateway/       # 网关文档
    └── plans/         # 设计计划
```

## 编写规范

- 所有文档使用 Markdown 格式
- 代码块标注语言类型以启用语法高亮
- 使用相对路径链接其他文档
- 遵循 [Rust API 文档风格](https://doc.rust-lang.org/rust-by-example/)
