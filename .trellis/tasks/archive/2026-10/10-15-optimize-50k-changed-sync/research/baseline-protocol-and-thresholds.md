# Research: 已发布基线、复测协议与 R4 数值门槛

- Query: 50k changed-sync 优化的"已发布基线"具体绑定在哪个源码/二进制/harness 上？R3 复测必须复用哪些协议要素？R4 的数值门槛精确是多少？实现应落在哪个仓库/分支？
- Scope: mixed（本地证据副本 + 公开仓库 PR #19 元数据）；只读调查，未修改任何产品代码。
- Date: 2026-10-15
- 同目录姊妹文件分工：协议/矩阵/2k-10k-anchor 全量数值与噪声披露以 `baseline-protocol-and-gate.md` 为准；checkout 关系以 `checkout-topology-and-implementation-target.md` 为准；本文件只在需要处补充 50k 门槛算术、本地 artifact 哈希与"目标仓库"结论。

## Findings

### 1. 基线的权威身份

| 项 | 值 |
| --- | --- |
| 被测量源码 S | `b0fc44d756909cc840050351dd1329896a8edf75`（commit message: `fix(evidence): bind final anchor storage`） |
| 发布载体 | `Lain-in-coding/agent-session-grep` **PR #19**（title: `docs(evidence): publish current-source baseline`；head `4f34287c6060fe0b40a5deb0feb99f358c28a931`；撰写时 state=open / mergeable_state=blocked） |
| Suite ID | `current-source-20261014-b0fc44d75690` |
| Harness / checker | `scripts/evidence/core_beta_benchmark.py`，SHA-256 `aeb23f909de805f507f45c1c61a20cf940a111ee962a0c6d6cd4ae37a49f39f8`，**由 S 本身执行（H=S）** |
| 冻结二进制 B | `baseline-cli.exe`，SHA-256 `39049bfd9340ce4ade9fc289b1ca89ab8af40e1d94942df7e0df565e791abe8e`，7,824,896 B |
| Protocol | `agent-session-grep.core-beta-benchmark/v1` + 可选 baseline 扩展 `agent-session-grep.current-source-baseline/v1` |
| 环境 | Windows 10.0.22631、x86_64-pc-windows-msvc、Intel Core Ultra 7 155H、22 logical CPU、31.5 GiB RAM、NTFS on NVMe（SAMSUNG MZVL81T0HELB-00BTW）、Microsoft Defender 实时防护开启；rustc 1.97.1；SQLite 3.53.2（locked bundled source + build linkage）；Python 3.10.11 |

### 2. 本地可用的基线副本（关键）

`.\.trellis\.runtime\attribution-closeout-20261012\canonical-repaired\`

- 这是一个 git 仓库（remote = `https://github.com/Lain-in-coding/agent-session-grep.git`），HEAD `de2c95d2c93f7453ce5b5ccc169614d3aa7892ee`（`docs(evidence): publish current-source baseline`，位于 `b0fc44d75690` 之上，中间只多一个 publications commit）。
- 从 `b0fc44d75690..HEAD` 的产品改动只有 `docs/performance-baseline-0.2.md`（+33 行）；即该 checkout 的 Rust 源码 == S。
- 该 checkout 同时包含：
  - harness：`scripts/evidence/core_beta_benchmark.py`（sha 与 `aeb23f90…` 一致）；
  - 冻结二进制：`evidence-output/current-source-20261014-b0fc44d75690/baseline-cli.exe`（sha 与 `39049bfd…` 一致）；
  - 三份完整报告：`evidence-output/current-source-20261014-b0fc44d75690/run-01|02|03/core-beta-benchmark-full.{json,md}`；
  - **原始 trace**：`run-03/index-trace-{initial,noop,shrink}.jsonl` 与聚合 `run-03/index-trace-observation.json`（sha256 前缀：initial `e4d572141509a844`、shrink `97232a8a4cd34c02`、observation `5650cbc98478fa42`）。
  - `docs/evidence/core-beta/b0fc44d75690/` manifest + 三 run 拷贝（`attribution-closeout-20261012/canonical-repaired/docs/evidence/...`；协调根 `docs/evidence/core-beta/` 只有旧的 `88d86f4` bundle）。
