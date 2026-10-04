# Golden fixture 来源声明（PROVENANCE）

> 依据 `docs/security/FIXTURE-REDACTION-POLICY.md`：合成优先、禁止真实 transcript、
> fixture 目录必须附本声明。

## fixture_revision

- `basic.jsonl` — revision 1（2026-08-16 引入）。
- `content-loss.jsonl` — revision 2（2026-10-04 引入）；manifest fixture_revision = 2。
- 格式修复必须新增 fixture 而非只改 parser（政策 §Provider fixture 要求）。

## 构造方式

`basic.jsonl` 为**逐行人工手写的合成数据**，不基于任何真实会话记录做删减或脱敏。
字节即权威：UTF-8（无 BOM）、LF 行尾，BLAKE3 pin 在 `basic.expected.json`
（`c1f74eba9611947eec6986af678b3a637b81714e444a11219027353d76182463`），根
`.gitattributes` 的 `-text` 规则防止行尾改写。

## 逐行覆盖

每行一个 OpenClaw v3 session JSONL 记录；`type` 判别。

| 行 | 内容 | 覆盖点 |
|----|------|--------|
| 1 | `type:"session"`，含 `id`+`cwd`+`timestamp` | 会话身份 + cwd pair 上报 |
| 2 | `type:"message"`，user 字符串 content | 用户消息；span |
| 3 | `type:"message"`，assistant 数组 content | `{type:text}` block 以 `\n` 拼接 |
| 4 | `type:"session_info"` | 非对话类型跳过 |
| 5 | `type:"message"`，空 content | 空正文跳过 |
| 6 | 非 JSON 行 | 破损行 → `skipped+1` + 诊断 |
| 7 | `type:"message"`，CJK + message.timestamp | 多字节 span；消息内时间戳透传 |

## 编码的真实格式知识（仅字段名与封套形状，无真实内容）

来自 OpenClaw v3 session JSONL（`~/.openclaw/agents/<agent>/sessions/*.jsonl`）的
格式观察，仅复用结构：`type:"session"` 头（`id`/`cwd`/`timestamp`）、
`type:"message"` 包裹 `message.role` 与 `message.content`（字符串或
`{type:"text",text}` 数组）。形状与 Pi adapter 同源（fast-resume MIT，
idea-level），未复制任何字节。

## 脱敏与合规声明

- **不含任何真实数据**：session id 为 `sess-1` 固定假值、路径为 `/home/user/proj`
  占位符、时间戳为 `2026-01-01T00:00:0NZ` 固定基准、正文为自述性合成文案；
- 无人名、邮箱、token、密钥、真实项目名或真实主机路径；
- 未从任何同类项目复制 fixture（政策 §许可证边界）。

## span round-trip

行式 JSONL：每个 span 覆盖"某一整行去掉行尾符"，golden 测试
`golden_spans_slice_back_to_exact_source_lines` 逐字节校验切片并核对
`type:"message"` + `message.role` 与消息角色一致。


## 无可索引文本的损失计数（`content-loss.jsonl`）

此 fixture 为逐行人工构造的纯合成反例，未读取、复制或脱敏任何真实 transcript。
文件为 UTF-8（无 BOM）、LF 行尾，共 1751 字节；BLAKE3 pin 位于
`content-loss.expected.json`：
`88599642543f8678b3b58681de0cc290e9fdb07015f5dba125cbb8c3f87ba487`。
原有 `basic.jsonl` 与 `basic.expected.json` 字节和输出均保持不变。

| 行 | 内容 | 覆盖点 |
|----|------|--------|
| 1 | 固定合成 session 头 | 会话身份与 cwd 关联不变 |
| 2–6 | 缺 content、null、空串、Unicode 空白串、空数组 | 合法空内容，不 emit、不计 skipped、不诊断 |
| 7 | thinking 与 redacted_thinking 块 | 整条消息 skipped+1，不按块数计数、不索引 reasoning |
| 8 | 两个未知块，其中一个带 text | 未知块不得伪装成支持的 text 块；整条消息 skipped+1 |
| 9–11 | 单对象 content、布尔 content、仅空白 text 块的非空数组 | 不扩展抽取形态；各 skipped+1 |
| 12 | thinking、未知块与两个 text 块混排 | 只保留支持的正文及空白/换行/CJK/emoji；无损失诊断；消息时间优先 |
| 13 | 保留首尾空白的普通字符串 | 字符串抽取不变；外层时间戳与连续 seq |
| 14–15 | 非对话角色与 session_info | 仍在会话消息损失计数范围外，不增加工具能力 |

固定期望为 committed=2、skipped=5；逐消息诊断仅包含行号和固定中文说明，
不包含正文、块类型、未知字段或 reasoning 内容。非空数组即使只有空白 text
块也不属于上述合法空内容集合。额外表驱动测试覆盖两种对话角色、空对象、数值、
数组内无效元素与 text 类型错误；这些是不支持输入的防丢失反例，并非上游格式认证。
短缓冲只读源（1/7/64 字节）、BOM+CRLF 与无终止换行变体验证 parse/parse_source
报告和输出等价；golden span 测试逐字节回切两份 fixture 的完整消息记录。

### 可核验的结构证据与边界

- 官方 [Session management deep dive](https://docs.openclaw.ai/reference/session-management-compaction)
  及其 [Transcript event structure](https://docs.openclaw.ai/reference/session-management-compaction/schema#transcript-event-structure)
  说明 session 头与 message 事件封套。2026-10-04 核验时前者已拆为索引页，
  当前文档也描述 SQLite 存储；本次不新增 SQLite 支持或宣称覆盖全部当前格式。
- 固定上游提交 `96f60182b72a31a8bd84390dd8879016b584a537` 的
  [`src/agents/session-transcript-repair.ts`](https://github.com/openclaw/openclaw/blob/96f60182b72a31a8bd84390dd8879016b584a537/src/agents/session-transcript-repair.ts)
  在 assistant 数组 content 上检查 thinking-like 块；非工具块保留到 nextContent。
  同提交 [`src/agents/thinking-block.ts`](https://github.com/openclaw/openclaw/blob/96f60182b72a31a8bd84390dd8879016b584a537/src/agents/thinking-block.ts)
  明确识别 `thinking` / `redacted_thinking`。已通过 GitHub API 按该 SHA 读取核验，
  不把可变分支或仅凭函数名的猜测当成证据。
- 证据仅支持封套与非文本块存在；本项目的损失计数策略是批准的检索契约，
  不是对上游工具行为的复制。fixture 正文、标记、身份、路径及时间均为固定假值；
  不复制上游实现或会话字节，不索引推理，不发出工具/usage 事件，provider 成熟度不变。
