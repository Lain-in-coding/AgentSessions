# 借鉴复用与许可审查报告（R0 产物）

> 治理记录（Governance Record）
>
> - decision_id: R0-REUSE-LICENSE-AUDIT
> - status: **Draft（待 approver 审阅）**
> - owner: （待指派）
> - approver: 项目最终验收人
> - due_milestone: R0 Feasibility / Contract Gate
> - evidence_path: `docs/operations/REUSE-LICENSE-AUDIT.md`、计划 §19
> - approved_at: —
>
> 本报告把计划 §19 的复用矩阵固化为独立审查文档。任何实际复用前，必须由本报告的 approver 逐项复核许可、来源坐标与 attribution 义务；本报告不替代逐次复核。

---

## 1. 审查范围

对 `Github_src/` 下 12 个同类开源项目的调研结论做许可与复用判定。判定分四级：

- **direct-copy**：许可 + 语言兼容，保留版权/NOTICE、标注修改后可直接复制到本项目 Rust 源码；
- **adapt**：设计与实现可移植，但需按本项目 Canonical/契约重写，不逐行照抄；
- **idea-only**：仅借鉴设计思想或契约形状；语言不匹配（Swift/TS）、许可存疑或有实测缺陷者降到此级；
- **reject**：许可红线、反模式或半成品，禁止复用。

## 2. 全局硬约束（Release 阻断级）

1. **cass 是唯一法律红线**：其 LICENSE 含 OpenAI/Anthropic Restricted-Party Rider，代码整体 **reject**，只能由未读其源码者 clean-room 借鉴抽象思想。任何直接 copy/analyze/index 其代码的行为都是许可违约。
2. **fixtures 一律重造**：所有调研项目的 fixtures provenance 未核实，禁止复制任何真实 transcript；本项目按 `docs/security/FIXTURE-REDACTION-POLICY.md` 生成合成数据。
3. **语言边界**：本项目是全新 Rust 项目。Rust 仓（ctx / agf / Recall / hstry / memex / sessiongrep / fast-resume）代码在许可允许下可 copy/adapt；Swift/TS 仓（agent-sessions / AgentRecall / claude-historian-mcp）只能 idea-only。
4. **依赖许可单列**：任何被复用资产引入的传递依赖（如 agf 的 nucleo 系列为 MPL-2.0，须 attribution；tantivy 传递引入 ort-sys 许可不干净——已由 spike 实证）必须进 SBOM 与 `cargo deny` 白名单，未过 deny 门不得合入。

## 3. 可复用项判定（经独立复核 CONFIRMED）

| 来源(许可) | 资产 | 判定 | 目标位置 | attribution |
|---|---|---|---|---|
| ctx (Apache-2.0) | WAL PRAGMA 组 + doctor integrity_check | direct-copy | §9.1/§8.2 | 保留版权头 |
| ctx | 契约层 + additive-field 保留测试、snake→camel | adapt | §7.2 | — |
| ctx | 单调 user_version 迁移 + events 去重索引 + sha256 前缀 checkpoint | adapt | §6.4/§9.1 | — |
| ctx | CJK bigram 分词（scriptgram） | adapt | §6.2 | — |
| ctx | 只读 SQL 沙箱 raw_sql.rs | idea-only（MCP 不暴露 SQL） | §7.4 | — |
| agf (MIT) | bounded reader（read_head_tail/char_prefix） | direct-copy | §5.2/§8.2 | 保留 MIT 声明 |
| agf | shell quoting / PowerShell 包装 | direct-copy | §7.1 | 保留 MIT 声明 |
| Recall (MIT) | FTS5 external-content + 触发器 | adapt | §6.4/§6.5 [FTS5] | — |
| Recall | RRF k=60、目录子树过滤、迁移幂等骨架、JSONL roundtrip | adapt | §6.3/§11.1 | — |
| sessiongrep (Apache-2.0) | 纯函数 find_repo_root/extract_text/highlight/timeline/escape | direct-copy | §5.2/§6.3 | 保留版权头 |
| sessiongrep | 两段式检索（FTS 召回→Rust 重排） | adapt | §6.3 | — |
| sessiongrep | 全 crates.io 无 vendored 供应链基线 | idea-only（模板） | §12 | — |
| hstry (MIT) | 事务化 migration runner、source-scoped purge、peek bundle | adapt | §7.2/§9.1 | — |
| hstry | version 单调 / outbox / retention 测试 | adapt | §11.1 | — |
| memex (MIT) | compact 分层去重约束 UNIQUE(session_id, source_offset) | adapt | §4.1 | — |
| fast-resume (MIT) | 唯一 Tantivy 集成参考（有实测跨平台 bug） | idea-only | §6.1 Selection Gate | — |
| AgentRecall (MIT, TS) | 迁移 writer 原子协议、capability registry | idea-only | §9.2/§5.1 | — |
| agent-sessions (MIT, Swift) | FTS5+三触发器、索引期脱敏、语料保全测试 | idea-only | §6/§8.2/§11.1 | — |
| claude-historian (MIT, TS) | MCP search→at→get_session 渐进披露契约 | idea-only | §7.4 | 署名归属存疑，引用前澄清 |
| cass (MIT+Rider) | fail-open/staged-publish/robot-freeze 思想 | idea-only（clean-room） | §11/§12.4 | 禁读源码 |

