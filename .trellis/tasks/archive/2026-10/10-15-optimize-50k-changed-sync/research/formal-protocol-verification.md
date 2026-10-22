# Research: R4 正式基线数值与三跑执行协议独立核验

- Query: 从原始 source-bound JSON 和同版本 harness 独立确定全部比较系列、精确 R4 门槛、跨轮 spread，以及 S2 冻结后的 run 1/2/3、trace、验证顺序。
- Scope: internal；只读原始证据、规范和 harness；唯一写入为本研究文件；未执行 Git、build、test、benchmark 或新报告验证命令。
- Date: 2026-10-15
- Active task: `.trellis/tasks/archive/2026-10/10-15-optimize-50k-changed-sync`
- 产品文件相对根：`./.trellis/.runtime/attribution-closeout-20261012/canonical-repaired`；任务/spec 相对根：`.`。

## Findings

### 1. 决定性结论

**正式 `validate-baseline` 输出 48 个主计时系列，不是 56 个。**
原始报告另有 8 个顶层 legacy latency 指标；单独披露后共 56 行 latency。
48 = anchor 12 + 3 scales ×12。legacy 与 anchor 来自同批调用，不能视为独立观测。
第九个顶层 legacy metric 是 throughput，单位不是 ms，不计入这 56 行。

- 主系列使用 `process_wall_ms`；legacy 使用 observer cleanup 后的 `duration_ms`；禁止混用。
- R4 主门：50k initial/shrink 分别严格低于各自基线中位数 ×0.85；其他 46 个主系列 ≤ 各自基线中位数 ×1.15。
- 按分工要求，另列 8 个 legacy latency 的 ×1.15 上限；这是附加兼容口径，不能声称 harness 本身比较了 56 项。
- **initial：10122.897400 ×0.85 = 8604.462790 ms，新中位数必须严格小于此值。**
- **shrink：9943.210300 ×0.85 = 8451.728755 ms，新中位数必须严格小于此值。**
- 既有文档里的 `10122.897`、`9943.210`、`8604.5`、`8451.7` 为舍入展示，不可用于精确判门。

代码依据：`scripts/evidence/core_beta_benchmark.py:2294`、`:2312`、`:2320`；双时钟规范 `docs/product/SLI-AND-BENCHMARK-FORMAT.md:143`。

### 2. 原始证据身份（本次重新计算哈希）

| 身份 | 复核值 |
| --- | --- |
| 原源码 S | `b0fc44d756909cc840050351dd1329896a8edf75` |
| 原 suite | `current-source-20261014-b0fc44d75690` |
| B SHA-256 | `39049bfd9340ce4ade9fc289b1ca89ab8af40e1d94942df7e0df565e791abe8e`；7,824,896 bytes |
| 原/current harness SHA-256 | `aeb23f909de805f507f45c1c61a20cf940a111ee962a0c6d6cd4ae37a49f39f8`；当前文件一致 |
| build receipt SHA-256 | `88b3b1e9f03531ee51b074a98c0863ed95f13e16aa2cd91ed8f7d40da97888b1` |
| Run 1 JSON SHA-256 | `76996fba2cbbd1fe305859371429de9c485cdae610bee2de3b5654fdc28d3623`；5,681,417 bytes |
| Run 2 JSON SHA-256 | `77d6f7d586df524170681ea65821f840b83411970ee7ee908c4734cc5d47057b`；5,682,021 bytes |
| Run 3 JSON SHA-256 | `5f3031d6895ffb886328d97335f485424c60252e44f4fb4c7f118c40aa8bb4e0`；5,681,712 bytes |

以上来自指定原 bundle；B 来自私有 `evidence-output/current-source-20261014-b0fc44d75690/`。
receipt SHA 是 `build-receipt.json` 内 `receipt` 对象 canonical JSON 的哈希，即 wrapper 的 `sha256`，**不是整个 wrapper 文件的文件哈希**。已重新计算并吻合。

### 3. 矩阵、计数、顺序

| Section | 输入 | 查询使用库 | 各 sync phase n | Search | Show / get | Context | Startup |
| --- | --- | --- | ---: | --- | --- | --- | --- |
| anchor | 20×200 = 4,000 | trial 3 post-shrink 2,000 | 3 | 总计100，每类20；无 query warmup | 各100，无 read warmup | 无 | cold/warm 各20；warm 前3 warmup |
| scale-2000 | 10×200 = 2,000 | trial 3 intact 2,000 | 3 | 每类3 warmup +100 measured | 各3 warmup +100 measured | mainline/full 各3 warmup +100 measured | 无 |
| scale-10000 | 50×200 = 10,000 | trial 3 intact 10,000 | 3 | 同上 | 同上 | 同上 | 无 |
| scale-50000 | 250×200 = 50,000 | trial 3 intact 50,000 | 3 | 同上 | 同上 | 同上 | 无 |

