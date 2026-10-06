# 设计：Hermes SQLite 变体接入

## 结构

- 在 `crates/agent-session-grep-provider-hermes` 内新增 SQLite 变体模块（probe/parse/build），复用该 crate 既有的 canonical 映射与 `ParseReport` 结构；主入口按变体分派，保持 `hermes/session-json-v1` 行为逐字节不变。
- 读取层：复用 SQLite 只读快照模式（参考 `agent-session-grep-provider-opencode` 的只读/有界实现），不新增依赖。

## 判别与映射

- probe：检查 `state.db` 存在 + `sessions`/`messages` 表与关键列；与 JSON 变体文件（`sessions/session_*.json`）互斥；同时命中或结构不符 → `AmbiguousVariant`/拒绝。
- 解析顺序：`SELECT ... FROM messages WHERE session_id=? ORDER BY timestamp, id`；`started_at`/`timestamp` 为 REAL 秒 → 毫秒统一换算；NULL 显式保留。
- profile：主库 `state.db` 与 `profiles/<name>/state.db` 分别形成来源；namespace 参与安装身份，保证同名 native session 在不同 profile 下不合并。
- tool_calls：JSON 文本 → 两形态解码；`tool_call_id` 匹配失败或重复 → 非权威候选列表，不合成 id。

## 证据门（beta 建议）

- 合成 golden：排序、profile、tool 形态、NULL、坏行、超限。
- 源不可变与并发提交测试（WAL 活跃时）。
- 上游证据：固定提交 `bac0c45d…` 的 `hermes_state_common.py` schema v30 行号引用。
- 缺口若无法在本任务内补齐（例如缺少独立真实格式样本），任务只提交晋级建议与缺口，不直接改矩阵 maturity。
