# ASG 双基线复核：旧审计与整改候选版本的适用边界

- 日期：2026-10-06。
- Query：竞品审计中的八项主结论，哪些仍适用于已经整改的候选分支，哪些必须保留版本限定或纠正适用范围？
- Scope：本地源码交叉核验、隔离合成验证、远端状态核验尝试；不是重新完成全量源码审计。
- 本次只新增本任务研究材料和被忽略的合成数据；没有修改产品代码、源会话、共享规划文件、任务状态，也没有 commit/push/merge/release。

## 1. 两个基线都保留，不能相互冒充

| 基线 | 工作区角色 / 分支 | 精确提交 | 证据用途 |
|---|---|---|---|
| A：原竞品审计 | baseline-workspace / `fix/session-relocation-identity` | `5b232cdedbff33a251e7fb266558be7af8496e1a` | `review-report.md`、`research/asg-main.md` 和 2026-10-05 实测的原始对象 |
| B：整改候选 | remediation-worktree / `fix/global-review-remediation-20261002` | `6a48640629c7c7f9d7682d77d9ad5cf4d0402581` | 本文新增复核及 `asg-candidate-*-20261006.json` 的对象；不代表已合并或正式发布 |

`git merge-base --is-ancestor A B` 返回 0，B 是 A 的后续提交。两处工作树已经分别核实。原报告对 A 的有效发现不会因 B 已修复而变成误报；反过来，也不能把 A 的旧行号、旧性能数据、旧测试数量直接写成 B 的现状。

当前另一个活跃审计线程拥有原任务的 `prd.md` / `design.md` / `implement.md` / `review-report.md` / `coverage-index.json`。本次增补保持独立，由该线程按需要合入；不覆盖它正在进行的逐文件审读与产品决策。

## 2. 八项结论的逐项判定

下表代码行号均属于 **B**，除非显式标记 A。原发现编号沿用原报告，仅用于追踪，不把产品整改优先级等同于安全事件严重度。

| 原编号 | B 上的复核结论 | 证据 / 反证 | 对计划的影响 |
|---|---|---|---|
| P0-01 竞品事实与差异化 | **仍成立：文档问题未随整改消失。** | `docs/product/COMPETITOR-COMPARISON.md:22-34` 仍有 cass/Python、agentsview/TS CLI、AgentRecall/Python、agent-sessions/Go、agf/Go、historian/Python 和 cc-switch/无检索等旧表述；该文件 A→B 无差异。`OPEN-SOURCE-ROADMAP.md:88-94` 仍有绝对化措辞。竞品代码反证仍以原审计的快照/路径为准。 | 保留事实账修订；逐条引用实际实现与适用范围，不再依据错误的产品形态定位对手。不得在本文顺带声称全部竞品已经审完。 |
| P1-02 重复 Git 探测 | **仍成立，已定位到 B 的实际调用链。** | `cli/src/lib.rs:2098-2104` 为获取过滤时钟构造 `resume_app`；`:2161-2169` 执行搜索再构造 `resume_semantic_app`；`:3140-3181` 两个 factory 都派生 current repo。`repo_identity.rs:31-54` 的成功路径每轮运行两个 Git 命令。 | 保留最小优化：读时钟不构造有环境副作用的 App；需要排序信号的请求只解析一次 repo；get/show 不做无关探测。A 的毫秒差额不能作为 B 的实测结果。 |
| P1-03 journal 增长 | **拆分：正文副本问题已有修复；终态批次累积仍成立。** | `adapters-sqlite/src/lib.rs:738-771` 的新 manifest projection 已使用 typed identity + `payload_blake3` / `text_blake3`，不再序列化原正文。两个针对性测试本轮通过。新 21 轮实验仍产生 21 个 activated batch，见第 3 节。 | 不重复实施已经存在的正文摘要化；D1 应聚焦历史操作元数据的保留与紧凑表示。旧数据迁移、消费者、幂等验证与恢复合同仍需专项确认。 |
| P1-04 默认向量与真实语义 | **默认能力事实仍成立；旧质量数字不得搬到 B。** | B 的实际 `providers` 输出为 `default_model=bigram-hash-v1`，默认构建的 feature/runtime 为 null；`cli/src/lib.rs:3021-3040` 已有“模糊词法、非语义”说明。`application/src/candle_embedding.rs:129-133` 仍设 512-token 截断。没有实跑 B 的真实 E5 权重与独立 holdout。 | 不把模型类型已有诚实说明再诊断成“完全未披露”。是否补齐每个使用入口及长文质量要单独验收；A 的 Recall@10 数字只作历史记录。 |
| P1-05 首次使用摩擦 | **仍成立，错误提示已动态复现。** | B 的 `asg search needle` 返回 exit 2，并称合法的 `search` “可能不是有效命令”；代码 `cli/src/lib.rs:1696-1733`。README 仍要求显式构建/DB。 | 可以保留准确报错与首用路径优化建议；默认 DB/自动发现仍是产品兼容与隐私决策，不由本次实验代替批准。 |
| P1-06 provider 支持深度 | **现有矩阵事实重新确认；不是“parser 全不可用”。** | B 实际输出 16 行：14 行 `parse=native` 且 maturity=experimental，2 行 parse=unknown / maturity=unsupported。14 行内 resume 为 8 derived / 3 unknown / 3 unsupported；tool_activity 为 2 partial / 12 unsupported；Aider/Cursor discovery unsupported。原始输出见单独 JSON。 | 保留按 provider/version/OS 证据晋级；不能把测试通过、行数或 derived 能力等同于 native resume/完整上下文已验证。 |
| P2-07 维护性债务 | **部分证据可保留，旧行数需更新；不是重写理由。** | B 的 SQLite `lib.rs` 为 22,654 行，主 `mod tests` 从 10,609 行开始。这里仅做行数和边界清点，没有因此声称全文件已读；测试、辅助代码不能全算生产膨胀。参数元数据所有入口是否可合并，本轮未做完整结构证明。 | 限定为有覆盖保护的职责拆分候选；不以大文件行数直接推出换数据库/框架/全部重写。参数统一保留待具体影响分析。 |
| P0/P1-08 发布与指标 | **指标口径仍成立；远端发布状态本轮未核验成功。** | `scripts/evidence/open_source_gate_benchmark.py:284-299,328-335` 仍只测 preview 命令可生成、未执行 provider。README:11-12 仍声明未发布。GitHub API 本轮返回 EOF，不能以 README 或旧 CI 日志替代当前远端事实。 | 明确区分本地验证、同 SHA CI、候选制品/安装、正式 release；不把 EOF 解释为 billing blocker，也不据此变更可见性。 |