- 每轮先跑 1×20 的 correctness preflight，共8个 verification 调用。每个 scale 的三个 trial 各用新 store/dataset，initial → immediate noop → shrink；只在第三 trial 的 noop/shrink 之间查询。shrink 每文件留前100条。
- anchor 末尾另有一次 doctor verification。最终 footprint 在全部读取及 doctor 后采集，不可拿 shrink 刚退出时的 footprint 替代。
- 每轮 observations = preflight 8 + anchor 353 +3×936 = **3,169**；其中 measured **3,076**、warmup **84**、verification **9**。
- 三轮主调用共9,507；run3后另有3个独立 trace verification 调用，不进主延迟池。
- scale 轮换：run1 `2000 →10000 →50000`，run2 `10000 →50000 →2000`，run3 `50000 →2000 →10000`。
- scale search 先完成所有类别 warmup，再类别 round-robin；query = `benchmarktoken` / `alpha` / `配置` / `"src/main.rs"` / `EVIDENCE42`。
- scale search：`--mode lexical --max-items 20 --max-bytes 4194304`；context：`--level raw --policy mainline|full --max-messages 500 --max-bytes 4194304`。show/get 按 generator file 轮转的100索引选实体；context 选 generator file 0 的 Session。
- root legacy search 是混合五类的100样本，不是任一 scale query class 的替代物。

代码：harness `:35`、`:45`、`:544`、`:1097`、`:1299`、`:1319`、`:1337`、`:1601`、`:1626`、`:1733`。
规范：`docs/product/SLI-AND-BENCHMARK-FORMAT.md:107`、`:122`。

### 4. 全部56行精确基线表

单位 ms。以 `Decimal` 读取三个原始 JSON，从每轮 raw observations/raw_samples 重算 nearest-rank P50，并逐项断言等于 summary；再取三跑 P50 中位数。
主系列 raw 路径：`baseline.anchor|workloads[*].observations` 的 `role=measured`；legacy raw 路径：`metrics[metric].raw_samples`。
⚠ 表示基线 spread 严格 >15%，**不放宽本表固定上限**。

