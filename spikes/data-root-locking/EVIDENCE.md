# EVIDENCE：data-root-locking spike

> 治理记录
> - decision_id: SPIKE-data-root-locking
> - status: **Executed**（初步证据已产出，待正式 Selection/RFC 引用）
> - owner: （执行中，AI 探针）
> - approver: 项目最终验收人
> - due_milestone: R0 Feasibility / Contract Gate
> - evidence_path: `spikes/data-root-locking/`（探针代码 + 本文件）
> - 对应计划位置：审查阻断项 #5（data-root writer lease）、§6.5（CAS activation）、§2.4（并发写单例）
>
> 本 spike 是可丢弃探针，不进 `crates/` 生产树。

---

## 1. 目的

为计划审查阻断项 #5（data-root 级全局 writer lease）与 §6.5 的 CAS activation 提供 Windows 上的实测证据，回答：

- OS 文件锁能否作为 writer lease 的权威，且进程崩溃时自动释放（避免 PID 超时抢锁）？
- CAS（`CURRENT == expected_base`）能否阻止旧基线覆盖更新的同步结果？
- lease record 诊断字段能否落盘供 doctor 读取？

## 2. 环境记录

| 项 | 值 |
|---|---|
| OS | windows |
| arch | x86_64 |
| target | x86_64-pc-windows-msvc |
| Rust | 1.97.1 stable-msvc |
| 锁原语 | `fs4` 0.13.1 `FileExt::try_lock_exclusive`（Windows `LockFileEx` 独占） |
| 进程模型 | 主进程 `spawn` 自身二进制的 `hold` / `try` 子命令，实测真跨进程 |

## 3. 结果（四断言全 PASS）

| 断言 | 结果 | 说明 |
|---|---|---|
| A 独占 lease | **PASS** | holder 持锁期间，第二进程 `try_lock` 立即被拒（不是两个都拿到） |
| B stale-lock 自愈 | **PASS** | 持锁进程被 `kill` 后，OS 自动释放句柄，新进程重获锁；无需手动清 stale 文件或 PID 超时判断 |
| C CAS activation | **PASS** | 首次 `CAS(17→18)` 成功；过期 `CAS(17→99)`（CURRENT 已是 18）被拒；最终 CURRENT=18，99 未写入 |
| D lease record | **PASS** | `instance_id / pid / process_start / operation_id / fencing_token` 可写入锁文件并读回 |

## 4. 对计划的意义（实测支撑，非假设）

- **审查 #5**：OS 独占文件句柄可作 writer lease 权威。进程崩溃 → OS 立即释放 → 新进程重获锁，这消除了"按 PID 或超时抢 stale lock"这一危险逻辑（该逻辑在 PID 重用时会误抢）。lease record 只作诊断，权威始终是 OS 句柄。
- **§6.5 CAS activation**：`CURRENT == expected_base` 的比较-交换在文件层用"读 CURRENT → 相等才 rename 新指针"实现，实测能阻止旧基线覆盖更新结果。生产实现必须在持有 writer lease 下做此 CAS。

## 5. 平台约束（新增守护，进生产实现）

**Windows `fs4` 独占锁是强制锁（mandatory lock）**：持锁期间，**同一进程另开第二个文件句柄读写同一文件会被拒绝（os error 33 / ERROR_LOCK_VIOLATION）**。

- 本 spike 的 D 断言最初用 `File::open` 另开只读句柄读回 lease record，即触发 os error 33；
- 修复：用**同一个持锁句柄** `seek(0)` 回开头读回，不另开句柄；
- 生产守护：**writer lease 持有者对 lease 文件的所有读写都必须复用同一句柄**，不得在持锁期间对同一路径另开句柄。这条要写入存储层实现规范。

## 6. tempfile 在 Windows 的已知行为（记录，避免误判）

经官方文档/Grok 核实：`tempfile::TempDir` 在 Windows 上，若曾 `spawn` 的子进程被 `kill` 且句柄未完全释放，drop 时 `remove_dir_all` 可能报 os error 33。这是 tempfile 已知平台限制，**其 drop 错误本应被静默忽略**。

本 spike 的 error 33 与此无关——真凶是 D 的第二句柄（见 §5），已定位并修复。但生产测试若用 tempdir + 子进程，应显式 `child.wait()` 后再清理，并容忍 tempdir 清理的 os error 33。

## 7. 局限

- 只在单机 Windows x64 实测；Linux/macOS 的 `flock`/`fcntl` 语义需在正式 CI 各测一次（尤其 stale-lock 自愈行为在网络文件系统上不成立，计划已排除 NFS 作为 data root）。
- CAS 用文件 rename 模拟，单进程串行验证语义正确性；真实多进程并发下的 CAS 竞争需在正式实现的 fault-injection E2E 覆盖。
- fs4 是 spike 选型，正式实现的锁 crate 由 0.2 storage 层最终决定（候选：fs4 / fd-lock / 原生 `LockFileEx`+`flock`）。
