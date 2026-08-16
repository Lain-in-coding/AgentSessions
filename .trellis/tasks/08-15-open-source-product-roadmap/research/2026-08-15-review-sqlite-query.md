# Research: SQLite Adapter 查询路径审查(Query/Search Hot Path)

- **Query**: 审查 agent-session-grep SQLite adapter 的 QUERY/SEARCH 路径——FTS5 查询构建、bm25 评分、EXPLAIN 级查询计划、cursor digest、facet EXISTS 探针、group_by_session 去重、prepared statement 复用
- **Scope**: internal(代码 + design 文档 + competitor clone 对照)
- **Date**: 2026-08-15
- **审查对象**: main 分支 `crates/agent-session-grep-adapters-sqlite/src/lib.rs`(SCHEMA_VERSION=7, f4175a0)+ metadata-search 隔离 checkout(SCHEMA_VERSION=11, 7b9675a, 含 session_fts / group_by_session)+ `.trellis/tasks/08-15-structured-activity-context-facets/design.md`(query_faceted v12 计划)

---

## 结论摘要

查询路径无全表扫描、无 N+1(批量 get_many / session_of 均已分块);FTS 注入/语法错误 bug 类(`hp-z8` → `hp NOT z8`)已修复且测试完备。主要问题是:**会话元数据合并查询的相关子查询成本**(S×L 探针)、**深分页每次整窗重排**、**跨投影 bm25 不可比合并**、以及一个 **SQLite 变量上限边界错误**(深分页大窗口下 NOT EXISTS 参数链超 32766)。全部为 P1/P2,无 P0。

---

## 1. FTS5 查询构建(安全与召回)

### 现状(已修复,无需改动)
- `safe_fts_query`(main `lib.rs:4351`;worktree `lib.rs:5626`)——按空白分词、`"` 双写转义、每词包引号、跳过纯标点词;应用层在边界拒绝控制字符(worktree application `lib.rs:1387-1392`);空查询短路(main `lib.rs:4311-4314`)。
- 与 hstry `sanitize_fts_query`(`Github_src/hstry/crates/hstry-core/src/db.rs:3177`)语义等价且更完备(含 13 条测试,worktree `lib.rs:6031`)。**`hp-z8` bug 类在本代码库不存在**——inventory I1 已实质落地。
- CJK bigram(ADR-0007):索引写入与查询两侧同一 transform(`bigram_cjk` 在 application `cjk.rs:30`;写入侧 main `lib.rs:3144-3160`;查询侧先 bigram 后字面量化,顺序敏感——注释与测试已锁定,正确)。

### P2 — 前缀搜索(`work*`)静默失效
- `safe_fts_query("prefix*")` → `"prefix*"`(`lib.rs:4351-4365` 与测试 `lib.rs:4389` 明示这是预期)——`*` 被包进引号内,成为字面字符,FTS5 前缀操作符失效。用户输入 `cargo bu*` 只会匹配字面含 `bu*` 的正文(现实中没有),静默零结果。
- **修复形状**(hstry db.rs:3192-3200 的做法,`*` 保留在引号外):
  ```rust
  let is_prefix = word.ends_with('*');
  let stem = word.trim_end_matches('*');
  if stem.is_empty() { continue; }               // 纯 `*` 跳过
  let escaped = stem.replace('"', "\"\"");
  if is_prefix { words.push(format!("\"{escaped}\"*")); }
  else          { words.push(format!("\"{escaped}\"")); }
  ```
- 注意:CJK 查询的 `*` 经 bigram 后已被拆为独立标点 token 并被跳过(现有逻辑),前缀修复只影响 ASCII 词元。
- 借用候选:hstry `sanitize_fts_query`(MIT,verbatim 可搬,自带测试);Recall `fts5_escape`(inventory I4,OR 语义宽召回)可作为可选补充,不冲突。

---

## 2. bm25 评分与合并排序

