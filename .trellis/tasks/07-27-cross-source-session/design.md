# Design: Cross-source session identity

本文件是本子任务的**冻结技术设计**。范围窄而深：一个会话跨多个源文件时，
容器实体（Session / Document 引用）必须按并集合并，而不是让每个源各自声明一份
互相冲突的投影。

## §0 关键决策与偏差（review 可否决）

### 0.1 合并发生在提交层，不改端口 trait

两条可行路线：

| 路线 | 改动面 | 否决/采纳理由 |
|---|---|---|
| A. 提交层按并集合并容器 payload | `adapters-sqlite` 冲突判定 1 处 + `cli` 写入 1 处 | **采纳** |
| B. 容器不存成员列表，读取时从 membership 反查 | `ports::CatalogStore` 加方法 + 所有实现 + App 读取路径 | 否决 |

否决 B 的实据：`crates/agentsessions-ports/src/lib.rs:82-97` 的 `CatalogStore`
只有 `get`/`put`/`list`/`count`/`active_generation`；按会话反查成员的能力目前
只存在于 sqlite adapter 的私有方法 `source_message_ids`
（`crates/agentsessions-adapters-sqlite/src/lib.rs:590`）。走 B 要给端口加一个
"按容器反查成员"的方法，波及 trait、blanket impl、测试替身与 App 的读取路径，
且把"会话成员从哪来"这一语义从 ingest 挪到查询期，改动面远大于本缺陷本身。

A 的代价是容器 payload 在每次相关源变化时都要重算并集——这正是它的正确语义。

### 0.2 会话定义：声明同一 native session id 的所有源的并集

一个 Session 的成员消息 = 所有声明该 `sessionId` 的源文件贡献的消息之并集，
按 wire id 升序稳定排序（不是按源顺序，也不是按时间——时间字段可缺失，
排序必须只依赖已有的确定量）。

实测依据：某真实会话被 55 个不同 `.jsonl` 文件各自声明（每个文件内部只有
1 个 distinct `sessionId`），即 Claude Code 的会话续写/分片形态。

**这不修改 RFC-0001。** `RFC-0001-canonical-model-and-stable-id.md` §3.2 的
`Session` 实体本就没有 `document` 字段，只有 `source_instance_id`；正典模型
从未声明"一会话一文档"。当前实现把 `{document, messages[]}` 写进会话 payload
是实现层越界。本任务让实现回归 RFC，不需要 RFC 修订或新 ADR。

### 0.3 `document` 字段从单值改为多值，保留单值别名

会话 payload 的形状变化：

```jsonc
// 旧（越界）
{ "document": "doc_v1_a", "messages": ["msg_v1_1", "msg_v1_2"] }

// 新
{
  "documents": ["doc_v1_a", "doc_v1_b"],   // 升序、去重，全部贡献源的文档
  "document":  "doc_v1_a",                 // = documents[0]，兼容别名
  "messages":  ["msg_v1_1", "msg_v1_2"]    // 并集，wire id 升序
}
```

保留 `document` 单值别名的理由：`crates/agentsessions-application/src/lib.rs:531`
用它取文档 payload 供证据 fingerprint 使用，`tests/e2e.rs:309` 也断言了它。
删掉会牵动 App 证据装配与既有断言；保留别名让本任务的读取侧改动为零。

**诚实边界**：`document` 别名在多文档会话上只是"某一个"文档，不代表全部。
App 的证据 fingerprint 因此在跨源会话上可能指向非该消息所属的文档。这是本任务
**明确不修**的已知不精确（见 §5），要如实记入 spec 与成熟度矩阵，不能假装
证据精度未受影响。

### 0.4 不做的事

- 不改 `ports` trait，不改 schema（无 v7 迁移）。
- 不改 provider adapter：会话 native id 的产出方式不变。
- 不修 §0.3 的证据 fingerprint 不精确问题（需要按消息反查所属文档，属独立任务）。
- 不改 cursor / 预算 / 分支选择 / MCP / TUI 的任何契约。
- 不把任何 R0 治理记录标 Accepted。

## §1 文件所有权（并行边界）

| Owner | 文件 | 说明 |
|---|---|---|
| agent `merge-commit` | `crates/agentsessions-adapters-sqlite/src/lib.rs` | §2 容器合并 + 单测 |
| agent `session-entries` | `crates/agentsessions-cli/src/main.rs` | §3 `staged_to_entries` 形状 |
| 主会话 | `crates/agentsessions-cli/tests/e2e.rs`<br>`.trellis/spec/**`<br>`docs/**` | §4 e2e、spec、证据与矩阵更新 |

两个 agent 的文件互不相交。`application/src/lib.rs` **无人改**——读取侧靠
§0.3 的别名保持不变，这是本设计的核心收益。

## §2 提交层容器合并（agent `merge-commit`）

改 `commit_source_batches_if_changed`（`lib.rs:484`）的合并循环
（当前 `lib.rs:513-534`）。

现状：同一 wire id 在两个源上 payload 不同即 `Err("conflicting projections")`。

