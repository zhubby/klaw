# Cron Manager 工具

## 功能描述

`CronManager` 工具用于管理定时任务，支持：
- 创建定时任务
- 列出已计划任务
- 查询任务状态
- 删除任务

定时任务会在指定时间触发执行，可以用于：
- 定期提醒
- 数据备份
- 日报周报生成
- 定期系统检查

## 配置

```toml
[tools.cron_manager]
enabled = true
max_jobs = 100          # 每个用户最大任务数
default_timezone = "Asia/Shanghai" # 默认时区
```

## 参数说明

### 创建定时任务

```json
{
  "action": "create",
  "schedule": "0 9 * * *",
  "description": "每日站会提醒",
  "payload": {
    "message": "请生成今日工作进展报告",
    "session_key": "daily:standup"
  },
  "timezone": "Asia/Shanghai",
  "enabled": true
}
```

参数：
- `action`: `"create"` - 创建新任务
- `schedule`: `string` - Cron 表达式 `分 时 日 月 周`
- `description`: `string` - 任务描述
- `payload`: `object` - 触发时传递的 JSON 负载
- `timezone`: `string` (可选) - 时区，默认使用系统时区
- `enabled`: `boolean` (可选) - 是否启用，默认 `true`

### 列出任务

```json
{
  "action": "list",
  "limit": 20,
  "enabled_only": true
}
```

### 查询任务详情

```json
{
  "action": "get",
  "job_id": "uuid-xxx"
}
```

### 更新任务

```json
{
  "action": "update",
  "job_id": "uuid-xxx",
  "patch": {
    "enabled": false,
    "schedule": "0 10 * * *"
  }
}
```

### 删除任务

```json
{
  "action": "delete",
  "job_id": "uuid-xxx"
}
```

### 列出历史运行记录

```json
{
  "action": "history",
  "job_id": "uuid-xxx",
  "limit": 10
}
```

## Cron 表达式说明

标准 Cron 格式：

```
┌────────── 分钟 (0 - 59)
│ ┌──────── 小时 (0 - 23)
│ │ ┌────── 日期 (1 - 31)
│ │ │ ┌──── 月份 (1 - 12)
│ │ │ │ ┌── 星期 (0 - 6) (0=周日, 1=周一 ... 6=周六)
│ │ │ │ │
* * * * *
```

常用示例：
- `0 9 * * *` - 每天上午 9 点
- `*/30 * * * *` - 每 30 分钟
- `0 0 * * 1` - 每周一凌晨
- `0 9 1 * *` - 每月第一天上午 9 点

## 输出说明

创建成功返回任务 ID 和调度信息。列出任务返回任务概况列表。

## 工作原理

任务持久化存储在 `CronStorage`，后台调度器定期扫描触发到期任务，触发后投递到 `agent.inbound` 主题。

## 使用场景

- 每日/每周/每月定时报告
- 定时备份数据
- 周期性提醒
- 定时系统健康检查
