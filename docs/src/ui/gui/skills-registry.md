# 技能注册表面板

## 功能说明

浏览远程技能注册表，搜索、预览、安装新技能到本地。

## 核心功能

- 从注册表拉取技能列表
- 按分类搜索技能
- 查看技能详情（描述、作者、示例）
- 一键安装技能到本地
- 查看已安装状态

## 注册表格式

技能注册表是一个 JSON 文件，格式：

```json
{
  "skills": [
    {
      "id": "skill-id",
      "name": "Skill Name",
      "description": "Description",
      "author": "Author",
      "repository": "https://github.com/..."
    }
  ]
}
```
