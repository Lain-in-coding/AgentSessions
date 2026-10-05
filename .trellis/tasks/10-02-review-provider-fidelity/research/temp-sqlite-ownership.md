# Research: P1-7 temporary SQLite ownership

- Query: 定位 Cursor、OpenCode、Hermes SQLite 临时副本的创建、写入、打开及清理所有权；给出不新增依赖、不改变解析语义的最小分工和回归设计。
- Scope: mixed — 指定工作树源码、任务/spec，以及必要的 Rust/Windows 官方文档和已锁定依赖源码。
- Date: 2026-10-04
- Worktree: isolated remediation worktree；所有产品源码证据均来自该工作树，不来自旧 checkout。
- Authority: 当前显式任务 `10-02-review-provider-fidelity`。忽略 `09-27` breadcrumb；没有查询运行时 task pointer。按 researcher 隔离规则未读取 `implement.jsonl` / `check.jsonl`，读取了 PRD、design、implement 中相关执行约束及适用 specs。
- Evidence level: 静态源码与原语契约核验，**没有运行 Cargo、测试、故障注入或权限实验**。下列 red 是待实现/执行的预期反例，不是已经取得的测试结果。主会话告知 metadata slice 已在 `83528d9` 提交推送；本研究未检查该提交或 CI。

## Findings

### 1. 可立即用于规划的结论

1. **三处同型缺陷均已由源码确认，Hermes 不能漏掉。** 都先构造会删除路径的 `TempDbGuard`，再调用会创建或截断既有文件的 `File::create`。因此：可写既有候选文件不是安全冲突，而会被覆盖；如果文件创建失败，尚未获得文件所有权的 guard 仍会尝试删除该名字及相邻 sidecars。实际删除是否成功取决于文件系统/句柄/权限；这里没有声称跨平台实测或实际利用。
2. 当前 `TempDb { conn, _guard }` 字段顺序是正确的；当前写句柄也在 guard 之后声明。**不能只把 guard 移到 `File` 声明之后**：写入/同步的 `?` 返回时可能改成先清理、后关闭写句柄。要同时保留写句柄先关闭、SQLite 连接先关闭的两个顺序。
3. 三处创建都没有显式 Unix `0600`。Rust 默认新文件模式是 `0666 & !umask`，所以当前代码不保证仅 owner 可访问；具体机器是否产生 `0644` 未测试。Windows 没有等价的 Unix mode 保证，默认 ACL/实际访问权未测，不能把“private copy”解释为已验证的 Windows owner-only ACL。
4. 已有 PID + 原子计数器不要退化成时钟命名。它们处理正常并发调用的名字区分，但**名字生成不是独占文件创建，也不是清理授权**。
5. **完整 DB/WAL/SHM 所有权还存在一个需要明确处理的边界**：仅对主 DB 使用 `create_new`，不能证明同名既有 `-wal` / `-shm` 也属于这次创建。现有 Drop 无条件按后缀删除。若本 slice 的“conflict never deletes another file”包括 sidecar-only 冲突，单改主文件创建不足，见第 5 节的同范围目录所有权方案。不要将尚未核验的 SQLite 读取/利用行为扩写成安全结论。

### 2. Files found：准确位置与作用

所有下面的项目路径均相对指定工作树；行号是本轮读取时的行号。

