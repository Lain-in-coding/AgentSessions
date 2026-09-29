# 全项目审查报告（2026-09-25）

## 结论

- 基线：`6cd1e6f6b43f2d9c42a080b2679a6d8a5d801f72`。
- 结论：常规质量门总体健康，但当前实现仍有 **9 个 P1 级正确性/数据完整性缺陷**，集中在 SQLite 活库快照、多会话 SQLite 源、语义检索过滤、MCP 语义入口、provider 过滤能力、native ID 单射性、Unicode 脱敏和时间解析。另有 9 个 P2/P3 缺陷与多个性能/运维优化点。
- 本报告是“审查发现的可行动问题清单”，不能证明不存在其他未知缺陷。

## 已确认缺陷

### P1-1 SQLite 活库快照只复制主库字节，忽略 WAL

- 位置：
  - `crates/agent-session-grep-ports/src/lib.rs:1296`（`read_bounded_source` 把完整 source 读成 `Vec<u8>`）
  - `crates/agent-session-grep-ports/src/manifest.rs:72`（OpenCode/Cursor 被归类为 whole-source SQLite）
  - `crates/agent-session-grep-provider-opencode/src/lib.rs:296`、`crates/agent-session-grep-provider-cursor/src/lib.rs:441`（只写主库 bytes 到临时文件再打开）
  - `crates/agent-session-grep-cli/src/lib.rs:3912`（捕获/校验的是主文件 len+mtime+fingerprint）
- 触发：源 SQLite 处于 WAL 模式且事务仍在 `-wal` 中；主库文件字节不包含最新 schema/rows。
- 影响：读取结果是不完整甚至无法识别的数据库；同步成功/失败都会漏索引，且主文件指纹可能保持不变导致后续 no-op。合成复现中逻辑库有 1 条 part、`-wal` 存在，`ingest` 返回 exit 2 且无 provider 识别（`evidence-output/review-20260925-wal.log`）。
- 建议：对 SQLite source 使用 SQLite Backup API / `VACUUM INTO` 等一致快照机制，或实现行级一致读；快照身份必须覆盖逻辑内容而不是主文件字节。继续严格只读源，不 checkpoint 源库。

### P1-2 一个 OpenCode/Cursor SQLite 文件中的多个会话被合并到第一个会话

- 位置：
  - `crates/agent-session-grep-provider-opencode/src/lib.rs:180`（遍历全部 message/part）
  - `crates/agent-session-grep-provider-opencode/src/lib.rs:229`（多 session 仅诊断“全部消息归属首个会话”）
  - `crates/agent-session-grep-provider-cursor/src/lib.rs:202`（同样把多会话归到第一个）
  - `crates/agent-session-grep-cli/src/lib.rs:3612`（staging 合同只有一个 session native id；3654 起全部 placement 复用它）
- 触发：一个 `opencode.db`/`state.vscdb` 包含两个以上 session。
- 影响：会话边界丢失、上下文串台、resume 元数据归属错误、后续修复需要大规模身份迁移。合成两 session 复现已入库 2 条消息，但 payload 显示两者均挂同一个 `ses_v1_807ca...`（`evidence-output/review-20260925-opencode-multi.log`）。
- 建议：扩展 canonical event/staging 模型，使每条消息携带 provider session identity，或将 SQLite 源拆成多个 session graph/source batch；不要再用 warning 掩盖错误归属。

### P1-3 semantic/hybrid 忽略 provider/time/repo/facet 过滤

- 位置：
  - `crates/agent-session-grep-application/src/lib.rs:1734`（lexical 路径传入 filters/facets）
  - `crates/agent-session-grep-application/src/lib.rs:1749`（semantic 直接 `query_semantic`，没有 filters/facets）
  - `crates/agent-session-grep-application/src/lib.rs:1755`（hybrid 用未过滤 semantic list 与过滤 lexical list 融合）
  - `crates/agent-session-grep-adapters-sqlite/src/lib.rs:8086`（语义查询仅按 model/dimension）
