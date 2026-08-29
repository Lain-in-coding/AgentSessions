# Public History Scrub Runbook

## Status and boundary

P0-1 (public-tree privacy path cleanup) has scrubbed the production tree:
provider module docs, product docs, and this script/runbook set carry no
personal usernames, local checkout roots, or agent worktree coordinates.

The scanner also reports paths that remain in `.trellis/tasks/` research notes
belonging to audit tracks that are still in flight (resume/handoff, session
metadata, web parity, rehearsal, privacy hooks, release contract). Those notes
are internal working records; each owning track cleans its own notes before the
release gate, and the owner must rerun the scanner below on the exact
publication SHA before publishing. The repository gate is:

```text
python scripts/evidence/privacy_scan.py --repo .
python -m unittest discover -s scripts/evidence -p "test_privacy_scan.py" -v
```

The owner may create a clean public-tree candidate without rewriting private
history by exporting the exact publication SHA. This is the preferred Option A
mechanical rehearsal:

```powershell
$destination = Join-Path $env:TEMP "agent-session-grep-public-tree"
python scripts/release/export_public_tree.py --repo . `
  --commit <publication-sha> --destination $destination
python -m unittest discover -s "$destination/scripts/release" -p "test_*.py"
```

The exporter copies only tracked files, excludes internal coordination prefixes
(`.trellis/`, `.codex/`, `.codebuddy/`, `.agents/`, `.claude/` and generated
`scripts/evidence/out/` output), writes `PUBLIC-TREE-MANIFEST.json` with the
source SHA and per-file SHA-256, and runs the same privacy rules against the
ordinary exported directory. It does not modify refs or repository visibility;
review the destination and publish it only after the owner chooses Option A.

Note which tool scans the destination. The exporter's own `scan_export` walks
the exported directory on the filesystem and applies the shared rules, so a
successful export *is* a clean destination scan — a finding makes the export
exit non-zero. Do **not** try to point the standalone scanner at the
destination: `privacy_scan.py` enumerates through `git ls-files`, so
`--repo <destination>` fails with exit 2 on a non-repository directory rather
than reporting anything about its contents.

Rehearsed 2026-08-29 at `3bcba5d`: 504 files exported, exporter scan clean
(exit 0), exporter unittests 6/6 green, manifest written. History untouched.

This does **not** clean older commits. Git history can still retain superseded
copies of personal paths. Do not make the repository public until the owner
chooses one of these publication strategies:

1. Publish a new repository from the cleaned current tree, intentionally
   excluding the private development history; or
2. Rewrite the private repository history with `git-filter-repo`, validate the
   result, coordinate every existing clone, and then publish the rewritten
   repository.

This document is a decision and verification runbook. It does not authorize or
perform a destructive history rewrite.

## Preconditions

- Rotate any exposed credential before rewriting. Paths alone do not require
  credential rotation, but use the same incident procedure if a secret is found.
- Freeze pushes and ask collaborators to stop work during the rewrite window.
- Install a current `git-filter-repo` release and use Git 2.36 or newer.
- Work only in a disposable fresh clone. When cloning from a local filesystem,
  use `--no-local` so the clone does not share objects with the source.
- Archive the private repository separately before any rewrite.

Never run the commands below in a working checkout that contains unique local
work. Do not normalize this procedure by adding `--force`; the fresh-clone
safety check is intentional.

## Analyze without modifying history

Set the private remote URL in the current shell, then create a disposable mirror
clone outside the source checkout:

```powershell
$sourceUrl = "<private-repository-url>"
git clone --mirror $sourceUrl public-history-audit.git
git -C public-history-audit.git filter-repo --analyze
```

`--analyze` does not modify the repository. Review
`public-history-audit.git/filter-repo/analysis/` for suspect paths and sizes.
Also search every historical blob for the exact strings identified during the
current-tree audit. Examples:

```powershell
git -C public-history-audit.git log -S"<private-username>" --all -p --
git -C public-history-audit.git log -S"<old-local-root>" --all -p --
git -C public-history-audit.git log --all --name-only --pretty=format: |
  Select-String -Pattern "Users/|/home/|/Users/|AgentSessions|AgentHub|\.claude/worktrees"
