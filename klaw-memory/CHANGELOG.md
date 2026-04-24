# CHANGELOG

## 2026-04-24

### Added

- `SummaryGenerator` async trait: pluggable strategy for generating archive summary content
- `TemplateSummaryGenerator`: default implementation that preserves the existing template-based concatenation as fallback
- `ArchiveGroupKey` is now public, enabling external crates to implement custom `SummaryGenerator` implementations
- `archive_stale_long_term_memories` accepts `Arc<dyn SummaryGenerator>` parameter; LLM call failures automatically fall back to template concatenation with a warn log

### Changed

- `archive_stale_long_term_memories` signature now requires `summary_generator: Arc<dyn SummaryGenerator>` parameter; all callers (runtime, GUI, tests) must pass the appropriate implementation
- `klaw-memory` no longer directly owns summary content generation logic; the strategy is injected by the caller



## 2026-05-12

### Changed

- 摘要记录（`summary=true`）不再从 system prompt 渲染中被硬排除，改为与普通 active 记录一起参与排序和预算竞争；预算机制（`max_items`、`max_chars`）自然调控摘要的可见性，避免归档后信息从 LLM 视角彻底消失
- 摘要内容格式从内部日志风格 `"Archived N low-priority memories for {label}: ... | ..."` 改为更自然的 `"Past notes on {label} ({total} entries): ...; ..."`, 不再暴露 "archived" / "low-priority" 等内部分类术语，对 LLM 上下文更友好；分隔符从 `|` 改为 `;`

## 2026-04-23

### Added

- 长期记忆治理新增 `priority` 元数据校验与默认优先级推导，可显式覆盖 prompt 注入顺序

### Changed

- 长期记忆 prompt 渲染现在优先按显式 `priority` 排序，再回退到 `kind` 优先级
- `SqliteMemoryService::search` 会统一过滤 `long_term` scope 下的 `archived` / `rejected` / `superseded` 记录，避免旧事实继续命中检索
- 低优先级长期记忆现在支持后台自动归档，并按 `kind + topic` 生成 `summary=true` 的摘要索引记录

## 2026-03-31

### Added

- `SqliteMemoryStatsService` 新增 `list_scope_records(scope)`，支持按 scope 返回完整 memory 记录明细，供 GUI detail 弹窗直接消费

## 2026-03-15

### Added

- `SqliteMemoryStatsService` for memory-layer statistics aggregation
- memory stats model types: `MemoryStats` and `ScopeStat`

### Changed

- GUI `Memory` panel can consume `klaw-memory` stats abstraction instead of placeholder content
