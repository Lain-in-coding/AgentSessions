# Design: Installation, smoke evidence, runbook, promotion

本文件是本子任务的**冻结技术设计**：文件所有权、脚本契约、报告 schema、
文档改动点。并行 agent 只写自己名下的文件，不改他人文件。

## §0 关键决策与偏差（review 可否决）

### 0.1 安装脚本 = 从源码构建 + 拷贝到用户级目录，不碰 PATH

无签名、无发布渠道（`CB-SIGNING-001` / `CB-NOTARIZATION-001` 仍
`externally_blocked`），因此 v0 安装脚本**只做**：检查 toolchain →
`cargo build --locked --release` → 拷贝二进制到用户级 bin 目录 → 打印如何把
该目录加入 PATH。

**脚本不修改 PATH、不写注册表、不装 shell profile、不 sudo、不下载任何东西。**
理由：修改用户 shell 配置是不可逆的环境改动，属"外部影响"；打印指令让用户自己
决定。这也让 uninstall 只需删一个文件。

目标目录：Windows `%LOCALAPPDATA%\AgentSessions\bin`，Unix
`${XDG_BIN_HOME:-$HOME/.local/bin}`。与 `config paths` 报告的 data/cache/logs
同族但独立（bin 不是数据）。

### 0.2 clean-machine 证据的诚实边界

GitHub hosted runner **不是** clean machine：镜像预装 Rust、Python、构建工具。
新增 CI job 能证明的是"在预装 toolchain 的干净工作目录上，安装脚本可跑通且装出
的二进制可执行"。因此新证据行的 claim 措辞必须是
**installer-script smoke**，不是 clean-machine installation。

`CB-CLEAN-MACHINE-*` 这类主张继续留在 `externally_blocked`：需要无 Rust 的裸机
或容器 + 预构建产物，本子任务不引入。

### 0.3 真实数据回归：本地专用，产出只含聚合数字

授权范围仅"数据留在本机"。因此回归 harness：

- 只读用户自己的 data root / transcript 目录（provider 只读边界不变）；
- 报告**只含聚合计数与不变量校验结果**（会话数、消息数、span 覆盖率、
  role 分布、parse 失败数），**不含任何消息文本、路径、native id、指纹**；
- 报告默认写到 `evidence-output/`（已 gitignore），**脚本不提交、不上传**；
- 仓库里提交的是 harness + 其单测 + 一份**合成夹具**跑出的示例报告，
  真实报告不入库。

这是 `PROVIDER-MATURITY-MATRIX.md` 缺口 4「可重复流程」的达成方式：流程与
校验器可审计，数据集不外传。它**不**把 provider 晋级为 Beta——缺口 5（三平台 CI
认证本次 PR 跑绿）仍未闭合，两 provider 保持 Experimental。

### 0.4 不做的事

不签名、不公证、不发布 release、不写 Homebrew/winget/scoop 清单、不建 Docker
镜像、不改 `deny.toml`、不把任何 R0 治理记录标 Accepted。

## §1 文件所有权（并行边界）

| Owner | 文件 | 说明 |
|---|---|---|
| agent `install-scripts` | `scripts/install/install.ps1`（新）<br>`scripts/install/install.sh`（新）<br>`scripts/install/uninstall.ps1`（新）<br>`scripts/install/uninstall.sh`（新） | 安装/卸载脚本（PRD R1），契约见 §2 |
| agent `smoke-scripts` | `scripts/install/smoke.ps1`（新）<br>`scripts/install/smoke.sh`（新） | 端到端 smoke（PRD R2），契约见 §2.5 |
| agent `regression-harness` | `scripts/evidence/real_data_regression.py`（新）<br>`scripts/evidence/test_real_data_regression.py`（新） | 回归 harness + 单测（PRD R5），契约见 §3 |
| agent `promotion-docs` | `docs/operations/INSTALL-AND-UPGRADE.md`（新）<br>`docs/operations/REAL-DATA-REGRESSION.md`（新）<br>`docs/operations/rebuild-and-migration-runbook.md`（新） | 三份 runbook（PRD R4），见 §4 |
| 主会话 | `.github/workflows/ci.yml`（改）<br>`docs/operations/core-beta-evidence-matrix.md`（改）<br>`docs/product/PROVIDER-MATURITY-MATRIX.md`（改）<br>`docs/evidence/integration-beta/real-data-regression.md`（新）<br>`.trellis/spec/**` | CI 接线、证据台账、成熟度（PRD R3/R6）、本机实跑证据、spec |

无人改 Rust crate：本子任务不动产品代码。`migration-v5-to-v6.md` 不改——
新 runbook 链接它，不重写它（单一事实源）。

路径以本表为准：PRD 早期草稿里的扁平路径（`scripts/install.ps1` 等）已被本表
的目录化路径取代，PRD 已同步。

## §2 安装脚本契约（agent `install-scripts`）

两平台脚本行为对齐。**任何一步失败即非零退出并打印可操作诊断，绝不静默继续。**

步骤：

