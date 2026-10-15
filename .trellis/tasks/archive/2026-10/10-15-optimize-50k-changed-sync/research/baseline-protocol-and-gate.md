# Research: 基线证据位置、测量协议与 R4 门槛数值

- Query: R3/R4 要求"同 harness、同矩阵、同 warmup/计数、三跑轮换顺序"复测；基线证据在哪里、协议细节是什么、门槛数值怎么算？
- Scope: internal
- Date: 2026-10-15

## Findings

### 1. 基线证据实际位置（重要）

PRD 写的 `docs/evidence/core-beta/b0fc44d75690/manifest.md` **不在当前工作区**
（本仓库 `docs/evidence/core-beta/` 只有 `88d86f4/`）。它存在于 canonical 产品
checkout：

```
.trellis/.runtime/attribution-closeout-20261012/canonical-repaired/
```

上面路径相对协调根；机器专属的绝对路径不进入共享记录。
该 checkout 的 `docs/evidence/core-beta/b0fc44d75690/` 内含：

- `manifest.md`（6,243 B）— suite 身份、冻结二进制、三份报告哈希、测量窗口、
  失败尝试披露、trace 说明、复现命令。
- `run-0{1,2,3}/core-beta-benchmark-full.json`（各 ~5.68 MB，raw samples 全保留）
  与同源 Markdown 投影。
- 同一工作树另存有 trace 试验与冻结二进制：
  `evidence-output/current-source-20261014-b0fc44d75690/`

### 2. 被测源与冻结物身份（S / B / H）

| 项 | 值 |
| --- | --- |
| Suite ID | `current-source-20261014-b0fc44d75690` |
| 被测源 S | `b0fc44d756909cc840050351dd1329896a8edf75`（canonical repo；`git -C <canonical> log` 可查，其 main 现为 `de2c95d`，公开基线由 `4f34287`/`de2c95d` 发布） |
| Harness | `scripts/evidence/core_beta_benchmark.py` 从 S 自身执行（H=S），SHA-256 `aeb23f909de805f507f45c1c61a20cf940a111ee962a0c6d6cd4ae37a49f39f8` |
| 冻结二进制 B | SHA-256 `39049bfd9340ce4ade9fc289b1ca89ab8af40e1d94942df7e0df565e791abe8e`，7,824,896 B；副本在 `evidence-output/current-source-20261014-b0fc44d75690/baseline-cli.exe` |
| 构建收据 | SHA-256 `88b3b1e9f03531ee51b074a98c0863ed95f13e16aa2cd91ed8f7d40da97888b1` |
| 环境 | Windows 10.0.22631 / x86_64-pc-windows-msvc / Intel Core Ultra 7 155H / 22 逻辑核 / 31.5 GiB / NVMe NTFS / Defender 实时防护开启；rustc 1.97.1；SQLite 3.53.2（bundled，构建联动收据）；Python 3.10.11 |
| 报告 JSON SHA-256 | run-01 `76996fba…d3623`、run-02 `77d6f7d5…4705b`、run-03 `5f3031d6…8bb4e0`（全值见 manifest） |

### 3. 测量矩阵与协议（"同矩阵"的具体含义）

来自 canonical 的 harness 源码（行号为 canonical checkout 内文件世界）：

- `SCALES = (("scale-2000", 10), ("scale-10000", 50), ("scale-50000", 250))`
  （`:45`），每文件 200 条消息 → 50k 规模 = **250 个源文件 × 200 消息**。
- 规模顺序轮换 `scale_order(ordinal)`（`:544`）：run1 small/medium/large、
  run2 medium/large/small、run3 large/small/medium。
- 每规模：sync 三相位（initial / noop / shrink）各 **3 个样本**；五类 search、
  show、get、context(mainline/full) 各 **100 个实测样本**（报告 observations
  记 103，含 3 个 warmup）。shrink 用 `shrink_dataset()` 把每个文件裁到前一半
  （50k → 25k 消息）后同步。
