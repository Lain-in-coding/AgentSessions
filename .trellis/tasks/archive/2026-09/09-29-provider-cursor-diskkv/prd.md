# Cursor IDE cursorDiskKV 变体（`cursor/disk-kv-v1`）

## Goal

在现有 `cursor/vscdb-chat-v1`（ItemTable chatdata/prompts）之外新增 `cursor/disk-kv-v1` 变体：读取 `state.vscdb` 的 `cursorDiskKV`（`composerData:<id>` + `bubbleId:<composerId>:<bubbleId>`）。目标成熟度 experimental。

## Requirements

- 变体判别：`cursorDiskKV` 表与 `composerData:`/`bubbleId:` key 形态；与 ItemTable 变体互斥；同时命中/结构不符显式拒绝。
- 顺序：严格按 `composerData.fullConversationHeadersOnly` 的 header 顺序，不得按 KV key、rowid 或插入序排序。
- 健壮性：缺失、NULL、坏 JSON、错形状、非法 UTF-8 的 bubble 必须保留显式槽位与状态计数（不得静默归零或整会话丢弃）。
- 工具输入：`rawArgs → params → 文本` 选择顺序；对象与 JSON 字符串两种编码；绝不执行输入。
- 身份：native bubble/composer id 原样保留为观测；无法证明全局稳定时不得声明 StableId/Native 稳定性；重复 bubble id 保留独立序位。
- 读取：有界（行/cell/总字节）；只读快照；源 DB/WAL/SHM 不可变。
- 不新增 resume 命令（矩阵 `resume=unknown`）；不宣称兼容认证。
- fixtures 全合成 + PROVENANCE（引用固定 Wake 实现与“无官方版本化存储契约”的边界）；禁止真实 transcript。

## Acceptance Criteria

- [x] probe 判别：与 ItemTable 变体互斥；歧义/缺表显式拒绝（`src/lib.rs:155-214` 双 claim；双面库 CLI 实测 exit 2 `bytes match both`；专名表结构不符 → `StructuralFatal`；`provider_probe_isolation` 3 passed）。
- [x] 顺序 golden：打乱 KV 插入序仍按 header 顺序输出；重复 header 保留独立序位（`tests/disk_kv_golden.rs:207` 用两个"同逻辑不同插入序"fixture 比对**完整 canonical 投影**；重复 id 见 `src/disk_kv.rs:1384-1407`）。
- [x] 坏行 golden：缺失/NULL/坏 JSON/错形状/坏 UTF-8/非文本正文/非法工具封套/空正文各自显式状态与计数，不整会话丢弃（`src/disk_kv.rs:1222`、`:1367`；golden composer A 15 槽 8 skipped；CLI e2e `a_broken_bubble_does_not_discard_its_session`）。
- [x] 工具输入编码与选择顺序 golden（`rawArgs → params`，对象/字符串两种编码，从不执行：全 crate 无 `Command`/`eval`）；native id 原样保留为观测（composer id verbatim 进会话观测与诊断，消息 `native_id=""` 以避免伪造 Native 稳定）。
- [x] 源不可变（字节/元数据）与有界预算测试：adapter 从不打开源文件、仅在私有副本上单次固定读事务，DB/WAL/SHM 前后 BLAKE3+mtime+len 一致（`src/disk_kv.rs:1704`）；6 条上限（整源/composer/单 composer header/全库 header/单 cell/总 cell）生产可触发且显式 `SourceTooLarge`/`RecordTooLarge`。
- [x] 现有 16 provider golden 与 workspace 测试无回归（`provider_matrix` 35 passed；workspace 84 suites / 1832 passed；既有 cursor golden/properties 断言未改写）。
- [x] 矩阵保持 experimental；PROVENANCE 记录"无官方版本化契约"的证据边界（`PROVIDER-MATURITY-MATRIX.md` 仅补记变体与边界、未晋级；`tests/golden/PROVENANCE.md` 含 Wake `71aeca67` 行号引用与合成 fixture 说明）。

## Verification

- 提交：`1dc2572` 实现 → `989fc5d` CLI 测试 clippy 修正 → `f93bfcb` 核查后补测（剩余槽位状态 + 洗序全投影）。
- 独立核查：verdict **PASS-with-findings**（3 条非阻断，无回归）；F1（`invalid_text_type`/`invalid_tool_shape` 等状态无产品测试）与 F2（洗序对比未覆盖全字段）已在 `f93bfcb` 修正；F3（计数型上限复用 ports 的 "bytes" 文案）属共享错误类型既有行为，作为 owner follow-up 记录，未改 ports。
- 关键设计决策（需 owner 复核）：① 消息 `native_id` 留空（bubbleId 无全局稳定证明；组合根按 document-scoped 派生 `Stability::Unstable`，不声明 Native），composer id 原样保留；② 双面库（同时含 ItemTable chatdata/prompts 与 cursorDiskKV composerData）按 PRD 要求**整体拒绝**（`AmbiguousVariant`），未做两面对照/分层合并——真实 `state.vscdb` 若为双面，需要后续"显式变体选择"决策。
- 已知记录差异：冻结探针把顶层重复 JSON key 记为 malformed，本 adapter 用 serde_json 保留末值、不检测（PROVENANCE 已如实标注，不是兼容承诺）。
- 与 hermes 的 capture 层口径一致：整链路 sync live-WAL 源可能由既有 capture 路径写 `-shm` 读标记（非 adapter 行为），adapter 层保证"从不打开源文件 + 只解析收到的快照字节"。

## Notes

- 研究探针与证据：归档 `research/experiments/providers/`；Wake 固定提交 `71aeca67…` 的 `adapters/cursor_ide.rs` 行号。
- 现有 Cursor 变体无 native 消息 id（合成 `cursor-msg-{seq}`）；新变体必须单独评估稳定性，不得机械继承旧变体的合成策略。