1. 定位仓库根（脚本位置的 `../..`），确认 `Cargo.toml` 在场；否则报错退出。
2. 检查 `cargo` 可用（`cargo --version`）；缺失时打印 rustup.rs 指引并退出 1。
3. `cargo build --locked --release -p agentsessions-cli`（`--locked` 保证复现）。
4. 计算产物 SHA-256，创建目标 bin 目录（存在则复用），拷贝二进制覆盖同名旧文件。
5. 运行装好的二进制 `--version` 自检；失败即退出非零。
6. 打印：安装路径、SHA-256、版本、以及"如何把该目录加入 PATH"的平台化一行指令。

参数：

- `-Prefix <path>` / `--prefix <path>`：覆盖目标 bin 目录。
- `-SkipBuild` / `--skip-build`：跳过构建，只拷贝已存在的 release 产物（CI 用，
  产物缺失则报错）。
- `-DryRun` / `--dry-run`：只打印将要做的事，不写任何文件。

uninstall：删除目标目录下的 `agentsessions[.exe]`，报告删除结果；文件不存在时
**退出 0 并说明"未安装"**（幂等）。**绝不递归删目录**，只删这一个文件。

约束：PowerShell 脚本用 `$ErrorActionPreference = 'Stop'` + `Set-StrictMode`；
bash 用 `set -euo pipefail`；两者都不引第三方工具（无 curl/jq/git 依赖）。

## §2.5 Smoke 脚本契约（agent `smoke-scripts`）

`scripts/install/smoke.ps1` / `smoke.sh`：用**已构建的真实二进制**在抛弃式临时
目录上跑一遍全部对外表面，任一断言失败即非零退出并打印实际输出。

参数：`-Binary <path>` / `--binary <path>`（缺省用
`target/release/agentsessions[.exe]`，不存在则报错退出，**脚本自己不构建**——
构建是 install 脚本/CI 的职责，smoke 只验证成品）。

夹具：脚本内联生成**合成** Claude Code 格式 `.jsonl`（3 条消息：root → reply →
sidechain probe，含固定检索词），写入临时目录。禁止读取用户真实数据。

断言序列（全部走 `--robot`，逐步校验 exit code 与 envelope 字段）：

| 步骤 | 断言 |
|---|---|
| `doctor --db <tmp>` | exit 0，`data.db == "ok"`，`data.schema` 为数字 |
| `sync <fixture>` | exit 0，`ok:true`，`data.messages == 3` |
| `search <term>` | exit 0，`data.hits` 非空，首命中 id 以 `msg_v1_` 开头 |
| `context <ses-id>` | exit 0，`data.messages` 非空，`data.evidence` 非空 |
| `status` | exit 0，`data.catalog_count >= 5`（3 消息 + 会话 + 文档） |
| `get <hit-id>` | exit 0，`data.payload` 非空 |
| `get <合法但不存在的 id>` | **exit 0**，`data.payload == null`（见下） |
| `context <合法但不存在的会话>` | **exit 4**，`error.code == "not_found"` |
| `search x --cursor garbage` | **exit 2**，`error.code == "cursor_invalid"` |
| MCP stdio | 喂 initialize → notifications/initialized → tools/list → 一次
`tools/call get_status`；断言 exit 0、stdout 每行都是合法 JSON、`result.tools`
恰 6 个、tools/call 回 `isError:false` |

会话 id 从 `sync`/`search` 的真实输出中提取（解析 JSON），**不得硬编码**。
结束时删除临时目录（失败路径也删，但先打印诊断）。

约束：PowerShell 用 `$ErrorActionPreference='Stop'` + `Set-StrictMode -Version
Latest`，JSON 用 `ConvertFrom-Json`；bash 用 `set -euo pipefail`，JSON 用
`python3 -c`（仓库 CI 已装 Python；不引 jq）。两脚本断言集必须一致。

## §3 回归 harness 契约（agent `regression-harness`）

`python scripts/evidence/real_data_regression.py --binary <exe> --sources <dir|file>... [--out <path>] [--json]`

流程：

1. 递归收集 `.jsonl` 源文件（可给多个 `--sources`）；空集合报错退出 2。
2. 建**临时** data root（`tempfile.mkdtemp()`，结束删除），对全部源跑一次
   `sync`（robot 模式，解析 envelope）。
3. 跑 `status`、`doctor` 取 catalog 计数/generation/schema/interrupted。
4. 对每个会话实体跑 `context --policy mainline`，累计不变量校验。
5. 跑 `index rebuild` 后再次 `status` + 抽样 `search`，校验重建不变量。
6. 输出报告（见下）；**任一不变量失败 → 退出码 1** 且报告标 `failed`。

不变量（全部只用聚合数字判定）：