新规则，按实体 kind 分流：

| kind | 跨源同 id 且 payload 不同时 |
|---|---|
| `IdKind::Message` | **仅 `session` 字段不同 → 合并该字段**（见 §2.1）；其余字段不同仍报错 `conflicting projections` |
| `IdKind::Session` / `IdKind::Document` | **合并**：见下 |

### 2.1 消息的 `session` 归属也是多值（2026-07-27 实测追加）

首轮修复上线后真实语料仍 exit 6，冲突对象换成了 `msg_v1_`。实测最小复现：
同一条消息（同 `uuid`、同 `parentUuid`、同文本、同时间戳）出现在 **3 个不同
`sessionId`** 的文件里。这是 Claude Code `resume`/fork 的正常形态——新会话文件
会把被续接的历史消息原样复制进去。

差异只在消息 payload 的 `"session"` 字段。根因与容器缺陷同源：**我们把"消息属
于哪个会话"写成了消息自身的单值属性**，而它实际是多对多关系。

因此消息合并规则：

- `sessions`：两侧 `session`/`sessions` 的并集，升序去重（归属集合无顺序语义）。
- `session`：`sessions[0]`，单值兼容别名（`tui/mod.rs:191`、`e2e.rs:286` 在读它）。
- **其他任何字段不同仍然报错**——文本、parent、span、时间戳的分歧是真冲突。

诚实边界：`session` 别名在被多会话共享的消息上只是"某一个"归属，不代表全部。

容器合并算法（纯函数，必须单独可测）：

```rust
/// 合并两份容器 payload 为并集。两侧都必须是 canonical JSON 对象，
/// 否则是 ingest 写坏了 —— 报错，不静默取一侧。
fn merge_container_payload(left: &[u8], right: &[u8]) -> PortResult<Vec<u8>>
```

- `messages`：两侧数组并集，去重，**wire id 升序**排序。
- `documents`：同上；若一侧只有旧的单值 `document` 字段（升级前入库的行），
  视作单元素集合参与并集。
- `document`：置为合并后 `documents[0]`（升序后的最小者，确定性）。
- 其他字段：两侧必须相等，否则报错——不猜。
- 非对象 / 非法 JSON：报错 `PortError::Backend`，措辞点明是容器 payload 问题。

`text`（索引正文）：容器实体的索引正文恒为空串，两侧都空则合并结果为空；
若任一侧非空则报错（容器不该进 FTS，非空即 bug 信号）。

合并必须发生在 `batch_manifest` 之前（`lib.rs:562`），因为 manifest 要求
upsert id 去重——现有 `merged` map 正是这个位置，合并逻辑就替换其冲突分支。

**tombstone 语义无需新增代码**：`message_referenced_by_unscanned_source`
（`lib.rs:645`）已按 membership 反查，容器 id 落在 membership 里，因此
"仍被未扫描源引用的会话不会被误删"自动成立。单测要锁定这条。

**no-op 判定无需改**：`source_batches_are_current`（`lib.rs:605`）逐源比较
membership 与该源 entries 全等；容器共享后每源 entries 仍含容器 id，比较依旧
成立。但注意：**已存库在本次升级后首次 sync 会因容器 payload 变化而正常推进
一个 generation**，这是正确行为（内容确实变了），要在单测里断言而非视为回归。

单测（写在该文件既有 `#[cfg(test)] mod tests` 内）：

1. `merge_container_payload` 纯函数：并集、去重、升序、`document` = 首元素。
2. 旧单值 `document` 与新 `documents` 混合输入的并集正确。
3. 两侧其他字段不等 → Err。
4. 容器 payload 非 JSON / 非对象 → Err。
5. 容器 `text` 非空 → Err。
6. 消息 id 跨源 payload 冲突 → 仍 Err（回归护栏，不能被合并逻辑放过）。
7. 两个源声明同一会话 → `commit_source_batches_if_changed` 成功，读回的会话
   payload 含两源全部消息。
8. 三源同一会话，其中一源在后续 sync 中消失 → 会话仍存在且成员为剩余两源之并集。
9. 同一会话的两个源分两次 sync（非同批）→ 第二次的会话 payload 是两源并集，
   不是覆盖成第二个源的成员。

第 9 条是本任务的核心正确性：跨批次也要并集，不能后写覆盖先写。实现上
合并的 `left` 需要取**库中已有的**容器 payload，而不只是本批内的另一个源。
这是与"仅本批合并"最容易写错的地方——`merged` map 初始化时，若 id 是容器
kind，必须先 `self.get(id)` 取库中现值作为合并起点。

## §7 实施记录：真实语料暴露的四层差异（2026-07-28）

按上述设计实施后，用真实语料（634 Claude + 73 Codex，605 MB）逐批 sync 复跑，
每修好一层就再撞下一层。四层差异全部同源：**消息身份跨会话共享，但它的"位置"
是 per-source 的量**。