- 触发：请求 `mode=semantic` 且设置 `since`（或 provider/repo/facet）。
- 复现：同一库在 lexical + `since=2050` 返回空；semantic + 同一过滤返回 3 条（`evidence-output/review-20260925-core.log`）。
- 影响：用户以为过滤生效，实际得到跨时间/provider/repo/sidechain 的结果；hybrid 还会被未过滤候选提升排名。
- 建议：在最终 limit 前对语义候选执行同一 metadata 过滤，理想路径下推到 SQL；分页必须处理过滤导致的候选耗尽。

### P1-4 MCP 的 semantic/hybrid 必然降级为 lexical fallback

- 位置：
  - `crates/agent-session-grep-cli/src/mcp.rs:500`（MCP 生成 query embedding）
  - `crates/agent-session-grep-cli/src/mcp.rs:557`（随后调用通用 `run_app`）
  - `crates/agent-session-grep-cli/src/mcp.rs:860`（`run_app` 固定构造无 semantic slot 的 `resume_app`）
  - `crates/agent-session-grep-cli/src/lib.rs:2910`（`resume_app` 使用 `NoSemanticIndex`）
  - 对照 `crates/agent-session-grep-cli/src/lib.rs:1902`（CLI semantic 使用 `resume_semantic_app`）
- 复现：合成库已执行 `index embeddings`，MCP `search_sessions mode=semantic` 返回 `retrieval_mode: lexical_fallback` 和 warning（`evidence-output/review-20260925-mcp-semantic.log`）。
- 影响：MCP 宣称的能力与实际不一致，浪费模型计算，且和 CLI/Web 行为分叉。
- 建议：抽出共享 semantic resolver/App 构造；MCP 在 semantic/hybrid 时注入同一 `SqliteStore` SemanticIndex，或让 `run_app` 接收已解析的 semantic context。

### P1-5 搜索 provider 过滤只支持 2 个，能力矩阵却登记 14 个可 ingest/search provider

- 位置：
  - `crates/agent-session-grep-ports/src/lib.rs:264`（`SearchProvider` 仅 Claude/Codex）
  - `crates/agent-session-grep-cli/src/mcp.rs:46`（MCP schema 同样只有 claude/claude-code/codex）
  - `crates/agent-session-grep-cli/src/lib.rs:3012`（实际注册 14 个 adapter）
`--provider grok-build` 同样返回 `unknown provider ... expected claude|claude-code|codex`，而 `providers` 命令列出 16 行、14 个 ingestible。
- 影响：Grok/Pi/Kimi/Qoder/OpenClaw/CodeBuddy/OpenCode/Cline/Hermes/Antigravity/Cursor/Aider 等已索引数据无法按 provider 收窄。
- 建议：以 capability registry 为唯一事实源生成 canonical provider ID/alias/filterable 集合，CLI/MCP/Web 共用 canonicalization。

### P1-6 `StableId::native` 对 provider 消息 ID 的清洗不是单射

- 位置：`crates/agent-session-grep-domain/src/ids.rs:143`（trim、删除全部控制字符、截断 256 chars）；`crates/agent-session-grep-cli/src/lib.rs:3561`（消息 native id 走该构造）。
- 复现：`StableId::native(Message, "a\0b") == StableId::native(Message, "ab")`，wire 均为 `msg_v1_ab`（`evidence-output/review-20260925-core.log`）。
- 影响：provider-controlled id 可导致两条不同消息合并、payload 冲突或静默覆盖；超长 id 也可能前缀碰撞。
- 建议：新增 checked native 入口：控制字符/空白/超长直接拒绝本次写入；或对原始 id 做显式 namespaced hash。历史已清洗 identity 需独立 alias/migration，不能普通 sync 静默改写。

### P1-7 跨边界脱敏在 `AKIA` + 多字节字符上 panic

- 位置：`crates/agent-session-grep-ports/src/redact.rs:108`（`s[..20]` 未检查 UTF-8 char boundary）。
- 复现：`redact_text("AKIA" + 6 个“中”)` 在 debug 下 panic：byte index 20 位于 UTF-8 字符中间（`evidence-output/review-20260925-core.log`）。
- 影响：Robot/MCP/Web/Handoff 的任一字符串值都可能让进程崩溃， secret 不但没有脱敏，还造成拒绝服务。
- 建议：改为 `chars()` 校验或 `is_char_boundary(20)`；补 CJK/emoji/组合字符 property 测试，保持 ruleset v1.1 行为不变。