```

Do not put real private values in this tracked runbook or in a committed
replacement file.

## Measured read-only history audit — re-measured 2026-08-29 at `e0ff3ad`

The snapshot above is kept for comparison but is stale: it was taken when the
tree held 277 exportable files, and the tree has since grown to 1017 tracked
files (504 of them inside the export set). Re-measured with strictly read-only
commands — `git log --all --extended-regexp -G<pattern>` over every ref, plus
`git ls-files` to classify each matching path. Counts only; no private value is
recorded here. History was not modified.

**Methodology difference, stated so the two tables are not misread as
comparable.** This pass counts *commits and distinct files* per family, not
introduced match-lines, and its patterns are deliberately **broader** than the
scanner's: no negative lookbehind on the Unix home forms and a looser worktree
form. A `-G` hit means some commit's diff added or removed a matching line, so a
file listed as "still in HEAD" may carry the match only in an older revision.
That is the point of the exercise — HEAD being clean says nothing about what the
history retains.

| pattern family | matching commits | distinct files | still in HEAD |
| --- | ---: | ---: | ---: |
| Windows user home paths | 21 | 18 | 15 |
| Unix user home paths (`/Users/`, `/home/`) | 24 | 20 | 18 |
| machine roots (checkout/reference roots) | 26 | 40 | 20 |
| worktree coordinates | 18 | 19 | 5 |
| secret shapes (GitHub PAT, AWS key, private key block) | 12 | 5 | 4 |
| union of all families | 63 distinct commits | 71 | 47 |

Union split by publication relevance:

- **32 union files are inside the export set** (tracked, not under an excluded
  internal prefix). Every one is either a source file whose match is a synthetic
  fixture the scanner already allowlists, a golden fixture whose placeholder home
  is BLAKE3-pinned and documented in its `PROVENANCE.md`, this runbook and
  `scripts/evidence/privacy_scan.py` (which necessarily contain the patterns
  themselves), or a document quoting an example path. The current-tree scan at
  this SHA reports **0 findings**.
- **24 union files no longer exist at HEAD** and are reachable only through
  history: 21 are `.trellis/` internal coordination records, plus
  `crates/agent-session-grep-application/src/handoff_markdown.rs`,
  `crates/agent-session-grep-cli/src/main.rs` (superseded by the CLI lib split),
  and `docs/release/go-no-go.2026-08-19.md`.
- The 5 secret-shape files are the redaction detector
  (`crates/agent-session-grep-ports/src/redact.rs`), its boundary application
  (`crates/agent-session-grep-cli/src/redaction.rs`), the `serve` POST-echo
  regression that asserts a synthetic token is *not* reflected, and two internal
  research notes. Documentation-style vectors only. **No credential rotation is
  triggered**, consistent with the 08-18 measurement.

**What this means for the owner decision.** The two options are not equally
gated by these numbers:

- **Option A** publishes an exported tree with no prior history, so the 63
  history commits are out of scope by construction. Its only privacy gate is the
  current-tree scan at the publication SHA, which is green. Option A is
  executable today.
- **Option B** publishes this history, so it must scrub all 63 commits across 71
  files — including the 24 paths that no longer exist at HEAD and therefore
  cannot be fixed by editing the working tree. Those 24 are dominated by
  `.trellis/` records, which a rewrite would most cleanly remove by path rather
  than by text replacement.

Re-run this measurement at the exact publication SHA; it is cheap and read-only.

## Measured read-only history audit (2026-08-18 snapshot, kept for comparison)

Snapshot measured 2026-08-18 at commit `f7e2a49` with strictly read-only
commands: `git log --all -G<pattern>` over every ref, plus per-commit diff
attribution (`git show <commit> --format= --unified=0`) to count introduced
match-lines per file. Counts only; no private value is recorded here.

| pattern family | matching commits | introduced match-lines | distinct files | files still in HEAD | files inside the export set |
| --- | ---: | ---: | ---: | ---: | ---: |
| Windows user home paths | 17 | 28 | 16 | 16 | 9 |
| Unix user home paths (`/Users/`, `/home/`) | 17 | 27 | 14 | 14 | 13 |
| machine roots (checkout/reference roots) | 24 | 73 | 40 | 40 | 11 |
| worktree coordinates (`.claude/worktrees/<name>`, `worktree-<name>`) | 9 | 31 | 15 | 15 | 0 |
| secret shapes (GitHub PAT, AWS key, private key block) | 6 | 18 | 3 | 3 | 2 |
| union of all families | 45 distinct commits | 177 | 61 | 61 | 29 |

File-family distribution of the union (match-lines / files): internal
coordination records excluded by the exporter 104 / 32; product crates 65 / 23;
docs and release notes 4 / 3; spikes 3 / 2; root-level schemas 1 / 1.

Reading the numbers:

- Every match-carrying file still exists at HEAD, and the current-tree scanner
  reports 0 findings. The 29 union files inside the export set carry only
  synthetic fixtures already allowlisted by the scanner (test vectors and
  redaction examples); the remaining 32 are internal coordination records that
  the exporter drops.
- The 6 secret-shape commits contain only redaction rule definitions and
  synthetic test vectors (AWS/GitHub documentation examples), not real
  credentials. This measurement triggers no credential rotation; rerun the
  exact-string searches in the previous section if a later audit finds a new
  shape.
- Worktree coordinates appear only in excluded internal records: 0 files inside
  the export set.

Export verification at this snapshot: the exporter copied 277 tracked files
from `f7e2a49`; its built-in scanner reported 0 findings, and the scanner and
exporter unittest gates were green. History was not modified by this audit.
(The original note also claimed a separate standalone scanner run over the
destination; that is not something `privacy_scan.py` can do, since it
enumerates through `git ls-files` — see the note above the export command.)

## Preview text replacement

Create an untracked replacement file outside the repository. Each line is a
literal replacement expression unless it starts with `regex:` or `glob:`.
Keep replacements explicit and reviewable, for example:

```text
<exact-old-user-home>==><user-home>
<exact-old-checkout-root>==><repo>
<exact-old-reference-root>==><user-home>/reference-src
```

Preview the rewrite in the disposable clone:

```powershell
git -C public-history-audit.git filter-repo --dry-run --replace-text "<absolute-path-to-replacements.txt>"
```

`--dry-run` does not change refs. It writes original and filtered fast-export
streams under `filter-repo/` for comparison. Inspect those streams and rerun the
history searches above before approving a real rewrite.

## Owner decision: rewrite or publish a clean repository

### Decision checklist

Complete this checklist at the publication SHA before either option is
executed. The rewrite itself is never executed from this runbook.

- [ ] Review the measured audit snapshot above and the current-tree scan
      result at the publication SHA.
- [ ] Choose and record the strategy: Option A (publish the exported clean
      tree as a new repository, dropping private development history) or
      Option B (`git-filter-repo` rewrite of the private history).
- [ ] Credential rotation check: the measured secret-shape hits are all
      redaction rules and synthetic test vectors. If any exact-string search
      finds a real credential instead, rotate it before proceeding and treat
      the exposure as an incident.
- [ ] Collaborator freeze (Option B only): ask every collaborator to stop
      pushing for the rewrite window and plan to delete or re-verify every
      clone afterwards. Option A needs no freeze, only that pushes stop at
      the chosen publication SHA.
- [ ] Backup: archive the private repository before any rewrite or
      publication decision.
- [ ] Record the chosen option, the publication SHA, and the date in the
      release record.

### Option A: new public repository

Create a new repository from an exported clean tree or a new root commit. This
is the lowest-risk choice when preserving private development commit identity is
not a public requirement. Run the current-tree scanner in the exported tree,
review `git ls-files`, then publish only after all release gates pass.

### Option B: rewrite the private history

Only after owner approval, repeat the operation in a second disposable fresh
mirror clone and enable sensitive-data cleanup:

```powershell
git clone --mirror $sourceUrl public-history-rewrite.git
git -C public-history-rewrite.git filter-repo --sensitive-data-removal --replace-text "<absolute-path-to-replacements.txt>"
```

A rewrite changes commit IDs. `git-filter-repo` normally removes `origin` to
prevent accidental mixing of incompatible old and new histories. Review the
result before restoring any remote. The final force-push or public-repository
creation is an owner action and is intentionally not included as an executable
step here.

## Validate the rewritten candidate

In a non-bare checkout made from the rewritten candidate, run:

```powershell
python scripts/evidence/privacy_scan.py --repo .
python -m unittest discover -s scripts/evidence -p "test_privacy_scan.py" -v
git diff --check
```

Search all refs for every replaced value and historical path family:

```powershell
git log -S"<private-username>" --all -p --
git log -S"<old-local-root>" --all -p --
git log --all --name-only --pretty=format: |
  Select-String -Pattern "Users/|/home/|/Users/|AgentSessions|AgentHub|\.claude/worktrees"
```

For each first changed commit reported by `--sensitive-data-removal`, verify that
no ref retains it:

```powershell
git cat-file -t <first-changed-commit>
git for-each-ref --contains <first-changed-commit>
```

The `cat-file` check must fail with a missing-object error after cleanup. Any
reported retaining ref blocks publication. Repeat the scan against a fresh clone
from the exact candidate remote; local object pruning alone does not prove the
server no longer retains old refs.

## Publication checklist

- Owner selected and recorded Option A or Option B.
- Current-tree privacy scan and unittest are green on the exact publication SHA.
- Historical exact-string and path-family searches return only reviewed
  synthetic fixtures or documentation commands.
- No real provider transcript, database, credential, local build artifact, or
  ignored reference checkout is tracked.
- Existing clones are deleted and recloned, or explicitly cleaned and verified,
  so an old branch cannot reintroduce rewritten history.
- Repository visibility changes only after the final owner review.

## References

- `git-filter-repo` project and fresh-clone safety:
  <https://github.com/newren/git-filter-repo>
- Official manual (`--analyze`, `--dry-run`, `--replace-text`, and
  `--sensitive-data-removal`):
  <https://github.com/newren/git-filter-repo/blob/main/Documentation/git-filter-repo.txt>