| 文件 / symbol | 证据位置与作用 |
| --- | --- |
| `crates/agent-session-grep-provider-cursor/src/lib.rs` | `CursorAdapter::probe` 在 153–154 调用副本 opener；`parse` 在 200–216 调用同一 opener 后分派 ItemTable / disk-kv，两种变体共享该修复。 |
| 同文件 `TempDbGuard` / `Drop` | 491–503；清理主 DB、`-wal`、`-shm`、`-journal`。 |
| 同文件 `TempDb` / `temp_path` | 515–526；字段 `conn` 在 `_guard` 前，测试可取得副本路径。 |
| 同文件 `open_readonly_from_bytes` | 532–559；533 生成路径，534 注册 guard，535 `File::create`，536–538 写入/同步/关闭；540–553 只读 open、1 秒 timeout、query-only 和 pinned read。 |
| 同文件 `temp_db_path` | 567–574；`asg-cursor-{pid}-{counter}-{tag}.db`，进程内 `AtomicU64`。 |
| `crates/agent-session-grep-provider-cursor/src/disk_kv.rs` | 1813–1828 已有副本 Drop 测试；1831–1845 `ScratchFiles` 是测试 fixture 清理，不是可直接挪进生产的安全创建 helper。生产解析不需要改动。 |
| `crates/agent-session-grep-provider-opencode/src/lib.rs` | `OpenCodeAdapter::probe` 77–84、`parse` 120–136 取得同一临时副本并执行后续 schema/query。 |
| 同文件 `temp_file_suffix` | 270–278；PID + 原子计数器；候选路径构造在 324–325。 |
| 同文件 `TempDbGuard` / `Drop` | 282–294；清理主 DB、`-wal`、`-shm`，**当前不包含 `-journal`**。 |
| 同文件 `TempDb` / `temp_path` | 306–317；连接在 guard 前。 |
| 同文件 `open_readonly_from_bytes` | 323–344；326 guard → 327 `File::create` → 328–330 写入/同步/关闭 → 332–338 只读 open + timeout。本函数没有 Cursor/Hermes 的首个 schema read。 |
| `crates/agent-session-grep-provider-hermes/src/lib.rs` | 33 声明 `sqlite_state`；120 / 133 将 SQLite 输入分派给该模块；无需改动 JSON variant 或分派语义。 |
| `crates/agent-session-grep-provider-hermes/src/sqlite_state.rs` | `probe` 178 / `probe_with_limits` 182，通过 195 取得副本；`parse` 552 / `parse_with_limits` 559，通过 576 取得副本。 |
| 同文件 `temp_file_suffix` | 980–988；PID + 原子计数器。 |
| 同文件 `TempDbGuard` / `Drop` | 992–1004；清理主 DB、`-wal`、`-shm`、`-journal`。 |
| 同文件 `TempDb` / `temp_path` | 1012–1023；连接在 guard 前。 |
| 同文件 `open_readonly_from_bytes` | 1032–1065；1035 路径，1036 guard，1037–1043 创建/写入/同步/关闭；1045–1059 只读 open、timeout、query-only 和 pinned read。 |
| `crates/agent-session-grep-adapters-sqlite/src/source_fs.rs` | 98–131 `sqlite_snapshot`；124–126 使用 `tempfile::NamedTempFile::new()?.into_temp_path()`，127–129 SQLite Backup。是现成安全创建/RAII 模式的参考，**不是这次可修改或可直接跨层调用的 helper**。 |

### 3. 创建、错误、Drop 的实际链路

共同主链路：

`bytes → 生成 temp 名字 → 提前注册 TempDbGuard → File::create → write_all → sync_all → drop(file) → Connection::open_with_flags(READ_ONLY | NO_MUTEX) → busy_timeout(1s) → 返回 TempDb`

Cursor / Hermes 另外执行 `PRAGMA query_only = ON; BEGIN; SELECT rootpage FROM sqlite_schema LIMIT 1;`；OpenCode 保持现状，不能借所有权修复顺便改变其事务/解析语义。

