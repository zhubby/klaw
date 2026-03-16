# Stdio 渠道设计与实现

本文档记录 `klaw-channel` 中 Stdio（标准输入输出）渠道的实现：交互式终端会话、用户输入处理、响应渲染与优雅关闭。

## 目标

- 提供基于终端的交互式对话能力
- 支持本地调试与直接命令行使用
- 实现简洁的输入输出循环（REPL）
- 支持可选的推理过程展示
- 优雅处理系统关闭信号

## 代码位置

- 渠道实现：`klaw-channel/src/stdio.rs`
- 运行时注册：`klaw-cli/src/commands/stdio.rs`

## 配置与启动

### 命令行参数

```bash
klaw stdio [OPTIONS]
```

| 参数 | 类型 | 必填 | 描述 | 默认值 |
|------|------|------|------|--------|
| `--session-key` | `string` | 否 | 本地会话标识 | 自动生成 `stdio:<uuid>` |
| `--show-reasoning` | `bool` | 否 | 是否展示模型推理过程 | `false` |
| `--verbose-terminal` | `bool` | 否 | 是否在终端打印 tracing 日志 | `false` |

### 启动示例

```bash
# 使用自动生成的 session key
klaw stdio

# 指定 session key
klaw stdio --session-key "stdio:my-local-session"

# 展示推理过程
klaw stdio --show-reasoning

# 在终端显示日志
klaw stdio --verbose-terminal
```

## 会话管理

### Session Key 生成

```rust
pub fn new(session_key: Option<String>, show_reasoning: bool) -> Self {
    let session_key = session_key
        .unwrap_or_else(|| format!("stdio:{}", Uuid::new_v4()));
    let chat_id = session_key.split(':').nth(1).unwrap_or("chat").to_string();
    Self {
        session_key,
        chat_id,
        show_reasoning,
    }
}
```

- 未指定时使用 `stdio:<uuid-v4>` 格式
- `chat_id` 从 session_key 解析（第二段）

### Session Key 格式

```
stdio:<chat_id>
```

例如：
- `stdio:abc123-def456-...`（自动生成）
- `stdio:my-local-session`（手动指定）

## 交互循环（REPL）

### 主循环结构

```rust
loop {
    tokio::select! {
        // Cron 定时任务
        _ = cron_tick.tick() => {
            run_until_shutdown(runtime.on_cron_tick()).await?;
        }

        // 运行时心跳
        _ = runtime_tick.tick() => {
            run_until_shutdown(runtime.on_runtime_tick()).await?;
        }

        // 关闭信号
        signal = shutdown_signal() => {
            println!("\nShutdown signal received. Bye.");
            break;
        }

        // 用户输入
        line = lines.next_line() => {
            // 处理输入...
        }
    }
}
```

### 用户输入处理

```
you> [用户输入内容]
agent>
--------------------
[answer]
[响应内容]
--------------------
you>
```

#### 特殊命令

| 命令 | 作用 |
|------|------|
| `/exit` | 退出程序 |
| 空行 | 忽略，重新提示 |

#### 输入流程

1. 读取一行输入
2. 去除首尾空白
3. 检查是否为空行 → 跳过
4. 检查是否为 `/exit` → 退出
5. 构造 `ChannelRequest` 提交到运行时
6. 等待并渲染响应
7. 回到步骤 1

### ChannelRequest 构造

```rust
let request = ChannelRequest {
    channel: "stdio".to_string(),
    input: input.to_string(),
    session_key: self.session_key.clone(),
    chat_id: self.chat_id.clone(),
    media_references: Vec::new(),  // Stdio 不支持媒体
};
```

## 响应渲染

### 渲染格式

```rust
fn render_agent_output(output: &ChannelResponse, show_reasoning: bool) -> String {
    let mut lines = vec![
        "--------------------".to_string(),
        "[answer]".to_string(),
        output.content.trim().to_string(),
    ];

    if show_reasoning {
        if let Some(reasoning_text) = &output.reasoning {
            lines.push(String::new());
            lines.push("[reasoning]".to_string());
            lines.extend(reasoning_text.lines().map(|line| format!("> {line}")));
        }
    }

    lines.push("--------------------".to_string());
    lines.join("\n")
}
```

### 输出示例（不含推理）

```
agent>
--------------------
[answer]
这是一个简单的回答。
--------------------
```

### 输出示例（含推理）

```
agent>
--------------------
[answer]
这是一个简单的回答。

[reasoning]
> 第一步推理...
> 第二步推理...
--------------------
```

## 关闭处理

### 信号处理

