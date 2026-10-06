# B7 收口复盘：热路径复测与维护性映射 — 结论

任务 `.trellis/tasks/10-07-final-perf-regression`（父任务 10-05，用户 2026-10-06 批准 D2）。
本文件是 B7 的总结论；逐项原始证据在同一 `research/` 目录。

## 0. 环境与口径（可复现）

| 项 | 值 |
|---|---|
| 工作区 / 分支 | `C:\AgentSessions` / `fix/session-relocation-identity` |
| 测量时 HEAD | `4bea67f4d113976d439e91a5bac825bb10a7106d`（任务开始时为 `006746b`；期间并行任务提交了 docs-only `4bea67f`，`crates/` 无未提交改动） |
| release 二进制 | `.trellis/.runtime/target-b7/release/agent-session-grep.exe`，sha256 `5924c24d9ecb625427ba97a52ecb2e60497ae1b3945e8bbf0edd4da050a6123d`，6,951,936 bytes |
| isolated target dir | `C:/AgentSessions/.trellis/.runtime/target-b7`（build/clippy/test 全部用它） |
| 机器 | Windows 10.0.22631 / AMD64、22 logical CPU、31.5 GB RAM、NVMe SAMSUNG MZVL81T0HELB-00BTW SSD、NTFS、Defender 实时防护开启 |
| SQLite runtime | 3.53.2（`spikes/sqlite-snapshot-wal` 在 HEAD 重建并运行，`rusqlite::version()`；workspace 与 spike 锁一致 rusqlite 0.40.1 / libsqlite3-sys 0.38.1；非被测 CLI 查询） |

## 1. 热路径复测（PRD 要求 1）

完整表：`hotpath-comparison.md`；原始数据：`after-measurements.json` + 6 份输出 JSON +
`hotpath-run.log`；命令复用 B1 `measure.ps1`（仅改 `$expDir`/`$outDir`）。

| 命令 | audit 基线（5b232cd） | B1 after（912f45c8） | **B7（4bea67f）** |
|---|---:|---:|---:|
| get 中位 / 探测 | 53.665 ms / 2 | 9.263 ms / 0 | **7.96 ms / 0** |
| show 中位 / 探测 | 52.438 ms / 2 | 9.326 ms / 0 | **7.91 ms / 0** |
| search 中位 / 探测 | 97.112 ms / 4 | 55.728 ms / 2 | **48.02 ms / 2** |
| seam 参照 search | 9.982 ms | 9.848 ms | 9.08 ms |

- 残余单轮 repo 解析成本（search git_detect − seam）：**38.94 ms ≈ 19.5 ms/git 子进程**。
- 变体输出逐字段一致（仅 `meta.duration_ms`）：`output-diff.txt` PASS。
- 夹具说明：B1 的 fixture 是 schema v18，当前二进制 fail-closed（exit 9）；B7 以同一
  seed/clock/request-id 建 v19 夹具，wire id 与 B1 逐字节一致
  （`msg_v1_cb6837d0712fbb4e022f869652978b74`）。

## 2. Core benchmark 复测（PRD 要求 1）

完整表：`core-benchmark-comparison.md`；报告：`core/core-beta-benchmark-full.json`
（`scripts/evidence/core_beta_benchmark.py run --profile full` + 独立 `validate-report`，
两次都输出 `valid agent-session-grep.core-beta-benchmark/v1 report`）。

| 指标 | 基线 P50 / P95 | B7 P50 / P95 | P95 Δ |
|---|---:|---:|---:|
| search | 114.505 / 162.896 | **63.600 / 72.133** | −90.76 |
| show | 53.965 / 59.781 | **9.230 / 13.567** | −46.21 |
| get | 54.426 / 62.897 | **9.405 / 11.018** | −51.88 |
| initial sync | 917.095 / 1059.322 | **748.014 / 777.271** | −282.05 |
| noop sync | 15.486 / 16.221 | **14.969 / 15.202** | −1.02 |
| shrink sync | 728.598 / 742.797 | **638.429 / 638.775** | −104.02 |
| initial index throughput | 1.167 / 1.221 MB/s | 1.431 / 1.447 MB/s | +0.23 |

数据集 hash 与基线完全一致（`8ebb21d29ce5…`，4000 消息/20 文件），同 harness 同 profile。
仍非 SLO，只是本地证据锚点；recovery 依旧 `not_implemented`。

## 3. 残余热路径判定（PRD 要求 2）

**判定：不实施代码改动。** 完整论证：`residual-hotpath-verdict.md`；原始探测证据：
`residual-repo-probe-evidence.log`。

- 单子进程候选（只跑 `git -C <cwd> remote get-url origin`）能省 ~19.5 ms/search，但实测在
  **bare repo** 与 **cwd 在 `.git` 内** 两种上下文把 `current_repo_slug` 从 None 变成
  Some(slug)（`rev-parse --show-toplevel` exit 128 vs `remote get-url` exit 0）。