| 阶段 | 当前所有权 / 错误路径 | 修复必须保持或改变的内容 |
| --- | --- | --- |
| 文件创建尚未成功 | 三处 guard 已存在；`File::create` 的 `Err` 经 `?` 触发该 guard Drop | **改变**：只有成功独占创建才授权清理该文件；冲突直接返回原错误，不删除冲突文件，不删除旁边 sidecars，不换 fallback 路径掩盖错误。 |
| `write_all` / `sync_all` 失败 | 写句柄在 guard 之后声明，局部变量反向 drop；写句柄先关闭，再清理 | **保持**：错误不吞掉，部分写入副本会清理；不要因移动 guard 而反转句柄顺序。 |
| SQLite open 失败 | 写句柄已显式关闭；已拥有的主文件由 guard 清理 | **保持**；不把 invalid-input 查询失败误说成 SQLite open 本身失败。 |
| timeout / pinned read 失败 | 已创建 `conn` 局部变量，声明在 guard 后；错误退出先关闭 `conn` 再清理 | **保持**当前查询、超时和错误映射。 |
| 正常返回后查询/解析报错 | `db` 在外层作用域退出；`TempDb` 依字段顺序关闭 `conn`，再 Drop guard | **保持**，不能改回 `(conn, guard)` 局部解构。 |
| Drop | 仅按保存路径拼接固定后缀，逐个 `remove_file`，现有代码忽略删除错误 | 不宣称无条件清理成功；本 slice 不扩展成清理扫描、重试器或新的错误接口。 |

错误分类不能顺手统一：

- Cursor / OpenCode opener 返回 `Result<TempDb, String>`；上层使用 `ProviderError::StructuralFatal("failed to open SQLite: ...")`，分别见 Cursor `src/lib.rs:153–154,200–201`、OpenCode `src/lib.rs:77–78,120–121`。
- Hermes 文件操作映射 `ProviderError::Io`（`sqlite_state.rs:1037–1042`）；连接 open 使用 `StructuralFatal("hermes state.db open failed: ...")`（1049–1051）；timeout / pinned read 经 `sql_error`（1052–1059）。
- 本次只追踪这些 entrypoint 的资源生命周期；不评价 metadata/probe 语义，也不改变 provider maturity、identity、timestamp、parser version 或源快照策略。

### 4. 已有安全 helper 与依赖：不需要 Cargo.lock 改动

**现成模式**：`source_fs::sqlite_snapshot` 是私有函数，输入真实源路径并通过 Backup 抓取逻辑页，和 provider 接收 `&[u8]` 再打开副本不是同一个接口。不要为了复用它让 provider 依赖 adapters-sqlite，也不要改该 crate 或把文件 I/O 放入 ports/testkit 生产接口。

已读锁定版 `tempfile 3.27.0` 的本地原始源码：

- `tempfile-3.27.0/src/file/imp/unix.rs:13–26`：`create_new(true)`；未指定权限时 `.mode(0o600)`；成功 open 才返回 File。
- `tempfile-3.27.0/src/file/mod.rs:1110–1124`：底层创建成功后的 `.map(|file| NamedTempFile { ... })` 才构造路径 owner。
- `tempfile-3.27.0/src/file/mod.rs:402–408`：`TempPath` Drop 删除其保存路径；902 起 `into_temp_path` 转移路径所有权。
- `tempfile-3.27.0/src/file/imp/windows.rs:26–40`：使用 `create_new(true)`，没有在该函数中构造 owner-only DACL。
- `tempfile-3.27.0/src/lib.rs:389–409`：Windows 权限参数有局限；Unix mode 受 umask 限制；**该版本 tempdir 默认 mode 是 `0777`，不是 `0700`**。不能用“用了 TempDir”替代私有目录权限证明。

| 声明/锁定位置 | 当前事实 |
| --- | --- |
| 根 `Cargo.toml:7–10` | edition 2024，声明 MSRV 1.90。 |
| Cursor `Cargo.toml:9–17` | runtime 有 ports、serde、serde_json、rusqlite；dev 有 testkit、blake3；没有 tempfile。 |
| OpenCode `Cargo.toml:9–18` | 另有 domain；没有 tempfile。 |
| Hermes `Cargo.toml:9–20` | rusqlite 同版本；没有 tempfile。 |
| 三者 rusqlite 声明 | `0.40.2`, `default-features = false`, `features = ["bundled"]`。 |
| adapters-sqlite `Cargo.toml:18–19` | rusqlite + `tempfile = "3"` 是该 crate 的直接生产依赖。 |
| `Cargo.lock:163–173,187–197,223–234` | 三个 provider 的依赖列表均不含 tempfile。 |
| `Cargo.lock:1348–1356,2087–2097,2409–2419` | 锁定 libsqlite3-sys 0.38.2、rusqlite 0.40.2、tempfile 3.27.0。 |

