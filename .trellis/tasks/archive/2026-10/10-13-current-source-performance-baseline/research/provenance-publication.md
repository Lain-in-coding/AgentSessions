# Research: Current-source baseline provenance and publication

- Query: 新的 current-source full 基线如何绑定 source/harness/binary/corpus，保持历史与 v1 兼容，并以最小产品写集发布？
- Scope: mixed；产品代码、契约、既有证据和治理规则只读调查；补查官方 Cargo/GitHub 文档。未运行 benchmark、构建、Git 操作或配置工具。
- Date: 2026-10-14
- Source: 主会话指定并于本日复核的 canonical main `8e05eb495160d5ff5979cdbef6f63856d5a7de81`。下列非 `.trellis/` 路径均相对该产品 checkout；不把本机 checkout、缓存或 runtime 路径写入产品内容。

## Findings

### 1. 结论与依赖

采用 **tooling-only commit S → 固定干净 S → 测量 S → publication commit P**。P 只增加证据及说明，JSON 的 `commit` 始终是 S；P 和最终 merge SHA 的 checks/验收另行记录，不能回填成被测 SHA。当前生成器读取 workspace HEAD，`--expected-commit` 只比较该 SHA，不证明工作区干净，也不识别另一个 checkout 中执行的脚本（`scripts/evidence/core_beta_benchmark.py:353`、`:358`、`:453`）。没有消费者要求报告 `commit` 等于包含报告的提交。

- 前置修复已交付：validator 修复任务记录 PR #18 于 2026-10-13 合并为上述 main，两端各 25 checks 通过；这是该历史修复的验收，不是新基线证据（`.trellis/tasks/archive/2026-10/10-13-gate-evidence-validation/prd.md:64`）。本研究未重新读取托管 checks。
- 修复对象是 **open-source-gate/v1** 的六指标一致性；未证明 Core/Beta 全字段 provenance，也未建立性能基线（该修复 PRD `:84`；`docs/release/rehearsal-runbook.md:401`）。不得把修复的 25 个 checks 当成新 full 的测量结果。
- benchmark 父任务负责更广的 benchmark/install/open-source evidence，本子任务不关闭其发布或竞品对比要求（`.trellis/tasks/08-15-benchmark-install-open-source-gate/prd.md:37`、`:56`）。旧父设计中的 discovery/resume 未实现假设已过时。
- 当前仍是 planning；复杂任务须审阅 PRD/design/implement 后才能执行，父子关系不自动表达依赖（`.trellis/workflow.md:344`、`:437`）。最终计划应明确依赖前置修复，并引用另一个研究文件的 workload/采样矩阵。

### 2. 文件与现有管线

| 文件 | 作用与关键锚点 |
| --- | --- |
| `docs/product/SLI-AND-BENCHMARK-FORMAT.md` | SLI、环境与 raw samples 契约；Draft 不因新结果升级；`:33`、`:56`、`:108` |
| `scripts/evidence/core_beta_benchmark.py` | stdlib 生成器、deterministic corpus、v1 validator 与 Markdown projection；`:29`、`:49`、`:217`、`:536`、`:668` |
| `scripts/evidence/test_core_beta_benchmark.py` | 现有四个统计/确定性测试，适合就地扩展；`:12` |
| `docs/evidence/core-beta/88d86f4/{manifest.md,core-beta-benchmark-full.json,core-beta-benchmark-full.md}` | 历史 full bundle；manifest `:3`、`:19`、`:26` |
| `docs/performance-baseline-0.2.md` | 现有性能说明入口，可新增带日期/source 的当前基线小节；`:34` |
| `docs/operations/core-beta-evidence-matrix.md` | 历史 imported 说明及状态词；`:3`、`:19`、`:79`；不宜在本切片重认证旧行 |
| `.github/workflows/core-beta-evidence.yml` | 四目标 smoke + gate，SHA 来自 `github.sha`，artifact 保留 7 天；`:101`、`:142`、`:205`、`:237` |
| `scripts/evidence/open_source_gate_benchmark.py` | 四阈值 + 两 informational recall；复用 Core helpers，单独 schema/validator；`:29`、`:42`、`:654` |
| `scripts/evidence/semantic_benchmark.py` | 已有固定 labeled corpus/manifest 与 lexical/semantic/hybrid 管线；验证 corpus hash 后测量；`:1516`、`:1588`、`:1792` |
| `scripts/evidence/semantic_mcp_latency.py` | 已有常驻 MCP semantic latency 工具；`:2`；不是本次基础 full 的替代品 |
| `scripts/benchmark.py`、`scripts/test_benchmark.py` | 老通用脚本；缺少当前证据链，不能作为新管线捷径，见下文 |
| `CONTRIBUTING.md`、`AGENTS.md`、`CLAUDE.md` | 精确 SHA 验收、隐私、治理与提交规则 |

