# Archive 存储

`klaw-archive` 负责把媒体文件写入本地数据目录，并在 `archive.db` 中建立索引。`SqliteArchiveService` 通过 `klaw-storage` 的 `DefaultArchiveDb` 提供异步 SQL 访问。

## 存储布局

`StoragePaths` 定义 `~/.klaw/` 下的目录结构：

```text
~/.klaw/
├── archive.db          # archive 索引（独立 SQLite）
├── archives/
│   └── YYYY-MM-DD/     # 按日期分目录
│       └── <uuid>.<ext> # 物理文件（扩展名由 sniff 决定）
├── klaw.db             # session/cron/heartbeat/audit 等
├── memory.db           # long-term memory
├── config.toml
├── sessions/           # JSONL 聊天历史
├── workspace/          # 工作区（copy_to_workspace 目标）
└── tmp/
```

物理文件路径由 `storage_rel_path` 表示（如 `archives/2026-04-23/550e8400-e29b.pdf`），绝对路径通过 `archive_absolute_path(root_dir, storage_rel_path)` 计算。

## 记录字段

`archives` 表完整字段：

| 字段 | 类型 | 说明 |
|------|------|------|
| `id` | TEXT PK | 归档记录主键（UUID v4） |
| `source_kind` | TEXT NOT NULL | 来源类型（见枚举章节） |
| `media_kind` | TEXT NOT NULL | 识别后的媒体类别（见枚举章节） |
| `mime_type` | TEXT? | 依据魔数推断出的 MIME 类型 |
| `extension` | TEXT? | 最终用于落盘的扩展名 |
| `original_filename` | TEXT? | 原始文件名 |
| `content_sha256` | TEXT NOT NULL | 内容 SHA-256 哈希 |
| `size_bytes` | INTEGER NOT NULL | 文件大小（字节） |
| `storage_rel_path` | TEXT NOT NULL | 相对数据目录的文件路径 |
| `session_key` | TEXT? | 关联会话键 |
| `channel` | TEXT? | 来源通道 |
| `chat_id` | TEXT? | 对话 ID |
| `message_id` | TEXT? | 来源消息 ID |
| `metadata_json` | TEXT NOT NULL | 扩展元数据 JSON |
| `created_at_ms` | INTEGER NOT NULL | 归档时间（毫秒 epoch） |

## 枚举类型

### ArchiveSourceKind

| 值 | 说明 |
|------|------|
| `user_upload` | 用户主动上传的文件 |
| `channel_inbound` | 渠道入站附件（如 WebSocket/Telegram 附件） |
| `model_generated` | LLM/工具生成的文件（如 TTS 合成音频） |

### ArchiveMediaKind

| 值 | 说明 |
|------|------|
| `pdf` | PDF 文档 |
| `image` | 图片（JPEG/PNG/GIF/BMP/WebP） |
| `video` | 视频（MP4/MOV/AVI/MKV/WebM） |
| `audio` | 音频（MP3/WAV/OGG/M4A/AAC） |
| `other` | 其他类型 |

## 输入结构

### ArchiveIngestInput

调用 `ingest_path` / `ingest_bytes` 时需要提供的输入：

| 字段 | 类型 | 说明 |
|------|------|------|
| `source_kind` | ArchiveSourceKind | 来源类型 |
| `filename` | String? | 原始文件名（可选，用于 sniff 扩展名兜底） |
| `declared_mime_type` | String? | 声明的 MIME 类型（可选，当前未用于识别） |
| `session_key` | String? | 关联会话键 |
| `channel` | String? | 来源通道 |
| `chat_id` | String? | 对话 ID |
| `message_id` | String? | 来源消息 ID |
| `metadata` | serde_json::Value | 扩展元数据（序列化后存入 `metadata_json`） |

### ArchiveQuery

查询过滤条件：

| 字段 | 类型 | 说明 |
|------|------|------|
| `session_key` | String? | 按会话键过滤 |
| `chat_id` | String? | 按对话 ID 过滤 |
| `source_kind` | ArchiveSourceKind? | 按来源类型过滤 |
| `media_kind` | ArchiveMediaKind? | 按媒体类别过滤 |
| `filename` | String? | 按原始文件名模糊匹配（`LIKE %keyword% ESCAPE '\\'`） |
| `limit` | i64 | 最大返回条数（≤0 时默认 20） |
| `offset` | i64 | 分页偏移 |

### ArchiveBlob

下载操作返回的结构：

| 字段 | 类型 | 说明 |
|------|------|------|
| `record` | ArchiveRecord | 对应的索引记录 |
| `absolute_path` | PathBuf | 物理文件绝对路径 |
| `bytes` | Vec<u8> | 文件内容字节 |

## 去重语义