| id | 断言 |
|---|---|
| `INV-SYNC-OK` | sync envelope `ok:true`，`data.messages > 0` |
| `INV-NO-PARSE-LOSS` | provider 报告的消息数 == catalog 中 `msg_v1_` 计数 |
| `INV-SESSION-PRESENT` | `ses_v1_` 实体数 >= 1 且 <= 源文件数 |
| `INV-CONTEXT-NONEMPTY` | 每个会话 context 至少 1 条消息，无 `internal` 错误 |
| `INV-SPAN-COVERAGE` | `precision == "byte"` 的证据占比 == 1.0（新 ingest 全应有 span） |
| `INV-REBUILD-STABLE` | rebuild 前后 catalog 计数一致，抽样 search 命中数一致 |

报告 schema（JSON；Markdown 版为其人读投影）：

```json
{
  "schema_version": "1.0",
  "kind": "real-data-regression",
  "generated_at_utc": "2026-07-27T00:00:00Z",
  "binary": { "path_basename": "agentsessions.exe", "version": "0.1.0", "sha256": "..." },
  "environment": { "os": "Windows", "release": "...", "python": "3.10.x" },
  "corpus": { "source_files": 12, "total_bytes": 3456789 },
  "totals": { "messages": 1234, "sessions": 12, "documents": 12, "catalog_entities": 1258 },
  "role_distribution": { "user": 600, "assistant": 620, "system": 14 },
  "evidence_precision": { "byte": 1234, "line": 0, "record": 0, "unknown": 0 },
  "invariants": [ { "id": "INV-SYNC-OK", "passed": true, "detail": "12 sources, 1234 messages" } ],
  "outcome": "passed"
}
```

**隐私硬约束（review 重点）**：报告里只允许上表字段。禁止出现源路径（只留
basename 且仅限二进制自身）、消息文本、native id、指纹、用户名、主机名。
harness 必须有一个单测断言"报告序列化后不含任何输入语料的文本片段"。

单测（`test_real_data_regression.py`，不需要真实数据）：合成 2 个 Claude 格式
`.jsonl` 夹具 → 用真实二进制（`--binary` 由环境变量或 `cargo build` 后路径传入；
缺二进制时 `skip`）跑完整流程 → 断言 `outcome == passed`、6 条不变量齐全、
报告不含语料文本、`--dry-run` 不写文件。纯函数（报告构造、不变量判定）单测
不依赖二进制。

## §4 文档（agent `promotion-docs`）

`docs/operations/INSTALL-AND-UPGRADE.md`：从源码安装（三平台）、参数、升级
（重跑 install 覆盖）、卸载、PATH 自助配置、常见失败（无 cargo / 构建失败 /
目标目录不可写）、以及**明确声明**产物未签名未公证、非 clean-machine 认证。

`docs/operations/REAL-DATA-REGRESSION.md`：为什么本地跑（授权边界）、前置、
命令示例、6 条不变量语义、报告字段含义、隐私保证（报告只含聚合数字、默认写
gitignore 目录、不要提交真实报告）、失败怎么读。

两份文档都不得声称任何签名/发布/最小 OS 认证；措辞与 §0.2 边界一致。

## §5 CI 接线（主会话）

在 `.github/workflows/ci.yml` 增加 job `installer`（矩阵 ubuntu/windows/macos）：

1. checkout + stable toolchain；
2. `cargo build --locked --release -p agentsessions-cli`；
3. 跑安装脚本（`--skip-build`，`--prefix` 指向工作区临时目录）；
4. 直接调用装好的二进制：`--version`、robot `config paths`；
5. 跑 **smoke 脚本**（`--binary` 指向装好的二进制）——这是 PRD R3 的三平台可重复
   证据载体，覆盖 §2.5 全部断言（含 MCP 握手与错误码）；
6. 跑 uninstall 脚本，断言二进制已消失且**重复 uninstall 仍退出 0**（幂等）；
7. 跑 `python scripts/evidence/test_real_data_regression.py`（合成夹具路径）。

Windows job 走 `pwsh` 调 `.ps1`，Ubuntu/macOS job 走 `bash` 调 `.sh`；两条路径
断言集相同（§2.5），因此三平台结论可比。job 名与证据行 ID 一一对应，便于台账
引用。不上传产物、不签名。

## §6 验收门禁（主会话）

- `cargo fmt --all --check` / `cargo clippy --workspace --all-targets -- -D warnings`
  / `cargo test --workspace` / `cargo deny check` 全绿（本子任务不改 Rust，
  仍需确认无回归）。
- 本机实跑：install → `--version` → uninstall → 重复 uninstall（幂等）→
  install `--dry-run` 不落文件。三平台里至少 Windows 本机实跑，Unix 脚本靠 CI。
- 本机实跑 `smoke.ps1` 全序列退出 0（PRD 验收第 1 条）。
- 本机实跑回归 harness 单测（合成夹具）。
- runbook 逐条核对：它提到的每个命令都必须在当前 CLI 的 `--help` 里存在
  （PRD 验收「no invented commands」），主会话逐一比对，不接受"看起来合理"。
- 证据台账新增行状态如实（`locally_verified` 仅限本机实跑项；CI 项在 PR 跑绿
  并记录 run id 前只能是 `ci_configured_only`）。