因此，“tempfile 已在 lockfile”不等于 provider 已可直接引用；给三者增加直接依赖仍会改变 provider 的 lock dependency entries。遵守当前 design 的 `no Cargo.lock` 约束，**复用其标准库原语和所有权顺序，不引入新 crate、不改任何 Cargo.toml**。创建时设 Unix mode 使用 `std::os::unix::fs::OpenOptionsExt`，无需新依赖。

### 5. 最小生产修复及真正未决的 sidecar 边界

#### 5.1 三处都必须做的最小下限

在三个现有 `open_readonly_from_bytes` 生命周期中：

1. `OpenOptions::new().write(true).create_new(true)`，Unix 创建前设置 `.mode(0o600)`；不要 `exists()` 后 `File::create`，也不要写完再 chmod。
2. 独占创建成功后才构造 DB guard；失败不注册任何 DB/sidecar 清理权限。
3. 将已取得的写句柄**移动进 guard 之后的内层 binding/scope**，保证所有写入和同步错误都先关闭文件。以下仅为所有权顺序示意，实际保留各 crate 的错误映射：

```rust
let created_file = options.open(&temp_path)?; // 尚无 DB cleanup owner
let guard = TempDbGuard { path: temp_path };  // 成功创建后才获得所有权
{
    let mut file = created_file;             // move，不是借用
    file.write_all(bytes)?;
    file.sync_all()?;
}                                           // 正常/错误都先关闭写句柄
// 原有只读 open / timeout / query-only / pinned read 保持不变。
// 返回结构仍是 TempDb { conn, _guard: guard }。
```

4. 保留 PID + counter 生成器及现有只读/事务设置。不要新加重试、覆盖、fallback、source reopen、反序 Drop 或全局 temp 枚举。

#### 5.2 不能忽略的所有权边界

当前 guard 只持有一个 PathBuf，没有记录 sidecar 是否由本次操作创建。源码可直接确认：如果主文件原本不存在、同 basename 的 sidecar 已存在，在主文件成功创建后 Drop 仍会删除那些 sidecar。**这项固定后缀删除事实确定；SQLite 是否提前使用/改写特定预存 WAL/SHM，未做平台实验，不作推论。**

本任务明确要求追踪 DB/WAL/SHM 所有权。若 acceptance 是整个文件组的“既有文件不受损”，建议在同一三个 provider scope 内取得**独占创建的每副本目录**，而不是仅做 5.1 后宣称文件组全受保护：

- 使用标准库 `DirBuilder::create` 独占取得新目录；Unix 创建前指定 `0700`。不能用 `create_dir_all` 把已有目录视作自有目录。
- 在新目录中以 `create_new` + Unix `0600` 创建 DB。目录所有权和 DB 创建成功状态分开：目录创建失败无任何 cleanup；DB 创建失败不能删除未经取得的 DB 文件。
- cleanup 只处理取得所有权的 DB 和该私有命名空间下的固定 sidecars，最后非递归 `remove_dir` 自有空目录；不要扫描/删除共享 temp 目录，也不要清除未知文件。
- 这不需要新 crate、Cargo manifest、lockfile 或公开 API；但比三行 API 替换多一个局部目录生命周期。主会话应在分派时明确采用这一完整边界，或明确记录只修主 DB 冲突的较窄 acceptance。**不要用 sidecar `exists()` 预检声称原子取得整个文件组。**
- Windows 目录/文件 ACL 仍依赖创建环境；上述结构不等于已经实现 Windows owner-only DACL。

这里没有要求移植 tempfile 的所有策略，也没有性能建议；目录方案是针对当前 guard 无法证明 sidecar 所有权这一具体限制。

