# Research: sync --discover discovery-scope-audit

- **Query**: Locate the code paths that `sync --discover` must touch: CLI sync dispatch, provider data-root resolution, source file enumeration, fingerprint + unchanged-skip, tombstone, list_providers.
- **Scope**: internal
- **Date**: 2026-08-14
- **Checkout**: isolated worktree on branch `integration-08-13-four-features-v2`

All `file:line` references are repository-relative.

---

## 1. CLI `sync` subcommand entry & argument parsing

### Dispatch entry

`crates/agent-session-grep-cli/src/main.rs:985` — `fn dispatch(store, rest, mode, request_id)`.

The subcommand selector is a `match cmd` on `rest[0]` at `main.rs:1004`. The
`sync` arm lives at `main.rs:1054`:

```rust
// main.rs:1051-1068
        // sync 对显式列出的多个源执行同一套只读快照 + staging，并在全部成功后
        // 通过一次 durable batch 提交，避免部分 source 已写入、后续 source 失败。
        // jsonl 模式下逐源发 progress frame（contract §4；--robot/Json 禁 progress）。
        "sync" => {
            let (data, warnings) = sync_files(
                store,
                &rest[1..],
                mode == protocol::OutputMode::Jsonl,
                request_id,
            )?;
            Ok((
                "sync",
                protocol::Outcome::Success,
                data,
                protocol::Page::default(),
                warnings,
            ))
        }
```

Key observation for implementers: the `sync` arm currently passes `&rest[1..]`
(all positionals after `sync`) straight into `sync_files`. There is **no
flag-scanning step** in the sync arm today — it does not call `extract_flag` or
`take_bool_flag` before handing off. A `--discover` flag would need to be
introduced here, mirroring the pattern used by the `search` arm
(`main.rs:1069-1108`), which does:

```rust
// main.rs:1069-1078 (search arm, for reference)
        "search" => {
            let mut args = rest.to_vec();
            let cursor = extract_flag(&mut args, "--cursor")?;
            let max_items = extract_flag(&mut args, "--max-items")?;
            ...
            let include_system = take_bool_flag(&mut args, "--include-system");
            let group_by_session = take_bool_flag(&mut args, "--group-by-session");
```

### Flag-scanning primitives (reusable)

- `extract_flag(args, name) -> Result<Option<String>>` — `main.rs:1295`. Takes
  a `--name value` pair out of the arg vector; missing value = usage error.
- `extract_repeated_flag(args, name) -> Result<Vec<String>>` — `main.rs:1309`.
  Repeated `--name value` pairs (used by `--provider`).
- `take_bool_flag(args, name) -> bool` — `main.rs:1323`. Removes a bare flag,
  returns presence.
- `no_extra_args(rest, expected, usage)` — `main.rs:2277`. Rejects leftover
  positionals after flag extraction.
- `arg(rest, index, usage)` — `main.rs:2269`. Fetches a single positional.

### Writer-lease gating

`main.rs:350-361`: before `dispatch` is called, `sync` is detected as a writing
subcommand (`matches!(c, "index" | "ingest" | "sync")`) and the store is opened
with `SqliteStore::open_for_write(&db)` (acquires the writer lease). A
`sync --discover` variant must remain in that write-path set so the lease is
acquired; the match at `main.rs:354` already covers `"sync"`.

### `sync_files` body

`main.rs:1898` — `fn sync_files(store, paths, progress, request_id)`. This is
the function a `--discover` path must either call (after expanding discovered
paths into the `paths` slice) or share helpers with. Body summary:

- `main.rs:1904-1906`: requires ≥1 file, else usage error.
- `main.rs:1911-1917`: rejects directories (explicit guard; message is
  platform-neutral and does **not** leak the path).
- `main.rs:1922-1928`: dedups repeated paths preserving order.
- `main.rs:1938-1944`: pre-fetches cached fingerprints + message counts via
  `store.source_fingerprints(paths)` and `store.source_message_counts(paths)`.
- `main.rs:1945-1991`: per-path loop — `capture` → compare fingerprint →
  `stage_with_registry` or skip (unchanged) → build `sources: Vec<SourceBatch>`.