`ingest_path` / `ingest_bytes` 都遵循相同的去重流程：

1. 计算内容指纹（`FileFingerprint`）：流式 SHA-256 + 文件大小 + 前 64 字节 header；
2. 用 `content_sha256` 查询已有记录（`lookup_existing_by_hash`）；
3. **若找到相同哈希**：复用已有记录的 `storage_rel_path` 和 `media_kind`/`mime_type`/`extension`，插入一条新索引行（保留来源和上下文审计信息），不写新物理文件；
4. **若未找到**：对 header 做媒体识别（`sniff_media`），写入物理文件，插入索引行。

每条 ingest 都会产生一条新的 `ArchiveRecord`，即使内容相同。这确保了不同来源/上下文有独立的审计记录。

## 文件写入安全

物理文件写入采用 temp file + atomic rename 模式：

1. 在目标目录下创建 `<uuid>.<ext>.tmp` 临时文件；
2. 写入全部内容并 flush；
3. `drop` 文件句柄；
4. `fs::rename` 将 `.tmp` 文件原子重命名为最终文件名；
5. 若索引插入失败且本次创建了新文件（`created_file = true`），在 `insert_with_cleanup` 中删除已写入的物理文件。

这确保：
- 写入中途崩溃不会留下半成品文件（`.tmp` 文件不会被查询到）；
- 重命名操作在 POSIX 系统上是原子的；
- 索引与物理文件的一致性由 cleanup 逻辑保障。

## 文件识别

`sniff_media` 优先基于文件魔数（前 64 字节 header）识别，未命中时通过原始文件扩展名兜底（`fallback_from_extension`）。

### 魔数识别覆盖

| 类型 | 魔数 / 特征 | MIME | 扩展名 |
|------|------------|------|--------|
| PDF | `%PDF-` | `application/pdf` | `pdf` |
| JPEG | `FF D8 FF` | `image/jpeg` | `jpg` |
| PNG | `89 50 4E 47 0D 0A 1A 0A` | `image/png` | `png` |
| GIF | `GIF87a` / `GIF89a` | `image/gif` | `gif` |
| BMP | `BM` | `image/bmp` | `bmp` |
| WebP | `RIFF...WEBP` | `image/webp` | `webp` |
| MP4 | `ftyp` + brand ≠ M4A/M4B/M4P/qt | `video/mp4` | `mp4` |
| MOV | `ftyp qt  ` | `video/quicktime` | `mov` |
| M4A | `ftyp M4A / M4B / M4P` | `audio/mp4` | `m4a` |
| AVI | `RIFF...AVI ` | `video/x-msvideo` | `avi` |
| MKV | `1A 45 DF A3` + 含 `webm` tag → WebP；否则 | `video/x-matroska` | `mkv` |
| WebM | `1A 45 DF A3` + 含 `webm` tag | `video/webm` | `webm` |
| WAV | `RIFF...WAVE` | `audio/wav` | `wav` |
| OGG | `OggS` | `audio/ogg` | `ogg` |
| MP3 | `ID3` 或 MPEG frame sync `FF E0` | `audio/mpeg` | `mp3` |
| AAC | `FF F0` / `FF F2` sync | `audio/aac` | `aac` |

未命中魔数且无扩展名 → `ArchiveMediaKind::Other`，`mime_type = None`，`extension = None`。

扩展名兜底映射覆盖上述所有类型，未知扩展名 → `Other` + 保留扩展名。

## 索引优化

`init_schema` 创建以下索引：

| 索引 | 用途 |
|------|------|
| `idx_archives_created_at_ms` (DESC) | 按时间排序/分页 |
| `idx_archives_content_sha256` | 去重查找 + 按哈希查重 |
| `idx_archives_session_key` | 按会话检索 |
| `idx_archives_chat_id` | 按对话检索 |
| `idx_archives_source_kind` | 按来源类型过滤 |
| `idx_archives_media_kind` | 按媒体类别过滤 |

## ArchiveService trait

后端无关的 archive 服务接口：

| 方法 | 说明 |
|------|------|
| `ingest_path(input, source_path)` | 从本地文件路径归档：指纹 → 去重检查 → sniff → 写文件 → 插入索引 |
| `ingest_bytes(input, bytes)` | 从内存字节归档：指纹 → 去重检查 → sniff → 写文件 → 插入索引 |
| `find(query)` | 多条件过滤查询（session_key, chat_id, source_kind, media_kind, filename LIKE），按 `created_at_ms DESC` 排序分页 |
| `get(archive_id)` | 按主键读取单条记录，不存在则返回 `NotFound` 错误 |
| `open_download(archive_id)` | 读取记录 + 加载物理文件字节，返回 `ArchiveBlob` |
| `list_session_keys()` | 返回所有有归档记录的 distinct session_key 列表 |

