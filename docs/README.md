# Klaw 文档

本文档目录包含 Klaw 项目的官方文档，使用 [mdbook](https://rust-lang.github.io/mdBook/) 构建，采用 [Catppuccin](https://github.com/catppuccin/mdBook) 主题。

## 快速开始

### 安装 mdbook

```bash
# 使用 cargo 安装 mdbook (推荐使用 v0.4.40 版本)
# 注意：v0.5.x 版本存在字体渲染问题
cargo install mdbook --version 0.4.40

# 安装 Mermaid 预处理器（图表支持）
cargo install mdbook-mermaid --version 0.14.0

# 可选：其他预处理器
cargo install mdbook-katex      # LaTeX 数学公式支持
```

**版本兼容性说明：**
- `mdbook` v0.5.x 存在字体渲染问题（Missing font github）
- `mdbook-admonish` v1.20.0 存在 TOML 解析 bug，暂时无法使用
- 推荐组合：`mdbook@0.4.40` + `mdbook-mermaid@0.14.0`

### 主题配置

本文档使用 Catppuccin 主题，提供 4 种配色方案：

| 主题 | 类型 | 描述 |
|------|------|------|
| Latte | 浅色 | 明亮柔和的浅色调 |
| Frappé | 深色 | 中等对比度的深色调 |
| Macchiato | 深色 | 平衡的深色调 |
| Mocha | 深色 | 高对比度的深色调（默认夜间模式） |

主题文件位于 `theme/` 目录：
- `theme/catppuccin.css` - Catppuccin 主题样式
- `theme/index.hbs` - 自定义主题选择器

在 `book.toml` 中配置默认主题：
```toml
[output.html]
default-theme = "latte"          # 默认浅色主题
preferred-dark-theme = "mocha"   # 默认深色主题
additional-css = ["./theme/catppuccin.css"]
```

### 构建文档

```bash
# 进入 docs 目录
cd docs

# 构建静态站点（输出到 docs/book/）
mdbook build

# 清理并重新构建
mdbook clean && mdbook build
```

### 开发模式（实时预览）

```bash
# 启动本地服务器，默认访问 http://localhost:3000
mdbook serve

# 指定端口
mdbook serve -p 8000

# 监听所有网络接口
mdbook serve -n 0.0.0.0
```

构建完成后，在浏览器中打开 `book/index.html` 即可查看文档。

## 目录结构

```
docs/
├── README.md          # 本文档（构建说明）
├── book.toml          # mdbook 配置文件
├── book/              # 构建输出目录（自动生成）
└── src/
    ├── SUMMARY.md     # 文档目录和导航结构
    ├── introduction.md # 项目简介
    ├── quickstart.md  # 快速开始指南
    ├── agent-core/    # Agent 核心文档
    ├── tools/         # 工具文档
    ├── storage/       # 存储文档
    ├── gateway/       # 网关文档
    └── plans/         # 设计计划
```

## 文档结构

| 目录 | 内容 |
|------|------|
| `introduction.md` | 项目简介和本文档使用说明 |
| `quickstart.md` | 快速开始指南 |
| `agent-core/` | Agent 核心架构文档（消息协议、运行时状态机、可靠性控制等） |
| `tools/` | 工具文档（内置工具、Web 工具、高级功能） |
| `storage/` | 存储文档（Session、Cron、Archive） |
| `gateway/` | WebSocket 网关文档 |
| `plans/` | 设计计划和架构决策记录 |

## 编写规范

- 所有文档使用 Markdown 格式
- 代码块标注语言类型以启用语法高亮
- 使用相对路径链接其他文档
- 遵循 [Rust API 文档风格](https://doc.rust-lang.org/rust-by-example/)

## 部署

构建生成的静态文件位于 `book/` 目录，可以部署到任何静态网站托管服务：

- GitHub Pages
- Netlify
- Vercel
- Cloudflare Pages
