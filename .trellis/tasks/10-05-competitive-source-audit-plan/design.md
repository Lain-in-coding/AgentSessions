# 整改设计建议（Draft / 待决策）

## 0. 状态与边界

本任务仍在源码审计/规划阶段。此文把已证实问题的解决方向写清楚，不授权实施。全量逐文件阅读仍有缺口；D1 journal 保留策略、D2 发布次序未由用户拍板。任何方案都不能绕过 source read-only、原子性、稳定身份、generation/cursor、预算和默认脱敏契约。

## 1. 产品边界：可验证的历史检索与继续工作

沿用已有正式路线的 local-first / CLI-first 和个人重度用户定位，不另造产品/规范真源。

核心链：**显式发现与同步 → 找到相关会话/消息 → 展示可核验上下文 → 原生恢复预览或有边界的交接。**

- 已存在的 CLI/MCP/Robot/TUI/Web 保留共同 Application；不重写五套引擎。
- 原生 resume、以 transcript 开新会话、生成 handoff pack 分别定义，不合并统计。
- provider 数、界面数、RFC 数都不是价值指标；评价首个成功耗时、正确证据定位、后续任务是否继续得下去。
- 新 GUI、云同步、图谱、自动摘要、更多模型/provider 不作为当前已证实问题的解决方式。

## 2. 事实与对外契约治理

既有 `docs/product/COMPETITOR-COMPARISON.md`、`OPEN-SOURCE-ROADMAP.md` 继续作为文档入口，修正文档，不建立另一个互相冲突的产品真源。

每个比较条目要有：本地/远端版本、实际产品形态、源码/实验出处、直接竞品或相邻产品分类、已实现/计划/未验证、许可限制。功能独占或性能优越必须有正向比较证据；未覆盖竞品不能写“无人”。

分开四种指标：preview 可生成；native resume 在指定 provider/version/OS 上可执行；pack schema/引用/预算有效；接收方完成后续任务。现有 dry-run 测量不篡改，只更准确命名和解释。

## 3. 消除热路径的重复 Git 探测

### 当前数据流

CLI parse → resume_app（派生 repo）取 clock → 再建 resume_app/semantic_app（再派生 repo）→ Application Search。
get/show 也走含 repo 探测的 factory，而其用例不需要排名信号。

### 建议数据流

1. 直接取得一次请求时钟，不为了取时钟构造有环境副作用的 App。
2. 仅 Search/Handoff 等需要 repo-aware ranking 的用例派生 repo。
3. 每次请求只派生一次，所有 Application 调用复用该上下文。
4. 按 ID 的 get/show/status 等纯读取不探测 repo。

实现时优先最小改动：分离 read/search factory 或传入已经解析的上下文，不先重写 CLI 框架。不要以跨请求永久缓存 origin 来换速度，除非定义了失效语义；MCP/Web 的长生命周期尤其不能忽略仓库配置变化。

**契约不变**：同 repo boost、无 origin/非 Git 时 None、cursor 固定评分时钟、显式过滤、机器输出枚举不变。已有 ASG_CURRENT_REPO 只用于注入测试，不是要求用户设置的“修复”。

**验收结构**：get/show 0 次 Git；需要 repo 的普通 search 最多一轮解析（有 origin 时两次子进程），不是四次；固定输入时 hits/score/page/cursor semantics 不变。性能基线用相同二进制配置与固定机器复测，不把本机 9ms 当全平台 SLA。

## 4. Journal 生命周期：先定义保留合同，再 compact/GC

### 当前事实

完整 upsert/delete/relations/source-replacement manifest 被保存在 activated outbox。200-message 固定当前数据、21 次单消息改写让库从 1.37MB 到 6.07MB。

### 不可妥协

- 未决 building、仍用于恢复/幂等校验的记录不可删。
- catalog、FTS、关系、活跃 generation 必须原子一致。
- relocation、冲突检测、重放、reader 并发和备份恢复必须保留。
- 不以“数据库没空间”为由静默丢日志或历史数据。

### 两个候选方向（D1 未决）

A. 必须永久保存完整历史：保留语义，优先消除重复/采用可验证的无损紧凑表示；给体积、备份和性能真实预算。

B. 允许有约束治理（推荐）：建立 terminal record 的明细保留上限/期限；先证明哪些 generation、digest、状态及必要身份为长期必需，再 compact 细节。提供可解释预览与受控维护，不直接删除整个 outbox。

在核清消费者和恢复协议前，不拍脑袋指定“保留 7 天”或直接执行 DELETE。迁移应向前兼容：旧格式可读、新格式有明确标记、未知格式 fail-closed；rollback 先恢复备份或在兼容版本范围内回退，不能让旧版本误读简化明细。

