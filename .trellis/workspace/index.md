# Workspace Journals

This directory is optional local Trellis history, not the authority for current project state.

## Shared versus local content

- This generic index may be shared to explain the directory's purpose.
- Per-developer directories, indexes, journals, session traces, and identity-derived data are local-only and ignored.
- Do not use workspace journals to determine the active task, milestone completion, approval status, or verified product behavior.

## Current-state sources

Use these instead:

```text
python ./.trellis/scripts/task.py current --source
python ./.trellis/scripts/task.py list
python ./.trellis/scripts/get_context.py --mode phase --step <X.Y>
python ./.trellis/scripts/get_context.py --mode packages
```

Then read the active task artifacts and relevant `.trellis/spec/` files. Formal product decisions live in RFCs, ADRs, contracts, schemas, and policies; tests, CI results, and Spike Evidence establish verified behavior; Git history records past changes.

## Local initialization

A contributor may initialize local journals with:

```text
python ./.trellis/scripts/init_developer.py <local-handle>
```

The resulting identity and per-developer workspace must remain untracked. Never place credentials, transcripts, real provider session data, local approvals, or machine-specific paths in shared repository content.