### 6. Red regression 与保留测试设计

测试必须走与生产相同的生命周期 helper。建议仅提取私有的“给定候选路径/命名空间”入口，生产 wrapper 仍用原生成器；不要预测全局 counter，不修改进程全局 TEMP/TMP，不用 sleep 制造竞争。故障注入只用局部 callback/测试 seam，不建新公共 trait 或全局 failpoint。以下所有实验均尚未执行。

| 用例 | 可重复构造及断言 | 预期基线性质 |
| --- | --- | --- |
| 既有正常文件冲突 | 在测试独占目录里预置可写 DB sentinel，及 WAL/SHM sentinel（Cursor/Hermes 另含 journal）；让同一生产 helper 使用这个确切候选。应返回错误；返回值/局部 owner 完全 drop 后，全部文件内容和存在性不变。分别检查“没有截断”和“没有删除”，不能只断言 Err。 | 预期 red：旧 `File::create` 可成功截断，guard 退出会删除。 |
| **创建失败但旁文件必须保留** | 把候选 DB 路径建成目录，在它旁边放 `-wal` / `-shm` 等 sentinel。创建文件必然失败；目录和旁文件均应保留。这比依赖只读位/ACL 的 PermissionDenied 测试稳定，直接命中提前注册 guard。 | 预期 red：旧 guard 会尝试删除所有后缀，普通旁文件会被清理；实际结果待跑。 |
| 仅 sidecar 已存在 | 主 DB 不存在，只放相同 basename 的 WAL/SHM sentinel。断言 helper 不删除/改写这些既有文件。若采用私有目录方案，预存同名 namespace 及内容应使目录获取失败并完整保留。 | 预期暴露“只换 create_new”仍不完整的边界；不能把该用例算作已通过。 |
| 非法 SQLite bytes | 用非空坏数据，调用可注入路径的 opener。Cursor/Hermes 的 pinned schema read 会走错误退出。OpenCode opener 没有首读，必须在 guard 作用域内执行 schema/query 再返回错误，不能假设 connect 就报 NOTADB。公共 probe 的 missing-magic 早退不覆盖临时文件路径。断言已创建副本/sidecars 清理。 | 主要是保留路径；旧实现并非已确认在此泄漏。 |
| 部分 write / sync 错误 | 局部注入 writer 操作：取得真实自有 File，写一段，再返回明确 I/O 错误；sync 分支单独注入。错误经同一生产 `?` 返回后检查路径与 sidecars，不在测试中预先手动关闭/删除而掩盖 Drop 顺序。 | 原实现已有合理局部顺序；防止“后移 guard”的修复引入新回归。 |
| SQLite open / setup 错误 | 在真实创建、写入并关闭后，让局部 opener/setup seam 返回确定错误；断言主文件/sidecars 清理且错误类别不被吞掉。将这与真实 SQLite malformed-schema 测试分开标注：注入错误证明所有权分支，不证明特定平台的 SQLite 故障。 | 补足未覆盖分支，不声称已经发现 open 错误泄漏。 |
| 并行不同源 | 预先生成不同内容的有效 SQLite bytes；多个线程各打开自己的副本，Barrier 保持同时存活；断言路径互异、各自读到对应 marker；一部分 owner drop 后其余副本仍可读；最后各自路径清理。不能只检查文件名字符串。 | PID+counter 已存在，预期大部分是保留测试，不应包装成已确认并发缺陷。 |
| 相同候选的并行占用 | 第一个 owner 成功且保持存活；第二次在相同候选尝试创建，应失败且不能截断/删除第一个 owner 的主文件及 sidecars；第一个仍可读取。 | 针对独占创建和失败不清理的强反例。 |
| Unix `0600` | Unix-only：在隔离子进程/外部 test gate 使用明确 `umask 022`，验证新 DB mode 为 `0600`；若采用目录方案再测 `0700`。不要在并行 Rust 测试进程中修改全局 umask。一般模式下可断言 group/other 位为 0；更严格 umask 可能进一步移除 owner 位。 | 旧默认 `0666 & !022 = 0644`，预期 red；若 runner 自带 `umask 077`，旧代码也可能过，不能当成有效 red。Unix 未运行。 |
| Windows ACL/清理 | 运行创建冲突、真实 WAL/SHM success/error 清理测试；记录文件系统/ACL/句柄限制。mode/read-only 属性不能替代 DACL 检查。 | 本轮未测试；不能声称 owner-only 或所有 Windows 环境删除必成功。 |

