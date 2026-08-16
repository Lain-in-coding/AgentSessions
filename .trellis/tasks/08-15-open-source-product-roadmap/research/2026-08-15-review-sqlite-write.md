# Research: SQLite 写入/摄取路径审查(performance + correctness)

- **Query**: 审查 agent-session-grep adapters-sqlite 的 WRITE/INGEST 路径(批量 INSERT 策略、事务边界、每批开销、outbox+lease、迁移链、内容寻址哈希),对标 hstry/fast-resume/memex
- **Scope**: internal(本仓库代码 + 固定 competitor clone 逐行核验)
- **Date**: 2026-08-15
- **审查对象**: `crates/agent-session-grep-adapters-sqlite/src/lib.rs`(384KB,约 9200 行;以下行号均为实测)
- **严重级**: 无 P0。P1 ×2(性能:同步粒度;正确性:迁移崩溃窗口),P2 ×4。

---

## 0. 前置澄清(任务书与代码不符之处)

1. **"migration chain v7→v12"** — 当前 worktree 实际 `SCHEMA_VERSION = 7`(`lib.rs:3769`),迁移链为 **v1→v7**。v12 是并行任务 `08-15-structured-activity-context-facets/design.md:223-226` 的设计目标("本 worktree base 是 v7…v12 是下一个自由版本"),**不是当前代码现状**。本审查按实际代码 v7 为准。
2. **"act_v1_" id 前缀** — 任务书提到 `msg_v1_/act_v1_/ses_v1_`;当前 `IdKind` 只有 `src_v1_/doc_v1_/ses_v1_/msg_v1_` 四种(`crates/agent-session-grep-domain/src/ids.rs:48-55`),**无 act_v1_**。若 roadmap 需要 activity 实体 id,当前无此 kind(可能与 08-15-structured-activity-context-facets 的 tool_activities 表相关)。
3. **写路径总览**: 两条入口共用一套 durable outbox 两阶段提交(begin_index_batch 持久化 intent → commit_index_batch 单事务 apply+activate):
   - **B1** `commit_batch`(`lib.rs:1488`)— 单/多实体裸 upsert,调用方:`cli/main.rs:1203`(`index_one`)与 `cli/mcp.rs:682`。
   - **B2** `commit_source_batches_if_changed`(`lib.rs:1618`)— 源批 + 关系 + tombstone/union 语义,调用方:`cli/main.rs:1554`(`ingest_file`)、`main.rs:1681`(`sync_files`)、`cli/tui/mod.rs:396,427`。

---

## 1. Batch INSERT 策略(对标 hstry bulk_insert)

### 现状
- `commit_index_batch_with_relations`(`lib.rs:3090-3172`):prepared statement 已提升到实体循环外(3101-3104 注释承认历史"200K 实体 × 5 条语句每次 prepare/finalize"问题,已修),但仍是**单行式**执行,每条消息 5 条语句:
  1. `INSERT INTO catalog ... ON CONFLICT DO UPDATE`(3108)
  2. `DELETE FROM fts WHERE rowid=(SELECT fts_rowid FROM fts_ids WHERE wire_id=?)`(3118,按 rowid 定位,已避免 O(N²) 内容扫描 — 注释 3112-3115)
  3. `DELETE FROM fts_ids WHERE wire_id=?`(3123)
  4. `INSERT INTO fts(id, text)`(3126,仅 Message;fts_rowid 依赖 `last_insert_rowid()` 顺序单行,3155)
  5. `INSERT INTO fts_ids(wire_id, id_json, fts_rowid)`(3129)
- 关系/成员路径再逐条:`message_placements`(3202)、`message_edges`(3229)、`source_membership` 每行(3258)、`source_placement_membership` 每行(3275)、`source_scans`(3282)、`source_relation_scans`(3298)。**合计约 8-10 条 execute/消息**。
- 另:循环内每实体 `serde_json::to_string(id)`(3139)重复分配,可提升到循环外。

### 对标 hstry(`Github_src/hstry/crates/hstry-core/src/db.rs`)
- `bulk_insert_messages_in_tx`(db.rs:2990-3062):**multi-row INSERT**,15 列 × 60 行/chunk = 900 参数,`ON CONFLICT(conversation_id, idx) DO UPDATE`(3015-3028);编译期断言 `const _: () = assert!(COLS * ROWS_PER_CHUNK <= 950);`(db.rs:3058);注释明言 "cuts the number of round-trips by ~60×"(2988-2989)。
- 其连接级 `PRAGMA synchronous = NORMAL`(db.rs:2976)+ `ANALYZE`(2979)。

