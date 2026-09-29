# 设计：提交写路径 profile 与逐项优化

## Harness

- 从归档研究复制 `search/`（`baseline.py`、`evidence.py`、`run_search.py`、fixtures）到本任务 `research/harness/`，把 `BASELINE_COMMIT` 硬 pin 改为显式参数（`--expected-commit`），并保留原始校验器语义（拒绝篡改、失败如实、有限超时）。
- 测量协议：新鲜库 A/B；1M × ≥3 次（before/after），10k/100k × 3；报告 median/p95、峰值 RSS、store 体积、语料哈希、二进制哈希。测量窗口独占机器，不并发构建/测试。
- SQLite trace：临时、测试专属的阶段计时（env 或 cfg 门控，不进入对外行为），按 batch 记录 begin/merge/insert/projection/finalize 耗时与行数，产出 JSON；不在生产默认路径常开。

## 候选与判据

1. 空转双读/双哈希消除（低风险、独立指标）：对指纹未变化的源只做一次读取/哈希；保持等长替换检测的现有语义，验证 noop p50/p95 与 1M 差值。
2. 批量语句与预编译复用：commit 路径中 per-message 的 placement/edge/fts upsert 改为批量或复用语句；以 trace 的 prepare/execute 计数与 100k 中位耗时为判据。
3. 整目录状态读取范围：按批邻域限定 `stored_placements/edges/membership` 载入；历史“非瓶颈”结论需在当前 main 复测，若 <5% 立即回退。
4. serde/id_json 复用：避免同一实体重复序列化/解析与 payload 克隆；以 100k 中位耗时与分配计数为判据。
5. 事务内 PRAGMA：page cache/temp_store 等会话级设置，保持 `synchronous` 持久性语义；以 1M 时间为判据。
6. 内存：若 trace 显示 staged batch/state map 是 RSS 主因，优先减小常驻集合（流式/增量），不改变提交原子性。

## 正确性与回滚

- 正确性依赖既有 outbox 两阶段、CAS generation、关系完整性、身份保真与来源只读测试；任何优化必须先过完整 SQLite 套件再进测量。
- 每个候选一个提交，附 before/after JSON；失败或收益不足的提交直接 revert，不保留死代码。