## 4. 反模式登记（作为验收反例，禁止复用）

| 反模式 | 来源坐标 | 已对应守护 |
|---|---|---|
| 扫描期写上游（读操作 DELETE 上游 SQLite） | agf codex.rs prune_orphan_threads | 不变量#2、§16.3 上游 checksum、source-snapshot spike |
| 全局 session_id 主键致跨 Provider 覆盖 | agent-sessions DB.swift | §4.2 身份/定位解耦、RFC-0001 |
| Provider 写入 SQL CHECK 约束致全表重建 | ctx ddl.rs + migrations.rs | §6.4 |
| FTS 空命中回退全表扫描 | sessiongrep db.rs | §6.4 |
| Unix 权限位在 Windows 为 no-op 却宣称已私有化 | ctx object_store.rs | §8.2 Windows ACL |
| 跨入口排序不一致（bm25 ASC vs CLI DESC） | hstry db.rs vs main.rs | §11.3 排序一致性门 |
| 硬编码平台路径前缀 /Users | sessiongrep cursor.rs、Recall cline.rs | §5.3 路径可移植性门 |
| 用户 query 直编正则/有损路径编码 | claude-historian search.ts | §8.2 输入校验 |
| adapter 子进程无超时/无输出上限致 OOM | hstry runner.rs、claude-historian | §5.2 进程内流式+bounded buffer、RFC-0002 |
| mtime as i64 纳秒截断、纯 mtime 判增量 | sessiongrep、agf cache.rs | §4.1 mtime caveat、source-snapshot spike |
| API key 明文入库 | AgentRecall schema.ts | §8 secure by default |
| 向量检索引入 protoc/联网模型构建依赖 | memex LanceDB、Recall candle | §1.4 排除 vector/embedding、cross-platform-packaging spike 安全构建硬门 |

## 5. spike 补充的实证结论

- **Tantivy 供应链风险已实证**：`cargo tree` 确认 tantivy 0.26.1 传递引入 `ort-sys`（ONNX Runtime），`cargo deny` 报 unlicensed。这与 §19.2 的先验判断一致，且从供应链维度进一步支持 FTS5 默认（见 `spikes/search-backend/EVIDENCE.md`、`spikes/cross-platform-packaging/EVIDENCE.md`）。

## 6. 退出条件

- 每项拟复用资产有明确判定、来源许可、attribution 义务；
- cass clean-room 边界有可执行约束（禁止读其源码者以外的人实现）；
- 依赖传递许可进入 cargo-deny 白名单流程；
- approver 签署后本报告转 Accepted，方可在 0.1 之后按判定复用。