不存在 Core/Beta 的独立 JSON Schema 文件：`schemas/` 当前只有 handoff/robot schema。Core/Beta 的可执行契约是 Python validator，规范文档是 SLI §2.1；不能因名为 schema 就假设存在 `additionalProperties: false`。

老 `scripts/benchmark.py` 的风险由 harness 研究代理核实：`--corpus` 只解析未使用（`:157`），discovery 无隔离 HOME（`:40`），丢弃失败请求及原始延迟，百分位与 Core 不同（`:77`）。不恢复该旧管线；scale diagnostics 沿用 Core 的安全 synthetic 生成、进程执行和统计 helper。已有 semantic 和 MCP 工具只作能力清单，不引入模型下载或额外语义基线。

### 3. 最小身份契约

建议原 JSON 字段保持不变，在可选扩展中补强缺失身份；bundle manifest 写出人类可审阅的对应关系。

| 身份 | 最低记录内容 | 当前行为 / 必须补充 |
| --- | --- | --- |
| Source S | 完整 lowercase 40-hex SHA；运行前后 tracked/index/untracked 状态；所用 Cargo.lock SHA-256 | `commit` 已有；现有代码不检查 dirty。输出置于 ignored 目录，输出不应掩盖代码污染。 |
| Harness H | 实际执行脚本的 source SHA、相对路径、文件 SHA-256、扩展版本、validator SHA/hash | `--workspace` 与 script 所在位置可不同；优先规定 H=S，禁止只以 workspace SHA 代替 H。 |
| Binary B | SHA-256、字节数、build provenance、准确命令、target、features、rustc/cargo 版本、release flags、Cargo.lock hash | `binary` 已有 hash/size/provenance（`:465`）；build receipt 与参数尚缺。 |
| Corpus C | generator H/hash、确定性参数、files/messages/source bytes、hash algorithm、tree hash、query/id set | 基础 corpus 已有（`:455`）；每个 diagnostic workload 自己持有这些字段，不能借用 4000-message corpus 的 hash。 |
| Environment E | OS/版本、target/架构、CPU/核数、RAM、临时数据所在 disk/filesystem、安全软件状态、Python、SQLite 来源 | 现有字段见 `:296`；不要只记录 checkout 所在磁盘。只收集白名单构建变量，不 dump 全环境。 |
| Run R | 有序 run id、开始/结束 UTC、workload/操作/状态/预热/重复次数、全部 raw samples、失败/未完成状态 | 三次独立运行应各自保存，不把跨 run 混合的样本冒充一次运行。明确时间含进程启动和序列化。 |
| Publication P | manifest 到相对报告路径/文件 SHA-256 的映射、S、验证者 H、隐私与检查摘要 | P 自身 SHA 由 PR/治理记录绑定，不让同一 commit 内的文件自引用其将来 SHA。 |

`tree_hash` v1 算法是按 POSIX 相对路径排序，对每个文件依次输入 `relative_path + NUL + file_bytes + NUL`，再 SHA-256（`:49`）。扩展复用算法即可，不另写排序/哈希规则。

