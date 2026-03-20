# Archive Tool

`archive` 工具用于把当前消息中的归档附件暴露给模型，并强制遵循“archive 只读、编辑前先复制到 workspace”的工作流。

## 目标

- 让模型知道当前消息附件对应的 `archive_id` 与 `storage_rel_path`
- 允许模型只读查看归档记录与文本类归档内容
- 如果后续需要改写、转换、生成衍生文件，先把归档文件复制到 `workspace/`

## 只读规则

`archives/` 下的文件视为原始归档材料：

- 只读
- 不直接编辑
- 不直接移动
- 不直接删除

如果模型要修改文件，正确流程是：

1. 调用 `archive.copy_to_workspace`
2. 在 `workspace/` 中对复制后的文件做编辑或转换
3. 保留 `archives/` 中的原件不变

## 模型可见上下文

当当前消息包含已归档附件时，runtime 会在用户消息里追加附件摘要，字段包括：

- `filename`
- `archive_id`
- `storage_rel_path`
- `mime_type`
- `size_bytes`
- `access=read_only`
- “如果需要修改，先复制到 workspace” 的提示

同时，这些附件也会写入工具上下文 metadata，供 `archive` 工具直接读取。

## 支持的操作

### `list_current_attachments`

列出当前用户消息中的归档附件句柄。

示例：

```json
{
  "action": "list_current_attachments"
}
```

适用场景：

- 模型想先确认当前消息到底带了哪些附件
- 需要拿到 `archive_id` 作为后续操作输入

### `get`

按 `archive_id` 查看归档记录。

示例：

```json
{
  "action": "get",
  "archive_id": "arch-123"
}
```

返回重点：

- 归档记录元信息
- `storage_rel_path`
- `mime_type`
- `size_bytes`

### `read_text`

只读读取 UTF-8 文本类归档文件。

示例：

```json
{
  "action": "read_text",
  "archive_id": "arch-123",
  "max_chars": 12000
}
```

适用场景：

- `.md`
- `.txt`
- `.json`
- 代码文件
- 其他 UTF-8 文本

限制：

- 非 UTF-8 二进制文件不会被当作文本直接读出
- 若文件需要转换处理，应先复制到 `workspace/`

### `copy_to_workspace`

把归档文件复制到 `workspace/`，供后续工具安全修改。

示例：

```json
{
  "action": "copy_to_workspace",
  "archive_id": "arch-123",
  "destination_path": "attachments/report.pdf"
}
```

若不传 `destination_path`，工具会根据归档元信息自动推导目标文件名。

返回重点：

- `workspace_path`
- `workspace_rel_path`
- `next_step=edit_or_transform_the_workspace_copy_only`

## 推荐工作流

### 读取并总结 PDF/文本附件

1. `list_current_attachments`
2. `get`
3. 如果是文本类：`read_text`
4. 输出总结

### 修改或转换附件

1. `list_current_attachments`
2. `copy_to_workspace`
3. 对 `workspace/` 下复制出的文件使用 `shell`、`apply_patch` 或其他工具
4. 不碰 `archives/` 下原件

## 配置

```toml
[tools.archive]
enabled = true
```

默认启用。关闭后，模型将看不到该工具定义，但当前消息附件摘要仍会保留在用户消息文本中。

## 代码位置

- 工具实现：`klaw-cli/src/runtime/archive_tool.rs`
- 附件摘要注入：`klaw-core/src/agent_loop.rs`
- 归档服务：`klaw-archive/src/lib.rs`