- 该副本属于私有 runtime：**原始 trace、失败尝试、scratch 只能用于诊断，不得进入产品提交**；发布仅允许复核过的聚合（与基线 bundle 的披露政策一致）。

### 3. 协议要素（R3 复测必须逐项复用）

- 矩阵（baseline 扩展）：anchor（20 文件 × 200 消息，查询于 shrink 后的 2,000 消息库；startup 冷/热各 20）+ 三个完整库规模 `scale-2000`（10 文件）/`scale-10000`（50）/`scale-50000`（250），每类 startup 观测、每种 sync（initial/noop/shrink）各 3 样本、每类查询/读取各 100 次。
- 三跑轮换：`scale_order(ordinal)` 轮换规模顺序（run1 small/medium/large，run2 medium/large/small，run3 large/small/medium）；run 1 由 harness 构建 release 二进制并出 build receipt，run 2/3 复用收据（provenance 保留 `caller_supplied_prebuilt` + `source-to-binary linkage` caveat）。
- suite 预算：三跑 + trace 试验共享 60 分钟 suite deadline、300 秒/child；失败尝试必须保留并披露，不能裁剪、不能重跑掩盖。
- 校验入口：
  - `python -B scripts/evidence/core_beta_benchmark.py run --profile full --baseline-run N --suite-id <id> --workspace . --output-dir <dir> --expected-commit <full sha> --disk ... --filesystem ... --antivirus-state ... --sqlite-version 3.53.2 --sqlite-version-source 'locked bundled source + this build linkage; no direct CLI runtime query'`
  - `validate-report <report.json>`（逐份校验哈希/字段/样本数/重算统计）
  - `validate-baseline <r1.json> <r2.json> <r3.json>`（三份都通过且输出 run 间中位数比较）
  - `validate-baseline` 会强制 environment 字段非 `not_recorded`，且与 build receipt 的 target/rustc/sqlite provenance 一致——复测时必须用同样的 flags 与工具链。
- trace 试验（诊断用，非主延迟）：仅 run 3 之后、`ASG_INDEX_TRACE` 只在其子进程环境生效；矩阵 = `write_dataset(root/"d", 250, 200)` 后依次 initial → noop → shrink（`shrink_dataset` 每文件保留前半）；trace 阶段/标签集合在 harness `TRACE_STAGES`（`scripts/evidence/core_beta_benchmark.py:1431-1437`）闭合校验。
- 生成器（local 与 S harness 的 `synthetic_record`/`write_dataset`/`shrink_dataset`/`directory_size` 经比对**逐字节一致**）：`synthetic_record` 交替 user/assistant、uuid=`msg_v1_00000000-0000-4000-8000-{index:012d}`、parent 每 10 条断开、4 类内容（alpha/配置/src/main.rs/EVIDENCE42）、timestamp 按 index 轮转。50k 语料 = 250 文件 × 200 消息，源字节 14,027,058（本研究复算）。

### 4. 基线数值与 R4 门槛（三跑 P50 中位数）

scale-50000（来自 run-01/02/03 的报告表）：

| 系列 | run-01 | run-02 | run-03 | 三跑中位数 | R4 门槛 |
| --- | ---: | ---: | ---: | ---: | ---: |
| `sync/initial` P50 (ms) | 10122.897 | 9440.515 | 10577.147 | **10122.897** | 必须 < 8604.5（>15% 改善） |
| `sync/shrink` P50 (ms) | 9943.210 | 9508.995 | 13726.753 | **9943.210** | 必须 < 8451.7（>15% 改善） |
| `sync/noop` P50 (ms) | 80.700 | 72.725 | 77.555 | **77.555** | 不得 > 89.2（>15% 回归） |
| `search/token` P50 (ms) | 552.367 | 608.956 | 617.148 | 608.956 | 不得 > 700.3 |
| `search/latin` P50 (ms) | 375.356 | 430.174 | 428.881 | 428.881 | 不得 > 493.2 |
| `search/cjk` P50 (ms) | 171.621 | 196.148 | 190.987 | 190.987 | 不得 > 219.6 |
| `search/path` P50 (ms) | 171.645 | 185.240 | 188.199 | 185.240 | 不得 > 213.0 |
| `search/identifier` P50 (ms) | 166.165 | 185.912 | 187.812 | 185.912 | 不得 > 213.8 |
| `show` P50 (ms) | 12.356 | 14.289 | 13.647 | 13.647 | 不得 > 15.7 |
| `get` P50 (ms) | 11.616 | 16.527 | 14.258 | 14.258 | 不得 > 16.4 |
| `context/mainline` P50 (ms) | 35.441 | 45.978 | 38.988 | 38.988 | 不得 > 44.8 |
| `context/full` P50 (ms) | 49.499 | 61.141 | 57.194 | 57.194 | 不得 > 65.8 |

