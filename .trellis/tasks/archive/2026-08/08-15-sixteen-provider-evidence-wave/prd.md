# 16 Provider Adapter Evidence Wave

> Parent: `08-15-open-source-product-roadmap`
> 状态:planning。证据先行(evidence-first),无证据不实现、不宣传。

## Goal

按冻结的 16-provider 名单,以"真实格式证据 → fixture → adapter → 能力矩阵
→ 成熟度分级"的顺序,把 provider 覆盖从 2 个扩到 16 个,并保持诚实分级。

## 冻结名单

```text
Claude Code, Codex CLI, DeepSeek Harness, Grok Build, Antigravity,
OpenCode, Pi, Aider, Cline, Cursor, OpenClaw, Hermes, Kimi Code,
ZCode, Qoder, Tencent CodeBuddy
```

## 证据基线(2026-08-15 事实核查,来源:deep-read-*.md、源码与本仓库矩阵)

> 表中 `当前 maturity` 是 `PROVIDER-MATURITY-MATRIX.md` 的当前事实;
> `目标分级` 是本任务的路线目标,不得用于当前宣传。source root、identity、resume
> 必须附 provider/version/variant provenance;冲突时保留 `unknown` 或 `null/—`。

| Provider | 当前 maturity | 目标分级 | source root / variant 证据 | format 证据 | identity 证据 | resume 证据 | 证据状态 |
|---|---|---|---|---|---|---|---|
| Claude Code | Experimental(矩阵) | certified | `~/.claude/projects/` | JSONL,`type:user/assistant`,`sessionId`/`uuid`/`parentUuid` | 本仓策略取首个非空 `sessionId`;外部实现存在 filename/index fallback,需按 variant 记录 | `claude --resume <id>` | 已实现;需跨平台 Gate D/正式 target CI 晋级 |
| Codex CLI | Experimental(矩阵) | certified | `~/.codex/sessions/`(+archived) | rollout 封套 JSONL,权威 `response_item/message` | 本仓 durable session identity 使用 `session_meta.payload.session_id`;外部报告/版本曾写 `payload.id`,与本仓契约冲突,必须按 source/version 分层;多 ID fail-closed 为 repo-specific | `codex resume <id>` | 已实现;字段 provenance 与跨平台 Gate D 待锁定 |
| DeepSeek Harness | unknown | beta | **unknown** | **unknown** | unknown | unknown | **证据不足**——发布前补真实 transcript;否则记录推迟公开,禁止猜测 |
| Grok Build | Unlisted(当前矩阵无行) | beta | `~/.grok/sessions/`,`GROK_HOME` 可覆盖 | 主 variant:`summary.json`+ACP `updates.jsonl`;另一 adapter/version 有 `chat_history.jsonl` fallback,不得混为单一格式 | `summary.json.info.id` | `grok --resume <id>`(已核验来源) | 主格式证据充分,variant 需分层 |
| Antigravity | Unlisted(当前矩阵无行) | beta | `~/.gemini/antigravity-cli/brain/*/.system_generated/logs/`;另有 `~/.gemini/antigravity/brain` 与 `conversations/*.db`/cache auxiliary variant | transcript JSONL + history + SQLite | unknown(需 fixture) | **null/—**:源码/测试见 `agy --conversation <id>`,报告摘要另写 `--resume`;版本/来源冲突,核验前不设默认 | 格式可先做,resume 未核验 |
| OpenCode | Unlisted(当前矩阵无行) | beta | `~/.local/share/opencode/`;`XDG_DATA_HOME` 可覆盖 | `opencode.db`(session/message/part)+ legacy JSON tree 双路径 | session 表 `id`(`ses_...`) | **null/—**:`opencode <dir> --session <id>` vs `opencode -s <id>` 版本差异,核验前不设默认 | discovery/parse 可先做 |
| Pi | Unsupported(矩阵) | beta | `~/.pi/agent/sessions`;`PI_CODING_AGENT_SESSION_DIR`、`PI_CODING_AGENT_DIR`/settings.json 可覆盖 | session JSONL | session 行 UUID | `pi --session <id>` | 路径覆盖与 fixture 待补 |
| Aider | Unlisted(当前矩阵无行) | experimental | hstry adapter / cass connector 是外部证据入口,不是已确认 source root | `.aider.chat.history.md` 作为待核验候选,不得视为主格式定论 | unknown | none | 需真实 fixture 与 root 证据 |
| Cline | Unlisted(当前矩阵无行) | experimental | `CLINE_DATA_DIR` / `~/.cline/data` / VS Code tasks;另有 `CLINE_DIR/data`、`CLINE_SESSION_DATA_DIR`、`CLINE_DB_DATA_DIR` | `api_conversation_history.json` 等 JSON family | unknown(需 fixture) | none(VS Code 内) | 需研究后实现 |
| Cursor | Unsupported(矩阵) | experimental | CLI `~/.cursor/projects/*/agent-transcripts/` JSONL;`~/.cursor/chats/<id>/store.db` SQLite KV;state.vscdb | 多代格式 | 版本相关,需分层 | `agent --resume <id>`(CLI) | 多代格式,需研究 |
| OpenClaw | Unlisted(当前矩阵无行) | beta | `~/.openclaw/agents/<agent>/sessions/*.jsonl`;legacy `~/.clawdbot`;`~/.moltbot`/`OPENCLAW_STATE_DIR` 待补证据 | v3 JSONL header `{type:session,id,timestamp,cwd}`+`{type:message,...}` | header `id` | intentionally unsupported(gateway 管理) | discovery/parse 可先做 |
| Hermes | Unlisted(当前矩阵无行) | beta | `~/.hermes/state.db`(`HERMES_HOME` 可覆盖) | SQLite + JSONL 双存储 | sessions 表 `id` | **null/—**:cc-switch/fast-resume 为 None,agf 记录 `hermes --resume <id>`;来源/版本冲突,fixture 前不设默认 | SQLite 路径可先做 |
| Kimi Code | Unlisted(当前矩阵无行) | beta | `~/.kimi-code/sessions`(`KIMI_CODE_HOME`) | `session_index.jsonl`+`sessions/*/*/agents/*/wire.jsonl`+`state.json` | 源码候选为 state.json 所在会话目录名,fixture 前仍标 unknown | **null/—**:报告写 `kimi --resume <id>`,当前源码构造 `kimi [--yolo] --session <id>`;版本/来源冲突 | 发现/解析可先做 |
| ZCode | unknown | experimental | **unknown** | **unknown** | unknown | unknown | **证据不足**——同 DeepSeek Harness,补证据或推迟决策 |
| Qoder | Unlisted(当前矩阵无行) | beta | `~/.qoder/projects/<project>/transcript/*.jsonl`;AgentRecall cache variant 单列 | JSONL(`session_meta/user/assistant/progress/tool_use/tool_result`) | 需 fixture | unknown | 官方 transcript 路径可先实现 |
| Tencent CodeBuddy | Unsupported(矩阵) | beta | CLI `~/.codebuddy/projects/**/*.jsonl`;extension `history/<md5>/<session>/index.json`+`messages/*.json` 两 variant | CLI JSONL(OpenAI 风格)+ extension JSON | 需 fixture(过滤根消息 `code`) | unknown | CLI/extension 分 variant 后实现 |

