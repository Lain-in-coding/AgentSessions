# Implement: Cross-source session identity

执行计划。设计（`design.md`）已冻结：路线取舍见 §0.1，会话定义见 §0.2，
payload 形状见 §0.3，文件所有权见 §1，各自契约见 §2/§3/§4。

## Phase A — 主会话预接线

无需 stub：两个 agent 各改一个既有文件，编译面互不依赖。主会话先确认基线
门禁绿，作为"改动未引入回归"的对照基准。

## Phase B — 并行实现（2 agent，按 §1 所有权）

| agent | 产出 | 依据 |
|---|---|---|
| `merge-commit` | `adapters-sqlite/src/lib.rs` 容器合并 + 9 条单测 | §2 |
| `session-entries` | `cli/src/main.rs` 会话 payload 形状 | §3 |

硬规则（每个 agent 的派工提示里必须重复）：

- **禁止任何 git 命令**（commit/push/add/stage 一律不许）。
- 只改自己名下的**那一个文件**，不改他人文件，不改 schema，不改端口 trait。
- 写完全文回读核查留存（本仓库 Defender 会间歇删写入）。
- 不得为了让测试通过而放宽既有断言；发现设计与实现不符时**报告**，不擅自改契约。
- 无 shell 工具时不得声称门禁通过——如实报告"未验证"。

`session-entries` 的改动极小（一处 json! 宏），可先完成；`merge-commit` 是主体。

## Phase C — 主会话集成与验证

1. 独立复核两份产出（读代码 + 对照 §2/§3 逐条核对，不采信 agent 自述）。
2. 重点验第 9 条单测语义：跨批次合并以**库中现值**为起点（§2 末段）。
3. 写 §4 的三条 e2e，确认既有 `document`/`messages` 断言未被放宽。
4. 全量门禁（fmt / clippy / test / deny）。
5. **真实语料回归复跑**：707 文件跑 harness，看 INV-SYNC-OK 是否转绿。
6. 已存库兼容性实跑：用旧形状会话行的库验证 show/context 不报错。
7. 按复跑实况更新证据文件、成熟度矩阵、spec——不预先声称通过。

## Phase D — 收尾

提交计划 → 用户确认 → 分片提交 → push → 归档 → journal。
父任务集成评审在此之后。
