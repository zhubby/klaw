# 模型提供者面板

## 功能说明

管理 LLM 模型提供者配置，添加、编辑、测试不同的 LLM API 服务商。

## 核心功能

- 列出所有已配置模型提供者
- 添加新提供者
- 编辑提供者配置（API Key、Base URL、模型默认参数）
- 删除提供者
- 测试连接有效性
- 设置默认提供者

## 支持的提供者类型

| 类型 | 说明 |
|------|------|
| openai | OpenAI 兼容 API |
| anthropic | Anthropic Claude |
| google | Google Gemini |
| ollama | 本地 Ollama |
| openrouter | OpenRouter |
| deepseek | DeepSeek |
| 更多... | 可扩展 |

## 配置示例

```toml
[model_providers.my-openai]
type = "openai"
api_key = "sk-xxx"
base_url = "https://api.openai.com/v1"
default_model = "gpt-4o"
```