**验收**：追加/等长改写/分支重排 soak；固定当前集合下区分数据体积与日志体积；GC/compact 中断、重放、并发读、relocation、活动记录保护、明确审计信息损失边界。

## 5. 首用与结果可读性

### 最小方案

- `--db` 显式值优先；未给时解析已有平台默认数据位置，而不是引入多套隐式 fallback。
- 读命令遇到未初始化库仍不写磁盘、不伪装空成功，给一条准确的显式 sync 指令。
- Quickstart 顺序必须是拿到二进制/检查版本 → 确定读取范围 → 显式 sync → search → context/resume/handoff。
- 不必先造复杂向导。先修错误信息、默认路径和会话结果投影。
- 人类结果优先展示标题/项目/provider/时间/命中片段/可采取动作；高级 wire-id、原始输出仍可获取。
- discovery 不可用的 provider 直接说明需要指定源，而非让用户猜“为什么零结果”。

### 边界

不自动扫描全部 HOME，不执行 provider，不修改其配置；保留 Robot schema、退出码、显式 DB 优先级和跨边界路径脱敏。若默认路径行为影响现有脚本，应有 opt-out/显式路径和版本说明，而不是破坏旧脚本。

## 6. 检索质量而非检索名词

### 默认/可选路线

- 默认 lexical 继续可用。
- bigram-hash 明确称 fuzzy lexical vector，在 help/model status/result 中复用现有模型元数据，使 capability 一眼可见。不能直接破坏现有 retrieval_mode 枚举；新增说明应 additive。
- Candle E5 保持离线、显式导入与 hash/model-id 约束，不在本轮擅自引入联网下载。

### 评测

固定现有 100-query 合成集作为回归，而非唯一优化目标。新增独立 holdout：中英文改写、两个汉字/单字、代码标识符/路径/错误栈、长消息尾部、近重复/噪声、来源失效、时间/repo/facet 选择性、分页一致性。

衡量 Recall@k、MRR/nDCG 或任务成功、误命中、引用正确率、无结果解释、延迟/存储/token 成本；按语言和类型分桶。真实语义需要真实模型权重运行，不能拿编译测试作质量证明。若没有稳定质量增益，保持 optional/experimental，而不是靠改阈值晋级。

### 性能与长文

当前向量 SQL 先过滤后 exact scan + bounded heap，这是正确性资产。先测候选数、维度和过滤选择性，再讨论 ANN。当前 512-token 截断和一消息一向量需要长文 chunk/aggregation 实验；若引入 chunk，必须仍可追溯到消息与原文范围、正确处理删除/重建/预算。

向量全量重建在单事务内编码的正确性不能为并行跑分牺牲；若大规模锁持有成为瓶颈，设计 staging generation + 原子切换，而不是对 live index 分批露出半成品。

## 7. Provider 与证据生命周期

- 高频 Claude/Codex 优先形成 provider/version/variant/OS 的可追溯证据，但不自动删掉其他入口和 12 家现有 adapter。
- 覆盖目录发现、source snapshots、合法/损坏/巨大记录、字段变体、UTF-8/CJK、分支/子代理、usage/tool metadata、append/shrink/rewrite/move、共享 SQLite 源与 WAL。
- 使用授权、合成/脱敏样本；真实本地回归结果可以汇总，不能把 transcript 放进仓库。
- 竞品新 parser（如 Wake 的新来源）只形成线索，不直接让 ASG provider 晋级；先取得格式与版本证据。
- 无 span 的 SQLite/整文件 JSON 不伪造 byte offset，可设计有类型的逻辑 locator 并说明证据精度，仍需验证。

## 8. 维护与交付

按稳定职责拆分 CLI 参数/装配/命令、SQLite schema/migration/catalog/projection/search/journal。先完成可测小修复，不以“大重构”代替问题修复。参数元数据单源和 schema/能力/入口漂移测试优先于框架替换。

Release gate 绑定具体 commit、toolchain、feature、target、二进制 hash 与安装/升级/卸载验证。记录真正的三 OS 成功证据；有 YAML 不算跑过。必要时加入 MSRV 验证或更正声明，但未跑不能说 1.90 不兼容。

公开、签署、账单、历史清洗与账号权限是 owner 边界；代码不能伪造解决，用户未批准不发布。

## 9. 风险与未决

D1/D2 待用户确认；源文件完整审读仍未完成；竞品动态对跑受许可/平台/依赖边界限制。所有下一阶段项目都要有独立 PRD/设计/实施批准；此建议稿不触发 task.py start。
