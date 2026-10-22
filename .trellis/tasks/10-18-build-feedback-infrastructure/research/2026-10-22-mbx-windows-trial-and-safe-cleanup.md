# MBX Windows trial and safe-cleanup checkpoint

- Date: 2026-10-22 Asia/Shanghai
- Source commit: `2fe3aba8dc6489a39d86882a16c1f024211fdcdd`
- Source tree: `eac2bb99b076d1f6a65ecdf9229fbf9642a47135`
- Toolchain: Cargo/Rust `1.97.1`
- MBX: `1.26.0` (official Windows MSVC archive, SHA-256 verified)
- Filesystem: `C:` NTFS, Developer Mode enabled

## Trial design

The trial used four detached Git worktrees at the same commit. Each build used
an explicit fresh target directory. MBX used one isolated private store, with
managed target views and its scheduler disabled so the result measures the
cross-worktree cache rather than target placement. Baseline and MBX builds used
`CARGO_INCREMENTAL=0`, `-j4`, `--locked`, and `--offline`.

## Result

| Metric | Native Cargo | MBX warm | Difference |
|---|---:|---:|---:|
| Three-run P50 | 66.554 s | 49.679 s | 25.35% faster |
| Nearest-rank P95 | 68.271 s | 52.371 s | 23.29% lower |
| Physical allocated footprint | 1.098 GiB | 5.793 GiB target + store | 5.28x |

- The MBX cold seed took 67.784 s, 1.85% slower than the native median.
- Each warm run had 111 hits and 104 misses across 215 lookups.
- 274 restored files were copied because NTFS did not use reflinks.
- All builds exited zero and both binaries passed `--version` smoke.
- The worktrees remained clean.
- `MBX_VERIFY=1` verified 85 outputs and reported 23 path-sensitive byte
  divergences. This is an important trust limitation, not a clean byte-for-byte
  equivalence result.

Decision: MBX is **opt-in only** for a bounded burst of multiple fresh
worktrees with an isolated store. It is not a single-checkout default and is not
a disk-saving mechanism.

## Safe cleanup

The following seven exact build-cache directories were inventoried before
mutation and moved to the Recycle Bin with exact per-item verification:

- Beta `target\managed-control-check`
- Beta `target\managed-control-check-wsl`
- Beta `target\background-followup`
- Beta `target\beta-native-linux`
- `wake-reuse-study\target\wake-reuse-scratch`
- `post-reuse-phase\target\index-throughput`
- `public-release-audit\target\release`

The pre-cleanup inventory was complete: 33,391,153,336 allocated bytes across
seven roots, zero errors. Each source path was absent after the operation and
the Recycle Bin exposed a matching new item and original location.

The MBX trial `targets` directory was also recycled and verified. A subsequent
attempt to recycle the generated trial `store` made the source disappear but
produced no matching `$R*` directory or Recycle Bin metadata. This indicates the
OS bypassed recycling and permanently removed that generated store. Cleanup was
stopped immediately. No product source, Git metadata, task evidence, Beta debug
cache, or other user data was affected. The four trial worktrees were not
removed after this incident.

Receipts and incident evidence:

- `.trellis/.runtime/build-feedback-infrastructure-20261018/cleanup-20261022/pre-cleanup-inventory.json`
- `.trellis/.runtime/build-feedback-infrastructure-20261018/cleanup-20261022/recycle-receipt.json`
- `.trellis/.runtime/build-feedback-infrastructure-20261018/mbx-trial-20261022/result-summary.json`
- `.trellis/.runtime/build-feedback-infrastructure-20261018/mbx-trial-20261022/trial-receipt.json`
- `.trellis/.runtime/build-feedback-infrastructure-20261018/mbx-trial-20261022/cleanup-incident.json`
