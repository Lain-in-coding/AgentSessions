# Research: 测试套件与 testkit 审查

- **Query**: 审查 agent-session-grep 测试套件与 testkit——速度、覆盖缺口、e2e_consistency harness、testkit 质量、flakiness、CI wiring
- **Scope**: internal(全仓库,含竞品固定 clone 对比)
- **Date**: 2026-08-15

## 结论摘要

- 全量 563 个测试,串行累计约 8.9s,并行墙钟由两个 suite 主导:**cli e2e.rs 4.19s(75 测)**与 **adapters-sqlite 3.55s(119 测)**,其余全部 <0.4s。**无 P0 发现**;P1 无;P2 共 10 项。
- 无 sleep、无 wall-clock 断言、无共享 db 文件、无端口绑定测试——flakiness 面整体干净;唯一本地隐患是已知的 Windows Defender 间歇删文件环境问题。
- 与竞品对比(固定 clone):hstry 每测一个 UUID 真实 SQLite 文件且**不清理**(`hstry-core/tests/db_operations.rs:9-15`);memex 以 inline 单测为主无 e2e。本项目 in-memory sqlite + `tempfile::TempDir` 自动清理,纪律同级或更好。
- **重要澄清**:任务清单里"刚落地特性"(redaction、handoff、retrieval-mode、resume preview、tool activities、facets、web serve)大部分**尚未落地**——它们是本路线图任务的子任务规划(`prd.md` 子任务表 #1–#10)。当前树内已落地的只有:错误消息脱敏(ADR-0005 风格)、cursor 分页/续读、interrupted-batch intent(`index_batches` 表)。故"缺负例测试"仅适用于已落地能力,规划中特性不做逐项缺口列举(落地时按各自 PRD 补)。

---

## 1. 测试速度(实测,2026-08-15 本地全量跑批)

| Suite | 测试数 | 耗时 | 说明 |
|---|---|---|---|
| cli `tests/e2e.rs` | 75 | **4.19s** | 每断言一次子进程 spawn + SQLite 打开(e2e.rs:17-25 `run()` helper),全文件约 250 次 spawn |
| adapters-sqlite `src/lib.rs` | 119 | **3.55s** | 全部 in-memory(`open_in_memory`,lib.rs:946-948),量级来自用例数 |
| cli `src/main.rs`(含 mcp/protocol/human/tui) | 142 | 0.28s | |
| application `src/lib.rs` | 75 | 0.01s | |
| domain `src/lib.rs` | 45 | 0.14s | |
| provider-claude(lib+golden+properties) | 37 | 0.05s | |
| provider-codex(lib+golden+properties) | 28 | 0.03s | golden 有 1 个 `#[ignore]`——手动再生成 helper(golden.rs:200-203),有意为之 |
| cli `tests/mcp_e2e.rs` | 12 | 0.26s | |
| cli `tests/e2e_consistency.rs` | 2 | 0.32s | 额外 spawn python + 二进制约 5 次 |
| cli `tests/provider_matrix.rs` | 2 | 0.00s | |
| ports / testkit | 22 | 0.00s | |
| sqlite `tests/process_evidence.rs` | 4 | 0.17s | |

**瓶颈定位**
- e2e.rs 主导因素是**进程 spawn 密度**:`perf_baseline_100_messages_index_and_search`(e2e.rs:2424-2452)单个测试就顺序 spawn 100 次(≈1s),且只打印观察值不做断言(e2e.rs:2451 `eprintln!`)。
- sqlite suite 的单测本身很快,3.55s 是 119 个用例的线性叠加,无异常。
- 无任何 `thread::sleep`/轮询;唯一 `Instant::now` 在 perf baseline(仅观察)。

**建议(P2)**
- perf baseline 降为 10 条或改用单次 `sync` 批量写入(100 次 spawn 的 IO 不是被测对象);它是 e2e.rs 里最大的单测成本。
- CI 的 "Robot contract tests" 步骤(ci.yml:46-47)是 `cargo test --workspace` 已跑过的子集再跑一遍(`robot_` 过滤),CI 白花约 2s,可删除或并入同一 run。

---

## 2. 覆盖缺口

### 已落地且覆盖充分(实测确认)
- **错误脱敏**:单测 `source_io_error_masks_source_path_in_message`(protocol.rs:546 一带)+ e2e `error_outputs_never_disclose_source_path_or_content`(e2e.rs:985)+ `mcp_stderr_stays_clean_on_protocol_errors`(mcp_e2e.rs:673);`get/show` not_found 通用文案双模式验证(e2e.rs:384, 465)。
- **cursor 契约**:分页不重不漏(e2e.rs:1642)、篡改令牌拒绝(e2e.rs:1697)、generation 换代失效 + details 结构化(e2e.rs:1739)、MCP 坏 cursor 业务错误分层(mcp_e2e.rs:475)。
- **预算截断**:context `--max-messages` partial/exit 10(e2e.rs:2191)、search `--max-bytes`(e2e.rs:2949)、human 模式截断渲染(e2e.rs:2254)。
- **sidechain/跨文件会话/共享消息**:e2e.rs:1849、1972、2026、2159 四条大用例 + mcp_e2e.rs 同形状夹具。
- **writer lease 竞争**(e2e.rs:962-982)、**只读契约** `assert_read_only`(testkit lib.rs:481)、**provider 自动选择与 event_msg 镜像去重**(e2e.rs:647)。

### 已落地但缺负例/边界(P2,附具体用例)
1. **EPIPE 只测了 `--help`/`--version`**(e2e.rs:896-923)。全局 pipe-safe 通道 `write_stdout_line`(protocol.rs:429-432)没有单测,`search | head` 这类输出中途关管道场景无覆盖。修法:protocol.rs 加一个 EPIPE 单测(写一个已关闭的管道),或 e2e 用 `search` 大结果集 + 提前 drop 读端。
2. **ingest 权限拒绝文件**无测试——只有"文件不存在 → exit 5"(e2e.rs:928)。PermissionDenied 应走 source_io 还是其他映射未锁定。
3. **context `--policy full` + `--max-messages` 组合**无测试(单策略和单预算都有,组合无)。
4. **MCP 交互序列矩阵不全**(mcp_e2e.rs 12 个用例覆盖协议错误/gate/参数):无第二次 initialize、无 `tools/call` 缺 `arguments` 键、无同一会话内并行多请求流水线、无带 id 的通知。
5. **e2e_consistency 只比较 2 个成功操作**(search/list_sessions,compare_entrypoints.py:61-65)——无错误路径跨入口一致性操作(如 `not_found` 在 CLI/MCP 的 canonical code 对齐)。
6. **TUI 无 PTY 级自动化**:核心逻辑单测在 `tui/core.rs`(24 测)+ `tui/mod.rs`(3 测)覆盖渲染/状态机,但真实键盘输入流无自动化——路线图 #7(web/tui parity)落地时需配套 PTY 或驱动框架。

### 规划中特性(按 prd.md 属路线图,非当前缺口)
handoff(#4)、retrieval-mode(#3)、resume dry-run 预览(#5)、tool activities/facets(#6)、web serve(#7)、ADR-0009 跨边界脱敏(#8)。代码树中无这些实现(无 HTTP 依赖、无 handoff 相关 crate),当前树内最接近的"resume"是 sqlite `index_batches` intent 表与 doctor 的 `interrupted_batches` 报告(e2e.rs:1178-1199 有覆盖)。

---

## 3. e2e_consistency harness 分析

- **python 解析**:`run_script` 按 `python` → `python3` 回退(e2e_consistency.rs:33-59)。三平台均覆盖:Windows runner 镜像有 `python`,Ubuntu 有 `python3`,macOS 只有 `python3`(spawn Err → 回退成功)。设计正确。
- **两个小问题(P2)**:
  - `python` spawn 成功但退出非 0(如脚本自身 exit 1)时也会回退到 `python3` **重跑整个脚本**——一致性失败场景下时间翻倍,且最终报错来自第二个解释器,归因混淆。
  - 本机装了 MS Store 版 python stub(交互式打开商店,spawn 退出 9009)时,报错是 "must exit 0, got 9009" 而非清晰的"未安装 Python",开发机排障体验差。
- **Windows runner 风险(P2)**:ci.yml 的 test job **没有** `actions/setup-python`,`cargo test --workspace` 跑 e2e_consistency 依赖 runner 镜像预装 python——目前通过,但属隐性环境依赖;installer job 已有 setup-python(ci.yml:62-64),test job 补一个即可。
- **fixture-db 复用**:**每次运行重建**。Rust 测试不传 `--workdir`(e2e_consistency.rs:78)→ 脚本 `mkdtemp` 新临时目录(compare_entrypoints.py:527),db 与夹具全新;同一进程内 ingest + 查询共享同一 db(compare_entrypoints.py:416-419);结束后 `shutil.rmtree` 清理(compare_entrypoints.py:553)。无跨运行复用,无陈旧状态风险。脚本的 `--workdir` 参数存在但无调用方——CI 想留证时可用。

---

## 4. testkit 质量

- **SessionBuilder**:fact 派生 id,同 fact 幂等(`session_builder_is_deterministic`,lib.rs:521-528);产出过 `validate`;source document 的 fingerprint/len 自洽有单测(lib.rs:710-717)。角色交替、placement/edge 生成线性图,足以覆盖大多数 application 层用例。**良好的 ergonomics**。
- **InMemoryStore**:忠实实现 CatalogStore + SearchIndex + ContextGraphStore 三端口;近似点均有文档化注释(query 子串匹配/score 恒 1.0/空查询空结果近似而非报错,lib.rs:237-243;`session_of` 多会话取 wire 字典序最小,lib.rs:317-335)。`generation`/`claims` 独立计数对齐 SqliteStore 批次提交语义(lib.rs:149-155),单测覆盖(lib.rs:594-607)。11 个自测。
- **FakeProvider 缺口(P2)**:`ProviderError` 有 4 个变体(ports lib.rs:AmbiguousVariant/StructuralFatal/SourceChangedDuringRead/Io),fake 只能产前两类(testkit lib.rs:367-430)。**无法模拟 `Io`(可重试读故障)与 `SourceChangedDuringRead`(源被改写回滚路径)**——ingestion 错误矩阵的这两支无测试替身。修法:加 `io_failure()` / `source_changed()` 构造函数。
- **隐私**:全合成数据,无真实路径/主机名/用户名。唯一的 "C:/Users/secret/..."(protocol.rs:546)与 `secret-path-transcript.jsonl`(e2e.rs:989)是脱敏测试的故意夹具,非泄露。

---

## 5. Flakiness 风险

| 风险 | 评估 |
|---|---|
| temp 目录冲突 | 无。全部 `tempfile::tempdir()` 每测独立(e2e.rs:106-111、mcp_e2e.rs:17-21);sqlite 单测全 in-memory;consistency 用 `mkdtemp`。`cargo test` 并行安全 |
| 端口绑定 | 无测试绑定端口(web 未落地,代码树无 HTTP 依赖) |
| 时间假设 | 无 wall-clock 断言(perf baseline 只 `eprintln!` 观察值,e2e.rs:2451) |
| writer lease | `writer_busy` 测试(e2e.rs:962-982)租约与子进程均在各自 tempdir,无跨测试互扰 |
| 子进程输出泄漏 | **发现一处**:mcp.rs 单测在进程内跑 MCP server 循环(mcp.rs:53 `write_stdout_line`),响应帧泄漏到真实 stdout(实测两次全量跑批均出现 help JSON 混入测试输出,源测试 `initialized_notification_is_silent_and_opens_the_gate_after_handshake`,mcp.rs:807)。不失败但污染 CI 日志,且这类"测试线程写 stdout 逃出捕获窗口"容易掩盖真正的泄漏回归。P2 |
| 本地环境 | Windows Defender 间歇删本仓库文件(已知 hazard)——tempdir 夹具偶发丢失会假失败;CI runner 无此问题 |

---

## 6. CI wiring

**现状**(`.github/workflows/ci.yml`)
- 3-OS 矩阵(fail-fast: false):fmt + clippy(-D warnings)+ `cargo test --workspace` + robot 子集重跑 + installer smoke(双平台脚本、双次 uninstall)+ cargo-deny 供应链检查。
- e2e_consistency 经 `cargo test --workspace` 自动纳入三 OS。
- `core-beta-evidence.yml`:4 平台(win-x64/linux-x64/mac-intel/mac-arm64)release 构建 + 基准**smoke** profile + validate-report + sqlite 证据测试 + 三个 spike + SHA/环境报告上传,timeout 45min。

**缺失/建议(P2)**
1. test job 未 `setup-python`(见 §3,隐性依赖 runner 镜像)。
2. "Robot contract tests" 步骤与 workspace test 重复(见 §1)。
3. **无 benchmark 回归门**:`core-beta-evidence.yml` 只跑 `--profile smoke`(108-118 行),正式门基准是手动流程(路线图 #9 子任务规划中)——建议 #9 落地时把 gate benchmark 接进 CI(可 cron 或 dispatch,避免每次 PR 全量跑)。
4. 无覆盖率统计(tarpaulin/codecov 均无)——对"负例缺失"类回归(§2)无量化抓手。
5. ci.yml test job 无 `timeout-minutes`(evidence 工作流有 45min),建议加防挂起。

**做得好的**:fail-fast:false、cargo-deny、installer 卸载幂等验证、evidence 工作流带 runner/SHA 环境报告且明确"非 release certification"边界。

---

## Caveats / Not Found

- 未找到:任何 sleep/轮询、共享 db 文件、端口绑定测试、真实个人路径/主机名泄露、`cargo test` 并行冲突。
- 竞品对比仅抽查 hstry(db_operations.rs/incremental_sync.rs)与 memex 测试文件列表,未逐文件精读。
- 测试计时为 2026-08-15 本地 Windows 实测,CI runner 数值会略异;之前"~20s"印象应含编译时间(本次热编译后纯跑批约 5-9s)。
- "刚落地特性"缺测项(§2 后半)按 prd.md 判定为规划中,若主 agent 掌握这些特性实际已落地的分支状态,需按对应分支重新核对。