## Requirements

1. 每个 provider 按"证据 → fixture → probe → parse → search golden → 能力矩阵
   → 分级"推进;无证据的(DeepSeek Harness、ZCode)先做证据采集。每一行必须
   记录 evidence provenance/version、fixture manifest 和公开制品路径;
   当前 maturity 与目标分级不得混写。
2. 每个新 adapter 必须遵循现有 ProviderAdapter trait(probe/parse 分离,
   歧义拒绝不猜测),fixture 全部合成脱敏。
3. resume 命令差异(OpenCode/Antigravity/Hermes/Kimi Code)必须按版本核验后落
   capability matrix,无权威值则 null/—,不得编造;未核验 provider 不得进入
   可执行 resume 默认值。
4. SQLite 类 provider(OpenCode/Hermes/Kimi)只读打开(`SQLITE_OPEN_READONLY`
   + busy_timeout),绝不写 provider 数据库。
5. 官方 16 个内置 Rust adapter;社区 adapter 走 Provider Adapter Protocol
   (外部进程,manifest 声明,默认只读无网络)。社区 adapter 的 network 权限必须
   进入 manifest,并由 `--offline`/沙箱策略强制拒绝未授权 egress。
6. 增量同步遵循"失败不删除"原则:扫描出错返回空增量,不产生 tombstone,
   不误删已索引会话。provider 级失败隔离到诊断结果,保留其他 provider
   的成功增量。
7. 本任务只负责 provider-specific evidence、probe/parse 与 capability declaration;
   structured tool activity、sidechain facet 的 canonical projection、索引和查询
   由 `08-15-structured-activity-context-facets` 负责,双方以 canonical event
   contract 对接,不得重复实现。

## Acceptance Criteria

- [ ] 16 个 provider 各有:真实格式证据记录(含 provenance/version)、合成脱敏
      fixture manifest、probe/parse 实现(或 unsupported/deferred 决策记录)、
      能力矩阵行、分级理由,公开制品路径明确;
- [ ] Claude Code / Codex 完成统一 Gate D 六不变量、Windows/Linux/macOS
      target matrix、golden fixture、incremental、source span、context graph、
      resume/handoff/tool activity 全能力链证据后,由 owner 决定是否晋级 certified;
- [ ] 至少 5 个“主流” provider(名单在 evidence manifest 冻结)达到完整 beta
      主路径:discover/parse/search/context/source span/incremental,且其 capability
      matrix 明确 beta;若声称 GA,还必须满足 matrix 中 GA/Certified 的历史 variant、
      混合版本、未知字段、崩溃恢复、正式 target、性能与回滚证据;
- [ ] DeepSeek Harness / ZCode:证据补齐并实现,或在发布截止日由 owner 记录
      `deferred` 决策;deferred provider 仍保留 16 行,不得宣传为已实现;
- [ ] PROVIDER-MATURITY-MATRIX.md 公开 16 行,与 evidence manifest、README
      数字和实际行为通过一致性检查;
- [ ] cargo fmt/clippy/test 全绿。

## Notes

- 吸收/取代:08-14-gemini-opencode-providers(仅 OpenCode 相关范围并入;
  Gemini CLI 其余范围不进入首发,原任务关闭条件由父任务记录)、
  08-14-second-wave-providers(container 相关范围并入)、
  08-14-provider-source-auto-discovery(discovery 语义并入)。
- 每实现一批 provider,同步更新 public capability matrix 与 README 口径。