默认 harness 使用 `cargo build --locked --release -p agent-session-grep-cli`（`:323`）。`--locked` 固定 lockfile 的依赖解析，**不证明**工作区干净或二进制可重复到相同字节。现有 binary 定位固定为 workspace `target/release`；若继承 `CARGO_TARGET_DIR`、build target 或其他非默认配置，必须记录并正确选中实际产物，不能静默复用旧位置的 binary。

显式 `--binary` 必须继续写 `caller_supplied_prebuilt` 并保留未独立 attested 的 source-to-binary linkage 限制（`:515`、`:660`）。即使新增 build receipt，也不要悄悄改成 `built_by_harness_from_workspace` 或宣称密码学证明。每个重复运行重新核对 binary hash；哈希不同须分组或解释，不能混合。

### 4. v1 可选诊断扩展的具体兼容方案

**可以在 v1 顶层增加可选、显式 version 的 extension，而不另建工具或 whole-report schema。** 建议最终命名为一个可选 `diagnostics` 对象（或等价单一名称），内部包含 `version: 1`、上述 provenance、run plan 和 `workloads`；正式键名由设计一次确定。

- 原 `schema_version` 保持 `agent-session-grep.core-beta-benchmark/v1`。`PROFILES`、原 `profile`、4000-message full `dataset`、九个 `metrics` 名称、原 required counts 及原 raw/summary 语义全部保留（`:29`、`:560`、`:572`、`:623`）。
- 2k/10k/50k-message workloads 是 **另外的诊断数据集**，放入扩展；不命名为三个新 full profile，也不把它们塞进原 `metrics` 或改动原 dataset。每个 workload 拥有自己的稳定 ID、参数、hash、counts、命令/查询身份和 raw/summary。
- 扩展缺省：按现有 v1 校验，旧 smoke/full 完全可读。扩展存在：新 validator 强制识别 version、对象/数组类型、有限非负数且拒绝 bool、预期 workload 集、唯一 ID、hash/身份关系、准确样本数及重算 summaries；未知 version/残缺扩展 fail closed。
- bounded sizes、resource limits、失败样本处理、正确性预检和 per-query 展开以已审批矩阵为准。preflight/runtime failure 不得通过略去 workload 或缺失字段变成有效 full+diagnostics。
- Markdown 是 JSON 投影；新增诊断内容只通过 `render_markdown` 生成，不手工改写数值（`:520`；SLI `:58`）。可仅展示摘要并保留 JSON 完整 raw。
- **旧 validator 会忽略扩展。** 必须把新 validator 的 SHA/hash 作为出版验证依据，不能拿旧 checker 的 pass 宣称诊断字段已验证。

本次只读实证：当前 checker 接受历史 full；把该报告的内存副本加上 `diagnostics={version:999, workloads:"invalid"}` 和额外 `provenance` 后仍接受。使用 `python -B`、`mock.patch(Path.read_text)`，未创建报告或修改历史文件。这证明当前 top-level 没有闭集限制，也证明新增字段必须补专用验证；不是推荐接受这种扩展。

最小回归集应在现有 test 文件覆盖：历史 full 正向；无扩展 smoke 正向；合法扩展 round-trip；未知 version/缺字段/重名 workload/越界尺寸/错误 corpus hash/count/错误 summary/NaN/Inf/bool/失败样本负向；旧核心 metric/profile/count 的负向仍拒绝。测试用小型 synthetic/mocked 输入，不让 50k workload 进入普通 unittest。

### 5. SQLite 来源：无需改 Rust 或旧 spike，但须有本次 build 证据

CLI `--version` 仅返回产品 version；doctor/status 也未暴露 SQLite runtime version（`crates/agent-session-grep-cli/src/lib.rs:1401`、`:1643`、`:1714`、`:5579`；对产品 Rust 搜索未找到 `sqlite_version` / `rusqlite::version` / `sqlite3_libversion`）。Python 的 `sqlite3` 版本不能代表产品库（Core `:319`）。

