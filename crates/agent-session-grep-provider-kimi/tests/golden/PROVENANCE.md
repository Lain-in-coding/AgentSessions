# Golden fixture 来源声明（PROVENANCE）

> 依据 `docs/security/FIXTURE-REDACTION-POLICY.md`：合成优先、禁止真实 transcript、
> fixture 目录必须附本声明。

## fixture_revision

- **fixture_revision**: 2
- `basic.jsonl` — introduced in revision 1 (2026-08-16); its input and expected bytes are unchanged.
- `turn-inputs.jsonl` — added in revision 2 (2026-10-04).
- 格式修复必须新增 fixture 而非只改 parser（政策 §Provider fixture 要求）。

## 构造方式

`basic.jsonl` 为**逐行人工手写的合成数据**，不基于任何真实会话记录做删减或脱敏。
字节即权威：UTF-8（无 BOM）、LF 行尾，BLAKE3 pin 在 `basic.expected.json`
（`ad708d7bbcf1f4be7d3318d725493bc93b84f9fcd18352b9d61abc8eb2c81185`），根
`.gitattributes` 的 `-text` 规则防止行尾改写。

## 逐行覆盖

每行一个 Kimi wire.jsonl 记录；`type` 判别。

| 行 | 内容 | 覆盖点 |
|----|------|--------|
| 1 | `context.append_message`，user 字符串 content | 用户消息；span |
| 2 | `context.append_message`，assistant 数组 content | `{text}` part 以 `\n` 拼接 |
| 3 | `context.append_loop_event` | loop/工具事件暂未解析，跳过 |
| 4 | `context.append_message`，system role | 非对话角色跳过 |
| 5 | 非 JSON 行 | 破损行 → `skipped+1` + 诊断 |
| 6 | `context.append_message`，空 content | 空正文跳过 |

## 编码的真实格式知识（仅字段名与封套形状，无真实内容）

来自 Kimi wire.jsonl 的格式观察，仅复用结构：`context.append_message` 包裹
`message.role`（user/assistant/system）与 `message.content`（字符串或 `{text}`
数组）；`context.append_loop_event` 承载 step/tool 事件（本切片 deferred）。
消息提取适配自 fast-resume（MIT）的 idea-level 结构，未复制任何字节。

## 脱敏与合规声明

- **不含任何真实数据**：uuid 为 `evt-1` 固定假值、正文为自述性合成文案；
- 无人名、邮箱、token、密钥、真实项目名或真实主机路径；
- 未从任何同类项目复制 fixture（政策 §许可证边界）。

## span round-trip

行式 JSONL：每个 span 覆盖"某一整行去掉行尾符"，golden 测试
`golden_spans_slice_back_to_exact_source_lines` 逐字节校验切片并核对
`type:"context.append_message"` + `message.role` 与消息角色一致。

## Revision 2: supported turn input probe

`turn-inputs.jsonl` is handwritten synthetic data, not a redacted or copied
transcript. It is UTF-8 without BOM, uses LF endings, and ends with LF. Its
BLAKE3 and full canonical output are pinned in `turn-inputs.expected.json`.
Only format keys and envelope shapes were observed upstream; every text value,
time value and marker in this fixture was independently invented. It contains
no real paths, credentials, names, session IDs, or project data.

| Line | Record | Coverage |
|------|--------|----------|
| 1 | `metadata` | Non-message prefix still consumes one probe sample slot |
| 2 | `turn.prompt` with top-level text blocks | User text, CJK/emoji, leading/trailing whitespace, ordered block joining |
| 3 | `turn.prompted` | Similar discriminator is not a supported message |
| 4 | `context.append_loop_event` / `tool.call` | Loop/tool records remain unparsed; no activity emission |
| 5 | `turn.steer` with top-level text blocks | User-role follow-up and verbatim tab/newline preservation |
| 6 | `turn.prompt` with an empty array | Supported input shape but no conversational text |
| 7 | Malformed JSON | One recoverable skip; probe tolerates the sampled broken line |
| 8 | `turn.steer` with an object input | Unsupported input shape supplies no probe evidence or message |
| 9 | `context.append_message` | Existing assistant text extraction, deliberately beyond the eight-line probe window |
| 10 | `turn.prompt` with a string input | Existing string compatibility, whitespace and embedded U+FEFF |

The first eight nonblank records contain no `context.append_message`. This
fixture therefore reproduces the old selection gap although the existing parser
produces four messages. It also pins the mixed prompt/steer/append path:
file-order `seq`, empty native message IDs, absent parent/session/timestamp
metadata, exact byte spans and the unchanged text-only projection.

### Verified upstream shape evidence

The earlier `WireRecord::input` comment cited the ctx adapter. On 2026-10-04 the
following evidence was rechecked at immutable ctx commit
`c13a92db70147533c256532384fe8baab6f87950`:

- [`kimi/event.rs`, `kimi_event_type`, `kimi_event_role`, `kimi_event_text`](https://github.com/ctxrs/ctx/blob/c13a92db70147533c256532384fe8baab6f87950/crates/ctx-history-providers-jsonl-shared/src/provider/providers/kimi/event.rs#L11-L79)
  explicitly matches `turn.prompt` and `turn.steer`, assigns user role, and reads
  the top-level `input` field.
- [`kimi/tests.rs`, `message`](https://github.com/ctxrs/ctx/blob/c13a92db70147533c256532384fe8baab6f87950/crates/ctx-history-providers-jsonl-shared/src/provider/providers/kimi/tests.rs#L59-L65)
  constructs a prompt with a string `input`.
- The [upstream shape fixture, line 2](https://github.com/ctxrs/ctx/blob/c13a92db70147533c256532384fe8baab6f87950/tests/fixtures/provider-history/kimi-code-cli/.kimi-code/sessions/wd_demo_abc123/kimi-session-1/agents/main/wire.jsonl#L2)
  was inspected only for field names and JSON types: `type`, `time`, `input`,
  `origin`, with `input` an array of `{type: "text", text: string}` blocks.
  No upstream fixture bytes were copied into this repository.

This is implementation/shape evidence from the existing reference adapter, not
format certification. Its broader loop/reasoning handling is not adopted.
Kimi remains Experimental; loop/tool activity and per-message timestamps remain
unsupported, and no native identity or Resume authority is invented.

### Probe confidence and regression boundaries

Only exact `turn.prompt` / `turn.steer` plus a top-level string or array `input`
adds new probe evidence. The shape alone is `High`; `Confirmed` additionally
requires non-whitespace text from the existing `kimi_content_texts` extractor.
Empty strings, whitespace and arrays with no extractable text never count as
conversational evidence. Missing/null/boolean/number/object inputs do not identify
Kimi on their own. This does not make probe a full content-block validator or
change how the parser skips unsupported parts. Existing append-message marker
and role-based confidence rules are unchanged.

Unit regressions cover both exact discriminators, strings and text blocks,
empty/unsupported inputs, misleading nested fields, unknown/similar type names,
eight nonblank sample slots (including malformed records), mixed evidence,
first-byte-only BOM stripping, embedded U+FEFF and byte-probe UTF-8 errors even
beyond the line sample. Golden tests compare byte and bounded-source probe/parse
entry points, full reports and canonical fields, exact source slices, and source
immutability. A separate synthetic case covers BOM + CRLF + a final line without
an ending. No shared reader, sampling, tolerance or parser semantics change.

The existing ignored manual printer now prints both expected outputs for review:

```text
cargo test -p agent-session-grep-provider-kimi --offline --locked --test golden print_actual_canonical_output_for_regeneration -- --ignored --nocapture
```
