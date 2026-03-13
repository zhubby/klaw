# Archive 存储

`klaw-archive` 负责把媒体文件写入本地数据目录，并在 `archive.db` 中建立索引。

## 存储布局

```text
~/.klaw/
├── archive.db
└── archives/
    └── YYYY-MM-DD/
        └── <uuid>.<ext>
```

## 记录字段

| 字段 | 说明 |
|------|------|
| `id` | 归档记录主键 |
| `source_kind` | 来源类型：`user_upload/channel_inbound/model_generated` |
| `media_kind` | 识别后的媒体类别：`pdf/image/video/audio/other` |
| `mime_type` | 依据魔数推断出的 MIME 类型 |
| `extension` | 最终用于落盘的扩展名 |
| `original_filename` | 原始文件名 |
| `content_sha256` | 内容哈希 |
| `size_bytes` | 文件大小 |
| `storage_rel_path` | 相对数据目录的文件路径 |
| `session_key` | 关联会话键（可选） |
| `channel` | 来源通道（可选） |
| `chat_id` | 对话 ID（可选） |
| `message_id` | 来源消息 ID（可选） |
| `metadata_json` | 扩展元数据 |
| `created_at_ms` | 归档时间 |

## 去重语义

- 写盘前先计算内容 `sha256`
- 如果已有相同内容，则复用已存在的物理文件路径
- 每次归档仍会插入一条新的 archive record，保留来源和上下文审计信息

## 文件识别

识别优先级：

1. 文件魔数
2. 原始文件扩展名兜底

首版覆盖：

- PDF
- 图片：JPEG / PNG / GIF / WebP / BMP
- 视频：MP4 / MOV / AVI / MKV / WebM
- 音频：MP3 / WAV / OGG / M4A / AAC

## 查询能力

`ArchiveService` 首版支持：

- `ingest_path`
- `ingest_bytes`
- `find`
- `get`
- `open_download`