### 现状
- `bm25(fts)` 默认参数(k1=1.2, b=0.75),`ORDER BY bm25(fts), id LIMIT ?`(main `lib.rs:4317`;worktree `lib.rs:5346`、`5435`);取负转正分(main `lib.rs:4335`);id 字典序 tiebreak 全序——钉住排序稳定,正确。

### P1(已知延期)— 跨投影 bm25 不可比合并
- 会话元数据命中(`bm25(session_fts)`)与消息命中(`bm25(fts)`)在 adapter 内按裸分合并排序后截断(worktree `lib.rs:5369-5375`、`5461-5467`)。两个 FTS 集合的统计量(idf/文档数)完全不同,分数不在同一尺度——同一查询下会话命中与消息命中的先后由"哪个投影分数尺度更大"决定,与相关性无关。design 已承认(08-14 `design.md:47-48` "backend-local,不是跨后端质量保证"),但**合并时的竞争行为是用户可见的排序缺陷**。
- **低成本修复形状**(adapter 合并循环内按投影归一,不用开完整校准任务):
  - 秩次归一(照搬 memex RRF,`memex/memex-rs/src/search/mod.rs:30,459`,k=60 经典值):每条命中记 `(投影, 秩)`,合并键用 `1/(60+秩)`——两条投影各自排序后再融合,尺度无关;或
  - min-max 归一每条投影的 bm25 到 [0,1] 再合并。
- 战略候选(inventory I8):sessiongrep 的 FTS 召回 limit×5 → 应用层加权重排(`Github_src/sessiongrep/src/db.rs:281-392`,title 600/summary 450/cwd 350/preview 250/transcript 100 + 全 token 命中 +150 + 新鲜度 + 当前 repo +200)。worktree 已有重排脚手架(`assemble_search_hit` + `guidance::literal_terms`,application `lib.rs:723`),只差"分数重排"这一层。SkimMatcherV2 需引入 fuzzy-matcher 依赖,可只搬子串加权表。

### P2 — snippet 是前缀截断,不居中命中词
- `text.chars().take(max_snippet_chars)` 前缀截取(worktree application `lib.rs:723+`),命中词可能在片段外。可选:sessiongrep `snippet_from_match`(db.rs:321/352)或 hstry SQL 层 `snippet()`(db.rs:2102)。纯展示质量,非正确性。

---

## 3. 查询计划(EXPLAIN 级推理)

### 好的部分(无全扫描)
- `fts` MATCH → FTS5 索引;`fts_ids.id_json` UNIQUE 索引;`message_placements` 复合索引 `(session_id, document_id, source_ordinal, placement_id)`(worktree `lib.rs:1437-1442`)完美服务 representative 子查询的 `WHERE session_id = ? ORDER BY document_id, source_ordinal, placement_id LIMIT 1`(worktree `lib.rs:5170-5205`),无需 sorter;catalog 按 PK;session_of / get_many 分块 IN(worktree `lib.rs:4225-4268`,main `lib.rs:67` chunk_ids 先例)。
- `asg_instant_sort_key` 为注册的标量函数(worktree `lib.rs:1131`),只在 EXISTS 探针内按行调用,单行成本可忽略。

### P1 — `append_session_metadata_hits` 的相关子查询成本(S×L 探针)
worktree `lib.rs:5153-5311`。对**每个**匹配 session_fts 的候选行(WHERE 求值先于 ORDER BY/LIMIT,即全部匹配会话,不是返回的 top-N):

1. **representative 子查询被求值两次**(worktree `lib.rs:5167` 与 `5192` 是两个逐字相同的 `(SELECT representative.message_id ... LIMIT 1)` 标量子查询,分别喂 id_json 与 message_wire)。SQLite 不会对同一行的两个相同标量子查询做公共子表达式消除——每行 2 次索引扫描。
2. **NOT EXISTS 链按消息命中数线性增长**(worktree `lib.rs:5213-5229`):`message_wires` 每条追加一个相关子查询(placements 定位 + catalog PK + json_extract role)。代价 = 匹配会话数 S × 消息命中数 L 次探针。宽查询(常见词命中 500 会话 × 100 条消息 = 5 万次探针,每次 2-5µs)→ 数百毫秒;30k 条消息时(见 §7 变量上限)更是 千万级。

