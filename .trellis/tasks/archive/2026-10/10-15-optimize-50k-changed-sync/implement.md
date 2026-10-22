# 优化 50k changed-sync 提交路径 — 执行计划

> 状态：已批准实施（用户 2026-10-15 指令"执行"）。复选框记录真实进度；
> 未勾选项若标明“不适用”，表示条件门未触发；其余为未完成或未通过项。
> 先读 `design.md` 与 `research/` 全部研究文件。
> 所有产品命令在 canonical checkout
> `./.trellis/.runtime/attribution-closeout-20261012/canonical-repaired`
> （main = `de2c95d2c93f`，含基线 harness 与证据）执行；协调根只放任务/研究/日志。
> 候选编号 O1–O7 与决策门 G0–G4 以 `design.md` §2/§3/§9 为准。

## 前置与冻结检查（Phase 1 收尾）

- [x] 用户批准规划并指令"执行"（2026-10-15）；`task.py start` 翻 status 为
  in_progress（见文末执行记录）。
- [x] 核对 canonical 树干净（`git status` 空）、`main == origin/main ==
  de2c95d2c93f`；从 main 建 feature 分支 `perf/50k-changed-sync`
  （2026-10-15 实施，base 为 de2c95d 干净状态）。
- [x] 核对保护 spec 哈希未变：`.trellis/spec/agentsessions-cli/backend/index.md`
  = `2F117568…`、`.trellis/spec/guides/cross-layer-thinking-guide.md` =
  `0FA97553…`；不回退协调根既有未提交的 spec/任务文件改动。
- [x] 建私有实验目录 `./.trellis/.runtime/optimize-50k-changed-sync-20261015/`
  （协调根）与 canonical 侧 gitignored `evidence-output/` 子目录；原始 trace/
  scratch/失败尝试只落这两处。
- [x] 基线 sanity：确认冻结二进制 B `39049bfd…abe8e`、harness `aeb23f90…39f8`
  仍在本地（sha256 复核一致）；E1/E3 又各实跑 B（initial 10.75/9.30 s、
  shrink 7.95/12.80 s），均落在基线包络内。

## G0：归因实验（实现前，私有，不入产品提交）

- [x] E1 提交内 I/O 归因：第二次整行写 WAL +49.0 MB（initial）/ +34.1 MB
  （shrink）；整行重写边际 0.35–0.40 s（initial）/ 0.30–0.35 s（shrink），
  提交侧 WAL 写+fsync+checkpoint 回拷为主要组成（含禁 autocheckpoint 差分）。
- [x] E4 变量上限：运行时 `sqlite3_limit(SQLITE_LIMIT_VARIABLE_NUMBER)` =
  **32766**、`PRAGMA compile_options` 同值；O4 目标分块 512 行（4096 变量）安全。
- [x] 聚合结论写入 `research/optimization-experiments.md`（只含复核过的聚合；
  原始日志留在私有 runtime，写文件时不含私有 trace 内容）。
- [x] **G0 门**：initial ≈0.44–0.97 s、shrink ≈0.36–0.38 s（语句+提交侧，
  两口径互证）≥ 0.4 s / 0.3 s → O5 假设成立（量级取原估算下沿；仍以 G3
  原型复核 end-to-end）。

## 第一组：O1 + O2（低风险，独立提交）

- [x] O1 `affected_sessions` 批量化（`d60091a`，引 C2）：循环内只收集 message
  id（BTreeSet 去重），循环外一次分块 IN 查询（复用既有分块 helper）；
  "查询先于写入"与"逐 id DISTINCT 并集"等价性论证在提交信息；placement 移动经
  既有 old_placement_ids 路径不受影响。
- [x] O2 去 manifest 阶段重复身份校验（`0fcfbbe`，引 C3）：证明 `upserts` id
  集合完全由 `source.entries` 构成且两次检查之间无写入；新增
  `tampered_identity_sidecar_fails_closed_without_state_change` 回归测试固定
  fail-closed 语义。
