# Windows build feedback and cache budget

## Objective
Implement the reviewed build-infrastructure plan for agent-session-grep. Windows NTFS/MSVC and multi-worktree feedback first; development tooling only, no CLI/MCP/schema/release-profile changes. The user's 2026-10-18 implementation request authorizes task creation, isolated Git snapshot/worktree, benchmark infrastructure, optional verified prebuilt cache tools and budgeted experiments. No follow-on phase planning.

## Accepted contracts
- Freeze the complete current Beta source including required dirty/untracked product files, lockfile and resources without modifying its index or working files. Preserve its Git common directory.
- Full related-cache inventory: coordinator, Beta, registered/known worktrees, runtime artifacts, historical build roots and MBX/sccache stores; actual NTFS allocated bytes, global file-identity dedup, logical totals/errors/timestamps, no parent-child double counting. Exclude source, Git objects and Cargo downloads.
- Fixed initial allocated baseline B; B+40 GiB review/B+60 GiB stop; C disk free >=80 GiB. Other-window growth uses allowance. No baseline rebasing mid-campaign. Long-term total40/60 GiB goal is not claimed achieved.
- Cache reservation after pilot peak P = ceil(1.25*P+1) GiB. Account all remaining stores, seeds, failures, trial targets. External admission blocks projected overrun; no automatic GC or cleanup.
- Only explicitly previewed and approved successful disposable trial target paths may be recycled early and individually purged after Recycle Bin verification. Daily caches, stores and failures protect72h. No old-cache cleanup in this task, no cargo clean, no manual .git operations.
- Standard-library Python benchmark CLI inventory/preview/run/compare. Docs and short reproducible protocol in repository; personal paths/raw evidence local only.
- Fixed six default-feature jobs, --locked --offline -j2; test --test-threads=2. Isolated six targets vs manual3 lanes: check+clippy; domain/ports then CLI; SQLite then application. MBX explicit targets/views off, no adoption/native check lane/global PATH/wrapper stacking.
- Pilot5 paired batches; select at most2 finalists: A native profile/incremental-once/layout combination vs original native; B best cache tool combination vs qualified A, or repilot against original if A fails. No formal claims for rejected pilot-only variants.
- Formal100 pairs per finalist, paired balanced predetermined ordering,20 health only, fixed-N. Pollution records excluded with preserved evidence and complete retry; failures/timeout/resource limits retained; no chasing significance.
- Primary new worktree/new target with prewarmed same-project store from a different frozen worktree; dependency fetch outside timing, offline measurement; cold-store cost disclosed. Daily edit incremental retained; disabling only explicit disposable one-shot mode.
- rustc/cargo1.97.1 full version/hash/MSVC/source/protocol identity per segment. Windows Job Object whole process tree24GiB committed-memory hard limit incl dedicated cache daemon; memory-limited failures distinct. If process capture/identity is incomplete, no formal start.
- Windows quiet time segments; other-window development allowed between segments; recompute budget without resetting B. No completion date promise; report compute-hour projection from pilot.
- Native configuration/lanes >=10% allocated savings, P50/P95 edit/check/test/clippy <=5% regression, all jobs and file/line diagnostics pass. Tool>=15% primaryP50, P95/feedback/release<=5% regression with paired bootstrap95% CI; uncertainty not adopt.
- Cargo JSON from this invocation identifies artifacts; hashes/identity receipts prevent stale executable use. Never relax official core_beta_benchmark build-environment validation.
- Apply shared Cargo changes only after evidence; maintain explicit full-debug profile. Tools/stores/lane entry opt-in. Appropriate formatting/Clippy/tests and optional-feature quality gate. Commit task changes only, finish without next-stage work.

## Required delivery
Working safe benchmark tooling with meaningful unit/native Windows smoke tests; full actual inventory and immutable snapshot receipts; budgeted pilot where safe; official adoption only after complete evidence. If a mandatory operational prerequisite fails, preserve partial evidence and state exact limitation; do not pretend a pilot is100 pairs or that an unfinished campaign is complete.