**修复形状**:

```sql
-- (a) representative 只求值一次:合并为单个派生表/左连接
LEFT JOIN (
    SELECT rep.message_id, fi.id_json
    FROM message_placements rep
    JOIN catalog m ON m.id = rep.message_id
    LEFT JOIN fts_ids fi ON fi.wire_id = rep.message_id
    WHERE rep.session_id = sfi.session_wire
      AND COALESCE(CASE WHEN json_valid(m.payload)
                        THEN json_extract(m.payload, '$.role') END, '')
          NOT IN ('system', 'developer')
    ORDER BY rep.document_id, rep.source_ordinal, rep.placement_id
    LIMIT 1
) rep ON 1=1
```
(等价语义,每行 1 次而非 2 次;LEFT JOIN 派生表 LIMIT 1 保持确定性)

```sql
-- (b) 逐条 NOT EXISTS → 一次会话级排除集合判定
AND sfi.session_wire NOT IN (
    SELECT DISTINCT session_id FROM message_placements
    WHERE message_id IN (?, ?, ...)      -- L 个参数,子查询物化一次
)
```
`IN (子查询)` 的参数列表是有界子查询,SQLite 求值一次;S×L 探针降为 1 次集合扫描 + S 次哈希成员检查(见 §7 对参数数的连带修复)。语义不变:排除"含任一命中消息且该消息为非 system 角色"的会话——原 NOT EXISTS 语义完全相同。

### P1(边界)— 深分页整窗重排 + 双倍 payload 读取
- 分页模型为"整窗重查 + skip":`fetch = offset + page + 1`,grouped 模式 ×16(`GROUP_SCAN_FACTOR`,worktree application `lib.rs:60-63`),封顶 `MAX_FETCH_WINDOW = 1<<20`(`lib.rs:58`)。每页重跑 FTS MATCH + bm25 排序(排序器按 LIMIT 有界,代价 O(M log N),M=全部匹配行),grouped 模式再对扫描窗做应用层分组(skip 前 O(N))。offset 越深,单页代价线性增长。
- FTS5 限制:`bm25()` 只能用于 ORDER BY / SELECT,不能进 WHERE——SQL 层 keyset 分页不可行;这是 FTS5 固有约束,非实现缺陷。
- **缓解形状**:(a) 维持窗口封顶(已有);(b) 战略性:sessiongrep 式两段检索(FTS 召回 top-K + 应用层重排)把钉住排序挪到应用层,可 keyset;或(c) generation 门控的排序 id 列表缓存(查询 + generation 作 key)。本地单用户工具,severity 维持 P1 下沿/P2 上沿,深分页场景是首要热点。
- **P2 明确浪费**:grouped 路径对扫描窗**两次** `get_many`(系统噪声过滤 `lib.rs:1448-1457` 一次、装配 `lib.rs:1464-1466` 一次),第二次整窗重读。修复:复用第一次的 payload 结果(同一函数内,按 kept/dropped 分流),窗口大时省一半 payload I/O。

---

## 4. Cursor digest(BLAKE3 签名)

- **成本:可忽略,无需缓存**。摘要按 token 计算,不是按页内行数:`issue()` = 1 次 blake3(约 150-200 字节 claims JSON)+ base64url(cursor.rs `119-187`);`verify()` = 解码 + 1 次 blake3 + 等值比较。单 token 约 1-2µs,每页一次。task 担心的"per page"成本不成立。
- 设计良好:域前缀 `as-cursor-v1` 绑定版本(cursor.rs:18)、8 字节截断摘要、`issue_is_deterministic` 测试(cursor.rs:279)、查询/排序/世代绑定全有(verify 顺序 结构→摘要→JSON→major→过期→generation→查询/排序)。
- P2 备注:`query_digest` 绑定原始用户查询(application `lib.rs:525`),safe_fts_query/bigram 在 adapter 侧——两个不同查询映射到同一安全查询也视为不同查询,偏保守,正确。无需改。