| 系列 | 每轮 measured n | Run 1 P50 | Run 2 P50 | Run 3 P50 | 三跑中位数 | 新中位数门槛 | 基线 spread % |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `anchor/get` | 100 | 14.355900 | 16.182000 | 12.703200 | 14.355900 | ≤ 16.509285 | 24.232546 ⚠ |
| `anchor/search/cjk` | 20 | 71.382000 | 67.672600 | 60.815500 | 67.672600 | ≤ 77.823490 | 15.614148 ⚠ |
| `anchor/search/identifier` | 20 | 63.926900 | 70.021100 | 60.895300 | 63.926900 | ≤ 73.515935 | 14.275368 |
| `anchor/search/latin` | 20 | 71.413600 | 72.361200 | 67.686100 | 71.413600 | ≤ 82.125640 | 6.546512 |
| `anchor/search/path` | 20 | 66.939100 | 70.833400 | 71.873400 | 70.833400 | ≤ 81.458410 | 6.966064 |
| `anchor/search/token` | 20 | 78.959300 | 85.655200 | 71.156600 | 78.959300 | ≤ 90.803195 | 18.362118 ⚠ |
| `anchor/show` | 100 | 15.755100 | 15.623400 | 12.518400 | 15.623400 | ≤ 17.966910 | 20.717001 ⚠ |
| `anchor/startup_cold` | 20 | 22.697600 | 28.000900 | 35.610400 | 28.000900 | ≤ 32.201035 | 46.115661 ⚠ |
| `anchor/startup_warm` | 20 | 8.561300 | 8.734600 | 9.191000 | 8.734600 | ≤ 10.044790 | 7.209260 |
| `anchor/sync/initial` | 3 | 754.178300 | 843.145600 | 719.086700 | 754.178300 | ≤ 867.305045 | 16.449545 ⚠ |
| `anchor/sync/noop` | 3 | 17.295400 | 17.628800 | 18.005600 | 17.628800 | ≤ 20.273120 | 4.028635 |
| `anchor/sync/shrink` | 3 | 619.762000 | 773.319900 | 658.437100 | 658.437100 | ≤ 757.202665 | 23.321575 ⚠ |
| `scale-2000/context/full` | 100 | 32.013100 | 40.095900 | 32.276400 | 32.276400 | ≤ 37.117860 | 25.042446 ⚠ |
| `scale-2000/context/mainline` | 100 | 18.285500 | 23.233100 | 18.306700 | 18.306700 | ≤ 21.052705 | 27.026171 ⚠ |
| `scale-2000/get` | 100 | 12.666100 | 15.724400 | 14.743600 | 14.743600 | ≤ 16.955140 | 20.743238 ⚠ |
| `scale-2000/search/cjk` | 100 | 66.615000 | 68.572300 | 64.109100 | 66.615000 | ≤ 76.607250 | 6.699992 |
| `scale-2000/search/identifier` | 100 | 59.790000 | 67.289400 | 65.990100 | 65.990100 | ≤ 75.888615 | 11.364432 |
| `scale-2000/search/latin` | 100 | 64.177100 | 71.299900 | 67.040200 | 67.040200 | ≤ 77.096230 | 10.624670 |
| `scale-2000/search/path` | 100 | 63.762200 | 67.985800 | 63.064800 | 63.762200 | ≤ 73.326530 | 7.717739 |
| `scale-2000/search/token` | 100 | 71.066200 | 79.089900 | 73.719300 | 73.719300 | ≤ 84.777195 | 10.884124 |
| `scale-2000/show` | 100 | 14.030100 | 17.967700 | 14.413500 | 14.413500 | ≤ 16.575525 | 27.318833 ⚠ |
| `scale-2000/sync/initial` | 3 | 416.738700 | 393.445100 | 412.144300 | 412.144300 | ≤ 473.965945 | 5.651807 |
| `scale-2000/sync/noop` | 3 | 15.276600 | 24.191500 | 37.423600 | 24.191500 | ≤ 27.820225 | 91.548684 ⚠ |
| `scale-2000/sync/shrink` | 3 | 350.669400 | 379.494300 | 365.194400 | 365.194400 | ≤ 419.973560 | 7.893029 |
| `scale-10000/context/full` | 100 | 35.171000 | 36.282500 | 36.905000 | 36.282500 | ≤ 41.724875 | 4.779164 |
| `scale-10000/context/mainline` | 100 | 24.277600 | 23.248500 | 27.976000 | 24.277600 | ≤ 27.919240 | 19.472683 ⚠ |
| `scale-10000/get` | 100 | 11.918600 | 14.315100 | 13.867200 | 13.867200 | ≤ 15.947280 | 17.281787 ⚠ |
| `scale-10000/search/cjk` | 100 | 95.298500 | 93.581500 | 102.160600 | 95.298500 | ≤ 109.593275 | 9.002345 |
| `scale-10000/search/identifier` | 100 | 95.334200 | 94.710400 | 95.261600 | 95.261600 | ≤ 109.550840 | 0.654828 |
| `scale-10000/search/latin` | 100 | 110.046900 | 109.230900 | 115.001900 | 110.046900 | ≤ 126.553935 | 5.244128 |
| `scale-10000/search/path` | 100 | 97.124200 | 91.277500 | 99.868800 | 97.124200 | ≤ 111.692830 | 8.845684 |
| `scale-10000/search/token` | 100 | 149.279400 | 143.476000 | 151.703000 | 149.279400 | ≤ 171.671310 | 5.511142 |
| `scale-10000/show` | 100 | 13.014600 | 13.184200 | 13.024500 | 13.024500 | ≤ 14.978175 | 1.302161 |
| `scale-10000/sync/initial` | 3 | 1889.733900 | 2063.675900 | 1687.886200 | 1889.733900 | ≤ 2173.193985 | 19.885853 ⚠ |
| `scale-10000/sync/noop` | 3 | 23.876800 | 24.220000 | 32.598300 | 24.220000 | ≤ 27.853000 | 36.009496 ⚠ |
| `scale-10000/sync/shrink` | 3 | 1580.967900 | 1616.693700 | 1560.987200 | 1580.967900 | ≤ 1818.113085 | 3.523569 |
| `scale-50000/context/full` | 100 | 49.499200 | 61.140500 | 57.194100 | 57.194100 | ≤ 65.773215 | 20.354023 ⚠ |
| `scale-50000/context/mainline` | 100 | 35.441000 | 45.978500 | 38.988400 | 38.988400 | ≤ 44.836660 | 27.027270 ⚠ |
| `scale-50000/get` | 100 | 11.616100 | 16.526700 | 14.257600 | 14.257600 | ≤ 16.396240 | 34.441982 ⚠ |
| `scale-50000/search/cjk` | 100 | 171.621300 | 196.148100 | 190.986600 | 190.986600 | ≤ 219.634590 | 12.842158 |
| `scale-50000/search/identifier` | 100 | 166.164600 | 185.912500 | 187.812400 | 185.912500 | ≤ 213.799375 | 11.644080 |
| `scale-50000/search/latin` | 100 | 375.355600 | 430.174300 | 428.881300 | 428.881300 | ≤ 493.213495 | 12.781788 |
| `scale-50000/search/path` | 100 | 171.645500 | 185.239800 | 188.198600 | 185.239800 | ≤ 213.025770 | 8.936039 |
| `scale-50000/search/token` | 100 | 552.367400 | 608.956100 | 617.148300 | 608.956100 | ≤ 700.299515 | 10.638025 |
| `scale-50000/show` | 100 | 12.355900 | 14.288900 | 13.646700 | 13.646700 | ≤ 15.693705 | 14.164597 |
| `scale-50000/sync/initial` | 3 | 10122.897400 | 9440.515200 | 10577.147100 | 10122.897400 | < 8604.462790 | 11.228326 |
| `scale-50000/sync/noop` | 3 | 80.699900 | 72.725300 | 77.555000 | 77.555000 | ≤ 89.188250 | 10.282509 |
| `scale-50000/sync/shrink` | 3 | 9943.210300 | 9508.994600 | 13726.752700 | 9943.210300 | < 8451.728755 | 42.418474 ⚠ |
| `legacy/cli_startup_cold_latency_ms` | 20 | 22.759400 | 28.085900 | 35.689300 | 28.085900 | ≤ 32.298785 | 46.036979 ⚠ |
| `legacy/cli_startup_warm_latency_ms` | 20 | 8.638600 | 8.794500 | 9.253900 | 8.794500 | ≤ 10.113675 | 6.996418 |
| `legacy/get_latency_ms` | 100 | 14.427500 | 16.266700 | 12.783100 | 14.427500 | ≤ 16.591625 | 24.145555 ⚠ |
| `legacy/initial_sync_latency_ms` | 3 | 754.250500 | 843.243400 | 719.153400 | 754.250500 | ≤ 867.388075 | 16.452094 ⚠ |
| `legacy/noop_sync_latency_ms` | 3 | 17.368500 | 17.740200 | 18.065800 | 17.740200 | ≤ 20.401230 | 3.930621 |
| `legacy/search_latency_ms` | 100 | 71.163300 | 74.425000 | 66.374900 | 71.163300 | ≤ 81.837795 | 11.312151 |
| `legacy/show_latency_ms` | 100 | 15.828100 | 15.708100 | 12.588100 | 15.708100 | ≤ 18.064315 | 20.626301 ⚠ |
| `legacy/shrink_sync_latency_ms` | 3 | 619.854600 | 773.444700 | 658.500300 | 658.500300 | ≤ 757.275345 | 23.324226 ⚠ |

