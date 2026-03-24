# klaw-skill

`klaw-skill` provides the skills storage and lifecycle layer for Klaw.

## Capabilities

- Manage local manual skills under `~/.klaw/skills`
- Sync configured registry mirrors under `~/.klaw/skills-registry`
- Sync individual registry sources on demand and delete source mirrors while cleaning manifest state
- Recover from stale git lock files during registry sync by removing leftover `*.lock` files and retrying once
- Index managed registry installations via `~/.klaw/skills-registry-manifest.json`
- Expose a registry interface for syncing/deleting sources plus listing, reading, and searching synced registry skills discovered by recursively scanning each registry git repository for `SKILL.md` / `skill.md`
- Expose a manager interface for installed-skill install/uninstall/list/show/load-all flows
- Load a merged runtime view of installed skills (managed registry + local manual)
- Expose source metadata for each loaded skill (`local` vs `registry`, registry name, stale state)

## Architecture

- `model.rs`: shared skill models (`SkillSource`, `SkillSummary`, `SkillRecord`)
- `store.rs`: split `SkillsRegistry` / `SkillsManager` traits
- `fs_store.rs`: default filesystem implementation, registry sync, manifest indexing, registry/manager composition
- `fetcher.rs`: network fetch abstraction (`SkillFetcher`) + reqwest implementation
- `error.rs`: `SkillError` domain error model

## Managed Registry Index Model

- `managed`: installed registry skills (`registry + skill`)
- `registry_commits`: latest known commit for each registry mirror
- `stale_registries`: registries currently served from local mirror cache after sync failure

Managed installs are indexed in manifest and read directly from registry mirrors. Local manual skills remain supported and are merged at read time.
