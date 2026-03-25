---
name: github-release-main
description: Enforce a GitHub release workflow for this Rust workspace. Use when Codex needs to publish a release by updating the root `Cargo.toml` version, defaulting to a patch bump when no version is provided, committing the change, creating a `vx.y.z` tag, pushing the branch and tag, and creating a GitHub release titled `release-vx.y.z`. Only operate on the `main` branch; if the current branch is not `main`, switch back before doing any release actions.
---

# Github Release Main

## Overview

Follow one deterministic release path:
- ensure the repo is on `main`
- ensure local `main` matches `origin/main`
- update the root `Cargo.toml` version
- commit the version bump
- create tag `vx.y.z`
- push `main` and the tag
- create a GitHub release named `release-vx.y.z`

Use `scripts/release_main.py` for the full one-command workflow. Use `scripts/bump_root_cargo_version.py` only when you need the version update step by itself.

## Release Workflow

1. Inspect git status before making changes. If the working tree contains unrelated or unexpected changes, stop and ask the user how to proceed.
2. Check the current branch. If it is not `main`, switch to `main` before continuing.
3. Fetch the configured remote `main` and confirm local `main` is exactly in sync with `origin/main`. If local `main` is ahead, behind, or diverged, stop and ask the user to reconcile it first.
4. Re-read the root `Cargo.toml` and determine the release version:
   - use the user-provided version when present
   - otherwise bump the patch part of `[workspace.package].version`
5. Prefer running `scripts/release_main.py` to execute the workflow end-to-end.
6. If running the steps manually, stage only the root `Cargo.toml` version change unless the user asked for more.
7. Commit with a release-oriented conventional commit, for example `chore(release): cut vX.Y.Z`.
8. Create the git tag exactly as `vX.Y.Z`.
9. Push `main` and push the new tag.
10. Create the GitHub release with title `release-vX.Y.Z`, using the same tag.
11. Report the final version, commit hash, tag, and release URL back to the user.

## Branch Rules

- Never perform the release on feature branches.
- If not on `main`, switch first and confirm the active branch is now `main`.
- If switching branches would discard local work or create conflicts, stop and ask the user.
- Prefer `git switch main` when available.
- Require local `main` to match `origin/main` before version bumps, tags, or pushes.

## Version Rules

- The version source of truth is the root `Cargo.toml` file.
- Update `[workspace.package].version` only.
- If the user gives no version, compute `patch + 1`.
- Use plain `x.y.z` inside `Cargo.toml`.
- Use `vX.Y.Z` for the git tag.
- Use `release-vX.Y.Z` for the GitHub release title.

## Recommended Commands

Prefer the one-command workflow:

```bash
python3 .agents/skills/github-release-main/scripts/release_main.py
python3 .agents/skills/github-release-main/scripts/release_main.py --version 1.2.3
```

Use this command sequence only when you need to perform the workflow manually:

```bash
git status --short
git branch --show-current
git switch main
git fetch origin main --prune
git rev-list --left-right --count main...origin/main
python3 .agents/skills/github-release-main/scripts/bump_root_cargo_version.py --file Cargo.toml --version 1.2.3
git add Cargo.toml
git commit -m "$(cat <<'EOF'
chore(release): cut v1.2.3
EOF
)"
git tag -a v1.2.3 -m v1.2.3
git push origin main
git push origin v1.2.3
gh release create v1.2.3 --title release-v1.2.3 --generate-notes
```

When no explicit version is given, omit `--version` and let the script compute the patch bump.

## Validation Checklist

- Confirm `git branch --show-current` returns `main` before changing files.
- Confirm local `main` exactly matches `origin/main` before changing files.
- Confirm `Cargo.toml` now contains the intended version.
- Confirm the commit succeeds before creating the tag.
- Confirm the tag name matches the version exactly, including the leading `v`.
- Confirm the GitHub release uses the same tag and title format `release-vX.Y.Z`.

## Resources

- `scripts/release_main.py`: run the end-to-end release workflow from branch switch through release creation.
- `scripts/bump_root_cargo_version.py`: update the root workspace version, either from an explicit version or by applying a patch bump.