- [x] O1、O2 各自独立原子提交（`d60091a`、`0fcfbbe`），便于二分回滚。
- [x] 质量门（每提交后受影响范围）：`cargo fmt --all --check`、
  `cargo clippy --workspace --all-targets -- -D warnings`、
  `cargo test -p agent-session-grep-adapters-sqlite` 全绿；组末
  `cargo test --workspace` 全绿（adapter 299、CLI e2e 153、journal_retention 13、
  mcp_e2e 45 等；日志在私有 runtime）。
- [x] O1+O2 spot（E3）：同日基线 initial 中位 10,024.9 → optimized 8,467.8 ms；
  shrink 中位 10,374.0 → 7,951.1 ms；插桩对照确认 O1 命中 affected_sessions
  （3.5–4.6×）、O2 命中 manifest（约 −110 ms）。
  **G1 门**：门槛余量 initial 1.6%、shrink 5.9%，未达 ≥10% → 判定"继续"
  （进入第二组 O3+O4），符合预期。

## 第二组：O3 + O4（独立提交）

- [x] O3 alias 重建批作用域化：**原型实现后经真实二进制同轮次 A/B 否决，未进入
  产品提交**（initial +1,010.9 ms、shrink +270.0 ms；候选集≈全表时"逐块点查 +
  双份取回"劣于整表 covering-index 扫描）。决定性证据、插桩归因与原型 diff 存档
  见 `research/optimization-experiments.md` §7.2（私有 runtime
  `o3-prototype-rejected.patch`）；原型期内既有 alias/mixed-completeness 测试全绿，
  "不得假装完整"的语义未被放宽。
- [x] O4 分块提升（`557833c`）：按 E4 实测上限把 `BULK_INSERT_ROWS_PER_CHUNK`
  100 → 512（8 列 × 512 = 4096 变量）；具名上限常量 `SQLITE_MAX_VARIABLES = 32766`
  + 保守回退常量 100，8 处编译期断言全部更新；新增满块 512 行语句冒烟与跨
  512/1 块边界提交测试。独立 A/B（512 vs 100，其余同源）：initial −412.1 ms、
  shrink −268.0 ms（§7.3）。
- [x] 独立提交；质量门同上（fmt/clippy 全绿、adapter 301 全绿；组末 workspace
  2009 passed / 0 failed）。
- [x] O3+O4 spot（E3）；**G2 门**：shipped（O1+O2+O4）两次同日 spot 均低于门槛且
  余量 ≥5%（initial 6.15–9.55%、shrink 6.31–9.97%）→ 字面进入"评估跳过 O5"分支；
  但考虑同二进制跨 spot 波动 ≈4%、当日机器比已发布基线快 10–17%，建议仍实施 O5
  （先 E2/G3），把 R4 边际做实。**主会话决策（2026-10-15）：采纳建议，不跳过
  O5，进入第三组。** 判定与算术见 §7.4。

## 第三组：O5（条件项，schema v20）

- [x] E2 原型（私有）：完成父行/子表原型及双格式读的真实二进制 A/B，
  `tx_commit + generation_activate` 中位收益 initial 320.0585 ms、shrink
  25.964 ms；所有 16 个计时相位的 harness census/帧校验通过。原始产物与
  无效前置尝试均保留，见 `research/optimization-experiments.md` §8。
- [x] **G3 门 = 否决 O5**（2026-10-15）：两场景均未达到 400 ms。原型已保存并
  精确反向应用，canonical 恢复到 `557833c` 干净树、schema 19；以下 O5 正式实现、
  迁移加固及提交项不执行。收益 <0.4 s 或崩溃恢复/retention 测试面无法收敛 →
  回退原型、保留 O1–O4，按 PRD 允许的"如实报告 + 后续建议"路径或补 O6；
  收益达标则进入正式实现。
- [ ] 正式实现（design §4）：新增 `index_batch_details`
  （`operation_id` PK + `detail_format` + 5 个明细列）；Phase A 写父行（小）+
  子表（大，一次）；Phase B 只 UPDATE 父行；不再触碰明细。