### 5. 正确算术

- 每轮 `Q50=sorted(x)[ceil(n/2)-1]`；偶数20/100样本取第10/50个，不取两中间值平均。三次sync取中间值。
- 基线 `b=Q50([b1,b2,b3])`，新值 `o=Q50([o1,o2,o3])`。改善门 `o <0.85*b`，即 `100*(b-o)/b >15`；恰好改善15%不通过。其他上限 `o <=1.15*b`，恰好慢15%通过。
- bundle 内 spread = `100*(max(p50)-min(p50))/Q50(p50)`，分母为三跑中位数，不是min或max。
- 基线 initial spread **11.228326%**；shrink **42.418474%**，后者原式 `100*(13726.7527-9508.9946)/9943.2103`。
- 中位数/range 同为0则 spread=0；中位数0且range>0则 null +告警，不输出Infinity。
- 不合并三轮raw池后求一次中位数，不混合计时口径，不扣startup，不挑最快一轮，不裁剪样本。spread只披露噪声；R4比较old/new三跑中位数。即使旧spread达91.5%，也不放宽1.15倍回归上限。

代码：harness `:89`、`:512`、`:519`、`:1122`；规范 `docs/product/SLI-AND-BENCHMARK-FORMAT.md:172`。

### 6. S2、receipt、环境与deadline条件

