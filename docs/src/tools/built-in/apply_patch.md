# Apply Patch 工具设计与实现

本文档记录 `klaw-tool` 中 `apply_patch` 工具的实现：批量文件编辑、路径安全约束、操作验证与测试覆盖。

## 目标

- 提供批量文件编辑能力（添加、更新、删除、移动）
- 支持多操作原子性验证，避免部分成功导致的中间状态
- Workspace 边界约束与路径安全控制
- 结构化输出，明确操作结果

## 代码位置

- 工具实现：`klaw-tool/src/apply_patch.rs`
- 配置模型：`klaw-config/src/lib.rs`（`tools.apply_patch`）
- 工具注册：`klaw-cli/src/commands/runtime.rs`

## 参数模型（强约束）

`apply_patch` 使用强类型请求结构并开启 `deny_unknown_fields`。

### 请求结构

```json
{
  "operations": [
    {
      "op": "add_file",
      "path": "src/new.rs",
      "content": "pub fn hello() {}\n"
    },
    {
      "op": "update_file",
      "path": "src/lib.rs",
      "content": "mod new;\n"
    },
    {
      "op": "delete_file",
      "path": "src/old.rs"
    },
    {
      "op": "move_file",
      "from": "src/temp.rs",
      "to": "src/util/temp.rs"
    }
  ]
}
```

### 操作类型

| 操作 | 字段 | 描述 |
|------|------|------|
| `add_file` | `path`, `content` | 创建新文件，若文件已存在则失败 |
| `update_file` | `path`, `content` | 覆盖现有文件内容 |
| `delete_file` | `path` | 删除现有文件 |
| `move_file` | `from`, `to` | 移动/重命名文件，若目标已存在则失败 |

### 约束限制

- `operations` 数组不能为空
- `operations` 最大数量：50
- `content` 最大字节数：1,000,000
- 路径不能为空字符串

## 配置模型（`tools.apply_patch`）

```toml
[tools.apply_patch]
enabled = true
workspace = "/path/to/workspace"  # 可选，未设置时按工作空间解析链回退
allow_absolute_paths = false      # 是否允许绝对路径
allowed_roots = ["/allowed/root"] # 允许的额外根目录
```

## 路径解析与安全控制

### 工作空间解析

1. 优先从 `ctx.metadata["workspace"]` 获取工作空间
2. 否则使用配置中的 `workspace` 字段
3. 最后回退到数据目录下的 `workspace`：`(<storage.root_dir 或 ~/.klaw>)/workspace`

### 路径验证规则

1. **相对路径**：基于 workspace 解析
2. **绝对路径**：仅在以下情况允许：
   - `allow_absolute_paths = true`
   - 路径在 `allowed_roots` 列表中
   - 路径在 workspace 内部

### 路径存在性处理

- 对于存在的文件：canonicalize 后验证是否在允许路径内
- 对于不存在的文件：解析到最近的已存在祖先目录并验证

### 安全边界

```rust
// 示例：允许 workspace 外的特定目录
[tools.apply_patch]
allowed_roots = ["/tmp/klaw-projects", "~/projects"]
```

## 操作验证（两阶段提交语义）

在执行任何文件操作之前，`apply_patch` 会进行预验证：

### 第一阶段：路径解析与白名单验证

- 解析所有路径为绝对路径
- 验证所有路径都在允许范围内

### 第二阶段：操作语义验证

使用内存中的已知状态（`BTreeMap<PathBuf, bool>`）模拟操作：

| 操作 | 验证规则 |
|------|----------|
| `add_file` | 目标路径不能已存在 |
| `update_file` | 目标路径必须已存在 |
| `delete_file` | 目标路径必须已存在 |
| `move_file` | 源路径必须存在，目标路径不能存在 |

### 原子性保证

- 所有验证通过后才会执行实际文件操作
- 如果任何一个操作验证失败，整个请求会被拒绝
- 不会留下部分修改的中间状态

## 执行语义

### 目录创建

- `add_file` 和 `move_file` 会自动创建目标文件所在的父目录

### 文件操作

- 使用 `fs::write` 写入文件内容
- 使用 `fs::remove_file` 删除文件
- 使用 `fs::rename` 移动文件（原子操作，如果在同一文件系统）

## 输出格式

结构化 JSON 输出：

```json
{
  "action": "apply_patch",
  "operations_applied": 4,
  "summary": [
    "add_file /workspace/src/new.rs",
    "update_file /workspace/src/lib.rs",
    "delete_file /workspace/src/old.rs",
    "move_file /workspace/src/temp.rs -> /workspace/src/util/temp.rs"
  ]
}
```

## 工具元数据（LLM 提示）

```json
{
  "name": "apply_patch",
  "description": "Apply batched file patches inside the workspace. Use this tool to add, update, delete, or move multiple files in one request.",
  "parameters": {
    "type": "object",
    "description": "Batch file edits scoped to the current workspace. Prefer one request containing the full set of related file changes.",
    "properties": {
      "operations": {
        "type": "array",
        "maxItems": 50,
        "items": {
          "oneOf": [
            { "op": "add_file", ... },
            { "op": "update_file", ... },
            { "op": "delete_file", ... },
            { "op": "move_file", ... }
          ]
        }
      }
    }
  }
}
```

## 使用示例

### 示例 1：创建新文件

```json
{
  "operations": [
    {
      "op": "add_file",
      "path": "src/components/Button.tsx",
      "content": "export const Button = () => <button>Click</button>;\n"
    }
  ]
}
```

### 示例 2：批量更新代码

```json
{
  "operations": [
    {
      "op": "update_file",
      "path": "Cargo.toml",
      "content": "[package]\nname = \"my-app\"\nversion = \"0.2.0\"\n"
    },
    {
      "op": "update_file",
      "path": "src/main.rs",
      "content": "fn main() { println!(\"v0.2.0\"); }\n"
    }
  ]
}
```

### 示例 3：重构目录结构

```json
{
  "operations": [
    { "op": "add_file", "path": "src/utils/mod.rs", "content": "// mod\n" },
    { "op": "move_file", "from": "src/helper.rs", "to": "src/utils/helper.rs" },
    { "op": "delete_file", "path": "src/deprecated.rs" }
  ]
}
```

## 测试覆盖

`klaw-tool/src/apply_patch.rs` 当前覆盖：

- **基本多文件操作**：add → update → move → delete 完整流程
- **未知字段拒绝**：传入 `action` 等无效字段会报错
- **Workspace 越界阻断**：`/etc/hosts` 等路径被拒绝
- **批量验证原子性**：如果一个操作失败，所有操作都不执行
- **绝对路径允许**：`allow_absolute_paths = true` 时可写入 workspace 外
- **白名单根目录**：`allowed_roots` 配置的目录可被访问

## 最佳实践

1. **单次请求包含完整变更集**：相关的文件修改应该放在同一个请求中
2. **使用相对路径**：提高可移植性，避免硬编码绝对路径
3. **先验证后执行**：工具会自动验证所有操作，确保原子性
4. **大文件注意限制**：单个文件内容不超过 1MB

## 与其他工具的关系

- **vs `shell` 工具**：`shell` 工具会拦截 `apply_patch` 命令，引导使用专用工具
- **vs `fs` 工具**：`apply_patch` 专注于批量编辑，`fs` 工具（如存在）可能提供更细粒度的操作
