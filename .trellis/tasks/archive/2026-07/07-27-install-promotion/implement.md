# Implement: Installation, smoke evidence, runbook, promotion

执行计划。设计（`design.md`）已冻结：文件所有权见 §1，脚本契约见 §2/§2.5/§3，
文档见 §4，CI 与门禁见 §5/§6。

## Phase A — 主会话预接线（先做）

本子任务不动 Rust 代码，无需 stub 接线。主会话先确认基线门禁绿
（`cargo fmt --all --check` / clippy / test / deny），作为"文档脚本改动未引入
回归"的对照基准。

## Phase B — 并行实现（4 agent，按 §1 所有权）

| agent | 产出 | 依据 |
|---|---|---|
| `install-scripts` | install/uninstall × 2 平台 | §2 |
| `smoke-scripts` | smoke × 2 平台 | §2.5 |
| `regression-harness` | 回归 harness + 单测 | §3 |
| `promotion-docs` | 3 份 runbook | §4 |

硬规则（每个 agent 的派工提示里必须重复）：

- **禁止任何 git 命令**（commit/push/add/stage 一律不许）。
- 只写自己名下的文件，不改他人文件，不改 Rust crate，不改 `deny.toml`。
- 写完全文回读核查留存（本仓库 Defender 会间歇删写入）。
- 脚本里禁止出现真实个人路径、主机名、用户名；夹具一律合成。
- 无 shell 工具的 agent 不得声称门禁通过——如实报告"未验证"。

## Phase C — 主会话集成与验证

1. 独立复核每份产出（读文件 + 对照契约表逐条核对，不采信 agent 自述）。
2. 本机实跑：`smoke.ps1` 全序列、install → `--version` → uninstall → 重复
   uninstall、install `--dry-run`。
3. 本机实跑回归 harness 单测（合成夹具）。
4. runbook 命令逐条与 `agentsessions --help` 比对，剔除不存在的命令。
5. 接线 `.github/workflows/ci.yml`（§5）。
6. 授权真实数据回归：本机跑一次，产出**只含聚合数字**的证据文件；提交前逐行
   人工核隐私。
7. 更新 `core-beta-evidence-matrix.md` 与 `PROVIDER-MATURITY-MATRIX.md`——
   状态如实，两 provider **保持 Experimental**。
8. 全量门禁 + spec 更新。

## Phase D — 收尾

提交计划 → 用户确认 → 分片提交 → push → 归档 → journal。父任务集成评审另开。