---

## 5. query_faceted EXISTS 探针(v12 设计,未实现)

来源:08-15 `structured-activity-context-facets/design.md:285-298` + schema `230-249`。

- **索引使用正确**:main_only 探针 `fts_ids.id_json` UNIQUE 定位 → `message_placements_message(message_id)` 二次定位(design.md:290-294);tool_kind/name 探针走 `tool_activities_message(message_id)`(design.md:296-298)。均为 seek 而非扫描,无全扫描风险。
- **同一相关子查询成本隐患**(同 §3):探针对**每个** FTS MATCH 候选行求值,先于 LIMIT。大目录宽查询下按行 ×3 次索引 seek。逃生口(目录大到成为瓶颈时):两段式——`CREATE TEMP TABLE AS SELECT ... FROM fts WHERE fts MATCH ? ORDER BY bm25(fts) LIMIT ?`(物化时保留钉住排序)→ 再与 facets 表索引连接。
- **P2 索引形状建议**:design 目前是三个单列索引 `tool_activities_message/kind/name`(design.md:240-242)。谓词恒为 `ta.message_id = ? AND ta.kind = ?`(或 name)——建复合索引 `(message_id, kind)` / `(message_id, name)` 一次 seek 覆盖两个谓词,免去按 message_id seek 后再过滤 kind(常见 kind 如 file 时过滤量大)。
- **语义检查(非 bug,记录)**:`main_only` 的 `NOT EXISTS (... is_sidechain = 1)` 意味着"消息只要有任一 sidechain placement 即被排除"——若一条消息同时有 main 与 sidechain 两个 placement,main_only 会丢它。与 PRD 的 main_only 语义是否一致需在实现时确认(design 未明说)。

---

## 6. group_by_session 去重与 occurrences

- **算法高效,无需改**:应用层 HashMap 分组(`group_index: HashMap<String, usize>`),O(扫描窗) 次哈希操作,保持钉住顺序内首次出现为组代表、`occurrences += 1` 计数(worktree application `lib.rs:1481-1495`)。String 键有分配,但窗口封顶 1M、典型 16×page,可接受。
- **语义边界(已知,记录)**:去重窗口是扫描窗——排除集只含窗内消息命中(adapter 的 NOT EXISTS 用窗内 message_wires)。命中全部落在窗外(如系统噪声饱和或高 offset)的会话仍会以 metadata-only 形式出现;设计已注明边界行为(08-14 design.md:42-48),非缺陷。
- **metadata-only 命中在默认路径不被系统噪声过滤误杀**(已核验):`payload_role_is_system_noise`(worktree application `lib.rs:708-718`)只对带 `role: system|developer` 的 payload 返回 true;session payload 无 role → 保留。正确。

---

## 7. Prepared statement 复用与参数绑定

- **P2 — 无语句缓存**:全部路径 `conn.prepare` 每次新建(main `lib.rs:4315`;worktree `lib.rs:5273/5344/5439`),无 rusqlite `prepare_cached`。每次查询重编译(约 10-50µs)。无 filter 热路径 SQL 形状固定,值得 `prepare_cached`;filter 路径 SQL 随谓词变化,缓存命中率低,保持现状即可。
- **P1(边界)— NOT EXISTS 参数链可能超 SQLite 变量上限**:
  - rusqlite 0.40.1 + libsqlite3-sys 0.38.1(bundled SQLite 3.50 系),`SQLITE_MAX_VARIABLE_NUMBER` 默认 **32766**(3.32+ 已从 999 提高)。
  - 深分页:offset ≥ 32755 时 `fetch = offset+page+1 > 32766`,消息查询返回 >32766 行 → `message_wires` 同长 → session SQL 拼接 >32766 个 NOT EXISTS 子句与参数 → **`too many SQL variables` 后端错误**直接抛给用户(worktree `lib.rs:5212-5229`);grouped 模式更早(offset ≥ ~2048 时 ×16 即超限)。
  - 这是合法深分页可触发,非仅伪造 cursor;错误分类为 `PortError::Backend`,用户侧表现为"查询失败",分页中断。
  - **修复**(与 §3(b) 同源):NOT EXISTS 链改为一次 `NOT IN (SELECT DISTINCT session_id FROM message_placements WHERE message_id IN (...))`,并把 message_id 列表分块(≤900/块,沿用 `chunk_ids` 先例 main `lib.rs:67`)——同时消灭参数上限与 S×L 探针两个问题。
