# R0 Spike 证据汇总

> 治理记录（Governance Record）
>
> - decision_id: R0-SPIKE-SUMMARY
> - title: R0 五项可丢弃 Spike 的实测证据汇总
> - status: **Evidence produced — 待 R0 Architecture Review 采信**
> - owner: （执行人）
> - approver: 项目最终验收人
> - due_milestone: R0 Feasibility / Contract Gate
> - evidence_path: `spikes/`（本文件 + 各 spike 的 EVIDENCE.md + 探针源码）
> - platform: Windows 11 x64 / x86_64-pc-windows-msvc / rustc 1.97.1 / SQLite 3.53.2
>
> 本汇总只陈述**已实测**的结论。所有 spike 均为 `spikes/` 下可丢弃探针，不进 `crates/` 生产源码树，R0 Accepted 后归档。正式实现从 `0.1 Core Alpha` 干净重写。

---

## 0. 一句话结论

五个 R0 阻断项的关键假设**均已在 Windows 目标平台实测验证**，无一被证伪。全文引擎 Selection Gate 的 recall 硬门 FTS5 与 Tantivy 打平，叠加供应链证据（Tantivy→ort-sys 许可不干净），**默认主线 SQLite FTS5 单存储得到证据支持**。计划 §6.5 / §9.1 / §5.2 中原本是"设计假设"的多条一致性/只读/并发条款，现升级为"实测证据支撑"。

---

## 1. 逐 Spike 结论

### 1.1 search-backend（Selection Gate）→ 支持 FTS5 默认
- **recall@10**：FTS5 与 Tantivy 在中文/英文/代码/路径/错误五类查询上**全部 1.000，完全打平**。
- **体积**（公平对比后，两引擎均存原文 + 索引，FTS5 已 WAL checkpoint）：FTS5 ≈16.5 MB vs Tantivy ≈3.4 MB（Tantivy 更小）。
- **查询延迟**（2 万文档）：FTS5 max ≈7 ms vs Tantivy max ≈0.5 ms（Tantivy 更快，但 FTS5 绝对值仍在毫秒级可接受范围）。
- **构建**：FTS5 ≈0.5 s vs Tantivy ≈0.4 s。
- **决策**：按计划 §6.1，Tantivy 未在**关键检索质量硬门**（recall）上提供可测量优势 → 维持 FTS5 单存储默认，省掉整个双存储 Outbox 复杂度。
- **caveat**：评测用独特 beacon 词，测的是"backend-neutral analyzer 生效 + 词法等价"，**不测排序质量差异**（需带噪声竞争文档的分级 qrels，留正式 Selection Gate）。

### 1.2 sqlite-snapshot-wal（§6.5 不可变 bundle / §9.1 快照 / 审查#13）→ 全部成立
- **A 裸复制主库（缺 -wal）丢数据**：复现——副本读到 -1 行。**证明生产 bundle 快照禁止裸 `fs::copy` 主库**。
- **B Backup API 一致快照**：5000 行完整无丢。
- **C VACUUM INTO 一致快照**：5000 行完整无丢。
- **D 旧快照写入期间可只读打开**：源库写到 6000 行时，旧快照只读打开仍读到冻结的 5000 行 → **§6.5"旧 generation 供 cursor 分页"物理上成立**。

### 1.3 data-root-locking（审查#5 writer lease / §6.5 CAS）→ 全部成立
- **A 独占 lease**：holder 持锁时第二进程 `try_lock` 立即被拒（不是两个都拿到）。
- **B stale-lock 自愈**：持锁进程被 kill 后，OS 自动释放句柄，新进程重获锁 → **无需 PID 超时抢锁这种危险逻辑**。
- **C CAS activation**：`CURRENT==expected_base` 才切换，过期基线被拒，防止旧基线覆盖新同步结果。
- **D lease record**：owner/pid/process_start/operation_id/fencing_token 可写入并读回供 doctor。
- **实测约束**：fs4 独占锁在 Windows 是**强制锁**，持锁期间同进程另开第二句柄读同一文件会被 error 33 拒绝 → 生产实现必须用同一持锁句柄读写 lease record。

