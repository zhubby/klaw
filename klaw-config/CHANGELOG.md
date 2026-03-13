# CHANGELOG

## 2026-03-13

### Added

- 新增根级可选 `model` 配置，用于覆盖活跃 `model_provider` 的 `default_model`。

### Fixed

- 配置校验新增 `model` 非空白约束，避免设置空字符串导致运行时模型选择异常。
