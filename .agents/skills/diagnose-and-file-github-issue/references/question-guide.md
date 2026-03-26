# Diagnostic Question Guide

Ask only the smallest set of questions needed to make progress. Prefer 2 to 4 focused questions, then investigate, then ask follow-ups only if needed.

## Core Questions

Use these first when the report is vague:

1. What exactly happened, and what did you expect to happen instead?
2. What action or command triggered it?
3. Can you share the exact error message, log excerpt, or screenshot text?
4. Is this always reproducible, intermittent, or only in one environment?

## Runtime / CLI Failures

- What command did you run?
- What were the full stdout and stderr outputs?
- Did this start after a recent code, config, dependency, or environment change?
- What OS, shell, runtime, or tool version are you using?

## Regression Reports

- What was the last known good commit, version, or release?
- What changed between the working and broken states?
- Does reverting or toggling a recent change affect the outcome?

## Config / Environment Problems

- Which config file, environment variable, or secret is involved?
- Is the problem local only, CI only, or both?
- Can the user confirm the effective value without sharing secrets?

## Intermittent or Hard-to-Reproduce Problems

- How often does it happen?
- Are there patterns around timing, load, retries, or specific inputs?
- Is there any correlation with network state, startup order, or concurrency?

## Closing the Intake

Once you have enough to investigate, stop asking broad questions and switch to diagnosis. Summarize the current understanding before creating the issue so the user can quickly confirm it.