- [ ] 读路径：`verify_pending_in_tx`/维护接口经统一读取层（子表存在时权威，
  否则回落父行旧列，v19 历史行双格式兼容）；`recover_interrupted` 逻辑不变。
- [ ] 迁移 `19→20`：additive、同事务、最后写 `user_version`；既有行明细复制
  进子表、旧列保留只读；旧二进制拒绝 v20（既有策略）。
- [ ] journal retention（B2）兼容：新旧格式 compaction 正确（`full` 子表
  替换/聚合、`aggregated_v1` 不聚合、未决行永不聚合、CAS apply 保持）；
  tamper/recovery/retention 测试全绿。
- [ ] 独立提交（含迁移/双格式读测试与 fixture）。

## 第四组：O6（可选）

- 2026-10-15 决策：G3 未通过且同机波动较大，评估 C6 的 manifest CPU/内存
  原型。具体候选为逐项序列化 canonical JSON 数组，避免同时驻留三个 Value
  数组，并缓存排序键；继续复用既有 canonical_value 与校验，输出字节和 digest
  域保持不变。先做字节等价与独立 E3 对照，再决定保留；不修改 schema。
- [x] O6 E3 对照支持保留：manifest 中位数 initial 690.637→497.177 ms，
  shrink 504.224→294.3605 ms；未插桩 wall 中位数 9198.0971→8162.85465 ms、
  8815.0227→7585.28935 ms。全部样本与独立 trace 保留于私有 `o6-e3-01/`；
  16 个相位的 harness census/帧校验通过。此为 spot 结论，不是 R4 成功。
- [x] G3 后保留 O6：逐项序列化 canonical JSON 数组与缓存排序键，
  保持 `operation_digest` 与存库字节完全不变；不改变 canonical JSON 形状。
  不做则记录决定。

## 正确性与质量门（冻结 S2 前）

- [x] 等价 oracle（私有脚本）：250×200 与 1×20 两种合成语料，冻结 B 与新
  二进制各跑 initial→noop→shrink；全用户表排序 dump 哈希（时间列归一化为
  常量）+ `active_generation` 一致。O5 变更额外验证：新旧格式 journal 行可读、
  recovery/retention 流程通过。
  （进展：O1/O2 阶段已完成 **verdict=match ×2 语料 ×3 phase**；O1+O2+O4
  （`557833c`）在私有 `oracle2/` **再次两语料全 match、0 findings**；仅按设计原因
  归一化随机 installation namespace/时间戳/operation_id/digest。O5 若落地，按本项
  重跑并补新旧格式 journal 行/recovery/retention 验证。）
  最终 O1/O2/O4/O6 严格 oracle：三组（另加 200 条同 ID 全量改写）全部 match，
  18 个 sync 相位、每相位 26 张用户表、12 个原始 digest proof、6 次同侧原始
  noop 精确相等；24 个 state/proof 与15个 frame/cursor 控制通过。
  见私有 `strict-oracle-01/recompare-01/result.json`；未读取真实语料。
- [x] `cargo fmt --all --check`；`cargo clippy --workspace --all-targets -- -D warnings`；
  `cargo test --workspace` 全绿（含 adapter `journal_retention`/
  `invariant_failure_semantics`/outbox-CAS/迁移/tamper 测试与 CLI e2e
  sync/noop/shrink/tombstone、multi-session）；MCP 相关既有测试通过。
  （进展：O1/O2 当前 diff 已全绿（adapter 299/workspace 全绿含 CLI e2e 153、
  journal_retention 13、mcp_e2e 45）；2026-10-15 O1+O2+O4（`557833c`）复跑全绿：
  fmt/clippy 全 workspace 通过，adapter 301、**workspace 2009 passed / 0 failed**
  （CLI e2e 153、journal_retention 13、journal_maintenance 31、mcp_e2e 45、
  provider_matrix 44）；冻结前按本项再复跑。）
  最终复跑：workspace **2012 passed / 0 failed / 23 existing ignored**（20 个手动
  fixture 输出、2 个 provider 微基准、1 个扩大维护实验；忽略项与 base 一致）；
  evidence **143 passed**、governance **93 passed**，两 Python 套件无 skip。
  adapter 单包 340 passed/0 failed/0 ignored，fmt/clippy exit0；完整日志与
  `full-gate-receipt.json` 位于私有 `final-quality/`。
