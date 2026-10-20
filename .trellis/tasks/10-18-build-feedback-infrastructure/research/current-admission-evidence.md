> Historical v1 research from 2026-10-19. The approved task PRD/design and versioned implementation supersede its admission/N statements. Read-only observations below are historical, not current admission or new performance evidence.

> Code line numbers and defect findings below describe the research-date snapshot. Current implementation, fix status and validation are recorded in implement.md and task.json; do not redispatch these historical findings without checking the current code.

# Research: 当前磁盘准入证据与真实测量剩余条件

- Query: 流式读取改进能否解除当前 build-feedback Pilot/Formal 的运行阻塞；当前磁盘可用量及原有准入拒绝证据是什么。
- Scope: internal（当前任务、可执行预算代码、当前 Windows 只读元数据查询）。
- Date: 2026-10-19

## Findings

### 当前结论与观测

真实 benchmark 仍不能在当前 C 盘空闲量下运行。下一对试验预测增长 39 GiB，必须在增长后保留 80 GiB，故仅磁盘准入就要求至少 119 GiB 可用。固定初始 B 不重置、预测不降低、不能清理未批准或受保护缓存来绕过准入。

本次只读调用 Windows GetDiskFreeSpaceExW，查询可用于当前调用者的 available bytes：

```json
{
  "observed_at": "2026-10-19T12:20:38.726182+00:00",
  "available_bytes": 73229205504,
  "available_gib": 68.20001220703125,
  "required_gib": 119,
  "deficit_gib": 50.79998779296875,
  "no_build_started": true
}
```

这是 2026-10-19 20:20:38（Asia/Shanghai）的一次观察。当前空闲本身已低于 80 GiB；若再扣除 39 GiB 预测，只有约 29.20 GiB。既有 task.json/文档中的约 76.04 GiB 是先前观测，不能覆盖这次新的只读结果；两者均不满足 119 GiB。

没有做全目录扫描，故本次不生成/声称完整 current-allocation inventory；当前相关缓存总分配量、project headroom、外部并发增长与完整根集合仍须在以后真实准入前重新核验。

### Files found

| 文件 | 说明 |
| --- | --- |
| .trellis/tasks/10-18-build-feedback-infrastructure/prd.md | 固定 B、B+40 GiB 评审/B+60 GiB 停止、80 GiB 最低空闲、保护 72h 等已批准契约。 |
| .trellis/tasks/10-18-build-feedback-infrastructure/task.json | B=156436568120 bytes、原冻结 commit/tree、0/5 与 0/100、原阻塞记录及 127 测试检查点。 |
| .trellis/tasks/10-18-build-feedback-infrastructure/implement.md | 完整 116 根、217450 文件身份、error0 的既有初始 inventory；首次并发漂移与随后磁盘拒绝。 |
| scripts/evidence/build_feedback_runner.py | 前置快速 C 盘查询、不可变拒绝 receipt、完整 preview 与每对前复核。 |
| scripts/evidence/build_feedback_inventory.py | 当前/预测分配量与 free-space 的固定 B 预算公式。 |
| scripts/evidence/test_build_feedback_runner.py | 快速磁盘拒绝须先于 inventory 与 target 创建的现有回归。 |
| docs/operations/BUILD-FEEDBACK.md | 39 GiB 预测、113.249/76.04 GiB 历史空闲观测、未执行实测的公开事实。 |
| research/unblock-streaming-evidence.md | 日志后处理的内存优化机会与可复现 Cargo 解析分歧；独立于磁盘准入。 |

### Code patterns 与证据链

- scripts/evidence/build_feedback_runner.py:436 的 _free_bytes 使用 GetDiskFreeSpaceExW 的 available.value，查询 C 盘，未知/调用失败会拒绝；本次只读查询使用同一 API/available 口径。
- scripts/evidence/build_feedback_runner.py:1678 初始化 run 前先确认 quiet 状态，:1680 读空闲量，:1681 判定 quick_free - predicted_growth < 80*GiB，:1682 写不可变 admission-disk-refused receipt，然后拒绝。完整 inventory、run_root 创建和 Cargo 启动均在此后。
- scripts/evidence/test_build_feedback_runner.py:613 覆盖 min_free + forecast - 1 byte 边界；_budget_sample 被设置为调用即报错以证明先拒绝，:621 断言 run_root 不存在，:623 断言唯一拒绝 receipt。该测试存在，但本研究未重新运行。
- scripts/evidence/build_feedback_inventory.py:890 计算 projected=current+forecast 与 projected_free=free-forecast；:895 判断 B+40 GiB 评审；:897 判断 B+60 GiB 已达/预测达到停；:901 判断空闲不足；:908 允许仅在 projected < hard 且 projected_free >= 80 GiB 时成立。恰好 80 GiB 可接受，恰好 B+60 GiB 停止。
- scripts/evidence/build_feedback_runner.py:449 在完整 inventory 的 schema/complete/errors 不符合时拒绝，:464 起核对初始 root/store 范围没有缩小。磁盘满足只是必要条件。
- docs/operations/BUILD-FEEDBACK.md:334 记载固定 B=145.6929 GiB，:340 说明 39 GiB 是保守预测而非已测 Pilot peak，:342 说明 113.249 GiB 历史样本被拒绝，:346 记载先前约 76.04 GiB，:351 明确 Pilot0/5、Formal0/100。

