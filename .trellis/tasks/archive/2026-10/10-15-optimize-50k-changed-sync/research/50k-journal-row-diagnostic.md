# Research: 50k journal 行/表体积诊断（冻结二进制实测）

- Query: 50k initial/shrink 提交后，durable journal 行、各表与主库的实际字节量级是多少？能否用冻结基线二进制 + 同一生成器复现基线包络？
- Scope: internal（诊断实验；使用冻结二进制 `baseline-cli.exe` + repo 自带生成器；scratch 为私有临时目录）
- Date: 2026-10-15
- 约束：本诊断只把聚合数字带入 research；scratch 目录、原始 DB 属私有 runtime，**不得进入产品提交**。

## Findings

### 1. 实验设置

- 二进制：`.trellis/.runtime/attribution-closeout-20261012/canonical-repaired/evidence-output/current-source-20261014-b0fc44d75690/baseline-cli.exe`（sha256 `39049bfd…abe8e`，即基线冻结二进制 B）。
- 语料：用协调根 `scripts/evidence/core_beta_benchmark.py` 的 `synthetic_record`/`write_dataset` 生成 250 文件 × 200 消息（与 S harness 的同名函数逐字节一致，已比对）；源字节 14,027,058。
- 流程：initial sync（250 源）→ 只读 sqlite3 盘点 → `shrink_dataset`（每文件保留前半）→ shrink sync → 再盘点。CLI 参数：`baseline-cli.exe --db <scratch>/catalog.db --output json sync <250 路径>`，cwd 为私有 scratch。

### 2. 实测结果

**initial（wall 9,990 ms，退出码 0，committed=50000，generation=1）**

| 项 | 值 |
| --- | --- |
| 主库大小（进程退出后，WAL 已 checkpoint） | 238,297,088 B（page_size 4096 × page_count 58,178；freelist 11,863 页 ≈48.6 MB） |
| `index_batches` 行数 | 1（state=activated，detail_format=full） |
| 该行明细 | `relation_upserts_json` **38,022,500** + `source_replacements_json` **8,210,751** + `upsert_ids_json` **2,321,001** + delete 2 + relocation 4 ≈ **48.6 MB** |
| `catalog` | 50,500 行，payload 合计 29,975,808 B |
| `fts` | 50,000 行，id 4,650,000 + text 2,500,000 B |
| `fts_ids` | 50,500 行，wire_id 2,169,500 + id_json 4,698,250 B |
| `message_placements` / `message_edges` | 50,000 / 45,000 行 |
| `source_membership` / `source_placement_membership` | 50,500 / 50,000 行 |
| 其他 | session_fts 250、session_titles 250、source_scans 250、resume claims 250；tool_activities/usage_events 0 行（合成语料无此类记录） |

**shrink（wall 10,227 ms，退出码 0，committed=25000，generation=2）**

| 项 | 值 |
| --- | --- |
| 主库 | 238,297,088 B（page_count 58,178；**freelist 18,105 页 ≈74 MB**） |
| `index_batches` 行数 | 2（新增行：relation_upserts_json **18,987,500** + relation_deletes_json **8,215,001** + source_replacements_json **4,260,751** + upsert/delete ids 1,171,001×2 ≈ **33.8 MB**；两行明细合计 ≈82.4 MB） |
| `catalog` | 25,500 行，payload 14,502,280 B |
| `fts` / `fts_ids` | 25,000 / 25,500 行（text 1,250,000 B） |
| `message_placements` / `message_edges` | 25,000 / 22,500 行 |

两次 wall 均落在基线三跑 50k P50 带宽内（initial 9.44–10.58 s；shrink 9.51–13.73 s），说明冻结二进制 + 本地生成器可以复现基线包络，可用于实验（E1–E3）的 before 采样。

### 3. 机制结论

1. `index_batches` 明细是提交路径中**最大的单一对象**；`outbox_intent`（Phase A INSERT）与 `generation_activate`（Phase B UPDATE）各写一次整行，SQLite UPDATE 会重写整行（含 overflow），freelist 增量与旧行体积吻合（initial：48.6 MB）。
2. 事务总写入量粗估：主库分配页（58,178）＋重新分配后被释放的页（freelist 11,863）≈70,041 页 ≈287 MB 曾经过 WAL；结合 `tx_commit` 3.35 s（trace），可解释为 WAL 写 + FULL fsync + commit 内 autocheckpoint（默认阈值 1,000 页）回拷。减少 journal 行的第二次写入直接减少该量。
3. `relation_deletes=95,000`（shrink）不是冗余实现：文件截断改变 document/session 身份 → placement id 全量重派生，旧 50k placements + 45k edges 全部删除、保留部分以新 id 重插。任何"跳过未变化 relation 写入"的设想与事实不符。

### 4. payload 形态探针（用于判断 alias 重建是否会触发 UPDATE）

- 用同一冻结二进制对 3 条同构消息跑 `ingest`（B1 单文件路径），再读 `catalog.payload`：键为 `is_sidechain/parent/parent_native_id/role/session/sessions/span/spans/text/timestamp`，且**字节等于 `json.dumps(loads(payload), sort_keys=True, separators=(",",":"))`**（即排序键 + 紧凑分隔符的规范形态）。
- 50k sync 库中同 id 消息的 payload 也是同一规范形态、同尺寸。
- 推论：`regenerate_compatibility_aliases_in_tx` 在单源 50k 语料下重建结果与已存字节一致 → 不产生 5 万次 catalog UPDATE；其 2.2 s 主要花在 JSON 往返与全表 placements/edges 装载（仍需 E3 用计时确认）。

### 5. 复现脚本要点（私有 scratch，勿提交）

```powershell
# Python（stdin 执行即可，不落仓库文件）：importlib 载入协调根 harness 模块
#   1) hb.write_dataset(<scratch>/d, 250, 200)
#   2) subprocess: baseline-cli.exe --db <scratch>/catalog.db --output json sync <paths...>（cwd=<scratch>，计时）
#   3) sqlite3 只读：PRAGMA page_size/page_count/freelist_count；逐表 COUNT(*)；
#      catalog.SUM(LENGTH(payload))；fts/fts_ids 列长；index_batches 逐行 LENGTH(5 个明细列)
#   4) hb.shrink_dataset(paths)；重复 2)-3)
```

（本次运行使用的 scratch 目录形如 `%TEMP%\asg-research-50k-*`；如需重跑同样流程即可得到同量级数字。）

## Caveats / Not Found

- 单机单次观测；数字是量级证据，不是复测基线。WAL 峰值未测得（进程退出即 checkpoint 归零；E1 需在同进程内观测）。
- 合成语料无 tool_activity/usage 记录，相关表为 0 行；这些路径的写放大未在本诊断覆盖（其当前体量小，非本任务目标）。
- 主库大小含已 checkpoint 的全部页；不能直接与"锚点 store/source 比"比较（那是 anchor 规模）。