- [x] 独立 `trellis-check` 复核全部 diff（对照 spec/design/研究）；findings
  修复后重跑受影响测试并记录。
- [x] 冻结 S2：canonical 工作树干净，S2 =
  `e8e4975817f71647c158f6d802a7b98cee2f1b15`，tree =
  `f1a99f12e093fd669958102a23e470c37a2ad784`。O6 实现提交
  `b8578b2d213b8153f1d4a6778c1daf0598c53128`，随后补测提交为 S2。
  共5条 introduced commits 语法/身份检查通过。此后源码冻结，测量只针对 S2。

## G4：首次正式复测（三跑协议，S2 未达标）

- 2026-10-15 12:17:11 +08:00 已启动正式 driver，自动顺序执行1→2→3与独立trace，
  已完成并判门失败（shrink 未达 >15% 改善）。suite = `opt50k-e8e4975817f7-20261015T121711`，产物在 canonical
  `evidence-output/` 下同名目录；源码 S2、tree 见上。私有执行器
  `optimize-50k-changed-sync-20261015/run-formal.ps1`，R4 精确比较器
  `compare_r4.py`。执行器已 exit0；原 exec session `25057`、PowerShell PID
  `752864` 已结束，不再用作等待目标。
  三跑期间不做其他构建/测试/数据扫描；run1使用全新suite专属Cargo target，
  `CARGO_BUILD_BUILD_DIR` 未传。若失败，保留全部产物，不自动重试。
- [x] 按 `research/baseline-protocol-and-thresholds.md` §3 跑
  `--baseline-run 1`（构建构建 receipt/B 后启动共享 deadline）→ run 2/3
  复用 B 不重建；suite ≤3600 s、child ≤300 s；fresh roots、固定规模轮换
  顺序；新 suite-id 与新 output 目录（不覆盖基线产物）。
- [x] run 3 后独立 trace 试验（诊断用，不入主延迟池）。
- [x] `validate-report` ×3 + `validate-baseline` ×1；环境字段与基线的
  磁盘/Defender/工具链/SQLite 一致（不一致须披露）。
- [x] 判定：三跑 P50 中位数 vs 精确 R4 门槛（initial < 8604.462790 ms、
  shrink < 8451.728755 ms）；核对全部 48 个主计时系列与另列的 8 个 legacy
  latency 指标，披露各自 spread，确认非目标系列无 >15% 回归。另查 trace
  `status=available` 与含 trace/收尾的总时长 ≤3600 s；validators 不替代这些门。
- [x] 未达标：不宣称成功；如实报告全部样本与原因，评估迭代或按 PRD
  "如实报告 + 保留现状"结束。

首轮结果（完整比较收据：私有 `r4-attempt-01.json`）：

| 场景 | 基线三跑 P50 中位数/ms | S2 三跑 P50/ms | S2 中位数/ms | 改善 | R4 |
| --- | ---: | --- | ---: | ---: | --- |
| 50k initial | 10122.8974 | 7819.6887 / 8661.5382 / 6953.7125 | 7819.6887 | 22.7525% | 通过 |
| 50k shrink | 9943.2103 | 7146.9575 / 10717.6615 / 8717.9847 | 8717.9847 | 12.3222% | 未通过 |

- 48 主系列有 47 项通过；两个目标之外的 46 项全通过，8 个 legacy guard 全通过。
  shrink 跨轮 spread 基线 42.4185%、S2 40.9579%；不把关联性当作环境因果结论。
- S2 B SHA256 = `03c168a0d146ec9d0e7cfc3d49602468457e9e3643bd17f443e0407ee70bd6cd`；
  run1 uncached，run2/3 复用。build receipt 内部摘要 =
  `381783258d4a82969d89b55ae294c3b92517a81b64628ef7b545452d4229b32c`。