当前 root lock 为 `rusqlite 0.40.2`、`libsqlite3-sys 0.38.2`（`Cargo.lock:2090`、`:1351`），adapter 启用 bundled（`crates/agent-session-grep-adapters-sqlite/Cargo.toml:18`）。旧 WAL spike lock 仍是 `0.40.1` / `0.38.1`（其 lock `:197`、`:134`），因此不能照抄历史 manifest 的 same-lock spike 证明。

本机只读核验了已缓存的 **精确 `libsqlite3-sys 0.38.2` crate archive**，未下载或解包到磁盘：

- archive SHA-256 `f1d20bef17f513b9b3004532233187769cd072d790971f4e4da0e346eb6401e8` 与 root Cargo.lock 完全匹配。
- archive 内 `sqlite3/sqlite3.h:149`：`SQLITE_VERSION="3.53.2"`；header SHA-256 `9e69a1353a4288450b0d5239ede11fc7f1f4c8e5eb07491fc8317eacb5b7de7e`。
- archive 内 `sqlite3/sqlite3.c:470` 同一版本，`:187296` 的 `sqlite3_libversion()` 返回该版本常量；source SHA-256 `0a409f1633283fa31a9126b11fbfd64a1991c5d30defad07e5745d4667f5e23d`。
- 同包 `build.rs:79` 可被 `LIBSQLITE3_SYS_USE_PKG_CONFIG` 或 `loadable_extension` 转为外部链接，`:84` 的 sqlcipher 也可覆盖 bundled；真正 bundled 分支在 `:97`，`:151` 编译包内 sqlite3.c，`:334` 产出 native library。

**计划可采用的诚实来源**：在冻结 S 的实际 build 中核验以上 package/checksum、resolved features、未采用外部-link override、实际 bundled build 输出及二进制 B；记录 `sqlite_version=3.53.2` 与明确来源“locked bundled source + this build linkage; no direct CLI runtime query”。这是由构建链确定的 linked library version，不能表述成执行了 runtime query。包内文件路径使用上述公共相对路径，绝不写缓存绝对路径。

当前 canonical `target/release/build` 不存在，本轮没有所测 B，也未完成这条 build-linkage 证明；缓存 header 本身不证明将来的 binary。若实际 build 无法证明 bundled 链接，则报告未知并阻止 **qualified full** 发布，不改用 Python、旧 spike 或猜测值。当前 validator `:649` 只拒绝 `not_recorded` / `not_exposed_by_cli` 两个占位词，不验证版本来源真假；填其他 unknown 字串虽然可能通过代码，也不满足 SLI `:71`。最小 tooling patch 可收集/验证上述 build receipt；SLI 文档同步说明 build-derived 与 runtime-observed 两种证据强度，无需新增 Rust CLI 功能或修改 spike lock。

### 6. 历史、当前与 CI 的比较边界

历史 full 为 2026-07-25 的 `88d86f44a0f8d4d0261aaddddb99524514e57d7b`，synthetic 4000 messages / 1,122,164 bytes；它仍是有效历史 v1，不因新工具而重写。历史目录中的 JSON、Markdown、manifest 及其他证据全部保持原样。

相同 corpus hash 只是可比性的一部分。历史与当前若缺少相同协议/queries、cache/startup 状态、target/build options、环境和重复-run 分布，不得计算“优化快了 X%”或冻结阈值。跨源码比较允许 B 不同，但必须分别绑定二进制并排除其他混杂；同次基线重复应固定 B。新诊断扩展的数字不能与旧 aggregate raw 直接做因果比较。成本排序区分整次调用耗时与内部瓶颈假说。

Core CI 只跑 smoke（workflow `:108`），不能代替 full 或 scale diagnostics。它的 PR checkout 未显式指定 ref（`:69`），报告与环境使用 `github.sha`（`:113`、`:220`）：按官方默认，这可能是 PR merge-preview SHA，而不是 PR head 或实际 main。保留其原 SHA，不能改标。CI artifacts 的 7 天保留期（`:245`）也不替代长期报告；本子任务不需要新增 CI full workload 或修改 workflow 来宣称完成。