- **参数绑定风格**:`params![...]` 与动态 `Vec<Box<dyn ToSql>>` 均干净,无注入风险(全部绑定参数,零字符串拼接进 SQL 的查询值)。

---

## 严重度汇总

| # | 位置 | 严重度 | 内容 |
|---|---|---|---|
| 1 | worktree lib.rs:5213-5229 | P1 | NOT EXISTS 参数链超 32766 上限,深分页硬错误;改为单次 NOT IN + 分块 |
| 2 | worktree lib.rs:5153-5311 | P1 | 会话合并相关子查询 S×L 探针 + representative 双次求值;修复形状见 §3 |
| 3 | worktree lib.rs:5369-5375 | P1 | 跨投影 bm25 裸分合并,排序竞争与相关性无关;RRF k=60 或 min-max 归一 |
| 4 | worktree app lib.rs:1425-1435 | P1/P2 | 深分页整窗重排 O(M log N) 每页;FTS5 限制下 keyset 不可行,两段检索为战略解 |
| 5 | lib.rs:4351-4365 | P2 | `work*` 前缀搜索静默失效;`*` 移到引号外(hstry 形状) |
| 6 | worktree app lib.rs:1448-1466 | P2 | grouped 路径扫描窗双重 get_many;复用第一次结果 |
| 7 | 全路径 | P2 | 无 prepare_cached;热路径固定 SQL 值得用 |
| 8 | 08-15 design.md:240-242 | P2 | tool_activities 建议复合索引 (message_id, kind)/(message_id, name) |
| 9 | cursor.rs:119-187 | 无 | BLAKE3 每 token 一次,1-2µs,无需缓存 |
| 10 | group_by_session(application lib.rs:1481-1495) | 无 | HashMap 分组 O(N),高效 |

## 借用候选(peer refs)

- hstry `sanitize_fts_query` — `Github_src/hstry/crates/hstry-core/src/db.rs:3177`(前缀 `*` 处理,§1 P2)
- hstry `search()` SQL 模板 — 同文件 `db.rs:2102`(snippet() + bm25 + 条件链,§2 参考)
- memex `rrf_fusion` + `RRF_K=60` — `Github_src/memex/memex-rs/src/search/mod.rs:30,459`(§2 跨投影归一,verbatim 约 20 行)
- Recall `rrf_merge` / `hybrid_search` — `Github_src/Recall/src/db/search.rs:76,263`(同上,二选一)
- sessiongrep 应用层重排 — `Github_src/sessiongrep/src/db.rs:281(search), 395(fts_candidate_ids)`(两段检索 + 加权打分 + snippet_from_match;Apache-2.0 需 NOTICE)
- 本仓库已有:分块 IN 先例 `chunk_ids`(main lib.rs:67)、重排脚手架 `assemble_search_hit` + `guidance::literal_terms`(worktree application)

## Caveats

- session_fts 合并/group_by_session 在 worktree `session-metadata-search-08-15`(7b9675a),尚未合入 main;main 分支(Schema v7)查询路径只有 §1/§2/§7 的 message-only 部分。
- 所有行号 2026-08-15 实测;worktree 后续合入 main 时行号会漂移。
- 未跑 EXPLAIN 实测(只读审查,未开库);§3 的计划推理基于 schema 与索引形状,建议实现侧用 `EXPLAIN QUERY PLAN` 复核 representative 子查询是否走 `message_placements_session_order`。
- 08-15 structured-activity design 的 query_faceted 尚未实现,§5 是对设计的审查而非代码。
