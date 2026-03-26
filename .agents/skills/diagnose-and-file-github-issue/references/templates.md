# Issue Templates and Command Notes

Use these templates as a starting point. Remove sections that do not apply.

## Duplicate Search

Search for an existing issue before creating a new one.

Examples:

```bash
gh issue list --state all --search "exact error text"
gh issue list --state all --search "component symptom keyword"
```

## Default Issue Template

```markdown
## Background
[Original user report in 1 to 3 sentences]

## Symptom
- Current behavior: ...
- Expected behavior: ...
- Frequency / impact: ...

## Reproduction
1. ...
2. ...
3. ...

## Evidence
- Error message / log snippet: ...
- Environment / version: ...
- Code anchor: `path/to/file` - responsibility

## Diagnosis
- Confirmed findings: ...
- Likely root cause: ...
- Confidence: high / medium / low

## Open Questions
- [ ] ...

## Next Steps
- [ ] add or collect missing logs
- [ ] confirm reproduction path
- [ ] validate the suspected root cause
```

## Minimal Issue Template

Use this when the diagnosis is still incomplete but the problem should still be tracked.

```markdown
## Problem
[Concise statement of the failure]

## What We Know
- ...

## What We Do Not Yet Know
- [ ] ...

## Suggested Investigation
- [ ] ...
```

## `gh issue create` Pattern

```bash
gh issue create --title "fix: concise symptom title" --body "$(cat <<'EOF'
## Background
...

## Symptom
- Current behavior: ...
- Expected behavior: ...

## Reproduction
1. ...

## Evidence
- ...

## Diagnosis
- ...

## Open Questions
- [ ] ...

## Next Steps
- [ ] ...
EOF
)"
```

## Title Guidance

- Prefer observable symptoms over guessed fixes.
- Include the affected component when known.
- Keep titles searchable and specific.

Examples:

- `fix: cli env check fails when OPENAI_API_KEY is unset`
- `fix: config panel save silently drops memory model changes`
- `fix: agent run hangs after tool loop retry exhaustion`