`evidence_status` 保留 `locally_verified`（Core validator `:540`），这不表示全部 provenance 已被工具独立证明。SLI 仍 Draft；不升级 SLO、release、minimum-OS 或签名状态（SLI `:60`；matrix `:19`、`:79`）。

### 7. 建议批准的精确产品写集

| 阶段 | 允许文件 | 边界 |
| --- | --- | --- |
| S tooling | `scripts/evidence/core_beta_benchmark.py` | 可选版本化扩展、必要的身份/build receipt 记录与验证、Markdown 投影；不改原 profile/metrics 语义 |
| S tooling | `scripts/evidence/test_core_beta_benchmark.py` | 上述兼容/负向/确定性测试 |
| S tooling | `docs/product/SLI-AND-BENCHMARK-FORMAT.md` | §2.1 附加扩展与来源/发布规则，保留 Draft 和所有原要求 |
| P publication | `docs/evidence/core-beta/<S-prefix>/manifest.md` | 精确 S/H/B/C/E、重现命令、所有 report hashes、运行/验证/限制/诊断结论 |
| P publication | `docs/evidence/core-beta/<S-prefix>/run-<NN>/core-beta-benchmark-full.json` | 每个已批准 independent run 的原始报告，不能覆盖另一次运行 |
| P publication | 同目录 `core-beta-benchmark-full.md` | 由同版 renderer 生成的投影 |
| P publication | `docs/performance-baseline-0.2.md` | 新增日期/source 明确的入口与简短结论；保留历史章节和数字 |

`<S-prefix>` 在 S 固定后取其无歧义前缀，manifest/JSON 始终记录 full SHA。按 `research/methodology.md` 的候选方案，NN 限定为 `01`、`02`、`03`，每次各含 full 与 2k/10k/50k intact-corpus diagnostics；最终仍以计划审阅为准。诊断结果内嵌原报告扩展，无需新 schema 文件、新 benchmark 脚本、新 corpus 提交或新的独立诊断 JSON 管线。

另行采集的 trace 不混入延迟样本，也不增加上述公开写集：原始 trace 留在 ignored 本地输出中，只将经隐私审阅的阶段汇总和假说写入新 manifest。trace 结果本身不能冒充未启用 trace 时的时延基线。

不在写集：Rust crates、Cargo manifests/locks、旧 spike、gate/semantic/real-data harness、CI/workflow、旧 `88d86f4` 目录、historical imports、roadmap/release 决策记录、scanner rules、治理配置。matrix 已整体标明 historical imported，本切片不用改它。若实施发现这些必须改，应在计划中显式扩围，不能随基线顺带落入提交。协调目录仅保存规划资料，不能进入产品 commit；两个受保护 dirty specs 保持原样。

### 8. 有限执行和验证顺序

1. 审阅扩展字段、workload/资源矩阵、SQLite build-derived 来源、上述写集和实施计划。任何编码/运行只在规划批准后开始。
2. 实现并检查三个 S 文件。通过现有 helper tests、历史 full compatibility 及 independent review。以当时 canonical main 为基础，经已授权的提交操作形成 S；运行前后确认干净、H=S、build inputs 不变，并保存文件 hash 与版本/参数白名单。
3. 依批准矩阵在 isolated synthetic scratch、ignored `evidence-output/` 运行。每个 independent run 独立 output directory；先验证 correctness，再计时；失败样本/资源上限如实保留。固定 target/toolchain/security 状态，不做全局调优。
4. 新版 `validate-report` 重算核心和扩展 raw summaries；核对 S/H/B/C、元数据、SQLite build receipt、Markdown projection、报告 hashes。选择全部计划运行，不能挑最好一次。
5. 在 P 中只复制审阅后的原 JSON、投影和新 manifest，并更新现有性能入口。P 相对 S 只含上述 publication 文件；若 Rust/harness/build/corpus 输入变化，不能把原测量称为该新输入的结果。需要“当时最新 main”时再次检查 main 变化，决定重测或明确标注 S。
6. 对 P 当前 SHA 执行质量/隐私/治理检查、独立复核，形成可审阅 PR。每个新 SHA 或重大 PR 文本变化都更新 checks/owner acceptance；普通 merge 需要单独授权。事后登记 P/merge 的证据，报告中的 S 不变。