说明：P1-02 的“四个 Git 子进程”限定在无测试覆盖变量、处于有效 Git 仓库且存在可解析 origin 的成功路径；非 Git、无 origin 或覆盖注入时会提前退出。`get`/`show` 在 `cli/src/lib.rs:2380-2408` 仍调用同一带 repo 的 factory。时钟源统一不等于重复环境探测已经消除。

## 3. 新 journal 实验：修复正文放大，不等于已解决历史增长

原始材料：`asg-candidate-journal-growth-20261006.json`。

### 方法

1. 在 B 上按默认 features 执行 locked/offline 的 debug 构建，随后拷贝候选可执行文件到被忽略的独立合成目录，避免共享构建产物被后续 relink 影响。
2. 仅生成一个 200-message Claude 格式的合成 JSONL；格式沿用 `scripts/evidence/core_beta_benchmark.py:198-214`，最后一条正文追加固定宽度 `audit-revision-NN` 标记。
3. 从全新库开始执行 21 次显式文件 sync。每轮仅改变最后一条标记，源文件字节数保持不变；不 discover 用户目录，不启动 provider，不下载模型。
4. 对 `index_batches` 的六个 JSON 列计数，分别记录字符数与 UTF-8 字节数。磁盘数值是探测时实际存在的 DB/WAL/SHM 合计，不是仅正文 payload 的体积；sidecar/句柄状态会影响这项快照值。
5. 第 21 轮后再对未变源执行一次 no-op sync，检查批次数、manifest 长度与 catalog 行数均不变化。

| 指标 | 第 1 次同步 | 第 21 次同步 |
|---|---:|---:|
| 源字节数 | 56,126 | 56,126 |
| catalog 行数 | 202 | 202 |
| activated 批次 | 1 | 21 |
| journal JSON 字符数 | 257,427 | 6,064,807 |
| journal JSON UTF-8 字节数 | 257,427 | 6,064,807 |
| DB + 存在的 sidecars 字节 | 1,830,912 | 7,737,344 |
| manifest 是否含合成正文标记 | 否 | 否 |