### 差距量化(静态推导,未实测)
- 200K 消息一批:B2 路径 ≈ 1.6-2M 次 `execute` vs hstry 消息表 ≈ 3.3K 次。按 rusqlite step 约 0.5-2µs/次估算,常数因子差 ~1-4s vs ~0.05s 每 200K 消息。**但注意**:本项目每条消息跨 6+ 张表写,multi-row 只能压缩单表,不能照搬 hstry 的 60× 数字。
- 可直接 multi-row 的表:`fts_ids` 插入(与 catalog upsert);`fts` 插入因 `fts_rowid = last_insert_rowid()` 依赖顺序单行,multi-row 需 `RETURNING rowid`(SQLite 3.35+),侵入性大。
- **P2**: 对 `fts_ids`(及可选 `catalog`)用 multi-row INSERT,常量因子可再降一半以上;`serde_json::to_string(id)` 提升到循环外。

---

## 2. 事务边界与 pragma

### 现状(`lib.rs`)
- `init` 只设 `PRAGMA journal_mode=WAL`(957)。**无 `synchronous` pragma → SQLite 默认 FULL → 每次 commit 一次 WAL fsync**。
- 每 changed batch **两次 commit**:intent 行 autocommit(2940)+ apply 事务 commit(3377)→ **2 fsync/batch**。B1 单实体路径(`index_one`,main.rs:1200-1208)每实体同样 2 次 commit — 循环 `index` N 实体 = 2N fsync(FULL 下)。
- `busy_timeout`:代码未设,但 **rusqlite 0.40.1 打开连接默认 5000ms**(`~/.cargo/registry/.../rusqlite-0.40.1/src/inner_connection.rs:118` `sqlite3_busy_timeout(db, 5000)`)— 有兜底;且 WriterLease 单写者下竞争窗口极小。
- `unchecked_transaction`(1149, 1194)仅迁移路径用,与 `transaction()` 在此场景无行为差异。

### 对标
- **memex**:`compact/db.rs:158-163` 三连 `journal_mode=WAL; synchronous=NORMAL; busy_timeout=5000`,注释明言 "synchronous=NORMAL: WAL 模式下平衡性能和安全" / "busy_timeout: 多进程写入时等待锁而不是立即失败";`server/ingest.rs:57` ingest 结束做 `wal_checkpoint(TRUNCATE)` 收尾。
- **hstry**:`db.rs:2976` `synchronous = NORMAL`。

### 结论
- **P1(性能)**: `init` 加 `PRAGMA synchronous=NORMAL`。WAL 下 NORMAL 对**进程崩溃无数据丢失**(WAL 内容已进 OS 缓冲,checkpoint 时才 fsync),仅掉电可能丢最近一次提交 — 对"可重建索引"(有 `rebuild_index`)是合理折衷,与两个对标一致。改后 2 fsync/batch → 接近 0。
- **P2 note**: 无显式 checkpoint 调优(autocheckpoint 默认 1000 页);若未来要控 checkpoint 峰值,可借 memex 的 `wal_checkpoint(TRUNCATE)` 模式。

---

## 3. 每批开销(无变化时的快路径)

### 现状链路
1. **CLI `sync_files`**(`main.rs:1619-1675`):先读指纹缓存(`source_fingerprints`,lib.rs:1288)→ 对**每个**文件 `capture`(**整读全文 + blake3**,source_fs.rs:32-49)→ fp 与缓存相同则跳过 parse(1634-1643,空文件强制走完整路径以保留 tombstone 语义,1632-1633 ✓)。
2. **但 `verify_snapshot`(main.rs:1676-1678)对每个文件无条件执行**(source_fs.rs:56-83):len/mtime 相同 → **再次整读全文 + 重新 blake3**。→ **每个不变文件每次 sync 被整读两次、哈希两次**。
3. store 侧 `sources_are_current`(lib.rs:2080-2320)只对 fp 未命中的源执行(未命中才进 `sources` 向量);其内部查询全部 chunked `IN`(500/块,BATCH_IN_CHUNK=64),按源分批,常量低 ✓。
4. 全 fp 命中时 `commit_source_batches_if_changed(&[])` → `sources_are_current(&[])` → `Ok(false)`,近零代价 ✓(1637-1639)。

