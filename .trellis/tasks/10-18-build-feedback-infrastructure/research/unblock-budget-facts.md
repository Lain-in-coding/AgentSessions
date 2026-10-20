> Historical v1 research from 2026-10-19. The approved task PRD/design and versioned implementation supersede its admission/N statements. Read-only observations below are historical, not current admission or new performance evidence.

> Code line numbers and defect findings below describe the research-date snapshot. Current implementation, fix status and validation are recorded in implement.md and task.json; do not redispatch these historical findings without checking the current code.

# Research: 构建实验预算阻塞的已核实事实

- Query: 核实当前 Windows 构建反馈实验为什么无法入场，固定 B、39 GiB 下一对预留量、80 GiB 空闲底线分别来自哪里，以及满足什么条件才能重新判断入场。
- Scope: internal（任务合同、当前代码、已有本机回执及只读磁盘观测）；不修改代码、配置、任务状态或缓存，不执行 Git、Cargo 构建、安装或清理。
- Date: 2026-10-19

## Findings

### 结论

现有 pilot 协议仍被磁盘预算拒绝。协议预留下一对实验增长 **39 GiB**，C 盘必须在预留后剩余 **80 GiB**，因此最低启动空闲量是 **119 GiB = 127775277056 bytes**。2026-10-19 20:24:33.982991 +08:00 的只读观测为 **72870166528 bytes = 67.865631 GiB**，距启动门槛差 **54905110528 bytes = 51.134369 GiB**。这个观测低于 80 GiB 底线，即使不计本实验的增长也不满足要求。

这次研究没有启动实验。当前回执对应的实验输出根不存在，没有 campaign.json；任务的 pilot **0/5**、formal **0/100** 仍是当前记录。磁盘余量不足已经足够拒绝入场，但不能据此声称其他过程捕获、源身份或预算条件已经重新通过。

### 已核实的固定基线与协议

| 项目 | 值 | 证据 |
| --- | --- | --- |
| 固定 B | 156436568120 allocated bytes = 145.6929073855281 GiB | fixed-baseline.json 与完整 inventory 回执、协议 baseline 字段一致 |
| 基线清点 | 116 roots、64 caches、217450 file identities、error 0、complete true | inventory-corrected-001.json；扫描区间 2026-10-18T15:43:55.222907Z 至 15:46:14.035454Z |
| 基线 SHA-256 | 7b0460045550c3f121673af96fb309fa3886049ca99d9b2881b7bbe23ab5eb6a | 对原始 inventory 回执重新算哈希，与冻结回执及协议引用相符 |
| 下一对预测增长 G | 41875931136 bytes = 39 GiB | pilot-native-protocol.json 的 predicted_growth_bytes |
| 阶段 | pilot、固定 5 pairs | 当前协议；pilot 字段为 null，没有实测 pilot 峰值 |
| baseline arm | native / full debug / incremental on / isolated | 当前协议 configs |
| candidate arm | native / line-tables-only / incremental on / 3lane | 当前协议 configs |
| 当前预分配 | 1 组 source allocations，pair_index 0、attempt 0 | 当前协议；不是五对均已准备完成的证明 |
| 协议规范化 SHA-256 | 2b97ed4149981d378daf22666579c837315104f2d2c3162f3b8ab57592f53ea4 | 按 runner 的 canonical_sha256 规则重算，与旧 preview 和 disk-refused 回执相符 |

39 GiB 是协调者协议中显式提供的**保守启动预测**，不是实测 pilot target 峰值，也不是缓存 store 的实测预留额。当前 pair 两个 arm 都是 native，没有 MBX/sccache 性能候选；不能把变更后的两 lane/profile 参数解释成缓存工具已经通过资格验证。

### 当前代码如何计算入场

设 A 为新完整清点中的关联缓存 allocated bytes，F 为新磁盘空闲观测，G 为该协议预测增长，GiB = 1024^3。

- 项目硬限制：`A + G < B + 60 GiB`。等于硬限制也拒绝。
- 磁盘限制：`F - G >= 80 GiB`。恰好剩余 80 GiB 可通过这一单项条件。
- 预算复核：`A + G >= B + 40 GiB` 时必须有覆盖该协议、固定基线及整批次的协调者复核回执；这不能豁免硬限制。
- 当前 G = 39 GiB，因此无复核入场要求 `A - B < 1 GiB`；项目硬条件要求 `A - B < 21 GiB`。
- B+40 GiB 为 **199386241080 bytes**；B+60 GiB 为 **220861077560 bytes**。这些是相对冻结 B 的增长门槛，不是 40/60 GiB 的总缓存目标。
- `run_protocol` 在创建 run root 之前做快速磁盘拒绝。通过快速检查后才进行完整库存复核、源/协议验证、复核回执检查及实际捕获。
- `preview_protocol` 检查初始 roots/store 范围没有缩小；有未知或不完整清点时拒绝。读取过去的 preview 并不能代替重新计算当前条件。

