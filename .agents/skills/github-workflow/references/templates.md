# Templates and Command Notes

Use these templates as starting points. Trim sections that do not apply.

## Default Branch Detection

```bash
git symbolic-ref refs/remotes/origin/HEAD 2>/dev/null | sed 's@^refs/remotes/origin/@@'
# or
gh repo view --json defaultBranchRef -q .defaultBranchRef.name
```

## Issue Template

```markdown
## Background
[Original user report or product request]

## Current Behavior / Target Behavior
[For bugs: current vs expected]
[For features: desired end state]

## Analysis
- Code anchor: `path/to/file` - responsibility
- Likely root cause or implementation plan: ...
- Research note: ...

## Acceptance Criteria
- [ ] ...

## Open Questions
- [ ] ...

## Notes
Environment, version, related issues, rollout concerns
```

## Branch Creation Sequence

```bash
git fetch origin
git switch <default-branch>
git pull --ff-only
git switch -c <type>/<short-slug>-issue-<N>
```

## Commit Guidance

- Prefer one logical change per commit.
- Match the repository convention, for example conventional commits when the repo uses them.
- Reference the issue in the body or footer when appropriate, for example `Fixes #123` or `Closes #123`.

## PR Template

```markdown
## Summary
- ...

## Related Issue
Fixes #123

## Test Plan
- [ ] `...`
- [ ] Manual verification: ...

## Risks
- ...

## Rollback
- ...
```

## PR Creation

```bash
git push -u origin HEAD
gh pr create --title "..." --body-file /path/to/body.md
```
