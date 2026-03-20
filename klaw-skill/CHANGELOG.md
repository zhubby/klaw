# Changelog

## 2026-03-20

### Added
- added `cleanup_registry` method to `FileSystemSkillStore` to remove all installed skills and registry metadata for a given registry name

## 2026-03-18

### Changed
- split the public skills API into `SkillsRegistry` and `SkillsManager` traits instead of one mixed store trait
- renamed filesystem-store installed-skill methods to manager-oriented names such as `install_from_registry`, `list_installed`, `get_installed`, and `load_all_installed_skill_markdowns`
- moved registry catalog search/list/show responsibilities into the registry-facing API while keeping installed-skill merge/load behavior in the manager-facing API

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
