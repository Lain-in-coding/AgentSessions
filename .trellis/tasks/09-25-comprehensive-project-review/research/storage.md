# Storage Workstream Review（截至 2026-09-27）

## Scope

覆盖 source snapshot、SQLite provider staging、catalog/projection、semantic/hybrid 检索、cursor、writer/read lifecycle、批量一致性、embedding rebuild、安装迁移及相关测试。

## Findings and disposition

| 编号 | 基线问题 | 当前证据 | 当前状态 |
|---|---|---|---|
| P1-1 | WAL 活库只复制主库字节 | 基线复现见 `research/review-report.md`；修复任务记录了逻辑 SQLite backup/WAL 回归与 killed mutation | 已在 `a20e8ab` 修复并有本地回归；远端 PR #12 的通用 CI 尚未完成全量测试 |
| P1-2 | OpenCode/Cursor 多会话被归并 | 修复任务记录 message-level session observations、placement、usage、resume claim 回归 | 已在 `a20e8ab` 修复；本地 workspace 测试通过 |
| P1-3 | semantic/hybrid 忽略 provider/time/repo/facet | 当前 ports 已提供 `query_semantic_filtered`；SQLite 与 application 测试覆盖过滤先于 top-k/page | 已在 `a20e8ab` 修复；不得把历史基线报告继续当作当前代码状态 |
| P1-9 | read open 可写并可迁移 | 当前修复任务记录 read-only open、writer lease、旧 schema 显式升级路径 | 已在 `a20e8ab` 修复；仍需远端 CI 绿灯确认 |
| P2-1..P2-6 | cursor state、噪声分页、readiness、非有限分数、RRF、no-op integrity | 当前测试包含 mode/model/readiness/score/noise/RRF/no-op 回归；本地 `cargo test --workspace --offline --no-fail-fast` 通过 | 已有代码和回归证据；跨平台 hosted CI 仍被 Clippy 阻断 |
| P2-9 | installation namespace 与路径迁移 | schema v18、registry/location/receipt、原子 locator migration 已在 `4a4b486` 实现 | PR #12 open，未 merge；三平台通用 CI 的 Clippy job 失败，不能视为发布完成 |

## Performance / operability

- semantic scan 保持 bounded top-k，但仍是精确扫描；没有生产 p95/RSS 证明。
- embedding rebuild、metadata SQL、projection recovery 有回归和 bounded-shape 证据，但没有全平台真实大数据基准。
- `index_batches` retention、read/write lease 和 durable intent 的逻辑测试已覆盖；发布任务仍要求 fresh-clone / 三平台复核。

## Verification

本机（Windows，2026-09-27）：

- `cargo fmt --all --check`：通过。
- `cargo clippy --workspace --all-targets --offline -- -D warnings`：通过（本机 Rust 1.97.1）。
- `cargo test --workspace --offline --no-fail-fast`：通过。
- `cargo test -p agent-session-grep-application --features semantic-candle --offline`：287 passed。
- Python scripts/release/evidence：18 + 9 + 58 passed（scripts 1 skipped 为既有平台条件）。

远端：PR #12 的 `ci` test matrix 在 Ubuntu、Windows、macOS 均因 Clippy 1.98 的 `clippy::chunks_exact_to_as_chunks` 于 `crates/agent-session-grep-adapters-sqlite/src/lib.rs:8304` 失败；core-beta-evidence、installer smoke、cargo-deny、security-audit 通过。

## Limits

没有使用真实用户 catalog、transcript、provider 账号或私钥；未证明 32-bit、真实 provider 数据分布、Linux/macOS 运行时和生产规模性能。历史 `research/review-report.md` 以 `6cd1e6f` 为基线，不能替代本文件对当前 HEAD 的 disposition。