1. O6决定、严格oracle、质量门、源码审查结束后才提交并freeze clean S2。本研究不宣布这些已完成。O5已否决，schema仍19；未提交原型不能进入formal suite。
2. 记录S2完整40位SHA与`HEAD^{tree}`，`--expected-commit`用S2；从S2自身执行同哈希harness/checker，不能从协调根旧harness启动。
3. 已有跨worktree共享Cargo target产生陈旧binary的事故。本次用**全新、从未存在、suite独占**的`CARGO_TARGET_DIR`，让run1通过Cargo JSON构建/挑选真实executable；额外要求receipt `artifact_was_cached=false`。不复用其他worktree target，不手拷`target/release`。
4. 原基线receipt为`artifact_was_cached=true`、`source-root/target`；本次预期false与`explicit-build-target`，该构建缓存策略差异必须披露。它在既有harness支持范围内；不改变release profile/features/SQLite或测量协议，也不是独立binary attestation。
5. baseline模式禁止`--binary`；没有`--skip-build`、`--suite-timeout`、`--child-timeout`。run2/3从共同parent自动复用run1 receipt/B；`binary_use=reused_run_1_receipt`，顶层仍为`caller_supplied_prebuilt`，不得改写为重建。
6. 三个output须为新`evidence-output/<suite>/`下的sibling目录，output必须为空；已有claim/receipt不可覆盖。run1写`baseline-cli.exe`、`build-receipt.json`、`suite.json`；不要先把日志放进run output目录。
7. 三轮保持`CARGO_HOME`、target绝对路径、scratch parent（TEMP/TMP）、PATH选中的cargo/rustc、环境/配置不变；private `build_context_paths_sha256`会检查这点。不要在两轮间切worktree或修改源码。
8. 层级`.cargo/config`/`config.toml`及未证明的`RUSTFLAGS`、`RUSTC_WRAPPER`、`RUSTUP_TOOLCHAIN`、SQLite/linker等覆盖会被拒绝（部分即使空值也拒绝）；不要静默清除/放宽。允许项包括`CARGO_HOME/TARGET_DIR/BUILD_TARGET/BUILD_JOBS`与指定terminal变量；本次保持原null native-target/jobs字段便于审计。
9. 主child自动剔除大小写变体的`ASG_INDEX_TRACE`、`ASG_CURRENT_REPO`、`ASG_CLOCK_MS`；cwd为source root。只有独立trace child注入trace target，不需修改全局环境。
10. run1构建、linkage、receipt、命令长度检查完成后才开启共享**3600s**时钟；三轮preflight/测量/间隔/run3 trace共用。单测量child timeout=`min(300,remaining)`，300s不是整个driver超时。构建在suite外，自有3600s timeout。
11. scratch用tempfile磁盘，不必然为output磁盘；初始可用≥8GiB，scratch边界≤4GiB，观测RSS≤4096MiB。保持磁盘/NTFS/Defender实时防护，不清OS cache。
12. `--disk`/AV/SQLite等声明须核对实况。原环境：Windows10.0.22631、native x86_64 MSVC、rustc/cargo1.97.1、Python3.10.11、bundled SQLite3.53.2；Python sqlite3 3.40.1不能冒充store版本。

代码：harness `:835`、`:861`、`:979`、`:1033`、`:1231`、`:1363`、`:1502`、`:1654`、`:1889`。
规范：`docs/product/SLI-AND-BENCHMARK-FORMAT.md:200`。

### 7. 可执行PowerShell顺序（主会话freeze后执行；本研究未运行）

磁盘/AV/SQLite参数来自原基线，执行前核实仍成立。先等oracle、测试、构建实验退出，三跑之间不开展其他work。
时间戳只用于suite命名；被测源码取实际freeze后的S2，不能取未来publication commit。

