# Changelog

## 2026-03-16

### Fixed
- registry sync now recovers from stale git lock files such as `.git/shallow.lock` by removing the leftover lock and retrying once

### Added
- added registry catalog listing and registry-specific managed uninstall APIs for GUI skill installation workflows

## 2026-03-14

### Changed
- switched managed registry skill installation to manifest indexing (`skills-registry-manifest.json`) instead of copying files into `~/.klaw/skills`
- changed `list/get/load_all_skill_markdowns` to merge managed registry skills from `~/.klaw/skills-registry` with local manual skills in `~/.klaw/skills`
- added managed-over-local precedence for same-name skills, with local conflict entries skipped
- added `stale_registries` tracking in manifest and exposed stale state in loaded skill metadata

### Added
- added source metadata fields to `SkillSummary` / `SkillRecord`: `source_kind`, `registry`, `stale`
- added filesystem-store APIs for managed install/uninstall:
  - `install_registry_skill`
  - `uninstall_skill`