**反证 / 防止夸大：**最后一次 no-op 不增加批次或 manifest。问题是有内容修改的历史批次持续累积，不是“所有 sync 无条件写新日志”。两个源码测试还证明 projection 描述符大小不随正文长度增加，且 payload/text/identity 篡改会改变封存摘要。

**不能从这些数字推出：**B 比 A 退化某个百分比、任意规模线性增长、历史旧库正文已清除、三平台表现相同或性能 SLA。A 的源文件为 56,320 bytes，本次为 56,126 bytes，schema/路径/构建也有区别；两轮实验形状相似，但不是可直接排名的配对 benchmark。

### 需要修订的实施建议

- 对 B 不再新增“将 manifest 正文替换为 hash”的重复工作；这已经存在。
- D1 仍是有效产品决策，但对象应写成“已完成操作的详细元数据/历史审计证据是否允许有边界治理”，不能含混成删除用户会话历史。
- 未决 building、活动 generation、源事实、relocation、幂等与诊断消费者的合同未核清前，不实施 GC，不拍脑袋定 7 天/固定条数，不把 sealed JSON 任意清空。
- 核查长期摘要的最小字段集，并验证旧版本读取、重放/失败恢复、并发 reader、备份及回滚。`adapters-sqlite/src/lib.rs:6411-6464,7049-7073,8216-8285` 是本轮已定位的写入、验证、恢复与诊断入口，不是消费者全集。
- 新旧数据迁移必须独立验证。本轮只证明新库的新写入不含正文；不能因此认定旧 schema 留下的 manifest 已全部清理。

## 4. 本轮实际执行与未执行

| 项目 | 结果 | 限定 |
|---|---|---|
| A 为 B 祖先；两工作树分支/HEAD | 通过 | 只读本地 Git；不证明远端已合并 |
| `cargo build --locked --offline -p agent-session-grep-cli --bin asg` | exit 0 | B；默认 features，debug；共享 build cache 的 Cargo 校验串行执行 |
| `cargo test --locked --offline -p agent-session-grep-adapters-sqlite --lib source_projection_manifest_ -- --nocapture` | 2 passed / 0 failed | 仅 descriptor_size 与 seals_original 两项；不是本轮重跑整个 workspace |
| 21 次修改同步 + 1 次 no-op | 通过 | 纯合成、全新 DB；原始逐轮数据/二进制 SHA256 已保存 |
| `providers` 默认能力矩阵 | exit 0 | B；原始 frame 保存在 `asg-candidate-provider-matrix-20261006.json` |
| 缺 DB 的合法 `search` | exit 2，复现含混提示 | 预期错误，不记为构建或测试失败 |
| GitHub repo / PR #21 / releases / runs 核验 | 未完成：GraphQL 和 REST 请求均 EOF | 没有获得可用新远端状态；公开页面抓取也未提供可判定正文 |
| 全 workspace / 浏览器 / PTY /真实 E5 /跨平台 /安装发布 | 本轮未执行 | 既往记录须保留原 SHA/日期；不能记作本轮新验收 |

运行参数遵守既有共享构建限制：jobs=2、test threads=2，Cargo 命令整体串行；未停止其他进程，未清理其他工作树，未改变仓库可见性。

## 5. 给主审计与下一阶段规划的交接

1. 原竞品审计继续以 A 为基线完成全部 15 项目覆盖；不要用本文替换其尚未完成的逐文件审读。
2. 若规划实施到 B，必须先套用本表的已修复/仍存在/仅历史数据/待验证分类，避免重新修复同一问题或重复计入收益。
3. 已批准的整改阶段顺序与本任务的产品取舍，仍以各自权威任务为准。本文不批准发布、默认数据路径、日志治理、功能冻结或任何产品改动。
4. 全局剩余交付门禁应以最终同一候选 SHA 对齐质量、制品、安装与升级证据；本次 EOF 只记远端核验缺口，不等于可跳过。
5. 本次两个并行竞品读稿发现与主审计线程重复后已要求停止新增扫描；仅保留精确的部分覆盖记录供主线程合并，不宣称 agf/sessiongrep 全量审完，也不缩减总任务范围。

## Caveats / Not Found

- 本文是交叉复核增补，不是最终合并方案、全量安全审计证明或实施批准。
- 对原报告八项以外的风险、B 其余新增修改和第三方依赖没有作穷尽结论。
- 当前远端可见性、PR 状态、release/制品与最新 CI 无可用在线证据，仍需重新查询。
- 保持 planning；不运行 `task.py start`，不改其它线程正在维护的规划/coverage 真源。