**已存在且必须保留的真实 WAL/SHM 测试**：

- Cursor `src/lib.rs:620–678`：`assert_wal_temp_copy_cleanup` 先真实读取以 materialize WAL/SHM，再测成功与 query error；682–711 测 DB Drop 和 PID 名字；`src/disk_kv.rs:1813–1828` 另有 private copy Drop。
- OpenCode `src/lib.rs:371–432`：相同真实 sidecar success/query-error 测试；436–454 测 Drop 顺序。
- Hermes `src/sqlite_state.rs:1822–1838`：private copy Drop；1844–1913：真实 sidecar success/query-error，结束时还检查 journal 不残留。
- 这些测试的存在不是本轮通过证据。若改 owner 构造器，fixture 必须显式取得测试资源所有权，不要继续用 raw guard 接管冲突路径。Hermes `Scratch` / `scratch` 在 1107–1125 使用 `create_dir_all`，是测试 helper，不能未经修改直接充当独占安全生产目录 helper。
- 保留既有后缀差异；没有证据要求本 slice 单独扩展 OpenCode rollback-journal 语义。

### 7. 最小、互不重叠的 implement scopes

| Owner | 可修改生产/测试范围 | 明确不改 |
| --- | --- | --- |
| Cursor implement | `crates/agent-session-grep-provider-cursor/src/lib.rs` 中 temp path/owner/opener 和同文件测试；仅在适配 fixture owner 时触及 `src/disk_kv.rs` 的测试区。 | ItemTable/disk-kv 解析、timestamp、identity、probe 判定等生产语义。 |
| OpenCode implement | `crates/agent-session-grep-provider-opencode/src/lib.rs` 中 suffix/owner/opener 和同文件测试。 | schema/query 语义、事务策略、provider manifest。 |
| Hermes implement | `crates/agent-session-grep-provider-hermes/src/sqlite_state.rs` 中 suffix/owner/opener 和同文件测试。 | `src/lib.rs` JSON variant / dispatch，以及 SQLite 字段解释、reasoning/maturity/limits。 |
| Main / 独立 check | 先固定第 5.2 节的完整所有权 acceptance；按现有门禁协调运行上述 package tests、Clippy 和 workspace checks，审查现有 fixtures；需要时由 main 更新相关共享 spec/任务证据。 | worker 不写 shared specs、CLI/application/adapter 代码、Cargo.toml/Cargo.lock，不提交/推送，不并发抢 Cargo。 |

三个 provider 的局部小原语保持同一顺序即可；不要为了去掉这几行重复，新增基础 crate、突破 ports 的无 I/O 边界，或把 testkit 升格为生产依赖。所有 Cargo 命令留给主会话解除独占后安排，本研究没有运行任何 Cargo 命令。

### 8. Related specs / task contracts

