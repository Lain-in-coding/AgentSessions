# 设计：Cursor cursorDiskKV 变体接入

## 结构

- 在 `crates/agent-session-grep-provider-cursor` 内新增 disk-kv 变体模块，与既有 ItemTable 变体并列；主入口按探测结果分派，既有变体行为不变。
- 读取层复用现有 SQLite 只读快照与有界读取实现；不新增依赖。

## 判别与解析

- probe：存在 `cursorDiskKV` 表，且能查到 `composerData:` 主键形态；ItemTable 变体同时命中或表缺失 → 显式拒绝。
- 会话枚举：`composerData:` 行 → composer id 与 `fullConversationHeadersOnly`；bubble 通过 `bubbleId:<composerId>:<bubbleId>` 精确查找（按 header id），不依赖 key 排序。
- 状态：每个 header 槽位产出 ok/missing/null/malformed_json/invalid_bubble_shape/invalid_utf8 等显式状态；计数进入 ParseReport。
- 身份：保留 native bubble id 文本；稳定性标注沿用本项目规则（缺证明不得声明 Native/Stable）；重复 id 以 header 序位区分。

## 证据与成熟度

- 合成 golden 覆盖顺序、坏行、工具参数、Unicode/分隔符 id、重复 id、超限。
- PROVENANCE 明确：Cursor 私有存储无官方版本化契约；证据为固定 Wake 实现 + 合成验证；维持 experimental。