- `current_repo` 同时进入 search 排序（ranking boost）与 cursor digest
  （`application/src/lib.rs:906-940`），None→Some 是**可观测契约扩张**，违反本任务
  “契约与 cursor digest 不变”的硬约束。
- 文件系统替代需自行复刻 git discovery/config 分层（含 insteadOf、worktree 指针、
  `GIT_*` 环境语义），与 `repo_identity.rs:5-13` 的既定决策冲突 → 高风险不采纳；
  持久缓存/空表短路同样改变实时语义，不采纳。
- 后续若要该收益：需 owner 先批准契约扩张（见 verdict 文档三选一），不是低风险优化。

## 4. 维护性映射（PRD 要求 3）

表与同步点清单：`maintainability-map.md`（前 5 大文件：adapters-sqlite 20,504 行
= 9,502 生产 + 11,002 测试；application/lib.rs 7,644；cli/lib.rs 7,437；mcp.rs 3,502；
provider-claude/codex ~2.7k/2.6k）。**未做任何物理拆分**：收益评估全部为
“无可量化收益”，真实同步风险在 4 个重复同步点（CLI flag 注册、schema 版本、
关系投影版本、协议 schema），属父任务 P2-07 的独立范围。

## 5. 文档同步（PRD 要求 4）

**结论：N/A，无文档改动。** 核查方式与结果：

- `README.md` 与 `docs/**/*.md`（排除历史 `docs/evidence/**`）全文搜索
  `53.6|52.4|97.1|162.9|16.22|114.5|54.4|59.78|62.89|17.83` → 0 条把旧性能数字当“当前值”的陈述。
- 复核时全文仅 2 处子串命中，均为 rustc 版本号 `1.97.1` 内的 `97.1`
  （`docs/operations/INSTALL-AND-UPGRADE.md:15`、`docs/release/OWNER-RELEASE-CHECKLIST.md:48`），
  不是性能数字；`docs/evidence/core-beta/88d86f4/` 等历史记录按“不改历史记录”原则不动；
  本轮无产品代码改动，不产生新的过时数字。
- 若 owner 希望把 B7 数字发布为 docs 级证据锚点，应另立证据 bundle（提交号/二进制
  hash/环境齐全），不属于本任务收口范围。

## 6. 验证门禁（isolated target dir，全部在本工作树、HEAD 4bea67f）

| 命令 | 结果 | 日志 |
|---|---|---|
| `cargo fmt --all --check` | exit 0 | `fmt-check.log` |
| `cargo clippy --workspace --all-targets --locked --target-dir C:/AgentSessions/.trellis/.runtime/target-b7 -- -D warnings` | exit 0，无 warning/error（首次全量 run 覆盖全部 crate；日志保存的是幂等复核 run） | `clippy.log` |
| `cargo test --workspace --locked --target-dir C:/AgentSessions/.trellis/.runtime/target-b7` | exit 0：**1811 passed / 0 failed / 20 ignored**（含 e2e、mcp_e2e、hotpath_repo_probe、network_egress、provider matrix 等全部 target） | `workspace-tests.log` |
| `node --test crates/agent-session-grep-cli/tests/web_ui.test.cjs`（补充） | exit 0：11/11 pass | `web-ui-tests.log` |

## 7. 逐条验收标准

| PRD 验收标准 | 判定 | 证据 |
|---|---|---|
| 前后对比表（探测次数/中位数/P95）有原始日志与可复现命令 | **PASS** | `hotpath-comparison.md` + `after-measurements.json` + `measure.ps1`；`core-benchmark-comparison.md` + `core/…json` |
| 残余优化：实施+测试，或给出不实施理由与数据 | **PASS（不实施 + 数据）** | `residual-hotpath-verdict.md` + `residual-repo-probe-evidence.log`（bare repo / `.git` 内 cwd 的 None→Some 实测反证） |
| 维护性映射表（文件:行、边界、风险、是否建议执行） | **PASS** | `maintainability-map.md` + `residual.*` 同步点清单 |
| 文档数字同步（如有过时） | **PASS（N/A）** | 第 5 节：核查 0 处因本轮过时的当前值陈述 |
| cargo fmt/clippy/test（workspace，isolated target dir）全绿 | **PASS** | 第 6 节三份日志 |

## 8. 未做 / 未跑（诚实边界）

- 未实施任何产品代码改动（残余优化按上文判定不实施）；因此没有新增/修改测试。
- 未做跨进程 slug 缓存设计与实验（超范围，仅给方向）。
- 未做 macOS/Linux 复测（本机仅 Windows；core benchmark 亦为 Windows x64 本地证据）。
- 未修改历史证据文档、README、spec；未 commit（按要求留给主会话）。
- core benchmark 的 `binary.provenance = caller_supplied_prebuilt`（与 audit 基线同一种），
  即二进制由外部提供（本任务 cargo build 产物，hash 已记录）。