- 含独立 trace 与执行器返回共 1717156 ms（约 28.6 分钟），未超 3600 秒；
  `suite-end-check.json` 证明 trace available 且三相位成功；四次官方验证通过。
- 主会话决策：继续基于真实热点实施 O7 有界原型，保留这次失败及全部原始证据。
  不能以原样重跑 S2、挑选最快轮或改阈值代替实质性改进。

## 第五组：O7（首轮 G4 后，批量关系删除）

- [x] 独立研究与实现代理只读确认 95,000 次逐条 prepare/execute；最小方案
  已记录到 `design.md` §9；当前 S2 HEAD 与干净树已复核。
- [x] 仅批量相邻同类型 Edge/Placement，复用 500 参数块，补齐边界、键集合、
  跨类型顺序、整事务回滚、durable-intent 篡改拒绝测试；不改 schema/harness。
  O7 源码 SHA256 `62853001be4194b680e206a5ae66aa9ab0af60e858c0118afba2425bd9fbd68f`；
  4 项定向测试、344 项 adapter 测试通过，当前源码 fmt/clippy 与缓存 freshness
  已复核。独立静态审查无阻断 findings，见 `research/o7-semantic-review.md`。
  独立 release target 构建 exit0，候选 B SHA256
  `0dfb0d40310556e631c695cd1e23de2987de0dff6664cefc063d8204cf343496`；
  来源/命令/时间/patch 收据保留于私有 `o7-prototype-01/build-receipt-02.json`。
- [x] 私有 `o7-prototype-01/` 中以 S2 正式 B 为对照完成交替 A/B 与独立 trace。
  核实两侧二进制、源码差异与 schema19；没有收益则撤销并保留失败证据。
  `ab-01/result.json` 已完成：shrink 两对节省 -202.3545/+310.8686 ms，
  中位节省 54.25705 ms（0.6635%），未超过 266.255945 ms；两项探索门失败。
  initial 中位回归 13.6854%，独立 trace 的局部下降不替代 wall-time 门。
- [ ] 有收益后针对新源码重跑严格 oracle 和全部质量门，独立 `trellis-check`。
  **不适用**：O7 未通过探索门，按设计否决；不使用 S2 收据认证 O7。
- [ ] 原子提交并冻结 S3；使用全新 Cargo target、新 suite ID 再跑正式三轮、
  独立 trace 和官方验证，按不变 R4 全矩阵判定。保留首轮失败披露。
  **不适用**：未产生合格的新候选，不冻结 S3、不重跑原 S2。

## 未达标收尾（PRD 允许的保留现状路径）

- [x] 保存 O7 原型与 patch，单文件恢复到精确 S2，记录回退收据。
  源码 SHA256 = `2ec45e89d3c10e58690d60a1816b62779bd4263624893b4388530fb39f9c97a1`，
  tree = `f1a99f12e093fd669958102a23e470c37a2ad784`；fmt exit0。
- [x] 独立复核探索门、S2 全范围质量收据与正式失败；不把收尾当作 AC3 通过。
  见 `research/o7-final-disposition-review.md`；原始样本、实存二进制、回退
  patch、S2 收据、五对公开身份提交的 tree/消息/姓名/时间均独立复核一致。
- [x] 提交协调任务研究与 spec 教训；产品原始被测提交在私有 ref 保留。
  协调工作提交 `d250c2f` 已记录研究、原型结果、源码身份与规范教训。
  GitHub GH007 拒绝私有邮箱推送后，五个尚未发布的提交仅改为同一账号的
  已核实 noreply 邮箱，保留各提交 tree、姓名、时间、消息与原子划分。
  public head = `c9af8bbc89e14e676471cd7be8eb55a4312273dd`，整树仍精确等于 S2。
  报告绑定原始 S2，不声称新 commit hash 被独立测量，不推送私有旧引用。