### 对标 fast-resume
- `session_needs_update`(`shared.rs:27-36`):纯 metadata mtime 比较(`MTIME_TOLERANCE=0.001`,`known_mtime` 差值判定),**零文件读取**。

### 结论
- **P1(性能)**: 主差距在"每个不变文件整读两次"。两个可选修法(按正确性取舍):
  a) **stat-first**:`source_scans` 增加文件 mtime 列,`sync_files` 先 stat,`len+mtime` 匹配则连 capture 都跳过(纯 fast-resume 模式)。注意本项目 source_fs.rs:6-7 明确拒绝过此折衷("等长异容替换必须靠 content fingerprint——len+mtime 不足",有测试 176-204 佐证)— 等长且 mtime 被恢复的替换会漏检,需产品决策;
  b) **保守版**:fp 命中时跳过 `verify_snapshot` 的整读指纹复核(保留 len/mtime 复核,128-131 之外再加一次 `stat` 即可)— 保留 capture 单次读,把 2 次降为 1 次。
- **P2**: 变更路径上 `source_entity_membership_state`(2322)+ `source_placement_membership_state`(2362)+ `stored_placements`(2388)+ `stored_edges`(2446)**四个全表 load** O(whole catalog)内存+时间,每个变更批都跑;可用"按批内实体/placement id 反查 claimers"的联合查询替代全表(注意:claimers 可能来自批外源,需 `UNION` 批外 membership 反查)。
- `batch_manifest`(653-718)对整批 payload 再跑一遍 blake3 digest — O(批字节),GB/s 级,非瓶颈 ✓。

---

## 4. Outbox + lease

### Outbox
- 两阶段(begin autocommit INSERT intent,2940 → apply 事务激活,3377)是 outbox 模式的**必要成本**,非冗余:崩溃时 recover 只见无副作用 `building` 行。
- `recover_interrupted`(3693-3704)每次写打开跑单条 `UPDATE ... SET state='aborted'`,便宜 ✓;`open_for_write` 顺序正确(lease → open → init → recover,924-942)。
- 每批**无 pragma 折腾、无显式 checkpoint** ✓。
- `verify_pending_in_tx`(2998-3090)generation CAS + intent 行状态 + manifest digest 三重校验 ✓(测试 8843/8880/8921 覆盖篡改 manifest 与 stale base)。

### lease.rs 审查
- Windows fs4 强制锁特判**正确**(lease.rs:29-39):error 32/33 → `WriterBusy`;`try_acquire`(85-151)进程内 `held_paths` 登记**先于** OS 锁、所有失败路径回滚登记(111-124)✓;diagnostic record 与后续 `read_record` 均**复用同一持锁句柄**(141, 159-164)规避 Windows error 33 ✓;`Drop` 解锁+注销(192-199)✓;error 32/33 有测试(232-243)✓。
- record 仅 `flush()` 不 `sync_all()`(167-173)— 可接受:OS 锁是权威,record 仅供诊断。
- 唯一小点:error 32(共享冲突)也归为 `WriterBusy` — 杀软瞬时占用会误报为"被持锁",但语义上可重试,不严重。
- **结论:lease 正确,无需改动。**

---

## 5. Schema 迁移(v1→v7;任务书 v7→v12 见 §0)

实际链(`lib.rs:967-1121`):
| 版本 | 内容 | 幂等守卫 |
|---|---|---|
| v1 (980) | catalog + contentful-id FTS5 | 无(全新建库) |
| v2 (992) | store_metadata + index_batches(outbox)+ generation | `IF NOT EXISTS` + `INSERT OR IGNORE` ✓ |
| v3 (1024) | fts_ids 边车 | `IF NOT EXISTS` ✓;回填逐行 autocommit(见 P2) |
| v4 (1054) | source_membership | `IF NOT EXISTS` ✓ |
| v5 (1067) | membership 多对多 + source_scans | **RENAME 无守卫(见 P1)** |
| v6 (1089) | document_id 列 | `PRAGMA table_info` 守卫 ✓(1095-1107,注释 1093-1094 承认崩溃窗口并修) |
| v7 (1195-1241) | 关系 schema + outbox 3 新列 + fingerprint 列 | 显式事务 + `PRAGMA table_info` 守卫(len_bytes/fingerprint,1249-1264)✓ |