既有运行接口示例（变量须来自本次核验；诊断 opt-in 参数将在 patch 中正式定义，不伪称当前已支持）：

```powershell
python -B scripts/evidence/core_beta_benchmark.py run `
  --profile full --workspace . `
  --output-dir evidence-output/current-baseline/run-01 `
  --expected-commit $baselineSource `
  --disk $baselineDisk --filesystem $baselineFilesystem `
  --antivirus-state $baselineAntivirus `
  --sqlite-version $baselineSqliteVersion `
  --sqlite-version-source $baselineSqliteEvidence
```

必需检查命令，全部从产品 checkout 执行；报告命令逐个已批准 run 执行：

```text
python -B -m unittest scripts/evidence/test_core_beta_benchmark.py -v
python -B -m unittest discover -s scripts/evidence -p "test_*.py" -v
python -B -m unittest discover -s scripts -p "test_*.py" -v
python -B scripts/evidence/core_beta_benchmark.py validate-report docs/evidence/core-beta/88d86f4/core-beta-benchmark-full.json
python -B scripts/evidence/core_beta_benchmark.py validate-report <new-report.json>
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

若改变 workflow/release 接线才额外运行 `python -B -m unittest discover -s scripts/release -p "test_*.py" -v`；目前写集不要求修改这些文件。相邻 evidence suite 包含前置 gate validator 测试。Markdown 可在内存中与 `render_markdown(validated_json)` 比较；增加的 provenance 验证不能只看摘要文本。

publication governance 按 `CONTRIBUTING.md:176`、`:212` 使用已核验的 base/head/PR/merge 输入和可信 checker：

```powershell
python scripts/governance/check_commit.py --repo . --base $base --head $head
python scripts/governance/check_commit.py --message-file $mergeMessage
python scripts/governance/check_pr.py --event-file $event `
  --repo . --expected-base $base --expected-head $head `
  --repository-id $repositoryId --merge-message-file $mergeMessage `
  --merge-metadata-file $mergeMetadata
python scripts/governance/scan_paths.py --repo . --base $base `
  --head $head --event-file $event --repository-id $repositoryId `
  --merge-message-file $mergeMessage --merge-metadata-file $mergeMetadata
python scripts/governance/scan_credentials.py --repo . --base $base `
  --head $head --gitleaks-archive $archive --platform $platform `
  --event-file $event --repository-id $repositoryId `
  --merge-message-file $mergeMessage --merge-metadata-file $mergeMetadata
```

必须覆盖精确 tracked tree 和全部 introduced commits/blobs/messages、PR/merge metadata；只分享安全摘要（CONTRIBUTING `:66`、`:230`）。缺少 verified pinned Gitleaks archive 不自动安装、不伪称通过。若该阶段需要治理 self-tests，显式 process-local `GOVERNANCE_TEST_ARCHIVE` 后运行其 suite，缺输入/skip 不是成功（`:278`）。检查当前主分支 protection 和真正 required-check results（`:163`）；旧 25 checks 或 YAML 不是 P 的证据。

### 9. 外部参考与相关规范

