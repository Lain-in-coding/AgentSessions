# sqlite-snapshot-wal Spike 证据

> 治理记录
>
> - decision_id: SPIKE-sqlite-snapshot-wal
> - status: **Executed（初步证据已产出）**
> - owner: 自主执行（R0 探针）
> - approver: 项目最终验收人（待确认）
> - due_milestone: R0 Feasibility / Contract Gate
> - evidence_path: `spikes/sqlite-snapshot-wal/`（探针代码 + 本文件）
>
> 本 Spike 是可丢弃探针，不进入 `crates/` 生产源码树，不形成正式 API/迁移承诺。R0 Accepted 后归档；正式实现从 `0.1` 干净重写。

---

## 1. 目的

为以下条款在 **Windows** 上提供实测证据，把它们从"设计假设"升级为"经验证的约束"：

- 计划 §9.1：WAL 库的一致快照方法；
- 计划 §6.5：不可变 Generation Bundle（旧 generation 在新写入期间仍可只读打开）；
- 架构审查第 13 项：裸复制 `.sqlite` 会丢失未 checkpoint 的已提交事务。

## 2. 环境（基准报告）

| 项 | 值 |
|---|---|
| OS | windows |
| Arch | x86_64 |
| Target | x86_64-pc-windows-msvc |
| SQLite | 3.53.2（rusqlite 0.40.1 bundled，libsqlite3-sys 0.38.1） |
| Rust | 1.97.1 stable-msvc |
| 数据规模 | 5000 行，`wal_autocheckpoint=0`（逼出"数据只在 -wal 中"的状态） |

## 3. 结论（四断言全部 PASS）

| 断言 | 结果 | 证据 |
|---|---|---|
| A. 裸复制主库（缺 `-wal`）丢数据 | **PASS（风险复现）** | live=5000 行，裸复制副本 `immutable=1` 打开读到 **-1 行**（副本无法完整读出） |
| B. Online Backup API 一致快照 | **PASS** | Backup 快照只读读到 **5000 行**，与源库相等 |
| C. VACUUM INTO 一致快照 | **PASS** | VACUUM INTO 快照只读读到 **5000 行**，与源库相等 |
| D. 旧快照在写入期间可只读打开 | **PASS** | 冻结时 5000 行 → 源库继续写到 6000 行 → 旧快照仍只读读到 **5000 行**（不受后续写入影响） |

## 4. 对计划的意义（可回填条款）

1. **§9.1 快照方法冻结**：生产 Generation Bundle 的 Catalog 快照 **禁止裸 `fs::copy` 主库文件**（断言 A 证明会丢数据），必须使用 SQLite Online Backup API 或 `VACUUM INTO`（断言 B/C 均产出一致快照）。二者都可作为 §9.1 的实现选项，Backup API 支持增量步进、VACUUM INTO 一步到位且顺带压缩。
2. **§6.5 不可变 Bundle 成立**：断言 D 证明旧 generation 快照可在新一轮 sync 持续写入期间被只读连接打开并读到冻结数据。这物理上支撑"旧 active generation 在新 generation 成功前保持可读"与"cursor 固定到旧 generation 分页"。
3. **审查第 13 项坐实**：裸复制风险在 Windows + SQLite 3.53.2 上真实复现，相关约束不是理论担忧。

## 5. Caveat（诚实边界）

- 本 spike 用 `wal_autocheckpoint=0` 放大了"数据滞留 -wal"的场景；生产默认 autocheckpoint 开启时，裸复制丢数据的窗口更小但依然存在（取决于 checkpoint 时机），因此结论方向不变：**不依赖裸复制**。
- 断言 A 的副本读到 -1 是"COUNT 查询失败/读不出"的哨兵值，表现为副本不可用；不同 SQLite 版本可能表现为"读到部分行"，但都属于数据不完整，均坐实风险。
- 未覆盖：多进程并发下的 `-wal`/`-shm` 竞争、杀毒软件文件锁延迟对 rename 的影响——这些留给 `data-root-locking` 与 `cross-platform-packaging` spike。
- SQLite 3.53.2 是 rusqlite 0.40 bundled 版本；计划 §9.1 要求 R0 冻结"最低安全 SQLite 版本"，本 spike 记录了实际链接版本作为起点，正式基线仍需按 SQLite 官方修复公告确认。

## 6. 复现

```powershell
cd spikes/sqlite-snapshot-wal
cargo run --release
```

确定性结果，无随机性；每次全新临时目录。
