---
name: diagnose-and-file-github-issue
description: Diagnose a reported system or application problem through a short back-and-forth with the user, investigate likely causes in the codebase or runtime context, and create or update a GitHub issue with a clear title, reproduction notes, findings, impact, and open questions. Use when the user reports a bug, failure, regression, environment problem, or unclear system issue and wants structured diagnosis plus issue creation, but does not want implementation or PR work.
---

# Diagnose and File GitHub Issue

## Goal

Follow one bounded workflow:

- clarify the reported problem
- ask only the follow-up questions needed to diagnose it
- investigate likely causes using available evidence
- search for an existing GitHub issue before creating a new one
- create or update the issue with actionable findings
- stop after reporting the issue URL and diagnosis summary

Do not implement a fix, create a branch, or open a pull request unless the user explicitly asks for a separate workflow.

## Intake Rules

- Ask short, high-signal questions. Do not interrogate the user with a long checklist up front.
- Prefer evidence over guesses: logs, command output, screenshots, config snippets, recent changes, repro steps, and environment details.
- Separate confirmed facts from hypotheses. If confidence is low, say so explicitly.
- Search for an existing issue or PR before creating a new issue.
- If the problem is still ambiguous after investigation, create the issue with findings and open questions rather than pretending the root cause is known.
- Keep the issue scoped to one concrete problem. Split unrelated symptoms into separate issues.
- Prefer `gh issue create` for GitHub operations. If `gh` is unavailable, use the web UI and preserve the same title and body structure.
- If the repository already tracks the problem, update or reference the existing issue instead of duplicating it.

## Diagnostic Workflow

### 1. Clarify the Symptom

- Turn the report into a concrete problem statement.
- Capture the current behavior, expected behavior, reproduction steps, frequency, user impact, and environment when relevant.
- Use `references/question-guide.md` when the report is vague or missing critical inputs.
- Ask for exact error text and the most recent command or action that triggered the issue whenever possible.

### 2. Investigate Before Filing

- Inspect the codebase, logs, docs, tests, configs, and recent changes that are most likely related.
- Record likely root causes, affected components, and confidence level.
- Capture a few code anchors as `path + one-line responsibility`.
- Distinguish between:
  - confirmed evidence
  - likely cause
  - unknowns that still need follow-up
- If external documentation or platform behavior matters, add a short research note with the link and one-line conclusion.

Write a compact diagnosis summary that can be reused in the issue:

- findings: 1 to 3 bullets
- evidence: logs, repro notes, or code anchors
- open questions: checklist items when unresolved

### 3. Decide Whether to Create or Update

- Search existing issues and PRs for duplicates or obvious matches.
- If an existing issue already covers the same problem, update or reference it instead of creating a new one.
- Create a new issue when the symptom, cause, or scope is distinct enough to track separately.

### 4. Create the GitHub Issue

- Use a specific, searchable title that names the symptom and affected component when known.
- For bugs, prefer issue titles that describe the observable failure instead of a guessed fix.
- Include background, reproduction, expected vs actual behavior, findings, impact, and open questions.
- Use `references/templates.md` for the default issue structure and `gh` command pattern.
- When the diagnosis is incomplete, include a next-step checklist so the issue is still actionable.

### 5. Stop After Filing

- Report the issue number and URL back to the user.
- Summarize the diagnosis in a few bullets.
- Call out remaining uncertainty and the most useful next diagnostic step.
- Do not start coding unless the user explicitly asks for implementation in a separate request.

## Output Checklist

Before finishing, report back:

- concise problem statement
- diagnosis summary and confidence level
- evidence or code anchors
- issue number and URL, or why no issue was created
- open questions or recommended next diagnostic step

## Resources

- `references/templates.md`: reusable issue template and `gh issue create` command pattern
- `references/question-guide.md`: targeted follow-up questions for vague or incomplete bug reports