```powershell
$ErrorActionPreference = 'Stop'
Set-Location -LiteralPath './.trellis/.runtime/attribution-closeout-20261012/canonical-repaired'
$optSource = (& git rev-parse HEAD).Trim()
if ($LASTEXITCODE -ne 0 -or $optSource -notmatch '^[0-9a-f]{40}$') { throw 'Cannot identify frozen S2.' }
$optTree = (& git rev-parse 'HEAD^{tree}').Trim()
if ($LASTEXITCODE -ne 0) { throw 'Cannot identify S2 tree.' }
$optDirty = & git status --porcelain=v1 --untracked-files=all
if ($LASTEXITCODE -ne 0 -or $optDirty) { throw 'Freeze requires a clean source tree.' }
$optHarness = 'scripts/evidence/core_beta_benchmark.py'
$optHarnessSha = 'aeb23f909de805f507f45c1c61a20cf940a111ee962a0c6d6cd4ae37a49f39f8'
if ((Get-FileHash -LiteralPath $optHarness -Algorithm SHA256).Hash.ToLowerInvariant() -ne $optHarnessSha) { throw 'Protocol bytes changed.' }
if (Test-Path Env:CARGO_BUILD_TARGET) { throw 'Review target override against original null baseline field.' }
if (Test-Path Env:CARGO_BUILD_JOBS) { throw 'Review jobs override against original null baseline field.' }
$optSuite = 'opt50k-{0}-{1}' -f $optSource.Substring(0,12), (Get-Date -Format 'yyyyMMddTHHmmss')
$optSuiteRoot = Join-Path (Get-Location).ProviderPath ('evidence-output/' + $optSuite)
$optTarget = Join-Path $optSuiteRoot 'cargo-target'
if (Test-Path -LiteralPath $optSuiteRoot) { throw 'Use a completely new suite directory.' }
New-Item -ItemType Directory -Path $optSuiteRoot | Out-Null
[ordered]@{
    source_commit = $optSource
    source_tree = $optTree
    harness_sha256 = $optHarnessSha
    freeze_utc = (Get-Date).ToUniversalTime().ToString('o')
    target_policy = 'fresh suite-owned explicit target; no cross-worktree reuse'
} | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $optSuiteRoot 'source-freeze.json') -Encoding utf8
$optHadTarget = Test-Path Env:CARGO_TARGET_DIR
$optPriorTarget = $env:CARGO_TARGET_DIR
try {
    $env:CARGO_TARGET_DIR = $optTarget
    foreach ($optRun in 1..3) {
        $optOutput = Join-Path $optSuiteRoot ('run-{0:D2}' -f $optRun)
        python -B $optHarness run `
          --profile full --baseline-run $optRun --suite-id $optSuite `
          --workspace . --output-dir $optOutput --expected-commit $optSource `
          --disk 'NVMe SSD SAMSUNG MZVL81T0HELB-00BTW' --filesystem NTFS `
          --antivirus-state 'Microsoft Defender Antivirus and real-time protection enabled' `
          --sqlite-version 3.53.2 `
          --sqlite-version-source 'locked bundled source + this build linkage; no direct CLI runtime query'
        if ($LASTEXITCODE -ne 0) { throw 'Formal attempt failed; retain evidence and do not auto-retry.' }
        $optReceipt = Get-Content -LiteralPath (Join-Path $optSuiteRoot 'build-receipt.json') -Raw | ConvertFrom-Json
        if ($optReceipt.receipt.artifact_was_cached -ne $false) { throw 'Unexpected cached artifact in fresh target.' }
        if ($optReceipt.receipt.source.commit -ne $optSource) { throw 'Receipt source differs from S2.' }
        if ($optReceipt.receipt.source.harness_sha256 -ne $optHarnessSha) { throw 'Receipt harness changed.' }
        $optBinarySha = (Get-FileHash -LiteralPath (Join-Path $optSuiteRoot 'baseline-cli.exe') -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($optBinarySha -ne $optReceipt.receipt.binary_sha256) { throw 'Frozen binary changed.' }
    }
    # Run 3 already performed trace; capture the total bound immediately.
    @'
import json, pathlib, sys, time
root = pathlib.Path(sys.argv[1])
suite = json.loads((root / 'suite.json').read_text(encoding='utf-8'))
trace = json.loads((root / 'run-03/index-trace-observation.json').read_text(encoding='utf-8'))
elapsed_ms = (time.monotonic_ns() - suite['started_monotonic_ns']) / 1e6
assert trace['status'] == 'available', 'Trace unavailable despite possible primary validation success'
assert [p['phase'] for p in trace['phases']] == ['initial', 'noop', 'shrink']
observations = trace['attempt']['observations']
assert len(observations) == 3
assert all(o['outcome'] == 'success' and o['exit_code'] == 0 and o['process_wall_ms'] <= 300000 for o in observations)
assert 0 <= elapsed_ms <= 3600000, 'Suite including trace and return exceeded 3600 seconds'
receipt = {'suite_id': suite['suite_id'], 'through_trace_and_driver_return_ms': elapsed_ms,
           'trace_status': trace['status'], 'trace_phase_count': len(trace['phases'])}
with (root / 'suite-end-check.json').open('x', encoding='utf-8') as handle:
    json.dump(receipt, handle, ensure_ascii=False, indent=2)
    handle.write('\n')
print(json.dumps(receipt, ensure_ascii=False))
'@ | python -B - $optSuiteRoot
    if ($LASTEXITCODE -ne 0) { throw 'Trace or total bound failed; retain evidence.' }
    $optReports = @(1..3 | ForEach-Object {
        Join-Path $optSuiteRoot ('run-{0:D2}/core-beta-benchmark-full.json' -f $_)
    })
    foreach ($optReport in $optReports) {
        python -B $optHarness validate-report $optReport
        if ($LASTEXITCODE -ne 0) { throw 'Report validation failed.' }
    }
    python -B $optHarness validate-baseline @optReports
    if ($LASTEXITCODE -ne 0) { throw 'Three-run baseline validation failed.' }
    $optAfter = (& git rev-parse HEAD).Trim()
    if ($LASTEXITCODE -ne 0 -or $optAfter -ne $optSource) { throw 'Frozen source changed.' }
    $optAfterTree = (& git rev-parse 'HEAD^{tree}').Trim()
    if ($LASTEXITCODE -ne 0 -or $optAfterTree -ne $optTree) { throw 'Frozen tree changed.' }
    $optDirty = & git status --porcelain=v1 --untracked-files=all
    if ($LASTEXITCODE -ne 0 -or $optDirty) { throw 'Source became dirty.' }
    Write-Output ('Validated suite: {0}; S2={1}; tree={2}; B={3}' -f $optSuiteRoot, $optSource, $optTree, $optBinarySha)
} finally {
    if ($optHadTarget) { $env:CARGO_TARGET_DIR = $optPriorTarget }
    else { Remove-Item Env:CARGO_TARGET_DIR }
}
```

计时终点检查在正式validators之前。`validate-baseline`的comparison仅为**新suite内部48项spread**，通过验证器**不等于R4通过**。
仍须将新报告全部主系列三跑中位数逐项对照§4，并披露old/new spread；8个legacy按其自身口径附加核对。

