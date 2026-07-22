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
| 阈值性质 | 软门槛，仅防止极端回归；不作为跨机器 SLO |

## 运行结果

测试会在 stderr 输出：

```text
[perf-baseline] index 100 msgs: <index_ms>ms  search: <query_ms>ms
```

一次 Windows 开发机运行通过：

- 100 条消息独立索引：**1912 ms**（含进程启动和测试环境开销）
- 单次全文查询：**16 ms**
- 当前 smoke 软门槛：索引 `< 10,000 ms`，查询 `< 3,000 ms`

## 解释与限制

该结果主要测量最差的 CLI 逐进程调用路径，不代表批量 sync 或单进程 Application API 的吞吐。它用于确认 durable outbox、writer lease、SQLite/FTS5 初始化没有出现数量级回归。

正式性能报告仍需补充：

- 固定 CPU/OS/Rust/SQLite 版本和冷/热缓存定义；
- 10k/100k sessions、10m messages 的标准 corpus；
- sync 首次、增量、no-op、收缩/tombstone 四种工作负载；
- search P50/P95/P99、索引体积、内存峰值和随机终止恢复时间；
- 中文、代码 identifier、路径和错误栈的 query set 与 recall/latency 对照。