- [x] 对 S2 完成发布前路径/凭据/introduced-range 扫描，建立真实披露失败的
  draft PR；读取当前候选 hosted checks，不合并、不生成成功性能文档。
  PR #22 已创建（`Lain-in-coding/agent-session-grep`），draft/open，公开 head
  为上述 `c9af8bbc`。原始 S2 和 public head 分别通过精确 base-trusted 路径/
  凭据扫描：588 个 tracked files、5 个 introduced commits、0 findings、
  0 unsupported。拟定 PR 文本已单独扫描；真实完整 PR event 由 hosted
  governance run `38729751352` 核验，其 `gate` 已 success。
  hosted release verification run `38729751327`、core-beta evidence run
  `38729751058` 均 success。最终 API 核对 25/25 check runs 全为 success，
  `gate` 与 `all required verification checks` 均来自 App 15368，绑定公开
  head `c9af8bbc`。完整 PR 正文与扫描版本一致，main 仍为 `de2c95d`。
  strict protection 与 enforce_admins 保持开启；没有 merge、发布成功 bundle
  或改写原始测量。私有 `negative-closeout/final-disposition-receipt.json`
  记录全部 source/base/head、运行 ID 与核验结果。
- [x] 以 `performance_gate_failed` 结果归档本实验并记录本地 journal；
  父路线和发布门维持原状态。

## 发布（仅后续 G4 达标；采用实际被测源码前缀）

- [ ] 证据提交：`docs/evidence/core-beta/<measured-source-prefix>/`（三份报告 + Markdown
  投影 + manifest）与 `docs/performance-baseline-0.2.md` 带日期跟进条目
  （含新源码身份、对比口径、原始报告、噪声与限制；O7 后报告绑定实际冻结 S3，
  不写未来文档/合并提交哈希，披露 S2 完整失败而不使用其样本替代 S3）。
  精确八路径写集、公开字段、治理命令与来源边界见
  `research/o7-publication-preflight.md`。
- [ ] 治理：提交语法/路径隐私/凭据扫描、PR 元数据与 introduced-range 校验
  按 canonical `CONTRIBUTING.md` 执行；hosted checks 全绿。
- [ ] PR 基于 `Lain-in-coding/agent-session-grep` main；按用户 2026-10-14
  "全部 Git 权限"授权执行普通 merge；不自动关闭父发布门
  （`08-15-open-source-product-roadmap`）。
- [x] 协调根任务文件归档与 session journal 记录（`session_auto_commit: false`，
  任务归档手动提交，journal 按仓库规则保持本地、不得提交）。

## 回滚点

- O1/O2、O3/O4、O5、O6 各自独立提交 → 各自可 revert；E2 原型不提交。
- G1/G2/G3 任一未过 → 弃用对应提交，回到上一冻结点重新 spot。
- 正式复测后回滚：revert 实现提交（不改写证据提交历史），重跑质量门。

## 私有产物边界（不入产品提交）

原始 trace、scratch、实验日志、失败尝试、私有脚本、机器路径 → canonical
gitignored `evidence-output/` 或协调根私有 runtime 目录；产品提交只含产品
代码、测试、复核过的聚合证据与发布文件（与 PRD Constraints 一致）。

## 执行记录

- 2026-10-15：用户指令"执行"批准实施。`implement.jsonl`（10 条）与
  `check.jsonl`（8 条）补齐并 `task.py validate` 通过；`task.py start`
  将任务置为 in_progress（source = session-scoped runtime pointer）。
- 2026-10-15：派发 `trellis-implement` 第一工作块 = G0（E1 归因 + E4 变量
  上限）+ O1/O2 实现与质量门 + G1 spot；实现与提交落 canonical
  `perf/50k-changed-sync` 分支。
