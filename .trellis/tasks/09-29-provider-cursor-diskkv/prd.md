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

- [ ] probe 判别：与 ItemTable 变体互斥；歧义/缺表显式拒绝。
- [ ] 顺序 golden：打乱 KV 插入序仍按 header 顺序输出；重复 header 保留独立序位。
- [ ] 坏行 golden：缺失/NULL/坏 JSON/错形状/坏 UTF-8 各自显式状态与计数。
- [ ] 工具输入编码与选择顺序 golden；native id 原样保留（不归一化）。
- [ ] 源不可变（字节/元数据）与有界预算测试。
- [ ] 现有 16 provider golden 与 workspace 测试无回归。
- [ ] 矩阵保持 experimental；PROVENANCE 记录“无官方版本化契约”的证据边界。

## Notes

- 研究探针与证据：归档 `research/experiments/providers/`；Wake 固定提交 `71aeca67…` 的 `adapters/cursor_ide.rs` 行号。
- 现有 Cursor 变体无 native 消息 id（合成 `cursor-msg-{seq}`）；新变体必须单独评估稳定性，不得机械继承旧变体的合成策略。