同步类其余量级（中位数，用于横向 sanity，非 R4 直接门槛）：2k initial 412.144 / 2k shrink 365.194；10k initial 1889.734 / 10k shrink 1580.968；anchor initial 754.178 / anchor shrink 658.437。跨轮噪声：50k shrink 跨轮 spread 42%（9943→13727），比较必须用三跑中位数并披露分布（PRD 已确认）。

### 5. 实现目标仓库与分支（重要）

- 协调根 `.` 的 remote 是**旧公共仓库** `qin-devs/AgentSessions`（GitHub 上已重定向为 `Lain-in-coding/AgentSessions`，最后推送 2026-10-07）；它的 `scripts/evidence/core_beta_benchmark.py`（sha `28017ffc…`）**不是**基线 harness，没有 baseline 扩展与 scale-50000 工作负载。
- 近期同级任务（`10-13-current-source-performance-baseline`、`10-13-gate-evidence-validation`）的 PR 都落在 `Lain-in-coding/agent-session-grep`（#18/#19）。本任务的实现与复测应在 agent-session-grep 一侧（canonical checkout 或由其 clone 的工作区）进行。
- 协调根产品代码与 S 的差异极小：`crates/agent-session-grep-adapters-sqlite/src/lib.rs` 仅 18 行（11+/7-，全部为注释/归属说明，无行为差异）；其余 crate 类似。因此本地走读行号对 S 有效（产品文件内 S 行号 ≈ 协调根行号 +6，见 `commit-path-hotspot-map.md`）。
- 若 PR #19 仍未合并：优化分支应基于其 head（`4f34287c`/本地 `de2c95d2`）以带上 harness/baseline 扩展；若已合并，则基于合并后的 main。注意 `docs/performance-baseline-0.2.md` 会被 R6 的跟进条目再次修改 → 与 PR #19 有冲突面，需在 PR 描述中说明。

### 6. 复测的两条比较口径（建议都做，成本可控）

1. **对基线报告比**（R4 硬门槛）：T 源码三跑 P50 中位数 vs 上表 S 的中位数。
2. **本机 control**（消除"本机当前状态 vs 2026-10-14 当天状态"的漂移）：先用本轮 harness 在**未优化源码**上跑一次同协议 suite，再与优化后对比；基线 bundle 允许这样做（同 harness、同矩阵）。若时间/预算只允许一套，至少用冻结二进制 `baseline-cli.exe` + 同 harness 生成器做一次 initial/shrink sanity。本研究已做该 sanity（见 `50k-journal-row-diagnostic.md`）：initial 9,990 ms / shrink 10,227 ms，落在基线三跑带宽内。

## Caveats / Not Found

- PR #19 的合并状态会变；本文件记录的是 2026-10-15 观察值（state=open）。落地前应重新核对（`gh pr view 19 -R Lain-in-coding/agent-session-grep`）。
- 基线环境收据（磁盘型号、Defender 状态、rustc、SQLite 版本）在 `validate-baseline` 中是硬校验字段；复测机若与之不同（例如磁盘型号变化），报告仍应如实记录，对比时在 PR/文档中披露。
- 未找到协调根内任何 b0fc44d75690 的提交对象（`git rev-parse` 失败）；基线源码的权威副本只有 runtime canonical checkout（及其 GitHub remote）。
- `run-01..03` 报告中 anchor/2k/10k 的全部查询级行未在本文转录；需要时从三份 `core-beta-benchmark-full.md` 直接读表（路径见 §2）。