- 2026-10-15：`trellis-implement` 第一工作块完成。
  - G0：E1 私有探针（真实 50k 库重放 activation 整行 UPDATE + tiny-row 对照，
    含禁 autocheckpoint 差分）确认第二次整行写 WAL +49.0 MB（initial）/
    +34.1 MB（shrink）、边际 0.35–0.40 / 0.30–0.35 s → **G0 门通过**
    （O5 假设成立，量级取估算下沿）；E4 运行时确认变量上限 32766 →
    O4 目标 512 行/块（4096 变量）。
  - 提交：`d60091a`（O1，引 C2，affected_sessions 批量化）、`0fcfbbe`
    （O2，引 C3，去重复身份校验 + tamper fail-closed 回归测试）；
    消息于当日按治理重写（见下），树哈希不变。
  - 质量门：fmt/clippy 全绿；每提交 adapter 套件全绿；组末 workspace 全绿
    （CLI e2e 153、journal_retention 13、mcp_e2e 45 等）；等价 oracle
    （250×200 与 1×20，initial→noop→shrink，全表 dump + generation）B vs
    新二进制 **全 match**。
  - E3 spot：initial 10,024.9 → 8,467.8 ms、shrink 10,374.0 → 7,951.1 ms
    （同日基线中位口径）；**G1 = 继续**（门槛余量 1.6% / 5.9%，未达 ≥10%）。
  - 聚合证据：`research/optimization-experiments.md`；原始日志、探针与
    scratch DB 在私有 runtime `optimize-50k-changed-sync-20261015/`（不入产品提交）。
- 2026-10-15：`trellis-implement` 第二工作块完成（O3 评估否决 + O4 落地 + G2 spot）。
  - O4=C5 提交 `557833c`：`BULK_INSERT_ROWS_PER_CHUNK` 100 → 512（8 列 × 512 =
    4096 变量），具名上限常量 `SQLITE_MAX_VARIABLES = 32766` + 保守回退常量 100，
    8 处编译期断言更新，新增满块语句冒烟与跨 512/1 块边界提交测试；独立 A/B
    （512 vs 100，其余逐字节同源、同轮次交替）initial −412.1 ms、shrink −268.0 ms。
  - O3=C4 原型（批作用域装载）实现后按 E3 方式 A/B：initial +1,010.9 ms、
    shrink +270.0 ms（本矩阵候选集≈全表 → 51 块点查 + 同一 placement 双份取回，
    劣于整表 covering-index 扫描；插桩 `session_projection` 1,543 → 1,889 ms）
    → **否决、不进入产品提交**；查询级预检、A/B 数据与原型 diff 存档见研究 §7.2 与
    私有 runtime `o3-prototype-rejected.patch`。
  - G2：shipped（O1+O2+O4）两次同日 spot 均低于 R4 门槛且余量 ≥5%
    （initial 6.15%/9.55%、shrink 6.31%/9.97%）→ 字面进入"评估跳过 O5"分支；因
    同一二进制跨 spot 波动 ≈4% 且当日冻结 B 比已发布基线快 10–17%，建议仍实施 O5
    （先 E2/G3）把边际做实。判定算术见研究 §7.4。
  - 质量门：fmt/clippy 全 workspace 全绿；`cargo test --workspace` **2009 passed /
    0 failed**（adapter 301、CLI e2e 153、journal_retention 13、mcp_e2e 45 等）；
    等价 oracle #2（250×200 与 1×20）两语料全 match。
  - 治理发现与修复（2026-10-15，主会话）：`scripts/governance/check_commit.py` 按
    CONTRIBUTING（header ≤ 50、body 行 ≤ 72）拒绝最初的 O1/O2 提交消息（`cd97bcd`/
    `bc0ccf6`：header 66 字符、body 行超宽）。三条实现提交已在本地重写为合规消息
    （内容不变、树哈希与重写前一致）：`d60091a`（O1/C2）、`0fcfbbe`（O2/C3）、
    `557833c`（O4/C5）。重写后 range 校验
    （`--base de2c95d… --head 557833c…`）= **passed**（3 commits）；
    重写前历史保留在分支 `safety/pre-msgfix`（不入发布链）。
- 2026-10-15（主会话）：G2 决策 = **继续实施 O5**（字面余量 6–10% 但波动 ≈4%、
  当日机器基线比已发布中位快 10–17%，不稳健）；并按治理重写三条实现提交消息为
  50/72 合规格式（树哈希不变，`check_commit.py` range=passed）。