- Cargo 官方 `cargo build` 的 `--locked` / `--offline` / `--frozen` 定义，2026-10-14 通过 Grok `web_fetch` 取得正文：`https://doc.rust-lang.org/cargo/commands/cargo-build.html`。仅用于区分锁依赖与 offline/来源证明。
- GitHub 官方 pull-request checkout 默认行为，2026-10-14 通过 Grok search 加官方正文交叉确认：`https://docs.github.com/en/actions/reference/security/securely-using-pull_request_target`；对应 event 文档：`https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#pull_request`。只采纳与 merge-preview/head 区分有关的段落，不推论本仓库当前 hosted 状态。
- 已安装且 archive checksum 对齐的 `libsqlite3-sys 0.38.2` 源码是 SQLite 构建来源的直接证据；公共来源身份为 Cargo.lock 所指 crates.io 包及上文文件 hashes，绝对 cache 路径不进入交付。
- `.trellis/spec/agentsessions-cli/backend/index.md:303`、`:329`：synthetic/evidence honesty。real-data closed report 规则（`:318`）是另一 schema，不能误用于禁止本次 Core 顶层扩展。
- `.trellis/spec/guides/code-reuse-thinking-guide.md:55`、`:86`：共用常量/helpers；避免另一统计/哈希管线。`quality-guidelines.md` 多为占位，具体约束以产品契约和 repository instructions 为准。
- `docs/security/FIXTURE-REDACTION-POLICY.md:16`、`:33`：仅 synthetic/合规脱敏 fixture；不复制受限竞品/真实数据。任务进一步限定 synthetic only。

### 10. Planning review and closure (2026-10-14)

复核范围为本 task 的 `prd.md`、`design.md`、`implement.md`、`research/methodology.md`，未重新探索产品、运行程序或读取 JSONL。主计划已将可选对象正式命名为 `baseline`，其版本为 `agent-session-grep.current-source-baseline/v1`；上文 `diagnostics` 是研究阶段示意名，实施以已审阅 design 的唯一名称为准。

| 严重度 / 状态 | 修正后计划锚点 | 定点复核结果 |
| --- | --- | --- |
| P2 / 已关闭 | `implement.md:85`、`:87`、`:94`、`:111` | 期望源码现在绑定 manifest/frozen receipt 的完整 S，再独立比较实际 HEAD；发布复现命令不会以 P/后续 main 自身充当期望 S。 |
| P2 / 已关闭 | `implement.md:39`、`:71`、`:107`；`design.md:191` | opt-in run 1 单独 build 并冻结 B/receipt，然后启动共享 suite 时钟；run 2/3 从同一 output parent 核验并复用原 receipt/B，不调用 Cargo、不重启 deadline。缺少/不匹配 receipt 即失败。ordinary full/smoke 行为保持。 |
| P2 / 已关闭 | `design.md:197`、`:199`、`:202`；`implement.md:140` | opt-in suite 拒绝 external `--binary`，非 suite 支持不变。所有 prebuilt 消费（包括 run 2/3）明确保留 `caller_supplied_prebuilt` 及 attestation caveat；receipt 独立表达 linkage，不能升级来源标签或假称重新构建。 |

初次规划复核未发现 P1 级阻断；三报告 conditional validation、无扩展历史兼容、SQLite build-derived 与 runtime-observed 区分、精确产品写集、S/P/merge 标签以及当前-head owner/governance 边界一致。2026-10-14 的关闭复核只读取上述修正区域，三项 P2 均已解决，无剩余 finding；未扩展审查范围，也不表示实现、构建或测量已验收。

## Caveats / Not Found

- 本研究只写此文件；没有改变产品、两处 protected specs、任务状态、父任务、Git、配置，也未运行 benchmark 或构建。
- 外部浏览器 search/open 未返回可用正文；本机没有已安装 `ctx7`，且 dispatch 禁止 provisioning，因此使用 `find-docs` 的 Grok 官方文档回退。未以训练记忆替代版本事实。
- 找到的缓存 SQLite header 不是已经测量的 runtime；必须在实际 S build 补足 linkage receipt。没有直接 CLI runtime-query 入口；不建议为本任务新增产品功能。
- Core v1 当前数值/容器检查不像已修复 gate 一样全面（例如 `:582` 仍以 Python numeric 类型判断）；本次需要保证新增扩展和真实 producer 的严格验证，不能宣称所有 provenance/tampering 问题均已解决。
- 当前 author/committer、发布/merge、资源矩阵和最终计划批准由主会话按用户授权与仓库规则处理。本方案不新增审批制度；缺少实际 build evidence 或 mandatory governance 输入时必须如实停在未验证状态。