```rust
async fn shutdown_signal() -> io::Result<()> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};

        if let Ok(mut terminate) = signal(SignalKind::terminate()) {
            tokio::select! {
                signal = tokio::signal::ctrl_c() => signal,
                _ = terminate.recv() => Ok(()),
            }
        } else {
            tokio::signal::ctrl_c().await
        }
    }

    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await
    }
}
```

支持的信号：
- **Unix**: `SIGINT` (Ctrl+C), `SIGTERM`
- **非 Unix**: `Ctrl+C`

### 优雅关闭流程

1. 运行时关闭信号检测
2. 打印关闭提示：`Shutdown signal received. Bye.`
3. 调用 `shutdown_runtime_bundle()` 清理资源
4. 退出程序

```rust
let run_result = tokio::select! {
    result = channel.run(&adapter) => result,
    _ = shutdown_signal() => {
        println!("\nShutdown signal received. Bye.");
        Ok(())
    }
};

let shutdown_result = tokio::select! {
    result = shutdown_runtime_bundle(runtime.as_ref()) => result,
    _ = shutdown_signal() => {
        std::process::exit(130);
    }
};
```

## 运行时集成

### 启动流程

```rust
pub async fn run(self, config: Arc<AppConfig>) -> Result<(), Box<dyn std::error::Error>> {
    // 1. 构建运行时资源
    let mut runtime = build_runtime_bundle(config.as_ref()).await?;
    let startup_report = finalize_startup_report(&mut runtime).await?;
    print_startup_banner(config.as_ref(), &startup_report);

    let runtime = Arc::new(runtime);

    // 2. 启动后台服务
    let background = Arc::new(BackgroundServices::new(
        runtime.as_ref(),
        BackgroundServiceConfig::from_app_config(config.as_ref()),
    ));
    let adapter = SharedChannelRuntime::new(runtime.clone(), background);

    // 3. 运行渠道
    let mut channel = StdioChannel::new(self.session_key, self.show_reasoning);
    let run_result = tokio::select! {
        result = channel.run(&adapter) => result,
        _ = shutdown_signal() => Ok(()),
    };

    // 4. 关闭运行时
    let shutdown_result = shutdown_runtime_bundle(runtime.as_ref()).await?;
    run_result?;
    shutdown_result
}
```

### 后台服务

Stdio 渠道支持以下后台服务：
- **Cron 服务**：定时任务（由 `cron_tick_interval` 控制）
- **Heartbeat 服务**：运行时心跳（由 `runtime_tick_interval` 控制）

## 可观测性

### Tracing 日志

```rust
info!(session_key = %self.session_key, "stdio channel started");
```

日志内容：
- 渠道启动时的 `session_key`
- 运行时事件（来自底层）

### 终端日志

使用 `--verbose-terminal` 参数时，tracing 日志直接输出到终端：

```bash
klaw stdio --verbose-terminal
```

否则日志写入 `~/.klaw/logs/stdio.log`。

## 特点与限制

### 特点

- **简洁**：无需额外配置，开箱即用
- **交互友好**：清晰的输入输出提示
- **调试友好**：支持推理过程展示与日志直出
- **优雅关闭**：响应 Ctrl+C 与 SIGTERM

### 限制

- **不支持媒体**：无法发送或接收图片、文件等
- **单会话**：每次启动仅处理一个会话
- **同步交互**：必须等待响应后才能输入下一条

## 测试覆盖

`klaw-channel/src/stdio.rs` 当前覆盖：

| 测试 | 描述 |
|------|------|
| `keeps_explicit_session_key` | 验证手动指定的 session_key 被保留 |
| `hides_reasoning_when_flag_disabled` | 验证 `show_reasoning=false` 时不显示推理 |
| `renders_reasoning_block_when_enabled` | 验证 `show_reasoning=true` 时正确渲染推理块 |

## 使用场景

### 本地调试

```bash
# 快速测试 Agent 响应
klaw stdio --show-reasoning --verbose-terminal
```

### CLI 直接对话

```bash
# 日常使用
klaw stdio --session-key "stdio:daily"
```

### 脚本集成

```bash
# 在脚本中调用
echo "请帮我解释这段代码" | klaw stdio --session-key "stdio:script"
```

## 与其他渠道的对比

| 特性 | Stdio | DingTalk |
|------|-------|----------|
| 交互方式 | 终端输入输出 | 钉钉消息 |
| 媒体支持 | 无 | 图片、语音、文件 |
| 审批卡片 | 无 | 支持 |
| 多会话 | 否 | 是 |
| 回调机制 | 无 | 支持 |
| 适用场景 | 本地调试、CLI | 企业协作 |
