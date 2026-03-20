# CHANGELOG

## 2026-03-20

### Changed

- `dingtalk` 入站媒体提取范围从图片/语音扩展到视频与通用文件附件；只要消息体带有 `downloadCode` / `pictureDownloadCode`，都会进入统一 archive 归档链路

## 2026-03-19

### Fixed

- `dingtalk` HTTP client now applies a request timeout so session webhook delivery failures do not hang indefinitely and block manual cron execution flows

## 2026-03-16

### Changed

- `dingtalk` 审批卡片 ID 提取规则新增兼容：支持 `approvalId`（camelCase）与自然语言“批准ID”表述

## 2026-03-14

### Changed

- `dingtalk` 通道在 `debug` 日志输出订阅回调原始事件 payload（保留原始事件排查能力）
- README 补充 IM channel 适配契约，明确会话命令与 provider/model 路由由 runtime 统一处理

## 2026-03-15

### Changed

- `dingtalk` 通道新增 `richText` 解析，支持从 `content.richText[]` 提取文本片段与多图媒体引用
- `dingtalk` 图片消息现在会调用下载接口拉取原始文件，并在入站阶段写入 `klaw-archive`（落库 `archive.db` + 文件存储）
- 归档成功的媒体引用会补充 `archive.*` 元数据，并在体积允许时内联 `data:` URL 供模型请求直接携带图片
- `dingtalk` 音频消息新增转写支持：优先读取消息体 `recognition`，缺失时自动走钉钉 ASR（`media/upload` + `topapi/asr/voice/translate`）并回填文本
- 优化媒体下载策略：同一消息会依次尝试 `pictureDownloadCode` 与 `downloadCode`，提升下载成功率并细化失败日志
- `dingtalk` 通道新增 shell 审批卡片流：检测 `approval_id` 时优先发送 ActionCard（批准/拒绝按钮），并支持解析卡片回调事件转成 `/approve`、`/reject` 会话命令
- `dingtalk` 审批卡片触发条件现在同时支持文本 `approval_id=...` 与 JSON `approval.id` 形态（兼容独立 approval tool 输出）

### Fixed

- `dingtalk` websocket 主循环改为“收包优先 + 后台 tick 协作轮询”，避免 `on_cron_tick`/`on_runtime_tick` 阻塞导致的收包延迟和连接重置
- `dingtalk` 事件去重从无界 `HashSet` 改为带 TTL 与容量上限的去重器，避免长期运行内存持续增长
- `dingtalk` 在 runtime `submit` 失败时不再回 ACK，避免先去重再 ACK 导致失败事件被永久吞掉

### Changed

- `dingtalk` HTTP 客户端默认禁用系统代理；仅在 `proxy.enabled=true` 且提供 `proxy.url` 时显式走代理
- `dingtalk` 通道新增可取消运行入口，接收到 shutdown 信号时会主动发送 websocket `Close` 帧并等待短暂握手回包

## 2026-03-13

### Added

- 新增 `klaw-channel` crate README，说明通道职责与 `stdio` 交互模型

### Changed

- `stdio` 通道保持标准行输入，避免 raw mode 对终端和中文输入法的兼容性问题
- `stdio` 模式的 tracing 日志默认写入 `~/.klaw/logs/stdio.log`，避免覆盖当前输入行
- `ChannelRequest` 新增 `media_references` 字段，钉钉非文本消息会产出结构化媒体占位信息
- `stdio` 在等待后台 tick、agent 提交和普通输入期间都能响应 `SIGINT` / `SIGTERM`，避免仅在主提示符下可中断