### 已有证据与尚缺证据

| 类型 | 当前有的证据 | 不能由它推出的结论 |
| --- | --- | --- |
| 工具实现质量 | task.json/implement.md 记载 Ruff、Mypy、127 测试、格式检查及独立 review 检查点。 | 已完成真实 benchmark、收益达到采用阈值。 |
| 官方 full validator 兼容性 | 授权冻结 validator、合成完整报告与 20/20/100/100/100 样本抽取 smoke。 | 实际 release 速度、真实 paired P95 或正式采用资格。 |
| 初始 inventory | 固定 B=156436568120 allocated bytes；既有完整身份去重扫描。 | 当前 allocation 仍相同、可以重设 B。 |
| 磁盘拒绝 | 原 113.249/76.04 GiB 样本与新 68.2000 GiB API 观察。 | 自动清理授权、可降低 forecast 或忽略 80 GiB。 |
| 流式基础机制 | 纯内存块哈希/跨块内存失败标记 smoke，Cargo 解析分歧复现。 | 峰值磁盘需求已减少、39 GiB 预测被新测量替代。 |
| 真实 campaign | 当前 task 元数据仍是 Pilot0/5、Formal0/100。 | 可把 smoke、拒绝或时序不完整的试验计入有效对数。 |

以后若外部状态变化，只能在保持 B 和协议身份的前提下重新预检当前空闲、完整 inventory、quiet 状态、MSVC/工具链与源身份、目标分配、进程捕获和预热证据。后续 cache reservation 需完整 Pilot peak P 才能推导 ceil(1.25*P+1) GiB；本次没有 P。

### External references（docs / versions）

- 本次只读 API 观测直接采用 runner 已实现并批准的 Windows 调用，未作新的外部 Windows API 研究或工具选型。
- 相关既有外部约束已在本 task/research/verified-build-tool-boundaries.md 记录（2026-10-18，Windows 分配量、JobObject commit-memory，MBX1.24.1/sccache0.18.0）。本文件不把那些历史 docs 当成当前 free-space 测量。
- 当前工具链版本契约为 rustc/cargo1.97.1/MSVC，本次没有启动 Cargo/rustc 或 cache tool，不能替代新的 segment toolchain receipt。

### Related specs

- .trellis/workflow.md 的持久化与角色隔离要求；只写当前 task/research。
- .trellis/tasks/10-18-build-feedback-infrastructure/prd.md 的 Accepted contracts 与 Required delivery 是本预算的直接授权依据。
- .trellis/tasks/10-18-build-feedback-infrastructure/design.md:14 要求实际分配量、固定 B、外部写入占用余量、时间段内禁止目录遍历和失败证据保留。
- .trellis/spec/guides/cross-layer-thinking-guide.md 要求区分来源、变换与消费边界；free-space 观测、完整 inventory、预算决定和正式资格是不同证据。
- .trellis/spec/agentsessions-cli/backend/quality-guidelines.md 尚为占位，没有可替代当前任务合同的 build-feedback 准入规范。

## Caveats / Not Found

- 派发正文为空，范围由 unblock_streaming_evidence 名称与当前 active task 推断；此文件负责说明内存读取优化与真实运行前提的关系。
- 可用空间是瞬时观察，会受其他窗口影响。新值不替代不可变初始 B，也不应直接写回 task.json 作为永久状态；本研究角色没有修改元数据权限。
- 本次未重新扫全部 roots/stores，没有正式准入 receipt、没有运行 Pilot 或 Formal、没有删除/回收缓存/目标、没有执行任何 Git 操作。
- 39 GiB 预测来自已冻结当前协议/任务记录；本研究未读取机器私有 protocol 及源 receipts，也未确认其当前 SHA-256。父代理真实执行仍须核对实际 protocol。
- 无新的端到端性能/内存/存储测量，不存在采用依据。当前只有研究交付完成，整个 build-feedback campaign 尚未完成。
