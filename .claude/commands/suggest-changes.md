Analyze the agent-monitor debug logs and codebase to discover concrete improvements, fixes, and optimizations. Follow this structured process:

## Step 1: Gather logs

Run `am logs --level debug --lines 500` to capture recent log output. If the log file is empty or too short, tell the user and ask them to run the TUI with active agents first, then retry.

## Step 2: Identify patterns

Analyze the logs for these categories of issues:

- **Performance problems**: Repeated API calls, redundant DB writes, unnecessary polling cycles, concurrent request thrashing, backoff failures
- **Log noise**: Messages that repeat excessively without adding value, log levels that are too verbose for their content (e.g. routine operations at INFO that should be DEBUG)
- **Architectural issues**: Race conditions visible in log ordering, state management bugs (global state that should be scoped, missing deduplication), resource leaks
- **Error patterns**: Error→recovery→error cycles, errors that are silently swallowed, missing error context that would aid debugging
- **Missing observability**: Key state transitions that aren't logged, decisions with no debug trail

## Step 3: Cross-reference with code

For each pattern found, read the relevant source files to:
- Confirm the root cause (don't guess from logs alone)
- Understand the surrounding code and constraints
- Check if there are existing mechanisms that should have prevented the issue
- Identify the minimal change needed

## Step 4: Produce a structured plan

Output a plan in this format:

```
# Improvements/Fixes Based on Log Analysis

## Context
[Brief summary of log volume analyzed, number of active agents/repos observed, and time span covered]

---

## Issue N: [Descriptive Title] ([Priority] Priority)

**Pattern observed:** [Quote or paraphrase the log pattern with concrete examples]

**Root cause:** [Explain why this happens, referencing specific file:line locations]

**Fix:** [Describe the minimal change. Include code snippets showing before/after where helpful. Reference exact file paths and line numbers.]

**Files:** [List of files to modify]

---

## Summary of Changes

| # | Issue | Priority | File(s) | Effort |
|---|-------|----------|---------|--------|

## Verification
[How to confirm each fix works — specific log patterns to watch for or their absence]
```

## Guidelines

- **Prioritize by impact**: High = degrades performance or correctness for users. Medium = creates unnecessary work or noise. Low = cosmetic or minor DX improvements.
- **Minimal changes only**: Each fix should be the smallest change that fully addresses the issue. Don't propose refactors or cleanups beyond what's needed.
- **Be specific**: Include file paths, line numbers, and code snippets. Vague suggestions aren't actionable.
- **Stay grounded**: Only propose fixes for patterns you can actually see in the logs and confirm in the code. Don't speculate about issues that might exist.
- **Respect existing architecture**: Work within the current patterns (pub/sub, SQLite, hook-event model). Don't propose architectural rewrites unless the logs reveal a fundamental flaw.

After presenting the plan, ask if the user wants you to implement all changes, specific issues only, or enter plan mode to discuss further.
