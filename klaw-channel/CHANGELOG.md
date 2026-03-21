# CHANGELOG

## 2026-03-21

### Added

- 新增 Telegram 后台出站发送入口，供 runtime/cron 后台分发器直接通过 Bot API 把非交互式结果推送回原聊天

## 2026-03-20

### Added

- `ChannelRuntime` / channel adapters now support streaming snapshot callbacks; Telegram can render incremental edits when `stream_output=true`, while unsupported channels keep the existing final-message fallback
- 新增通用 `ChannelManager`、`ChannelConfigSnapshot`、`ChannelInstanceConfig`、`ManagedChannelDriver` 与 `ChannelDriverFactory`，统一管理多实例 channel 生命周期和配置 diff
- 新增 `telegram` channel：基于 Bot API `getUpdates` long polling，支持文本、caption、图片和文件消息入站，并复用 archive 媒体归档链路
- `telegram` 通道新增 inline keyboard 审批交互：检测 `approval_id` 时发送 `Approve` / `Reject` 按钮，并支持 `callback_query` 回调转成 `/approve`、`/reject`

### Changed

- `dingtalk` 现在通过通用 managed driver 接口接入 `ChannelManager`，不再依赖 CLI 层专用 pool
- `ChannelManager` / `ChannelConfigSnapshot` / 默认 driver factory 现在支持 `telegram` 实例
- `telegram` 实现已拆分为 `klaw-channel/src/telegram/` 模块目录，并新增 Telegram 专用 HTML 渲染层，修复直接发送普通 Markdown 导致的格式错乱
- `telegram` 轮询主循环改为后台 long polling 任务 + 主循环消费结果，避免被 runtime tick 取消后高频重建连接
- `telegram` 入站媒体范围从图片/文件扩展到 `audio` / `voice` / `video`
- `dingtalk` 入站媒体提取范围从图片/语音扩展到视频与通用文件附件；只要消息体带有 `downloadCode` / `pictureDownloadCode`，都会进入统一 archive 归档链路
- `dingtalk` 入站媒体引用现在会额外提取消息体中的 `mimeType` / `contentType` / `fileType` / `extension`，补充到媒体 metadata 并透传声明 MIME
- 抽出 `klaw-channel::media` / `klaw-channel::render` 共享模块，沉淀媒体引用构造、archive 回填和输出渲染逻辑，减少 `dingtalk.rs` 与后续 channel 的重复实现
- `dingtalk` 审批 ActionCard 现在会优先显示结构化 `command_preview`，明确展示待执行命令而不只显示 approval id

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