### 8. trace/完整性/发布陷阱

- **单独查trace `status=available`。** `run_trace_trial`捕获缺文件/阶段、oracle、资源或预算错误并返回unavailable；run3仍可exit0/claim complete。两个validator不读外部trace，不能证明它成功。代码harness`:1470`、`:1495`、`:1573`、`:1576`、`:1580`。
- run3 `completed_elapsed_ms`在trace之前记录，不代表含trace总时长。原基线最后trace child退出于`2026-10-14T13:42:32.409824Z`，距suite起点3146.460182s；还要加最后census、聚合和边界检查。manifest约52.7分钟包含完整trace，不能用最后child时间伪装完整终点。
- 原trace序号3170/3171/3172；三个调用均为verification，不是额外正式measured sync样本。
- trace嵌套阶段/不同crate时钟不共享原点，不能求和或混入主分布。原始trace仅私有保存，发布只用reviewed aggregates。
- 失败attempt、陈旧binary无效实验及已有样本保留；不改receipt/sample/deadline/规模来修失败。另开suite须用新ID并披露前次失败，禁止静默重试或只发布最快组。
- 仅R4全通过才发布成功跟进；报告始终绑定被测S2，不改成随后publication SHA。本研究不发布或批准Git操作。

### 9. 文件、规范与参考

| 文件 | 作用 |
| --- | --- |
| 协调任务`prd.md`/`design.md`/`implement.md` | R3/R4、G4、S2 freeze；直接读取，未加载研究角色禁止的jsonl |
| `.trellis/workflow.md` | 研究持久化、Phase 2/3边界 |
| `.trellis/spec/agentsessions-cli/backend/index.md:303` | 证据真实性、私有output边界 |
| `.trellis/spec/agentsessions-adapters-sqlite/backend/index.md:375` | 不为性能放宽提交正确性/durability |
| `.trellis/spec/guides/cross-layer-thinking-guide.md:1` | 边界验证责任 |
| 产品`docs/evidence/core-beta/b0fc44d75690/manifest.md` | 原S/B/H、receipt、报告哈希、环境、复现协议 |
| 产品`docs/evidence/core-beta/b0fc44d75690/run-01/core-beta-benchmark-full.json` | 原始run1样本 |
| 产品`docs/evidence/core-beta/b0fc44d75690/run-02/core-beta-benchmark-full.json` | 原始run2样本 |
| 产品`docs/evidence/core-beta/b0fc44d75690/run-03/core-beta-benchmark-full.json` | 原始run3样本 |
| 产品`scripts/evidence/core_beta_benchmark.py` | 同哈希harness、48项比较、receipt/计时；本次仅调用`run --help` |
| 产品`docs/product/SLI-AND-BENCHMARK-FORMAT.md:91` | baseline v1扩展正式契约 |
| 产品`docs/performance-baseline-0.2.md` | 已发布结论与优化验收要求 |
| 产品私有`evidence-output/current-source-20261014-b0fc44d75690/` | 原B、receipt、suite、trace只读复核 |

External references：本研究无外部联网依赖；依据为指定本地source-bound原始证据及同哈希源码。上述工具版本为原报告事实，不声称核验了最新版本。`run --help`验证本地参数面，无构建或测量副作用。

## Caveats / Not Found

- 未执行benchmark、build、test、Git或新报告validators；未验证未来S2的正确性/性能，不可据本文件宣布G4通过。
- 主会话仍评估O6；S2/B2 SHA及新suite路径必须来自实际freeze与正式运行，不得从`557833c`或未提交原型猜测。
- 48主+8legacy区分、anchor每类20、原始四位小数精度、trace独立完成检查，是对既有摘要的细化，不修改基线协议。
- 开跑前核对磁盘、NTFS、Defender、实际工具链/SQLite；差异如实披露。单机Windows结果不泛化为跨平台或SLO结论。

## Follow-up: 运行中观测与 CARGO_BUILD_BUILD_DIR（2026-10-15）

- Query: 不改变正式 harness/protocol，如何每约60秒低干扰观测活跃运行？`CARGO_BUILD_BUILD_DIR` 能否与 fresh `CARGO_TARGET_DIR` 同时设置？
- Scope: source-only；追加本文件；未执行监控命令、Git、构建或测量。

### 运行中没有逐样本进度文件或阶段 stdout/stderr

`BaselineRun.invoke` 仅把 observation 与 `failed_phase` 更新在内存（harness `:1214`、`:1229`）；CLI stdout/stderr 全部通过 PIPE 收集（`:189`、`:212`）。构建 provenance 命令同样 `capture_output=True`（`:823`）。正式 driver 仅在最终成功后 stdout 打印 report path（`:1587`），失败最终打印 `error: ...`（`:2591`）；没有每个 scale/trial/sample 的进度消息。

