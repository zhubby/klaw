# Changelog

## 2026-03-13

### Changed
- renamed the file mutation tool from `fs` to `apply_patch`
- refactored the `apply_patch` tool to expose only batched file mutations
- tightened the `apply_patch` request schema and tool description around multi-file edit workflows

### Fixed
- validated all `apply_patch` operations before applying changes so invalid later steps do not partially mutate earlier files