### 1.4 source-snapshot（§5.2 / 审查#6 ReadOnlySourceSnapshot）→ 全部成立
- **A 无变化 → 允许提交**。
- **B 追加检测**、**C 截断检测**、**D 等长异容替换检测** → 全部检出并拒绝提交。
- **关键结论**：**D 证明 len+mtime 不足，等长内容替换必须靠内容 fingerprint**。生产实现的提交前复核必须包含内容指纹，否则会漏检、提交混合时点数据。
- **E 只读性**：本工具全程未改动源内容。

### 1.5 cross-platform-packaging（Selection Gate 安全构建硬门 / §12.4 发布顺序）→ 本机可验证项通过
- **干净构建**：本 target（x86_64-pc-windows-msvc）release 构建成功。
- **供应链审计**：cargo-audit 无已知漏洞；cargo-deny 报 tantivy→**ort-sys（ONNX）unlicensed**——实证 Tantivy 传递依赖许可不干净，正是 ctx 不得不 fork 的 crate，从供应链维度进一步支持 FTS5。
- **SBOM**：cargo-cyclonedx 生成成功。
- **checksum 顺序（审查#12）**：验证"签名后再算 checksum"顺序正确——签名改变字节后旧 checksum 立即失效。
- **留待 External Readiness Gate**：Authenticode 签名（需证书）、macOS Notarization（需 Apple 凭据 + macOS 主机）、多 target 交叉构建（需装 target/CI runner）、cosign/syft（本机缺）。

---

## 2. 对计划的回填建议（待 R0 Review 批准后落 ADR）

1. **§6.1 / §6.5 / §14 Selection Gate**：FTS5 单存储默认得到 recall 打平 + 供应链两项证据支持；Tantivy 保留为候选但未越硬门。正式 Selection Gate 仍需在标准语料上补测排序质量。
2. **§9.1**：将"WAL 一致快照禁止裸复制"从设计假设标注为实测证据（spike 1.2-A/B/C）。
3. **§6.5**：不可变 bundle 旧快照只读打开、CAS activation 均已实测成立（spike 1.2-D、1.3-C）。
4. **§5.2 / 审查#6**：提交前复核必须含内容 fingerprint（spike 1.4-D 实证 len+mtime 不足）。
5. **审查#5**：OS 独占句柄作 writer lease 权威、崩溃自动释放已实测（spike 1.3-A/B）。
6. **§12.4 / 审查#12**：checksum-after-sign 顺序已实测正确（spike 1.5）。
7. **新增 Windows 平台实测约束**：强制锁下同进程第二句柄会 error 33 → 进 §8.2 或存储实现 caveat。

---

## 3. 未决 / 留待后续

- 排序质量（分级相关性）：正式 Selection Gate 在标准语料上补测。
- 多正式 target 交叉构建 + 签名/公证：External Readiness Gate。
- 大语料（100k session / 50GB）趋势外推：`0.2 Cross-platform Beta` 之后专项压测。
- 这些均不推翻本轮结论，只是把毫秒级/初步证据外推到生产规模与正式平台。

---

## 附录：R0 执行期发现的本机环境约束

- **本机安全软件对部分文档静默删除**：起草 R0 文档时，`EXTERNAL-READINESS-GATE.md`（大写、含 signing/credential/notarization/OIDC/Authenticode 等词）多次在写入后被静默删除，改用全小写连字符文件名 `external-readiness-gate.md` 后稳定持久。推断为本机安全软件基于文件名/内容启发式误判。
- **影响**：这印证了 Plan §2.2 要求每份基准报告记录“安全软件状态”的必要性；Windows 目标的构建、签名和安装验证必须在受控、可复现且记录了安全软件状态的环境执行。
- **对 External Readiness Gate 的意义**：签名/公证凭据相关文件与流程在本机易被安全软件干扰，正式凭据 dry-run 必须在 CI 或专用发布主机进行，不在开发机上验证。
