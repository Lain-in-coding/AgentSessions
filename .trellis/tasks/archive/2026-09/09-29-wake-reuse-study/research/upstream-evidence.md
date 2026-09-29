# 独立上游格式证据（非兼容性认证）

除 Wake 源码外，以下证据来自 provider 本身的公开仓库。固定到不晚于 2026-09-27 的仓库快照，通过 GitHub API 只读提取；未克隆或执行这些项目，也未读取其 transcript fixtures。证据只能确认列/事件定义，不能证明所有部署版本、数据迁移、resume 或本项目适配器的完整兼容性。

## Hermes

- 仓库：<https://github.com/NousResearch/hermes-agent>
- 固定提交：`bac0c45d8593ed9d53a8e3fcacecdc920b71a2c4`。
- `hermes_state_common.py`，blob `e35e61a06a3c69ebea6b37accec72d10cead20dc`：L263 schema version 30；L362 起 sessions；L379 started_at REAL；L401 title；L428 起 messages；L433-L440 tool_call_id / tool_calls / tool_name / timestamp REAL / reasoning。
- `hermes_state_schema.py`，blob `ffd06529fbb34321e8da8c7b876d1b9542e4e48c`：schema/migration 入口；当前布局已拆分，不应继续把旧的单文件名当作永恒格式契约。
- 结论：Wake 的 SQLite 会话/消息列知识有独立上游支持。schema 已演进到 30，Wake 探测少数可选列不足以证明全版本覆盖。两种 tool-call 形状、profile 路由及启发式配对仍须分开证明。

## ZCode

- 仓库：<https://github.com/zai-org/ZCode>，GitHub 显示 Apache-2.0（本轮只引用格式事实，不复制其实现）。
- 固定提交：`29628c9acdb81b703bbd4080c207a0e7ce5e276e`。
- `apps/zcode-cli/packages/adapters/src/storage/session-store/migrations.ts`，blob `a6ccefc5fc438f0824ae2790f4b285dc4f86a6ee`：L14/L41/L52 建立 session/message/part；L230 增加 task_type；L382 title_source；L546-L549 增加 message/part.sequence。
- `packages/services/src/session/tasksDatabase/schema-v1.ts`，blob `97a06e7cb0cd637dd930ef9e34585fb00966fa2a`：L3 tasks；L13 migration_source；L21 deleted；L29/L33 的 deleted=0 条件。
- 结论：Wake 所称“不是把 OpenCode 换个根路径”得到列/迁移证据支持。它的角色过滤、semantics、隐藏策略及不能 resume 的说明仍不能当作独立验证过的产品契约。

## Cursor IDE

没有取得 Cursor 官方、版本化的私有存储格式承诺。`cursorDiskKV`/`composerData`/`bubbleId` 的证据为 Wake 固定源码和本轮合成探针；只标结构可复现，不标上游认证。不要由能解析推导能恢复。

## 文档核对

- SQLite FTS5 官方：<https://www.sqlite.org/fts5.html#the_trigram_tokenizer>，短于三个 Unicode 字符的 trigram MATCH 不匹配；无足够非通配符子串时 LIKE/GLOB 可能线性扫描。
- rusqlite 官方 Connection/CachedStatement 文档通过 Context7 检索：prepare_cached 在句柄 Drop 后归还连接缓存，且正在使用的语句不可被再次借出。最终 API/feature 以实验锁定的 0.40.2 源码和编译结果为准。
- Grok 搜索摘要仅作导航，不作为性能数字、线程 trait 或最终依赖语义的证据。所有性能结论来自本轮原始样本。

## DeepSeek Harness（DSH）

- 仓库：<https://github.com/deepseek-ai/deepseek-harness>。
- 固定提交：`da84e3a3f7ff9121c8c1d6bd8dab428e64c05085`。
- `packages/core/session/src/known-event-types.ts`，blob `adaca29255a181c793a73476baddc25324083cb2`：L29 assistant/message、L37 developer/message、L63 system/message、L76 tool/result。
- `packages/core/session/src/types.ts`，blob `593c86d52a5c26d531913ffaff9aac651732c2da`：L94 SessionHeader、L99 version；L311/L330/L341/L375 定义相关事件载荷。
- `packages/session/session-persistence-jsonl/src/format.ts`，blob `781c5d83c32e14e30b37d7ae5c3a68cfe7f7b10b`：L40-L84 的 JSONL/zstd 后缀与 generation 文件名函数。
- `packages/session/session-persistence-jsonl/src/generation.ts`，blob `ff01e9a038a848b70342af6672533388056c40fc`：L54 zstd checksum；L492 起 generation 校验入口。
- 结论：事件名称、会话头和代际日志不是 Wake 独自臆造；但“只保留最终 assistant、忽略 surfaceOp.replace、屏蔽 subagent”仍是读取/呈现取舍，不能据此规定本项目规范化语义。解压总量、行长度、格式未知项与源身份需要本项目重新约束。