- 串行三跑：run1 构建并冻结 B + 收据，run2/run3 校验并复用 B，不再碰 Cargo；
  共享 suite 截止线 3600 s，单 child 上限 300 s；scratch 上限 4 GiB、
  可用空间 ≥8 GiB、RSS 上限 4096 MiB（`:48-52`）。
- child 环境策略 `CHILD_POLICY`：trace 关闭、`ASG_CURRENT_REPO`/`ASG_CLOCK_MS`
  剔除、cwd=source-root（`:55-60`）。
- 校验入口：`validate-report <report.json>` 与
  `validate-baseline <run-01> <run-02> <run-03>`（canonical harness `:2573,2575`）。
- trace 试验：仅 run3 结束后单独执行（`run_trace_trial`，`:1470`），对
  250 文件语料依次跑 initial(50k)/noop/shrink(25k)，`ASG_INDEX_TRACE` 只注入
  这些 child；原始 trace 仅存本地 `evidence-output/.../run-03/index-trace-*.jsonl`，
  harness 校验 stage 名并产出 `index-trace-observation.json`；instrumentation
  观测**永不进入主延迟样本**。合法 stage 集合见 `TRACE_STAGES`（`:1431-1437`）。
- 测量的主延迟是 `process_wall_ms`（wall 在 observer 清理前采集，
  `process-wall-before-observer/v1`）；报告同时保留 legacy 计时。

### 4. 基线 P50（三跑各自值；单位 ms；`process_wall_ms`）

50k 规模（门槛对象）：

| 系列 | run-01 | run-02 | run-03 | 三跑中位数 |
| --- | ---: | ---: | ---: | ---: |
| sync/initial | 10122.897 | 9440.515 | 10577.147 | **10122.897** |
| sync/shrink | 9943.210 | 9508.995 | 13726.753 | **9943.210** |
| sync/noop | 80.700 | 72.725 | 77.555 | 77.555 |
| search/token | 552.367 | 608.956 | 617.148 | 608.956 |
| search/latin | 375.356 | 430.174 | 428.881 | 428.881 |
| search/cjk | 171.621 | 196.148 | 190.987 | 190.987 |
| search/path | 171.645 | 185.240 | 188.199 | 185.240 |
| search/identifier | 166.165 | 185.912 | 187.812 | 185.912 |
| show | 12.356 | 14.289 | 13.647 | 13.647 |
| get | 11.616 | 16.527 | 14.258 | 14.258 |
| context/mainline | 35.441 | 45.978 | 38.988 | 38.988 |
| context/full | 49.499 | 61.141 | 57.194 | 57.194 |

10k 规模：initial 1889.734 / 2063.676 / 1687.886 → 中位 **1889.734**；
shrink 1580.968 / 1616.694 / 1560.987 → **1580.968**；noop 中位 24.220；
token 149.279、latin 110.047、cjk 95.299、path 97.124、identifier 95.262；
show 13.024、get 13.867；context 24.278/36.282。

2k 规模：initial 416.739 / 393.445 / 412.144 → **412.144**；shrink 350.669 /
379.494 / 365.194 → **365.194**；noop 中位 24.192；token 73.719、latin 67.040、
cjk 66.615、path 63.762、identifier 65.990；show 14.414、get 14.744；
context 18.307/32.276。

启动与 anchor（非分规模）：cold startup P50 = 22.759 / 28.086 / 35.689
（中位 **28.086**）；warm startup P50 = 8.639 / 8.794 / 9.254（中位 **8.794**）；
anchor initial = 754.250 / 843.243 / 719.153（中位 **754.250**）；
anchor shrink = 619.855 / 773.445 / 658.500（中位 **658.500**）；
anchor search 中位 71.163；anchor show 15.708；anchor get 14.428（各自 3 跑中位）。

### 5. R4 门槛算术（可直接引用）

- 50k initial：三跑 P50 中位数必须 < 10122.897 × 0.85 = **8604.46 ms**。
- 50k shrink：< 9943.210 × 0.85 = **8451.73 ms**。
- 其余系列（五类 search、show/get、context、noop、startup，及 2k/10k 规模）
  新中位数不得高于基线中位数 ×1.15（此处为 PRD"超出 15% 噪声带的回归"的
  等价操作化口径：同系列、同口径的 3 跑中位数对比）。