代码引用：

- `scripts/evidence/build_feedback_inventory.py:874` — budget_status；整数/未知值拒绝，项目与磁盘双约束、软复核及硬限制的边界。
- `scripts/evidence/build_feedback_inventory.py:765` — inventory；全局文件身份去重，complete=false 不能作为入场或固定基线。
- `scripts/evidence/build_feedback_runner.py:446` — preview_protocol；绑定固定 B、当前库存及 roots/stores 范围。
- `scripts/evidence/build_feedback_runner.py:934` — _budget_sample；不完整清点保留失败回执并拒绝。
- `scripts/evidence/build_feedback_runner.py:1008` — _check_review；复核覆盖同一协议/基线、allocated 上界和磁盘下界。
- `scripts/evidence/build_feedback_runner.py:1667` — run_protocol；磁盘预测不足时在完整清点和创建实验 run root 前拒绝。
- `scripts/evidence/build_feedback_runner.py:382` — 协议直接接收 predicted_growth_bytes；formal 才要求匹配完成的 pilot 与 measured margin。
- `scripts/evidence/build_feedback_runner.py:65` — canonical_sha256；规范化 JSON 哈希不能与格式化 JSON 文件的原始字节哈希混淆。

### 已执行的低成本验证

只通过 `python -B` 调用现有 `budget_status`，没有清点整盘、创建缓存、运行 Cargo 或写入 Python bytecode：

| 合成边界 | allowed | review_required | 结果 |
| --- | --- | --- | --- |
| A=B、F=119 GiB、G=39 GiB | true | false | 磁盘余量恰好 0，预算允许这一单项边界 |
| A=B+1 GiB、F=160 GiB、G=39 GiB | true | true | project_review_threshold_reached |
| A=B+21 GiB、F=160 GiB、G=39 GiB | false | true | projected_project_hard_limit_reached |

这验证的是预算函数和算术边界，不是完整实验入场或性能证据。

### 旧回执与当前观测为什么不同

2026-10-18 的已保存 preview 以约 121.622 GiB 空闲空间、A=B 算出 allowed=true；它与当前协议的规范化哈希一致，但时间已过。后续真实 disk-refused 回执在 **113.249004 GiB** 空闲时拒绝：减去 39 GiB 只剩 74.249004 GiB，低于 80 GiB。

本次只读观测在 2026-10-19 20:18:38.9611783 +08:00 为 70.298492 GiB，20:24:33.982991 +08:00 又降为 67.865631 GiB。时间点不同的磁盘值不能平均后用作入场，也不能作为恒定保证。这里只能确认系统空闲空间变化，没有清点外部写入归属，不把变化归因于某个窗口或任务。

### 可复核的重新入场条件

1. 以当前同一冻结 B 和原始 baseline 哈希为准，保留 39 GiB 预测，重新观测 C 空闲空间至少 119 GiB；不能用旧 preview 直接运行。
2. 在安静时段重新完整清点相同及新增关联 roots/stores，要求 complete=true/error=[]；A+39 GiB 严格低于 B+60 GiB。达到 B+40 GiB 时需要覆盖该具体协议/批次的预算复核回执。
3. 通过全源清单、Git/tree/lock/resources/toolchain/MSVC 身份验证及完整 Job Object/进程捕获要求，再决定实际 pilot 的第一对是否允许执行。
4. 五对完整 pilot 的 source allocations、保护期、逐批次预测和实际峰值仍须逐项满足；一次磁盘条件通过不能证明整个固定 N 活动可完成。

现有合同禁止为制造进度调低 forecast、重置 B、清理旧缓存或执行 cargo clean。清理授权、具体路径和保护期不是这份研究能够自行改变的条件。满足保护期也不会自动授权删除。当前没有成功 trial outputs，因此没有由本次 pilot 成功产生、可据此申请回收的 target 路径。

### Files found

以下为仓库相对路径；本机原始回执仅说明位置，不复制其中机器路径或详细日志。