- `main.rs:1993-1995`: `verify_snapshot` for each captured snapshot before
  commit.
- `main.rs:1997-1999`: single `store.commit_source_batches_if_changed(&sources)`.
- `main.rs:2000-2017`: emits `sources/emitted/committed/unchanged/...` JSON.

The `--discover` implementation must feed discovered file paths into this same
pipeline; the directory-rejection guard at `1911-1917` must be bypassed or
relaxed for the discovery case, since discovery is inherently directory-rooted.

---

## 2. Provider data-root path resolution

### `installation_namespace` (path-derived, not home-dir resolution)

`main.rs:1576` — `fn installation_namespace(path, provider_id) -> String`.
This is the **only** function that currently touches provider data-root
markers, and it operates purely on a *given path string* — it does **not**
resolve a home directory or locate a data root itself. Its job is to derive a
stable namespace from a path that already contains a `.claude` or `.codex`
segment:

```rust
// main.rs:1576-1593
fn installation_namespace(path: &str, provider_id: &str) -> String {
    let marker = match provider_id {
        "claude-code" => ".claude",
        "codex" => ".codex",
        _ => "",
    };
    let segments: Vec<&str> = path.split(['/', '\\']).filter(|s| !s.is_empty()).collect();
    if !marker.is_empty()
        && let Some(index) = segments.iter().rposition(|s| *s == marker)
    {
        return format!("{provider_id}:{}", segments[..=index].join("/"));
    }
    let parent = match segments.split_last() {
        Some((_, parent)) if !parent.is_empty() => parent.join("/"),
        _ => ".".to_string(),
    };
    format!("{provider_id}:{parent}")
}
```

Called from `staged_to_source` at `main.rs:1626` to build a
`SessionIdentityNamespace` for `StableId::native_session_scoped`.

### There is NO home-dir / data-root discovery function today

Search for `home_dir`, `dirs::home`, `.claude/projects`, `.codex/sessions`,
`data_root` in the provider crates returns **zero hits** in the adapter source:

- `crates/agent-session-grep-provider-claude/src/lib.rs` — only
  `impl ProviderAdapter` (probe/parse); no path resolution.
- `crates/agent-session-grep-provider-codex/src/lib.rs` — same.
- `crates/agent-session-grep-ports/src/lib.rs:631` — `ProviderAdapter` trait
  has `provider_id()`, `probe()`, `parse()`. There is **no** `data_root()` or
  `discover()` method on the trait.

The `platform_paths_impl` functions (`main.rs:465` Windows, `main.rs:474`
macOS, `main.rs:491` Linux, `main.rs:514` fallback) resolve **this tool's own**
config/data/cache/logs directories — they are for the agent-session-grep data
root, not for provider transcript roots. They do use `HOME` / `USERPROFILE`
env vars, so the pattern for resolving a home directory exists and can be
reused.

### Implication for `--discover`

`sync --discover` needs a **new** function (or a new method on
`ProviderAdapter` / a new trait) that resolves the canonical provider
data root from the current user's home directory:
- Claude Code: `~/.claude/projects/` (per CONTEXT / trellis-meta SKILL notes)
- Codex: `~/.codex/sessions/`

The marker strings `.claude` / `.codex` already appear in
`installation_namespace` (`main.rs:1577-1580`); the new discovery code should
reuse those same constants to stay consistent.

---

## 3. Existing source-file enumeration (none — sync is explicit-path only)

There is **no recursive directory scan or file discovery** in the current sync
path. `sync_files` receives an explicit `paths: &[String]` slice from the CLI
positionals and iterates it. The only `SourceDiscovery` implementation that
does enumeration is:

`crates/agent-session-grep-adapters-sqlite/src/source_fs.rs:126` — `SnapshotFs`,
which is a **test/fixture helper**: it holds a pre-built `Vec<SourceSnapshot>`
and returns clones from `discover()`. Its `read_verified` calls
`verify_snapshot`. It is not wired into the CLI.