以 `evidence-output/<suite>/` 为共同 parent，可用的轻量文件里程碑如下；它们都不是主测量的 append-only progress log：

| 路径 | 更新时间与可证明范围 |
| --- | --- |
| `run-01.claim.json` / `run-02.claim.json` / `run-03.claim.json` | 开始即写 `running`，包括 provenance/build 阶段；结束才改 `complete`/`failed`。运行中不更新 sample/phase。 |
| `run-0N/attempt.json` | 开始写 `running`、`failed_phase=provenance`、`sections=[]`；只在结束/失败重写。运行中显示 provenance **不证明仍卡在 provenance**；`scratch_root` 也先只赋值于内存。 |
| `build-receipt.json`、`suite.json` | run1冻结完成后创建；后者存在表示共享计时已开始，之后三轮复用，不实时更新。 |
| `run-0N/core-beta-benchmark-full.json` 和 `.md` | 本轮主测量完成并完成内存验证后才写；写入期间可能暂时不完整，等claim complete再读取。run3报告出现时独立trace可能仍在运行。 |
| `run-03/index-trace-initial.jsonl`、`index-trace-noop.jsonl`、`index-trace-shrink.jsonl` | **只有三轮主测量之后的独立trace**才使用的append文件；可只stat是否存在/长度，不能用于主测量进度。 |
| `run-03/index-trace-observation.json` | 整次trace尝试结束后写；须读`status`才能判断available，不以文件存在替代。 |

以上生命周期依据：harness `:1383`、`:1392`、`:1487`、`:1498`、`:1514`、`:1519`、`:1526`、`:1573`、`:1585`、`:1596`。trace append依据：`crates/agent-session-grep-cli/src/trace.rs:63` 与 `crates/agent-session-grep-adapters-sqlite/src/trace.rs:63`；主运行仍按harness `:533`清除trace环境。

### 建议的最轻只读监控

每约60秒查询一次**已知 harness Python driver PID**及其当前直接child，不读取CommandLine、不遍历250个source、不打开SQLite、不重算报告/二进制hash。PID从主会话现有启动/进程句柄取得；下面不负责另建launcher或查全机命令行。只保留PID/父PID、进程名、创建时间、累计CPU、当前working set，最多8行；累计CPU增加说明仍有活动，不是已完成样本数。

```powershell
# Set $optDriverPid from the existing driver launch record; never assign $PID.
$optMonitorProperties = @(
    'ProcessId', 'ParentProcessId', 'Name', 'CreationDate',
    'KernelModeTime', 'UserModeTime', 'WorkingSetSize'
)
Get-CimInstance -ClassName Win32_Process `
    -Filter "ProcessId = $optDriverPid OR ParentProcessId = $optDriverPid" `
    -Property $optMonitorProperties |
    Select-Object -First 8 -Property ProcessId, ParentProcessId, Name, CreationDate,
        @{Name='CpuSeconds'; Expression={([double]$_.KernelModeTime + [double]$_.UserModeTime) / 10000000}},
        @{Name='WorkingSetMiB'; Expression={[math]::Round([double]$_.WorkingSetSize / 1MB, 1)}}
```

该命令仅为待执行示例，本次未执行。短child在两次查询之间启动/退出很正常；没有child可表示Python在生成/census/处理结果，不能判为停滞。没有driver行时，再读当前run的小型claim/attempt判定已完成或失败。若只需存活/CPU而不需child，已知PID的`Get-Process`字段投影更简单。每次查询有很小的外部观测开销；不可声称绝对零干扰，也不据CPU变化编造百分比或当前scale。

### CARGO_BUILD_BUILD_DIR：当前协议明确不允许

**不能设置，即使值与 fresh `CARGO_TARGET_DIR` 完全相同。** harness `:872` 的 allowed 集合只有 `CARGO_HOME`、`CARGO_TARGET_DIR`、`CARGO_BUILD_TARGET`、`CARGO_BUILD_JOBS` 及三个terminal变量；`:877` 将所有其余 `CARGO_BUILD_` 前缀视为禁止项，`:878` 直接报 `unsupported build environment override prevents linkage proof`。判断基于变量名，不比较它与target值，空值也不能豁免。

receipt公开配置只记录 `CARGO_BUILD_TARGET`、`CARGO_BUILD_JOBS`、`CARGO_TARGET_DIR` 三项（harness `:897`）；因此这不是“可以额外设置但不改变记录字段”，而是正式run在构建前就被拒绝。保持既有协议的做法是：仅设置suite独占的fresh `CARGO_TARGET_DIR`，正式driver环境中不传 `CARGO_BUILD_BUILD_DIR`；若继承该变量，只在此次launcher子环境中移除/恢复，不修改全局设置。不改harness允许列表，也不以Cargo本身支持这个选项推导本基线协议支持。
