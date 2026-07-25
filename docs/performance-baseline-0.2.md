# AgentSessions 0.2 性能基线

> 状态：初始可复现基线，不是正式 SLO；正式阈值只能在固定基准环境和标准 corpus 上冻结。
> 采集方式：`cargo test -p agentsessions-cli --test e2e perf_baseline_100_messages_index_and_search -- --nocapture`

## 测试配置

| 项目 | 值 |
|---|---|
| 数据规模 | 100 条独立 Message |
| 写入路径 | 100 次独立 CLI `index` 进程，SQLite + FTS5 + durable batch |
| 查询 | `search "performance baseline"` |
| 计时范围 | 包含 CLI 进程启动、数据库打开、lease/SQLite 初始化与终止开销 |
| 阈值性质 | 记录型 smoke，不设普通 CI 墙钟阻断；正式回归门由固定环境基准制定 |

## 运行结果

测试会在 stderr 输出：

```text
[perf-baseline] index 100 msgs: <index_ms>ms  search: <query_ms>ms
```

一次 Windows 开发机运行通过：

- Windows 记录型 smoke 示例：100 条消息独立索引 **1912 ms**、单次全文查询
  **16 ms**（历史观测，不作为当前阈值）。
- 普通 CI 只验证功能并输出本机观测值，不使用固定墙钟断言；跨环境结果不可直接比较。

## 解释与限制

该结果主要测量最差的 CLI 逐进程调用路径，不代表批量 sync 或单进程 Application API 的吞吐。它用于确认 durable outbox、writer lease、SQLite/FTS5 初始化没有出现数量级回归。

## 可复现证据 bundle（commit-pinned）

本页的单点 smoke 数字已被一份满足 SLI 方法的可复现基准取代，权威文件是
`docs/evidence/core-beta/88d86f4/`：

- 生成器：`scripts/evidence/core_beta_benchmark.py`（纯标准库、合成 Claude Code
  JSONL、不读取任何 provider 数据根）；报告契约见
  `docs/product/SLI-AND-BENCHMARK-FORMAT.md` §2.1。
- 样本量满足冻结方法：startup 冷/热各 20、search/show/get 各 100、sync 三种工作
  负载（首次 / no-op / 收缩）各 3。
- 每个 metric 保留 `raw_samples` 并按 nearest-rank 重算 P50/P95/P99、mean、样本标准
  差；peak RSS 100ms 采样，不可用时为 `null`。
- 环境、release 二进制 SHA-256、数据集 SHA-256、artifact 与 store 体积一并记录。
- SQLite 3.53.2 由同 commit、同 `rusqlite`/`libsqlite3-sys` 锁定版本的 WAL spike
  `rusqlite::version()` 输出交叉记录；Python sqlite3 版本不冒充 store 运行时版本。
- `recovery` 标为 `not_implemented`：CLI 在 open 时恢复，但无 fault-injection 入口，
  因此不宣称恢复耗时。
- 最终 capture 由 harness 在固定 commit 上执行 `cargo build --locked --release`，并记录
  `binary.provenance = built_by_harness_from_workspace`、release 二进制 SHA-256 和完整 Git SHA。

这仍是本地证据锚点，不是正式 SLO 或 release 认证。

## 已补齐 / 仍缺口

已在上述 bundle 中补齐：

- 固定 CPU/OS/Rust/SQLite 版本与冷/热定义；
- sync 首次、no-op、收缩/tombstone 工作负载；
- search/show/get P50/P95/P99、索引落盘体积、内存峰值采样。

仍缺口（未在本地采集，不得宣称通过）：

- 10k/100k sessions、10m messages / 50GB 标准 corpus 规模；
- 随机终止后的生产恢复耗时（需 production fault-injection 入口）；
- 中文、代码 identifier、路径和错误栈的 query set 与 recall/latency 对照；
- Linux（glibc 2.31 基线）/ macOS 上的同规格复测。