```rust
// source_fs.rs:126-147
pub struct SnapshotFs {
    snapshots: Vec<SourceSnapshot>,
}

impl SnapshotFs {
    pub fn new(snapshots: Vec<SourceSnapshot>) -> Self { Self { snapshots } }
}

impl agent_session_grep_ports::SourceDiscovery for SnapshotFs {
    fn discover(&self) -> PortResult<Vec<SourceSnapshot>> {
        Ok(self.snapshots.clone())
    }
    fn read_verified(&self, snapshot: &SourceSnapshot) -> PortResult<Vec<u8>> {
        let path = Path::new(&snapshot.path);
        verify_snapshot(path, snapshot)
    }
}
```

### The `SourceDiscovery` port

`crates/agent-session-grep-ports/src/lib.rs:64`:

```rust
pub trait SourceDiscovery {
    fn discover(&self) -> PortResult<Vec<SourceSnapshot>>;
    fn read_verified(&self, snapshot: &SourceSnapshot) -> PortResult<Vec<u8>>;
}
```

This port exists but is only exercised by tests (`ReadOnceDiscovery` at
`ports/src/lib.rs:837`, `SnapshotFs` above). The CLI does not use it.
`--discover` is the natural first production consumer of this port — a new
implementation that walks `~/.claude/projects/**/*.jsonl` and
`~/.codex/sessions/**/*.jsonl` would implement `SourceDiscovery`, or the
discovery could produce a `Vec<String>` of paths fed directly into the existing
`sync_files` pipeline.

---

## 4. Source fingerprint + unchanged-skip semantics

### Capture + fingerprint

`crates/agent-session-grep-adapters-sqlite/src/source_fs.rs:32` —
`pub fn capture(path) -> PortResult<(SourceSnapshot, Vec<u8>)>`. Reads file
bytes, computes `BLAKE3` hex fingerprint (`source_fs.rs:24`), captures
`(len, mtime_ms, fingerprint)`. Truncates to initial `len` if the file grew
during read.

```rust
// source_fs.rs:32-49
pub fn capture(path: &Path) -> PortResult<(SourceSnapshot, Vec<u8>)> {
    let mut file = File::open(path).map_err(backend)?;
    let meta = file.metadata().map_err(backend)?;
    let len = meta.len();
    let mtime = mtime_ms(&meta)?;
    let mut buf = Vec::with_capacity(len as usize);
    file.read_to_end(&mut buf).map_err(backend)?;
    buf.truncate(len as usize);
    let fingerprint = fingerprint_hex(&buf);
    let snap = SourceSnapshot { path: path.to_string_lossy().into_owned(), len, mtime_ms: mtime, fingerprint };
    Ok((snap, buf))
}
```

### Pre-commit verify

`source_fs.rs:56` — `verify_snapshot(path, snap)`. Re-reads metadata + content,
returns `PortError::SnapshotChanged` on any drift (len, mtime, or fingerprint).
`post_read_verify` (`source_fs.rs:94`) closes the stat→read race window.

### Unchanged-skip in `sync_files`

`main.rs:1936-1960`:

```rust
// main.rs:1936-1960
    // 指纹缓存：capture 后先与已存指纹比对，未变化的源跳过重复解析
    let cached = store.source_fingerprints(paths).map_err(ProtocolError::from)?;
    let mut unchanged_messages = 0usize;
    let unchanged_counts = store.source_message_counts(paths).map_err(ProtocolError::from)?;
    for (index, path) in paths.iter().enumerate() {
        let path_ref = std::path::Path::new(path);
        let (snap, bytes) = capture(path_ref).map_err(ProtocolError::from)?;
        let cached_fp = cached.get(path).and_then(|(_, fp)| fp.clone());
        // 空文件（0 字节）不能走指纹跳过：它必须作为"整源清空"批次提交
        let (staged, variant) =
            if !bytes.is_empty() && cached_fp.as_deref() == Some(snap.fingerprint.as_str()) {
                unchanged_messages += unchanged_counts.get(path).copied().unwrap_or(0);
                (None, None)
            } else {
                let (staged, variant) = stage_with_registry(&bytes)?;
                (Some(staged), Some(variant))
            };
```