| 层 | 冲突实体 | 差异字段 | 真实成因 | 状态 |
|---|---|---|---|---|
| ① | `ses_v1_` | `messages` 成员表 | 一会话跨多文件（实测 55 个文件声明同一 sessionId） | ✅ 已修（并集 + `document`/`documents`） |
| ② | `msg_v1_` | `session` | resume/fork 把历史消息复制进新会话文件（实测同一 uuid 出现在 3 个 sessionId） | ✅ 已修（`sessions` 并集 + 单值别名） |
| ③ | `msg_v1_` | `span` | 同一消息在各文件的字节偏移不同（实测同记录 rawLen 929 vs 810） | ✅ 已修（`spans` 按 document 键并集 + 单值别名） |
| ④ | `msg_v1_` | `parentUuid` | fork 时同一消息被重新挂到不同父节点（实测 parent 为 `5c6d196c…` vs `5423276f…`） | ❌ **不在本任务修**，见下 |

### 为什么第 ④ 层必须停手

前三层可以并集，因为它们是可叠加的事实：一条消息属于多个会话、在多个文件里有
多个位置，这些同时为真。第 ④ 层不是——`parent` 参与**拓扑语义**：
`select_mainline`（`crates/agentsessions-domain/src/thread.rs`）沿单一 parent 链
从叶子回溯，一条消息若有多个 parent，"主线"本身没有定义。把 `parent` 并成数组
只会让分支选择在多个合法答案里静默取一个，等于用不确定性换取 sync 不报错——
比报错更糟。

RFC-0001 §3.2 早已给出正确形状：父子关系是独立的 `MessageEdge`
（`parent_message_id` / `child_message_id` / `relation`）关系表，而不是消息实体
上的字段；同理 `Message` 带 `session_id` / `thread_id` / `branch_id`，位置不压在
内容里。四次撞墙说明当前实现把"位置"塞进了消息 payload，因此每暴露一个位置字段
就多一处冲突。继续在 payload 里叠数组是在修症状。

**结论**：本任务交付前三层（有专项单测 + e2e + 真实语料验证推进：sync 已从 0 条
消息提交推进到 7060 条），第 ④ 层作为独立子任务处理，范围是让实现回归 RFC-0001
的边表模型。真实语料 sync 端到端通过**尚未达成**，证据文档必须如实记录。

## §3 会话/文档条目形状（agent `session-entries`）

改 `staged_to_entries`（`crates/agentsessions-cli/src/main.rs:758`）里的
`session_payload` 构造（当前 `lib` 行 `840-843` 附近）：

```rust
let session_payload = serde_json::json!({
    "documents": [document_id.as_str()],
    "document":  document_id.as_str(),   // 兼容别名 = documents[0]
    "messages":  member_ids,
})
```

单源视角下 `documents` 恒为单元素；跨源并集由 §2 的提交层完成。CLI 不做
跨源聚合——它只看见自己这一个源，聚合是存储层的职责。

`member_ids` 保持现状（该源消息按 seq 序），排序由 §2 合并时统一为 wire 升序。

不改该文件其他任何内容。`document_payload`、消息 payload、span 逻辑均不动。

## §4 e2e 与文档（主会话）

`tests/e2e.rs` 新增：

- `session_spanning_two_sources_merges_members`：两个不同文件、同一 `sessionId`
  的合成夹具 → 一次 `sync` 两文件 → `show <ses-id>` 断言 `messages` 含两文件
  全部消息、`documents` 两个、`document` 为升序首个。
- `session_spanning_sources_synced_separately_accumulates`：同上但分两次 sync，
  断言第二次后成员是并集（跨批次不覆盖）。
- `context_over_cross_source_session_walks_the_merged_chain`：跨源会话上跑
  `context --policy mainline`，断言消息链跨越两个文件。

既有 `tests/e2e.rs:309-318` 的 `document`/`messages` 断言必须继续通过
（别名保证），若需调整只能是**新增** `documents` 断言，不得放宽原断言。

文档：spec 记录容器合并规则与 `document` 别名的诚实边界；
`docs/evidence/integration-beta/real-data-regression.md` 追加"修复后复跑"结果；
`PROVIDER-MATURITY-MATRIX.md` 与证据台账按复跑实况更新（**不预先声称通过**）。

## §5 已知不修（必须记录，不得隐瞒）

跨源会话的证据 fingerprint 用 `document` 别名（某一个文档），因此某条消息的
证据可能引用并非它所属的那个文档。修它需要"按消息反查其源文档"的能力
（membership 已有 `document_id` 列，但 App 无端口访问它）。本任务不做，
如实记入 spec 与矩阵已知限制。

## §6 验收门禁（主会话）

- `cargo fmt --all --check` / `cargo clippy --workspace --all-targets -- -D warnings`
  / `cargo test --workspace` / `cargo deny check` 全绿。
- 真实语料回归 harness 复跑：`sync` 必须成功且六条不变量给出真实判定
  （目标是 INV-SYNC-OK 通过；其余不变量若暴露新问题，如实记录不掩盖）。
- 已存库兼容：升级前入库的会话行（只有单值 `document`）在新二进制下
  `show` / `context` 不报错，再次 sync 后升级为 `documents` 形状。