- 当前任务 `prd.md` 的 Requirements 明确要求 exclusive/private temp files、只有成功创建才拥有 cleanup、冲突不删除别人文件，并点名 Cursor/OpenCode/Hermes。
- 当前 `design.md` 的 Exclusive ownership：provider crates/tests；不改 Cargo.lock、CLI、application、adapters-sqlite。shared docs/spec 与提交由 main 管理。
- 当前 `implement.md:3–8`：最小 root-cause 修复、独立 check、不得 child commit/push/递归 agents、协调 Cargo；第 11 行亦说明并非每个 provider 都有专用 spec index。
- `.trellis/spec/agentsessions-adapters-sqlite/backend/index.md:347–402`：provider 不打开原始源，`conn` 先关闭，清理私有 DB/WAL/SHM；要求真实 sidecar success/query-error 测试。**374–375 的“写 bytes 前注册 guard”应保留，但明确细化为“成功独占创建后、写 bytes 前”**；不能理解成 file create 前就注册清理权限。spec 由 main 更新。
- `.trellis/spec/agentsessions-ports/backend/index.md:3–16`：ports 无文件 I/O、不依赖具体后端。
- `.trellis/spec/agentsessions-testkit/backend/index.md`：test-support only，不把生产逻辑移入 testkit；fixture synthetic/redacted。
- `.trellis/spec/guides/index.md` / `code-reuse-thinking-guide.md`：先找可复用模式；review 的缺陷要以实际源码确认。当前不存在目标三个 provider 各自的专用 spec 目录；没有据此套用 Codex-only 字段规则。

### 9. External references（仅原始资料）

- Rust 1.90 `OpenOptions::create_new`：独占创建；已存在文件/符号链接不是“打开已有文件”；避免 exists→create 的竞态。定位：`https://doc.rust-lang.org/1.90.0/std/fs/struct.OpenOptions.html#method.create_new`。
- Rust 1.90 Unix `OpenOptionsExt::mode`：新文件默认 `0666`，创建 mode 受 umask 约束。定位：`https://doc.rust-lang.org/1.90.0/std/os/unix/fs/trait.OpenOptionsExt.html#tymethod.mode`。
- Rust Reference destructors：struct fields 按声明顺序，局部变量按相反声明顺序 drop。定位：`https://doc.rust-lang.org/reference/destructors.html`。
- Microsoft File security and access rights：默认 security descriptor 的 ACL 从父目录继承；不能用 Unix mode 或命名宣称 Windows owner-only。已读取正文：`https://learn.microsoft.com/en-us/windows/win32/fileio/file-security-and-access-rights`。
- `tempfile 3.27.0`：第 4 节列出的**本机锁定版原始源码**，不是从记忆推断最新版本或默认权限；与仓库 Cargo.lock 交叉核对。无需新增依赖或下载源码。

## Caveats / Not Found

- 未执行任何 red/green、Cargo、Unix umask 实验、Windows DACL 检查或文件故障注入；不能把源码推导、既有测试名称或 spec 的历史证据说成本轮运行结果。
- 真实 open、write、sync 失败的 OS 触发方式和 Windows 删除成功性尚未实验。错误分支可以用局部可控注入覆盖，但必须清楚区分注入与平台复现。
- 需要 main 明确 sidecar-only 冲突属于完整 acceptance；若是，仅主 DB `create_new` + 后置 guard 仍不能作为整个 DB/WAL/SHM 所有权完成证据。推荐同 scope 的独占目录方案，不引入新 crate/fallback。
- 工具阻塞的精确记录：针对**指定工作树**调用 CodeGraph 后，返回 `timed out awaiting tools/call after 300s`。未重试、未建索引、未查询默认/其他工作树；使用该工作树的 ast-grep outline 与只读源码检查完成研究。
- `Get-Command ctx7` 未找到已安装命令；为遵守唯一文件写入范围，没有运行 npx 安装/设置。没有 Context7 查询结果；使用了已读取的官方文档及本机锁定版依赖源码，没有回退到未验证的 API 记忆。后续没有继续网络/库安装。
- 未进行任何 git 操作、未触碰 metadata/probe 实现或其他 reviewer scope、未创建其他研究文件；唯一产物为本文件。


## Main integration decision
Main accepts the full directory + DB/sidecar ownership boundary in section 5.2.
P1-7 implementation has not yet run; see the current design/execution slice for
scopes, error-injection limits and gates. This report is research, not a substitute
for baseline regression or platform evidence. No cross-layer helper/dependency
expansion is approved.