### P1-8 Application 时间解析存在 debug panic 与非法负数时间接受

- 位置：
  - `crates/agent-session-grep-application/src/lib.rs:771`（hour/minute/second 只检查上界，不检查负数）
  - `crates/agent-session-grep-application/src/lib.rs:804`（`days_from_civil` 多步 unchecked 乘加）
- 复现：
  - `9223372036854775807-12-31T00:00:00Z` 在 debug 下 overflow panic；release 会 wrap 为错误 instant。
  - `2026-01-01T-1:00:00Z`、`00:-1:00`、`00:00:-1` 均被 CLI 接受（`evidence-output/review-20260925-negative-time.log`）。
- 影响：用户可触发 panic/错误过滤结果；时间窗口语义不可信。
- 建议：所有日期/时间/offset 计算使用 checked arithmetic；hour/minute/second/day 全部限定 `0..=` 上界。保留与 domain 宽排序 parser 的有意差异并测试锁定。

### P1-9 “只读打开”路径实际可写且可迁移，绕过 writer lease

- 位置：`crates/agent-session-grep-adapters-sqlite/src/lib.rs:1474`（`SqliteStore::open` 用 read-write `Connection::open`）；`:1534` 起 `init` 执行 `PRAGMA journal_mode=WAL` 和 migration。
- 影响：search/get 等读进程可在无 writer lease 情况下创建/升级库；与持 lease 的写进程并发时可能争写或部分迁移，违背注释中的“只读打开”边界。
- 建议：当前 schema 的读路径用 SQLite `OPEN_READ_ONLY`；版本过旧时返回 schema incompatible/需要维护命令，由 `open_for_write` 在 lease 内迁移。并设置有限 busy timeout。

## P2/P3 缺陷

1. **P2 cursor 未绑定 retrieval mode / embedding model / ranking context**：`search_query_digest` 只绑定 query/filters/facets/repo 等（`application/src/lib.rs:845`）， lexical 页 token 可继续 semantic 页并返回重复第一条；模型/排序版本变化也不失效（复现见 core log）。
2. **P2 semantic 系统噪声可提前终止分页**：semantic 是 offset-dependent fetch，系统噪声过滤发生在 `+1` sentinel 之后；复现中后续仍有 user hit，但第一页 `has_more=false`、第二页空（`application/src/lib.rs:1792`、core log）。
3. **P2 `SemanticIndex::is_ready` 吞掉 backend/schema/busy error**：trait 返回 bool，SQLite 实现 `unwrap_or(false)`（`ports/src/lib.rs:723`、`sqlite/src/lib.rs:8158`），真实错误被伪装成合法 lexical fallback。
4. **P2 非有限 semantic score 可进入结果**：坏向量/NaN 经 cosine 得 NaN，`partial_cmp(...).unwrap_or(Equal)` 继续排序并返回（`sqlite/src/lib.rs:8137`）；复现返回 nonfinite score。
5. **P2 message/session FTS raw BM25 直接比较**：两个不同 corpus 的 BM25 在 `query_filtered` 中合并后直接 sort（`sqlite/src/lib.rs:7875-7904`），语料统计不同，排序不可比；应使用 rank/RRF 并版本化 cursor。
6. **P2 no-op 快路径绕过完整 batch integrity validation**：`sources_are_current` 在 duplicate id/placement/overlap 检查之前提前 `Ok(false)`（`sqlite/src/lib.rs:3187` vs `3214` 后续校验）。
7. **P2 OpenCode SQL prepare/row 错误被静默丢弃**：三个查询均 `if let Ok(...)` / `filter_map(Result::ok)`（`provider-opencode/src/lib.rs:133-180`），schema/类型 drift 可解析为空成功。
8. **P2 domain 宽日期 parser 接受不存在日期**：`domain/src/thread.rs:303` 只检查 1..31，不校验月长/闰年；`02-30` 等会得到错误排序 instant，而不是回退/拒绝。
9. **P2 session identity 依赖源绝对路径**：`installation_namespace` 把 `.claude`/`.codex` 之前的完整路径纳入 digest（`cli/src/lib.rs:3117`），用户目录/磁盘迁移会导致 Session ID 改变，与注释的 relocation-invariant 目标相悖；需要既定的 namespace registry/alias migration。
10. **P3 Grok rewind 的 provider-controlled `u64` 直接 `as usize`**：`provider-grok/src/lib.rs:334-337`，32-bit 平台截断后可能 rewind 到错误位置；应 `usize::try_from` 并按 skip/error 处理。
11. **P3 hook 预算判断用 bytes、截断用 chars**：`cli/src/hooks.rs:103-106`，CJK/emoji 会过早截断；`u64 -> usize` 对极端配置在 32-bit 也可能截断。应统一 conservative Unicode-aware estimator。
12. **P3 Web search 是受限 subset**：`serve.rs:835` 未暴露 `max_bytes`、sidechain/tool facets；虽然定位 preview，但应在 capability metadata/UI 中明确，不应被理解为 full parity。