Important: the empty-file carve-out at `main.rs:1952` (`!bytes.is_empty()`)
ensures a 0-byte source is **not** skipped — it must be staged as an empty
batch to tombstone prior messages. This is the mechanism by which an emptied
source triggers tombstoning (see §5).

### Store-side fingerprint cache queries

- `SqliteStore::source_fingerprints(paths) -> BTreeMap<String, (len, fingerprint)>`
  at `adapters-sqlite/src/lib.rs:1543`. Queries `source_scans` table; unseen
  paths absent from the map.
- `SqliteStore::source_message_counts(paths) -> BTreeMap<String, usize>`
  at `adapters-sqlite/src/lib.rs:1569`. Counts `msg_v1_*` rows in
  `source_membership` per path.

### `sources_are_current` (fast no-op gate inside commit)

`adapters-sqlite/src/lib.rs:2336` —
`fn sources_are_current(ordered_sources) -> PortResult<bool>`. Called at the
top of `commit_source_batches_if_changed` (`lib.rs:1892`) as a cheap early-out:
if every source's stored `(len, fingerprint)` matches and all catalog entries /
FTS text are byte-identical, returns `Ok(false)` (no generation change) without
building the merged view. This is what makes an unchanged re-sync fast.

---

## 5. Tombstone logic: emptied / removed sources

### `SourceBatch` + `relation_complete`

`adapters-sqlite/src/lib.rs:885` — `pub struct SourceBatch`. Key field:
`relation_complete: bool` (`lib.rs:904`).

```rust
// lib.rs:880-912 (abridged)
/// 一个 source 完整成功 scan 后的全部消息条目。
/// sync/ingest 为每个只读源构造一个 SourceBatch，store 据此推导：本次出现的
/// message id 是 upsert；该源上次成功 scan 有、本次没有的 id 是 tombstone（删除）。
/// 只有整批全部源都 stage 成功后才提交；missing/tombstone 只能由完整成功 scan 确认。
pub struct SourceBatch {
    pub source_path: String,
    pub entries: Vec<(StableId, Vec<u8>, String)>,
    pub placements: Vec<MessagePlacement>,
    pub edges: Vec<MessageEdge>,
    pub relation_complete: bool,
    pub len_bytes: Option<i64>,
    pub fingerprint: Option<String>,
    pub resume_claim: Option<SourceResumeClaim>,
}
```

### Empty-source staging → tombstone

`stage_with_registry` (`main.rs:1503`) returns an **empty** `StagedBatch`
(`relation_complete = true`, 0 messages) for 0-byte files:

```rust
// main.rs:1503-1523
fn stage_with_registry(bytes: &[u8]) -> Result<(StagedBatch, String), CliError> {
    if bytes.is_empty() {
        return Ok((StagedBatch {
            messages: Vec::new(),
            report: ParseReport { committed: 0, skipped: 0, ... },
            session_native_id: None,
        }, "empty".into()));
    }
    ...
}
```

And `staged_to_source` (`main.rs:1595`) sets `relation_complete` from the
staged report (a complete scan with 0 skipped → `true`). An empty source thus
becomes a `SourceBatch` with `entries = []`, `relation_complete = true`,
`fingerprint` from the (empty) snapshot.

### Tombstone derivation in `commit_source_batches_if_changed`

`adapters-sqlite/src/lib.rs:1873`. The tombstone derivation is at
`lib.rs:2161-2198`:

```rust
// lib.rs:2161-2198 (abridged)
        let mut deletes = BTreeMap::new();
        let mut placement_delete_ids = BTreeSet::new();
        for prepared in prepared_sources.values() {
            if !prepared.relation_complete {
                continue;   // ← incomplete scans derive NO tombstones
            }
            let final_entity_ids: BTreeSet<&str> = prepared.replacement.entity_memberships ...;
            for prior in prepared.prior_entity_memberships.keys() {
                if !final_entity_ids.contains(prior.as_str())
                    && !final_entity_claimers.contains_key(prior)   // ← still claimed by another source?
                {
                    deletes.insert(prior.clone(), id);   // tombstone
                }
            }
            let final_placement_ids ... ;
            for prior in &prepared.prior_placement_ids {
                if !final_placement_ids.contains(prior.as_str())
                    && !final_placement_claimers.contains_key(prior)
                    && stored_placements.contains_key(prior)
                {
                    placement_delete_ids.insert(prior.clone());
                }
            }
        }
```

