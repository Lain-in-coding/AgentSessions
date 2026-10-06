# B1 热路径修复：消除重复 Git 探测 — 实测与验证记录

任务：`.trellis/tasks/10-06-hotpath-git-probe`（父任务 10-05，P1-02）。
改动范围：`crates/agent-session-grep-cli/src/lib.rs`、`src/mcp.rs`、新增
`tests/hotpath_repo_probe.rs`。契约不变是硬条件。

## 1. 结论

- get/show/status/list/context/get-message/resume/get-session-resume 等按 ID 纯读
  路径经 `resume_app`/`resume_semantic_app` 构造 App，**不再解析当前仓库** → 零 git 探测。
- Search / Handoff / Hook / MCP 搜索仍在需要 repo-aware 排序时解析 repo，但**每请求
  只解析一轮**并把 slug 复用给 lexical 与 semantic 两个分支；MCP 仅对 Search 请求解析。
- 取时钟改用 `app_clock_ms()`，不再为了取时钟构造带环境副作用的 App。
- before/after JSON 逐字段 diff：除 `meta.duration_ms`（墙钟测量值）外**完全一致**。

## 2. 配对实测（release 二进制，20 次/变体，同一 DB/同 cwd）

cwd = `C:\AgentSessions`（带 origin 的 git 仓库；一轮解析 = rev-parse + remote get-url
= 2 个 git 子进程）。探测次数用 `GIT_TRACE`（每个 git 进程一行 `built-in:`）计数。

| 命令 | before git_detect | after git_detect | before seam | after seam | 探测数 before→after |
|------|------------------:|-----------------:|------------:|-----------:|--------------------:|
| get  | 49.991 ms | 9.263 ms | 9.100 ms | 9.086 ms | 2 → 0 |
| show | 49.714 ms | 9.326 ms | 8.715 ms | 8.957 ms | 2 → 0 |
| search | 91.075 ms | 55.728 ms | 10.016 ms | 9.848 ms | 4 → 2 |

- `seam` = `ASG_CURRENT_REPO=''` 既有测试注入（零探测基线）。
- search 保留一轮的合法成本（repo boost 契约不变）；get/show 回到 ~9ms 量级。
- 父任务基线（另一 checkout 路径，同 commit 5b232cd）：get 53.66 / show 52.44 /
  search 97.11（git_detect）对 9.15 / 8.97 / 9.98（seam）——本次复测同量级。
- before 二进制 sha256 `6d7ebe73…`；after 二进制 sha256 `912f45c8…`。

## 3. 回归测试（新增，无网络）

`crates/agent-session-grep-cli/tests/hotpath_repo_probe.rs`：临时 git 仓库
（git init + remote add origin，无网络）+ 合成 fixture，用 `GIT_TRACE` 计数。

- get / show / status → 断言 **0** 个 git 子进程（修复前各 2 个）。
- search（lexical）→ 断言**恰好 2** 个（一轮；修复前 4 个）。
- search --mode semantic（向量表空 → 显式 lexical_fallback）→ 断言恰好 2 个
  （修复前 semantic 分支同样每请求两轮）。
- 观测用真实 git（Windows 上 `Command::new("git")` 不解析 `.cmd` shim，实测），
  依赖与 `tests/e2e.rs` 既有 repo identity 测试一致。

## 4. 验证命令与结果（日志同目录）

| 命令 | 结果 | 日志 |
|------|------|------|
| 聚焦回归 `cargo test -p agent-session-grep-cli --test hotpath_repo_probe` | PASS | hotpath-test-focused.log |
| `cargo test -p agent-session-grep-cli --locked` | PASS（全 target） | cli-tests.log |
| `cargo test --workspace --locked` | PASS | workspace-tests.log |
| `node --test crates/agent-session-grep-cli/tests/web_ui.test.cjs` | PASS 11/11 | web-ui-tests.log |
| `cargo clippy -p agent-session-grep-cli --all-targets -- -D warnings` | 仅因**并行任务文件** `tests/invariant_resume_args.rs:9,12`（doc list item without indentation）失败；my 范围 scoped clippy 全绿 | clippy.log / clippy-scoped-hotpath.log |
| `cargo fmt --all --check` | 仅因并行任务文件（application/adapters-sqlite/cli 的 `invariant_*.rs`）失败；本任务 3 个文件 `rustfmt --check` 全绿 | fmt-all.log / fmt-scoped-hotpath.log |

说明：六项不变量自查任务（10-06-six-invariant-selfchecks）与本任务共享同一工作树、
同一 `agent-session-grep-cli` package，其未格式化/带 clippy warning 的在写文件导致
workspace 级 fmt/clippy 门暂时变红；本任务源文件与新增测试均已通过 scoped 检查。
并行任务落定后应重跑 workspace 级 fmt/clippy 门。

## 5. before/after 输出 diff

`output-diff.txt`：6 组（2 variant × get/show/search）JSON 逐字段比较，非易变字段
0 差异；唯一差异为 `meta.duration_ms`（get 41→2、show 46→1、search 89→48 ms），
hits/score/page/next_cursor/retrieval_mode/redaction/schema_version/outcome 全部一致。

原始数据：`before-measurements.json`、`after-measurements.json`、`measure.ps1`、
`output_diff.py`、`{before,after}-output-<variant>-<command>.json`。