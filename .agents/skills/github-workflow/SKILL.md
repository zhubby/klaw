---
name: github-workflow
description: Turn a reported bug or feature request into a tracked GitHub delivery flow: investigate the codebase, confirm scope, create or update a GitHub issue, branch from the repository default branch, implement the change, commit with repository conventions, push, and open a pull request. Use when the user wants an end-to-end issue-to-PR workflow, asks to file an issue and then implement it, or needs a structured GitHub workflow for bugs or features.
---

# GitHub Issue to PR Workflow

## Goal

Use one tracked path from request to pull request:

1. understand the request
2. analyze the codebase
3. create or confirm the tracking issue
4. branch from the default branch
5. implement only the scoped change
6. push and open a pull request

If the user asks for only one stage, stop after that stage instead of forcing the full workflow.

## Required Inputs

Classify the request as a bug or a feature. Ask short follow-up questions when essential details are missing.

- Bug: current behavior, expected behavior, reproduction steps, impact, environment if relevant
- Feature: goal, acceptance criteria, constraints, non-goals if relevant

## Operating Rules

- Search for existing issues or PRs before creating a new issue.
- Inspect `git status` before changing branches or editing files.
- Stop and ask the user how to proceed if the working tree contains unrelated or unexpected changes.
- Detect the repository default branch from the remote. Do not assume `main`.
- Prefer creating the issue before starting implementation unless the user explicitly wants to skip issue tracking.
- Keep the implementation tightly aligned with the issue scope. Do not bundle opportunistic refactors.
- Follow repository-specific build, test, formatting, and commit conventions.
- Use `gh` when available for GitHub operations. If `gh` is unavailable, use the web UI and keep the same title and body structure.

## Workflow

### 1. Clarify and Classify

- Turn the request into a concrete bug or feature statement.
- Identify missing information that blocks investigation or acceptance.
- Confirm whether the user wants issue creation only, issue plus implementation, or the full issue-to-PR flow.

### 2. Investigate Before Filing

- Search for the relevant files, symbols, logs, tests, and call paths.
- Record the likely root cause or implementation entry points.
- Note affected modules, risks, and open questions.
- Capture a few code anchors as `path + one-line responsibility`.
- If external behavior or upstream documentation matters, add a short research note with the link and one-sentence conclusion.

Write a short analysis summary that can be reused in the issue:

- conclusions: 1 to 3 bullets
- code anchors: file paths and responsibilities
- open questions: checklist items when unresolved

### 3. Create or Update the GitHub Issue

- Prefer `gh issue create`.
- Reuse or update an existing issue when the repository already tracks the same work.
- Make the issue title specific. Prefix with `fix:` or `feat:` when that matches repository style.
- Include background, current or target behavior, investigation notes, acceptance criteria, and open questions.
- Save the issue number and URL for later branch naming, commit references, and PR linking.

Read `references/templates.md` for reusable issue and PR templates.

### 4. Branch From the Default Branch

- Fetch the remote and detect the default branch.
- Switch to the default branch and fast-forward it before creating a feature branch.
- Name branches with the work type and a short slug. Include the issue number when available.

Examples:

- `fix/template-nil-issue-123`
- `feat/pipeline-retry-issue-456`

Stop if switching branches would overwrite local work or if the local default branch is not cleanly in sync with the remote.

### 5. Implement and Validate

- Make only the changes needed for the issue acceptance criteria.
- Add or update tests when the repository expects them.
- Run the relevant format, lint, and test commands for the touched code.
- Update documentation or configuration when behavior changes require it.
- If the work expands significantly, split it into multiple issues or PRs instead of silently growing scope.

### 6. Commit, Push, and Open the PR

- Use the repository's commit message convention.
- Reference the issue in the commit body or footer when appropriate.
- Push the branch and open the PR against the default branch unless the user specifies another target branch.
- Use a draft PR when the work is incomplete, risky, or still needs validation.
- Include a clear summary, linked issue, risks, rollout or rollback notes when relevant, and a concrete test plan.

## Output Checklist

Before finishing, report back:

- request classification
- issue number and URL, or why no issue was created
- branch name
- validation commands run and the result
- PR URL, or the reason PR creation stopped early

## Escalation Rules

- If the repository already contains a suitable branch or issue for the same work, continue from that state instead of duplicating it.
- If the request is too large for one reviewable PR, propose a split plan before coding.
- If investigation shows that no code change is needed, create or update the issue with findings and stop unless the user asks for more.