**Root-completeness semantics (critical for `--discover`)**: tombstones are
derived **only** when `relation_complete == true`. An incomplete scan
(`relation_complete == false`) at `lib.rs:1933-1937` *unions* observed claims
into the prior set (`final_entities = prior_entity_memberships.clone()`) and
derives **no deletes**. This is exactly the R2 requirement: "An incomplete scan
must not tombstone Sources that were merely not seen."

`scanned_paths` (`lib.rs:1896`) is the set of paths in the current batch.
Sources **not** in `scanned_paths` retain their prior memberships
(`lib.rs:2122-2132`) and are never tombstoned — they are "unseen, not removed."

### Implication for `--discover`

To tombstone a source that has been deleted from the provider root, discovery
must include that source's path in the batch as an **empty** `SourceBatch`
(`entries = []`, `relation_complete = true`). The existing empty-file path
(`stage_with_registry` for 0 bytes) already produces this. For a *missing*
file (deleted from disk, not just emptied), `capture` would fail with a
`SourceIo` error — the discovery code must synthesize an empty `SourceBatch`
for known-prior sources that are no longer present, **only** when the root scan
is complete. This is the new logic `--discover` must add.

---

## 6. `list_providers` command

### MCP tool handler

`crates/agent-session-grep-cli/src/mcp.rs:341`:

```rust
// mcp.rs:341-344
            "list_providers" => {
                reject_unknown_keys(args, &[])?;
                Ok(providers_payload())
            }
```

### `providers_payload` (authoritative provider list)

`mcp.rs:843`:

```rust
// mcp.rs:842-854
/// list_providers 不经 App：组合根的 registry 即权威清单（design §3）。
fn providers_payload() -> Value {
    let providers: Vec<Value> = provider_registry()
        .iter()
        .map(|adapter| json!({ "id": adapter.provider_id() }))
        .collect();
    success_payload(Outcome::Success, json!({ "providers": providers }), &protocol::Page::default(), &[])
}
```

### `provider_registry` (the composition-root source of truth)

`main.rs:1491`:

```rust
// main.rs:1488-1496
/// 组合根持有的 provider adapter 清单。ingest/sync 用它 probe-select，
/// 由 select_and_stage 挑出认领此源的 adapter（见 RFC-0002 §3）。
/// 新增 provider 只需在此登记一行。
fn provider_registry() -> Vec<Box<dyn ProviderAdapter>> {
    vec![
        Box::new(ClaudeCodeAdapter::new()),
        Box::new(CodexAdapter::new()),
    ]
}
```

`provider_registry` is defined in `main.rs` (private, not exported to `mcp.rs`
as a separate module — `mcp.rs` is a sibling module in the same crate and calls
it directly). The registry is the single point of truth for which providers
exist. A `--discover` implementation should iterate this registry to know which
provider roots to scan; each adapter exposes `provider_id()` but **not** a
data-root path, so root resolution must be added either as a new method on
`ProviderAdapter` or as a parallel mapping in the CLI composition root.

### Note: `list_providers` is MCP-only

There is **no CLI subcommand** `list-providers`; it exists only as an MCP tool.
`known_subcommand` (`main.rs:670`) and `subcommand_help_text` (`main.rs:694`)
do not list it. This is out of scope for `--discover` but worth noting: the
provider list is not surfaced on the CLI today.

---

## Implementation surface for `sync --discover`

### Files that need changes

