# Ask Question 工具

## 功能描述

`AskQuestion` 工具用于在会话中向用户创建带有多个选项的选择题，暂停当前执行流等待人工回复后继续。

典型场景：
- 决策分支：让用户选择下一步执行方向
- 信息确认：需要用户确认敏感操作
- 信息补充：询问用户额外输入信息

工具会将问题持久化存储在数据库中，支持跨会话异步回复。

## 配置

```toml
[tools.ask_question]
enabled = true
default_expires_minutes = 60  # 默认过期时间（分钟）
max_expires_minutes = 10080   # 最大过期时间（7天）
```

## 参数说明

### 创建问题

```json
{
  "action": "create",
  "question_text": "请选择部署环境",
  "title": "选择部署环境",
  "options": [
    {"id": "dev", "label": "开发环境"},
    {"id": "staging", "label": "测试环境"},
    {"id":prod", "label": "生产环境"}
  ],
  "expires_in_minutes": 30
}
```

参数：
- `action`: `"create"` - 创建新问题
- `question_text`: `string` - 问题描述文本
- `title`: `string` (可选) - 问题标题
- `options`: `array` - 选项列表，每个选项包含 `id` 和 `label`
- `expires_in_minutes`: `number` (可选) - 过期时间，默认 60 分钟

### 查询问题状态

```json
{
  "action": "get",
  "question_id": "uuid-xxx"
}
```

### 列出待回答问题

```json
{
  "action": "list",
  "status": "pending"
}
```

## 输出说明

创建成功后返回：
- 问题 ID
- 创建时间
- 过期时间

当查询到已回答问题时，返回用户选择的选项 ID。

## 使用示例

```
你需要部署应用到哪个环境？可用选项：
1. 开发环境 (dev)
2. 测试环境 (staging)
3. 生产环境 (prod)

请回复选项 ID。
```

## 工作原理

1. 工具在数据库创建 `pending` 状态的问题记录
2. 返回 `stop_current_turn` 信号暂停当前执行流
3. 用户通过渠道（GUI/TUI/IM）回复选择
4. 下次会话启动时自动处理回答，恢复执行流