`SqliteArchiveService` 是当前唯一实现，通过 `DefaultArchiveDb` 执行 SQL。可通过 `open_default_archive_service()` 打开默认实例。

## Runtime 集成

### 初始化

`build_runtime_bundle` 根据配置决定是否初始化 archive 服务：

- 当 `config.gateway.enabled` 或 `config.tools.archive.enabled()` 为 true 时，创建 `Arc<dyn ArchiveService>`；
- `VoiceTool` 启用时也需要 archive 服务（用于存储 TTS 合成音频）；
- 否则 `archive_service = None`，相关工具不注册。

### MediaReference 流转

渠道入站附件通过 `MediaReference` 进入 runtime，`source_kind` 标记为 `UserUpload`。WebSocket 附件的 metadata 中包含 `archive.id`、`archive.mime_type`、`archive.size_bytes` 等字段，供 tool 上下文使用。

## ArchiveTool

`klaw-tool` 中的 `ArchiveTool` 提供只读查询和 copy-to-workspace 操作，依赖 `ArchiveService` + `SessionStorage`（用于会话范围解析）：

### 支持的 action

| action | 说明 |
|--------|------|
| `get` | 按 `archive_id` 读取单条记录详情 |
| `list_current_attachments` | 从当前消息上下文的 `metadata` 中提取附件列表 |
| `list_session_attachments` | 查询当前会话链（base + active）的归档记录 |
| `read_text` | 读取归档文件的文本内容（UTF-8），支持 `max_chars` 截断（默认 20000，最大 100000） |
| `copy_to_workspace` | 将归档文件复制到 workspace 目录，支持 `destination_path` 指定相对路径 |

### 会话范围解析

`resolve_session_scope` 合并 base session 和 active session 的记录：

1. 从 `metadata["channel.base_session_key"]` 或 `get_session_by_active_session_key` 解析 base session；
2. 查找 base session 的 `active_session_key`；
3. 收集 `[base_session_key, active_session_key, ctx.session_key]` 去重后的会话列表；
4. 对每个 session_key 查询 `ArchiveQuery { session_key, limit }`，默认上限 `DEFAULT_SESSION_ATTACHMENT_LIMIT`；
5. 按 `archive.id` 去重合并结果。

### workspace 安全

- `destination_path` 必须是相对路径，不能包含 `..` 或绝对路径前缀；
- 目标路径必须在 workspace 目录内；
- 若未指定 `destination_path`，从 `original_filename` 或 `archive-{id}.{ext}` 推导默认文件名。

## VoiceTool 集成

`VoiceTool` 持有 `Arc<dyn ArchiveService>`，在 TTS 合成时调用 `ingest_bytes` 归档生成的音频：

- `build_archive_input` 构造 `ArchiveIngestInput`，`source_kind = ModelGenerated`；
- `synthesize_and_archive` 返回 `(TtsOutput, ArchiveRecord)`；
- STT 操作 (`stt`) 从 `archive_id` 加载音频文件进行转录；
- `ensure_audio_record` 校验归档记录必须是音频类型。

## ChannelAttachmentTool 集成

`ChannelAttachmentTool` 支持两种附件来源：

- `ArchiveId { archive_id }`：从 archive 加载文件并发送到渠道；
- `LocalPath { path }`：从本地路径（workspace 或 allowlist 内）加载文件；
- `kind` 字段支持 `auto`（根据 `ArchiveMediaKind` 自动选择 image/file）、`image`、`file`。

## GUI ArchivePanel

桌面 GUI 的 `ArchivePanel` 提供归档记录浏览和预览：

- **过滤器**：session_key（下拉选择）、chat_id、source_kind、media_kind、filename 模糊搜索；
- **分页**：page + size 参数，offset = `(page - 1) * size`；
- **列表**：`TableBuilder` 渲染归档记录表格；
- **预览**：`open_download` 加载文件字节 → `build_preview` 生成 egui 可渲染的预览（图片/文本等）；
- **详情**：点击记录查看完整字段；
- `load_filters` 通过 `list_session_keys()` 填充 session_key 下拉选项。

## 错误处理

`ArchiveError` 主要变体：

| 变体 | 说明 |
|------|------|
| `Storage` | 来自 `klaw-storage` 的底层 SQL 错误 |
| `InvalidQuery` | 无效查询参数 |
| `NotFound` | 指定 archive_id 不存在 |
| `SerializeMetadata` | JSON 序列化失败 |
| `ReadFile { path, source }` | 文件读取失败（含路径信息） |
| `WriteFile { path, source }` | 文件写入失败 |
| `RenameFile { from, to, source }` | 原子重命名失败 |