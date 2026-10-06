# Research: ASG 候选基线对账独立证据复核

- Query: 限定核对新备忘录的重复 repo 解析、manifest 摘要化与终态增长、no-op/实验数字与可比性、provider 计数、历史测试/CI 归属。
- Scope: internal；planning-only 独立证据审阅，不是 implementation/check 生命周期，也不是完整产品审查。
- Date: 2026-10-06
- 结论：**在已复核范围内未发现实质性错误（no material errors found in reviewed scope）**。有一项低严重度、非阻塞的计数措辞建议。

## Findings

### 基线及审阅输入

- A = baseline-workspace，任务指定提交 `5b232cdedbff33a251e7fb266558be7af8496e1a`；B = remediation-worktree，任务指定提交 `6a48640629c7c7f9d7682d77d9ad5cf4d0402581`。以下源码均直接读取 **B**；未用 A 的 CodeGraph 代替。未独立执行 Git 验证提交、祖先或工作树状态。
- 研究文件路径相对于本任务目录；源码及规范路径相对于标注工作区根目录。
- `research/asg-baseline-reconciliation-20261006.md:12-17,26-34,40-81,93-98`：本次被复核的结论、数字和证据边界。
- `research/asg-candidate-journal-growth-20261006.json:1-373`：解析全部 21 个 samples 和 unchanged_source_control；数值主锚点为 `:19-32,339-371`。
- `research/asg-candidate-provider-matrix-20261006.json:1-280,305-311`：解析 16 行 provider 和成功响应标记。两份 JSON 的 commit、binary_sha256 一致；这是记录间一致性，不是重新鉴定二进制。

### 1. CLI 重复上下文解析与合法排序：结论成立

- B `crates/agent-session-grep-cli/src/lib.rs:2098-2105,2158-2169,3140-3181`：先构造 `resume_app` 取时钟，再构造 `resume_semantic_app` 执行搜索；两者都解析 current repo，每次创建新的 resolver，实例内缓存不能跨这两次构造共享。
- B `crates/agent-session-grep-cli/src/repo_identity.rs:31-54,132-168`：每个新 resolver 在成功路径执行两条 Git 命令。因此备忘录对这两次解析合计四个子进程的成功路径限定有源码依据；本审阅未动态计数或计时。
- B `crates/agent-session-grep-application/src/lib.rs:1940-2002` 与 `crates/agent-session-grep-application/src/ranking.rs:36-44,67-78`：repo 命中确实参与 lexical 排序加分，不能把全部 repo 解析删掉当作无语义变化的优化。
- B `crates/agent-session-grep-cli/src/lib.rs:2380-2408` 对照 `crates/agent-session-grep-application/src/lib.rs:2129-2135`：get/show 使用带 repo 的 factory，但相应 App 分支只取 catalog payload；备忘录所述不必要探测成立。

**低严重度措辞建议，非实质性错误：**备忘录 `:34` 的“无 origin 会提前退出”不能用来推断少于四次启动。有效工作树但 origin 不存在时，每个新 resolver 仍可先执行 rev-parse，再执行失败的 remote 查询，随后返回 None。建议澄清“非 Git 可在第一条命令后退出；无 origin 返回 None，但两次解析仍可能合计四条命令”。不改变重复解析的主结论，也不要求当前重写备忘录。

### 2. Hash manifest 与终态元数据增长：拆分正确

- B `crates/agent-session-grep-adapters-sqlite/src/lib.rs:710-781`：内存 projection 保留原 observation；写入 canonical manifest 的 projection descriptor 是 typed identity 加 payload/text 的 BLAKE3 摘要，不是原正文副本。不能把这概括为所有持久化原始证据都已删除。
- 同文件 `:6411-6464,7049-7073,7692-7709`：写入六个 JSON manifest 列，校验后将批次标记 activated；该激活 SQL 不清空这些列。`:8216-8257` 的恢复入口处理 building，并提供诊断读取；不是终态历史 GC 的证据。
- 同文件 `:12845-12944`：确实存在两项 `source_projection_manifest_` 测试，分别断言两种正文长度的 descriptor 大小相同，以及 identity/payload/text 改动影响摘要并拒绝不一致提交。**父会话报告两项通过；本审阅仅核对源码，未独立重跑。**
- 原始 JSON 记录 21 个 activated 批次，与新 descriptor 不再存正文不矛盾。结论仅覆盖所给新库样本，不能推出历史旧库已清除正文、所有消费者已审完或可直接执行 GC。