- 新库拒绝 future version(`current > SCHEMA_VERSION`,971-977),有测试(5217)✓;`ensure_fts_ids_rowid`(1136-1181,旧 v7 库的加法扩展)守卫 + 单事务 ALTER+回填 ✓。
- **P1(低概率、不可恢复)**: v5 的 `ALTER TABLE source_membership RENAME TO source_membership_v4`(1069)无守卫 — 若在 RENAME 与后续 `CREATE TABLE source_membership`(1070)之间崩溃,重开时 `current<5` 重跑 v5 → `no such table: source_membership` → **数据库永久打不开**(需人工 sqlite3 改名)。v6 注释承认了同类窗口并加守卫,v5 漏掉。修法:执行前查 `sqlite_master` 表名,或把 v5 包进显式事务。
- **P2**: v3 回填(1043-1049)逐行 autocommit `INSERT OR REPLACE` — 旧 v2 大库升级 = N 次 fsync(FULL 下);应包单事务。
- **P2 note**: v5 全表重写 source_membership(RENAME+CREATE+INSERT SELECT,1069-1077)一次性 O(消息数),可接受;**无 catalog 全表重写迁移** ✓ — 与 fast-resume 的"schema 不匹配即 remove_dir_all 删库重建"(`INDEX_SCHEMA_VERSION`)形成对比,本项目方向正确。

---

## 6. 内容寻址 id(blake3)

- `StableId::derive`(`ids.rs:140-157`):每 id 新建 blake3 Hasher,kind prefix 域分离 + 长度前缀事实防碰撞(`["a","bc"]` vs `["ab","c"]`),截 32 hex(128 bit,`DIGEST_HEX_LEN=32`,ids.rs:98)。小输入哈希 ~100ns/次,每条消息 id+placement 派生 2-3 次 — 相对 JSONL parse 完全可忽略 ✓。
- facts 限定 relocation-invariant(ids.rs:131-136,不含路径/mtime/运行期计数)✓;`msg_v1_/ses_v1_` 等前缀(ids.rs:48-55)。
- 每批 digest(`batch_manifest`,lib.rs:685-716)对全部 payload 二次 blake3 — GB/s 级,非瓶颈 ✓。
- 微优化空间(`to_hex` 64 char 再 format 拼接,151-156)存在但不值得。
- **结论:哈希策略高效,热路径无问题。**

---

## 借用候选汇总(peer refs,license 均 MIT 已核验,见 2026-08-15-borrowable-code-inventory.md §0)

| 候选 | 来源(固定 clone) | 落点 | 方式 |
|---|---|---|---|
| `synchronous=NORMAL`(+busy_timeout 组合) | memex `compact/db.rs:158-163`;hstry `db.rs:2976` | adapters-sqlite `init`(lib.rs:956-960) | verbatim 3 行 pragma |
| multi-row INSERT + `const _: () = assert!(COLS*ROWS <= 950)` | hstry `db.rs:2990-3062`(断言 3058) | `fts_ids`/`catalog` 批量插入(lib.rs:3105-3172) | port,列数按本项目表改 |
| mtime 快路径(stat-first 跳过整读) | fast-resume `shared.rs:27-36`(`session_needs_update` + `MTIME_TOLERANCE=0.001`) | sync_files + source_scans 加 mtime 列 | verbatim 逻辑;正确性折衷需产品决策(见 §3) |
| `wal_checkpoint(TRUNCATE)` 收尾 | memex `server/ingest.rs:57` | 大批 ingest 结束 | pattern |
| JSONL 健康度三级判定 | fast-resume `shared.rs:21-25`(JsonlHealth)、`194-224`(jsonl_health) | provider 解析侧(非本次写路径范围,顺带登记) | verbatim |

---

## Caveats / Not Found

- lib.rs 为 384KB 单文件,已精读写路径全部相关函数(§1-§6 覆盖 1488-1611、1618-2320、2895-3385、3680-3720、630-760、940-1280、1331-1480),未逐行通读其余区域;检索/读路径(`SearchIndex`、`load_session_graph` 等)不在本次范围。
- §1 的常数因子(0.5-2µs/execute)为经验区间,**静态推导未实测**;建议与任务 #16/#18(gate benchmark、scaling benchmark)交叉验证 P1/P2 的实际影响。
- 未发现 P0(常态运行数据丢失/损坏)问题;两个 P1 中,v5 迁移崩溃窗口为低概率但永久性,建议优先修。