- `.trellis/tasks/10-18-build-feedback-infrastructure/prd.md` — 固定 B、40/60 GiB 增长、80 GiB 底线、禁止清理及固定 N 接受合同。
- `.trellis/tasks/10-18-build-feedback-infrastructure/design.md` — inventory/runner/stats 所有权、捕获/复核及受保护原始证据边界。
- `.trellis/tasks/10-18-build-feedback-infrastructure/implement.md` — 真实拒绝、当前 0/5、0/100 及最新 127-test 历史检查点；本研究没有重跑这些测试。
- `.trellis/tasks/10-18-build-feedback-infrastructure/research/verified-build-tool-boundaries.md` — 已有官方 Cargo/MBX/sccache/Windows 文档验证。
- `scripts/evidence/build_feedback_inventory.py` — NTFS 身份/allocated bytes 清点及预算函数。
- `scripts/evidence/build_feedback_runner.py` — 协议、入场、复核、process/job 捕获和活动状态。
- `docs/operations/BUILD-FEEDBACK.md` — 预算协议、保留与当前实验状态，尤其第 34、334、359 行起。
- `.trellis/.runtime/build-feedback-infrastructure-20261018/fixed-baseline.json` — 忽略的本机冻结 B 元回执。
- `.trellis/.runtime/build-feedback-infrastructure-20261018/inventory-corrected-001.json` — 忽略的本机完整基线清点；本次只读聚合和 SHA-256 核验。
- `.trellis/.runtime/build-feedback-infrastructure-20261018/pilot-native-protocol.json` — 忽略的当前 39 GiB / pilot5 配对协议。
- `.trellis/.runtime/build-feedback-infrastructure-20261018/pilot-native-preview.json` — 忽略的历史预览；与协议身份相符，磁盘观测已过期。

### External references（已有验证，本次不新增外部事实）

预算缺口来自本机回执和合同算术，无需外网解释。下列原始官方资料来自已读的 verified-build-tool-boundaries.md；本次没有重新联网验证，也不以这些 URL 主张任何新版本行为或性能数字。

- Windows allocation API：<https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getcompressedfilesizew>；支撑 allocated/logical 不可混用，版本属于 Windows Win32 API。
- Windows committed-memory Job Object：<https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_extended_limit_information>；JobMemoryLimit 与 PeakJobMemoryUsed 不应标为 RSS。
- Cargo profiles：<https://doc.rust-lang.org/cargo/reference/profiles.html>；本地协议固定 cargo/rustc 1.97.1，当前研究不改 profile。
- 现有 MBX 1.24.1 / sccache 0.18.0 核验链接见同任务的 verified-build-tool-boundaries.md；当前 native pair 的预算不依赖两工具的缓存命中推断。

### Related specs

- `.trellis/workflow.md:353` — research 必须持久化到任务 research/；本次角色不加载 implement.jsonl/check.jsonl，也不转换为 implement/check 代理。
- `.trellis/spec/agentsessions-cli/backend/index.md:1006` — Windows build-feedback infrastructure 场景完整合同；第 1036 行固定 B/双预算、第 1063 行工具 store、第 1073 行 fixed5/fixed100、第 1106 行拒绝矩阵。
- `.trellis/spec/guides/index.md` — 关键跨层/复核规则；所有关键发现必须回到实际代码确认。
- `.trellis/spec/agentsessions-cli/backend/quality-guidelines.md` — 仍为占位模板，不作为预算参数权威；上面的场景合同和当前任务优先。

## Caveats / Not Found

- 父代理 NEW_TASK 和后续 MESSAGE 的 payload 都为空。已有 active task 路径明确；研究按本代理任务名 unblock_budget_facts 和任务当前预算阻塞收敛，没有推断清理或实现授权。
- 没有新完整关联-cache 清点，因此当前 A 未知。能确认磁盘不足，不能给出最新项目 headroom 或当前 root/cache 排名。历史 baseline 缓存大小不能当作今日可回收清单。
- 39 GiB 预测能追溯到协议和 operations 文档，但没有实测 pilot 峰值、store 峰值或独立估算分项的不可变回执；不把它叫实测，也不下调它。
- 没有完成 pilot 或 formal，不能推断速度收益、长期总缓存 40/60 GiB 目标达成、profile/tool adoption 或正式 release 性能通过。
- 实验源/工具 binary/MSVC hashes 保存在协议中；本次没有重新执行 Git 或完整 toolchain/source 验证，不能证明它们在当前时间仍可启动。
- 样本磁盘空闲量会继续变化；51.134369 GiB 缺口只绑定 20:24:33 观测。119 GiB 是该协议当前预测下的最低条件，不是给外部并发写入额外留裕量的建议值。
- 本次只写此研究文件；没有改代码、spec、manifest、缓存、任务状态或原始 runtime 回执，没有创建子代理，也没有执行 Git 操作。