## 优化建议

- **语义索引内存/延迟**：`query_semantic` 每次全表加载所有同模型向量并全部评分后才 truncate（`sqlite/src/lib.rs:8086-8155`）。先修过滤正确性，再引入分页扫描/近似索引/预过滤；用固定数据集 benchmark 验证 p95 与峰值内存。
- **embedding rebuild 流式化**：`build_embeddings` 调 `store.list(usize::MAX)` 并持有全部 entries/payload（`cli/src/lib.rs:2826`）。改为 wire-id keyset/batch 分页，每批嵌入后释放。
- **Session metadata SQL 有界化**：`append_session_metadata_hits` 为每个消息 hit 拼一个 correlated `NOT EXISTS`（`sqlite/src/lib.rs:7614-7629`）。改为 bounded candidate CTE/temp values + chunk helper，并用 `EXPLAIN QUERY PLAN` 验证。
- **依赖清理**：cargo-deny 报 duplicate `unicode-width`/`windows-sys`；cargo-audit 有 3 个 allowed warnings（paste unmaintained；ratatui 依赖的 lru 0.12.5 两条 unsound advisory）。升级 ratatui/依赖后复跑 audit，不应急于 ignore。
- **跨入口合同测试**：为 CLI/Web/MCP 增加 semantic readiness、provider 全集、filter/facet、cursor mode/model mismatch 的非 vacuous 对比测试。

## 质量门与证据

| 检查 | 命令/证据 | 结果 |
|---|---|---|
| 格式 | `cargo fmt --all --check` | 通过 |
| Clippy | `cargo clippy --workspace --all-targets --offline -- -D warnings` | 通过 |
| Debug tests | `cargo test --workspace --offline` | 80 个 suite：1656 passed，20 ignored，0 failed |
| Semantic feature | `cargo test -p agent-session-grep-application --features semantic-candle --offline` | 261 passed，0 failed |
| Python scripts | scripts / release / evidence suites | 18 + 9 + 58 tests 全通过（scripts 中 1 skipped 为既有预期） |
| Release tests | `cargo test --workspace --offline --release` | 80 个 suite：1656 passed，20 ignored，0 failed |
| Supply chain | `cargo deny check` | advisories/bans/licenses/sources 全 ok，有 duplicate dependency warnings |
| RustSec | `cargo audit --file Cargo.lock` | exit 0；3 个 allowed warnings |
| 复现 | core/MCP/WAL/OpenCode multi/negative time logs | 全部在 `evidence-output/review-20260925-*.log` |

## 覆盖范围与限制

覆盖了 20 个 workspace crate：domain、ports、application、SQLite adapter、CLI/MCP/Web/TUI/hooks/resume、14 个 provider、testkit，以及 schemas/scripts/CI/install/release 的静态审查和可用测试。未使用真实用户 transcript、外部账号或私钥；未验证 Linux/macOS 运行时、32-bit target、真实 provider 数据分布、浏览器端视觉完整性或模型推理质量。源码证据和合成数据只能证明上述路径存在，不代表所有真实格式变体均已覆盖。