### 3. 数字、no-op 与跨基线可比性：未见错抄或夸大

| JSON 记录指标 | 第 1 次 | 第 21 次 |
|---|---:|---:|
| 源字节数 | 56,126 | 56,126 |
| catalog 行数 | 202 | 202 |
| activated 批次 | 1 | 21 |
| 六列 JSON 字符数 | 257,427 | 6,064,807 |
| 六列 JSON UTF-8 字节数 | 257,427 | 6,064,807 |
| DB + 当时存在的 sidecars 字节 | 1,830,912 | 7,737,344 |

全部 21 条记录的 sync_number 连续、源大小和 catalog 行数固定、source_sha256 各不相同，正文标记检查均为 false；表中数字与备忘录 `:48-56` 一致。no-op 控制 `:361-371` 记录的 catalog=202、activated=21、manifest 字符数=6,064,807 均与末轮相同；不能泛化成“每次 sync 都增加批次”。

备忘录 `:60` 明确排除了 A/B 百分比排名和任意规模外推，与 JSON limitations `:355-359` 一致。A 的 56,320 字节及两轮 schema/路径/构建差异未在本限定审阅中重新取证；不得把本报告当作这些历史细节的新实测凭证。

### 4. Provider 行数与能力等级：分母正确

重新聚合原始 frame：共 **16 行**；其中 **14 行 parse=native 且 maturity=experimental**，另 **2 行 parse=unknown 且 maturity=unsupported**。仅在前述 14 行内，resume 为 **8 derived / 3 unknown / 3 unsupported**，tool_activity 为 **2 partial / 12 unsupported**；Aider、Cursor 的 discover 均为 unsupported。与备忘录 `:30` 完全一致。

`maturity_target` 不等于现有成熟度；native parse、derived resume 和测试存在都不是完整上下文、原生执行或跨版本认证证据。本审阅没有运行 provider。

### 5. 历史测试/CI 与本轮证据归属：区分保留

备忘录 `:15,26,28,60,75-81,90` 没有把 A 的性能/质量数字、旧全量测试或历史 CI 改写成 B 的本轮结果；本轮构建、两项定向测试与未执行的全 workspace/跨平台/安装等明确分开。构建和测试通过仍归属父会话执行说明，不是本审阅独立复跑，也不是完整质量门禁。

这里只核对表述归属；不验证备忘录所述联网请求或其失败原因，不形成任何当前远端、CI、PR、release 或可见性判断。

### Related Specs

已读取 A 的 `.trellis/workflow.md:331-380`（planning/research 与研究落盘），以及以下适用合同；它们用于解释证据边界，未作为 B 实现源码的替代：

- `.trellis/spec/agentsessions-application/backend/index.md:125-160`：lexical repo 排序与强相关性优先。
- `.trellis/spec/agentsessions-cli/backend/index.md:212-227`：计数分母、实验成熟度及 CI/发布证据诚实边界。
- `.trellis/spec/agentsessions-adapters-sqlite/backend/index.md:41-55,295-313`：封存校验、building 诊断、no-op 与恢复合同。

### External References / Versions

无新增外部检索或第三方文档结论。版本锚定上列任务指定 A/B 提交及 JSON 记录；不是在线版本核验。

## Caveats / Not Found

- 仅解析提交的原始合成 JSON 和读取上述 B 源码锚点；未重新运行合成实验、打开实验数据库或复算源文件。JSON 是本轮提供的观测证据，不是本审阅新生成的测量。
- no-op 控制只记录上述三个对照字段，没有控制后 source_sha256、manifest UTF-8 字节数或磁盘快照；不声称这些未记录字段也经过独立验证。
- 未执行 Git、Cargo、构建、安装、resume、模型或生命周期命令；未修改产品、规范、运行态、共享规划/coverage 或其它研究文件。
- 未覆盖其余竞品事实、八项之外风险、全部 B 改动、消费者全集、迁移或全量产品质量；没有实施或发布批准。
- 复核前后以下输入 SHA-256 均未变化，结论对应这些快照；并发会话之后若更新输入，应重新核对受影响锚点：
  - memo：`31b4023404a9ccc15a46da14aaad46f2b33ac29eb10077c09f3d7048995d495c`
  - journal JSON：`c21cfe46c87c3128711f721849373f14a642a6fd4c6938e02711d475b1ea5f5f`
  - provider JSON：`f74c4a2eb46174eba1e37e2790b79564df01798f4877afc99dae043a0a29ca79`
