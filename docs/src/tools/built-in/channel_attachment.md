# Channel Attachment 工具

## 功能描述

`ChannelAttachment` 工具用于在输出渠道发送附件，支持：
- 图片附件
- 文件附件
- 归档文件中的附件引用

附件会通过渠道特定的格式展示给用户，GUI/IM 渠道会正确渲染图片和文件下载链接。

## 配置

```toml
[tools.channel_attachment]
enabled = true

[tools.channel_attachment.local_attachments]
base_url = "http://localhost:8080/attachments"
```

## 参数说明

### 发送归档中的图片

```json
{
  "source_type": "archive",
  "archive_id": "abc123-def",
  "kind": "image",
  "caption": "架构图"
}
```

### 发送本地文件

```json
{
  "source_type": "local",
  "path": "/path/to/file.pdf",
  "filename": "report.pdf",
  "kind": "file",
  "caption": "项目报告"
}
```

参数：
- `source_type`: `"archive"` | `"local"` - 附件来源
- `archive_id`: 归档 ID（`source_type=archive` 时必填）
- `path`: 本地文件路径（`source_type=local` 时必填）
- `filename`: 文件名（可选，默认从路径提取）
- `kind`: `"image"` | `"file"` | `"auto"` - 附件类型，默认 `auto` 根据扩展名判断
- `caption`: 说明文字（可选）

## 输出说明

工具发送成功后会通过 `channel_attachment` 信号通知渠道渲染附件，返回操作状态给模型。

## 使用场景

- 生成图片后展示给用户
- 生成报告/数据文件提供下载
- 从归档中分享历史文件
