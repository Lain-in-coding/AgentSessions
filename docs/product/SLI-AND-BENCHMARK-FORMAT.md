# SLI 定义与基准报告格式

> 治理记录（Governance Record）
>
> - decision_id: DOC-SLI-BENCHMARK
> - status: **Draft**（待 R0 评审）
> - owner: （待指派）
> - approver: 项目最终验收人
> - due_milestone: R0 Feasibility / Contract Gate
> - evidence_path: `docs/product/SLI-AND-BENCHMARK-FORMAT.md` + `spikes/*/EVIDENCE.md`
> - 对应计划 §2.2 / §2.3 / §2.4

本文冻结 R0 阶段的 **SLI 指标定义、采样方法和基准环境格式**。R0 只冻结定义与方法，不提前用缺少测量依据的精确数值阻断开发；具体数值 SLO 在 0.1 垂直切片取得基线、0.2 完成真实存储与检索后冻结。

---

## 1. SLI 指标清单（R0 冻结定义）

| SLI | 定义 | 采样方法 | 冻结时机 |
|---|---|---|---|
| `cli_startup_latency_ms` | 从进程启动到可接收命令的墙钟时间 | 冷/热各 20 次取中位数 | 0.1 |
| `search_latency_ms_p50/p95/p99` | 从 search 请求到返回首屏结果 | 固定 query set，100 次 | 0.2 |
| `show_latency_ms_p95` | show 请求到返回有限上下文 | 固定 id set，100 次 | 0.2 |
| `initial_index_throughput_mb_s` | 首次全量索引吞吐（MB/s） | 标准语料冷构建 | 0.2 |
| `incremental_scan_latency_ms` | 无变化增量扫描的墙钟时间 | 连续 3 次取中位数 | 0.2 |
| `peak_rss_mb` | 索引/查询期峰值常驻内存 | 采样 100ms 间隔 | 0.2 |
| `index_size_ratio` | 索引落盘体积 / 原始语料体积 | 落盘后 checkpoint 测量 | 0.2 |
| `recall_at_10` | 中/英/代码/路径分类的 top-10 召回 | 带 qrels 的固定 query set | 0.2 |
| `source_parse_coverage` | 认证 Provider fixture 合法记录的 Canonical 覆盖率 | golden 对照 | 0.2 |
| `recovery_time_ms` | 崩溃/中断后恢复到可服务的时间 | fault injection 后测量 | 0.2 |
| `robot_response_bytes` | robot/JSON 单次响应字节 | 固定预算下测量 | 0.2 |

## 2. 基准报告必填字段（每份报告强制）

```text
commit           = <git sha>
os               = windows | linux | macos
target_triple    = x86_64-pc-windows-msvc | ...
cpu              = <型号 + 核数>
ram_gb           = <总内存>
disk             = <SSD/NVMe/HDD + 型号>
filesystem       = NTFS | ext4 | APFS
dataset_hash     = <合成语料的 BLAKE3>
state            = cold | warm
sample_count     = <样本数>
warmup           = <预热方法>
median           = <中位数>
p95 / p99        = <尾延迟>
variance         = <方差或标准差>
antivirus_state  = <Windows Defender 开/关等安全软件状态>
sqlite_version   = <sqlite3 version()>
```

未记录上述字段的性能数字不得进入 SLO 冻结决策，也不得作为 Release 阻断依据。

## 3. 标准数据集（计划 §2.3）

- 100,000 Session / 10,000,000 Message-Event / 50GB 原始 transcript；
- 中/英/代码/路径/错误栈混合；
- 全部合成，遵循 fixture 脱敏规范，禁止真实 transcript；
- 该规模用于 0.2 之后的专项压测，不要求每个 PR CI 执行。

## 4. 已有实测锚点（来自 R0 spike）

search-backend spike 在 20k 合成文档上的初步数字（非正式 SLO，仅作趋势锚点）：

- recall@10：FTS5 与 Tantivy 均 1.000（backend-neutral analyzer 下打平）；
- 索引体积：FTS5 16.5MB / Tantivy 3.4MB；
- 查询延迟：FTS5 max 7.1ms / Tantivy max 0.56ms；
- 构建时间：FTS5 ~530ms / Tantivy ~430ms（20k 文档冷构建）。

正式 SLO 需在标准语料、正式 target 上按 §2 格式复测冻结。

## 5. 性能回归门

- 回归门槛在基线稳定后按各指标噪声分别制定，不统一硬编码 10%；
- 性能阈值只在固定基准环境冻结，不在普通 PR 机器上做硬阻断（计划 §11.2）。