| File | Nature of change |
|---|---|
| `crates/agent-session-grep-cli/src/main.rs` | **Additive flag**: `take_bool_flag(&mut args, "--discover")` in the `sync` arm (`~1054`); new discovery function that resolves provider data roots (home-dir logic mirroring `platform_paths_impl` at `~465-526`), walks each root for `.jsonl` files, and feeds the discovered paths into the existing `sync_files` pipeline (or a shared helper extracted from `sync_files`). Update `subcommand_help_text` (`~737`) and `help_text` (`~549`) for the new flag. The directory-rejection guard at `~1911-1917` must be bypassed for the discovery path. |
| `crates/agent-session-grep-cli/src/mcp.rs` (optional) | If `--discover` is also exposed as an MCP tool / param, add handler; otherwise no change. |
| `crates/agent-session-grep-ports/src/lib.rs` (optional) | If a new `data_root()` method is added to `ProviderAdapter` trait (`~631`), or a new `ProviderRootDiscovery` port is introduced. Alternatively, root resolution can stay CLI-local. |
| `crates/agent-session-grep-provider-claude/src/lib.rs` (optional) | If `ProviderAdapter` gains a `data_root()` method, implement it here returning `~/.claude/projects/`. Otherwise no change. |
| `crates/agent-session-grep-provider-codex/src/lib.rs` (optional) | Same: `~/.codex/sessions/`. |
| `crates/agent-session-grep-adapters-sqlite/src/source_fs.rs` (possible) | A new `SourceDiscovery` impl that walks a directory tree (production counterpart to `SnapshotFs` at `~126`). Alternatively, discovery can return `Vec<String>` paths directly to `sync_files` without going through the port. |
| `crates/agent-session-grep-cli/tests/e2e.rs` and/or `tests/provider_matrix.rs` | New tests for `sync --discover` with temp provider-root dirs. |

### Change nature summary

- **Additive flag**: `--discover` bool flag in the `sync` arm — mirrors existing
  `take_bool_flag` pattern.
- **New function(s)**: provider data-root resolver (home dir + marker), directory
  walker producing `Vec<String>` or `Vec<SourceSnapshot>`.
- **Possibly new trait method / port**: if root resolution belongs on
  `ProviderAdapter` rather than CLI-local.
- **No change** to `commit_source_batches_if_changed`, `sources_are_current`,
  `capture`/`verify_snapshot`, or the tombstone derivation — the existing
  store-layer semantics already satisfy R2 (incomplete scan = no tombstone) and
  R3 (read-only + verify). The discovery code only needs to produce the right
  `SourceBatch` list and feed it to the existing pipeline.

### Key design decisions for implementers

1. **Root-completeness marker**: the existing `relation_complete` field on
   `SourceBatch` already encodes complete-vs-incomplete. Discovery must set
   `relation_complete = true` on every discovered source only when the root walk
   completed without error; a partial walk (e.g. permission error mid-scan)
   must set `relation_complete = false` to suppress tombstones.
2. **Deleted sources**: a source previously synced but no longer on disk must
   be represented as an empty `SourceBatch` (entries = [], relation_complete =
   true) to trigger tombstoning. This requires the discovery code to enumerate
   **prior** source paths from the store (a new query on `source_scans`
   filtered by provider namespace) and diff against discovered paths.
3. **Empty-file vs missing-file**: the current `capture` fails on a missing
   file; the empty-file path in `stage_with_registry` handles 0-byte files but
   not missing files. Discovery must synthesize the empty batch for missing
   files without calling `capture`.

---

## Caveats / Not Found

- No existing home-dir → provider-data-root resolver exists anywhere in the
  codebase. The `platform_paths_impl` family resolves *this tool's* data root,
  not provider roots. A new resolver must be written.
- `ProviderAdapter` trait (`ports/src/lib.rs:631`) has no `data_root()` or
  `discover()` method. Whether to add one is a design decision, not a lookup.
- The `SourceDiscovery` port (`ports/src/lib.rs:64`) exists but has no
  production implementation — only test stubs. `--discover` would be its first
  real consumer.
- `list_providers` is MCP-only; there is no CLI `list-providers` subcommand.
- The marker strings `.claude` / `.codex` appear only in `installation_namespace`
  (`main.rs:1577-1580`) and in test assertions (`main.rs:2330-2351`). The
  canonical provider data-root subdirectories (`projects/` under `.claude`,
  `sessions/` under `.codex`) are documented in trellis-meta SKILL files and
  CONTEXT.md but are **not** encoded in any Rust source today.