- 判定必须用同一 bundle 的对应系列中位数对比，且新报告先过
  `validate-report`/`validate-baseline`；不达标必须在文档中如实披露并保留
  原始样本（不得裁样、不得只挑最快轮）。

### 6. 复现命令（canonical checkout 内执行）

manifest 附的标准流程（占位值来自收据/清单）：

```powershell
$baselineSource = 'b0fc44d756909cc840050351dd1329896a8edf75'
if ((git rev-parse HEAD).Trim() -ne $baselineSource) { throw 'Use the clean measured source S.' }
$baselineSuite = 'current-source-20261014-b0fc44d75690'
foreach ($baselineRun in 1..3) {
    $baselineOutput = 'evidence-output/current-baseline/run-{0:D2}' -f $baselineRun
    python -B scripts/evidence/core_beta_benchmark.py run `
      --profile full --baseline-run $baselineRun --suite-id $baselineSuite `
      --workspace . --output-dir $baselineOutput `
      --expected-commit $baselineSource `
      --disk 'NVMe SSD SAMSUNG MZVL81T0HELB-00BTW' --filesystem NTFS `
      --antivirus-state 'Microsoft Defender Antivirus and real-time protection enabled' `
      --sqlite-version 3.53.2 `
      --sqlite-version-source 'locked bundled source + this build linkage; no direct CLI runtime query'
    if ($LASTEXITCODE -ne 0) { throw 'Baseline attempt failed; retain its evidence.' }
}
python -B scripts/evidence/core_beta_benchmark.py validate-baseline `
  evidence-output/current-baseline/run-01/core-beta-benchmark-full.json `
  evidence-output/current-baseline/run-02/core-beta-benchmark-full.json `
  evidence-output/current-baseline/run-03/core-beta-benchmark-full.json
```

优化后复测：同一 harness、同一命令，仅把 `--suite-id` 换新、`--expected-commit`
换为优化提交的完整 SHA、`--output-dir` 换新目录（已存在/非空的输出目录会被拒绝）。

### 7. 已披露的噪声与失败（必须随对比一起披露，不得隐藏）

- 跨轮 spread >15% 的系列（manifest 原文）：anchor get/search cjk/search token、
cold startup、anchor 与 10k initial sync、anchor shrink、50k shrink、部分
context 与 noop 系列；50k shrink 三轮 spread 达 **42%**（13.73 s vs 9.51 s）。
harness 的 `spread_percent`/`noise_warning` 字段（`:525-529`）会如实标出。
- 失败尝试：`07f75b0fe5aa…` 零样本构建 staged；`6dc0e55792db` 3,169 个观测后
anchor-size linkage 失败（589.4 s，不计入 suite）；两次尝试合计 ~62.5 min，
本 suite 用 52.7 min（上限 60 min）。
- 边界：单 Windows 主机、OS 缓存未受控、startup 20 样本/同步 3 样本分辨率有限；
本 bundle 是本地证据锚点，不是 SLO、跨平台或 recall 认证。

## Caveats / Not Found

- 本文件所有 baseline 数值提取自 canonical 的
  `docs/evidence/core-beta/b0fc44d75690/run-0{1,2,3}/…json`
  （`baseline.workloads[*].summaries[*].process_wall_ms.p50` 与顶层
  `metrics[*].summary.p50`），提取脚本未落盘；复算口径与 `validate-baseline`
  一致（nearest-rank P50 + 三跑中位数）。
- 当前工作区 checkout 不含该证据目录，也不含支持 `--baseline-run` 的 harness
  （见 `checkout-topology-and-implementation-target.md`）。
- 50k 之外规模的 search 分类值以本节表格为准；不要用 2k 的 anchor 数字替代。
- `--sqlite-version` 是操作者声明值，须与本 checkout 构建联动收据一致，否则
  run1 在 provenance 阶段拒绝。
