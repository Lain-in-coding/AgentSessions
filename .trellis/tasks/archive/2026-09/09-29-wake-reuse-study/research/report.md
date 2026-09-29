# Wake 深度借鉴研究

> 研究状态：源码对照已完成；实验结果在独立运行结束后补入本报告。
> Wake：v0.8.5 / `71aeca67ec80f8645d1f9d5199290c2c732036ce`（2026-09-27）。
> 本项目基线：`2b8f89562e43bd1f68ad3c8e5cf8ccc9281f9af2`。
> 范围：研究与隔离验证，不修改生产实现，不作发布或 provider 认证。

## 1. 结论摘要

**可以借鉴，而且存在有价值的增量；不建议整体引入 wake-core，也没有已批准直接搬入产品的模块。**

1. 实测排序：**初始索引吞吐是第一瓶颈**（100 万条约 8 分钟、峰值 RSS 4.58 GB、库 4.34 GB）；命中位置摘要是最确定的小范围体验增量；SQL 语句缓存只在长驻进程的廉价语句上有效（长连接约 15–24% 微秒级收益）；trigram 只换来子串语义，同时损失短词精度并放大索引成本。
2. Cursor IDE `cursorDiskKV` 和 Hermes SQLite 是真实的格式覆盖增量，不是已有适配器换个根路径。
3. DSH、ZCode 值得建立独立 provider 证据任务；本轮不做完整适配。独立上游 schema/事件资料见 [upstream-evidence.md](upstream-evidence.md)。
4. 本项目已经具备 CJK unigram/bigram、前缀查询、标题/正文排名融合、时间信号、稳定分页、统一 Application 投影、恢复预览和有界 handoff。不能把这些写成 Wake 带来的新能力。
5. Wake 的自动重建、源清理、软输出预算、shell/剪贴板回退与 GUI/远程功能不得直接移入本项目。

四级判定与后续要求见 [reuse-matrix.json](reuse-matrix.json)；固定源文件的 blob、SHA-256、行数见 [sources.json](sources.json)。

## 2. 仓库、许可与可复现边界

- Wake 是 Rust workspace：`wake-core` 负责适配/索引/查询/服务，`wake` 负责 GPUI 桌面端；固定快照有 235 个跟踪文件。
- 原仓保存在用户指定的 `Github_src/Wake`，保留 Git 历史并 detached 到固定提交；它是外部参考，不是主仓 submodule 或 workspace member。
- LICENSE 已核验为 MIT，Copyright (c) 2026 Corey Chiu。复制或改编实质代码必须保留版权及完整许可。本轮实验采用独立、明确标注的研究实现，不批准生产复用。
- `wake-core` 的 rusqlite 版本为 0.32，另含 notify、zstd、trash 等；GUI 使用 Zed GPUI git 依赖。引入整个 crate 会带入本轮不需要的运行时和副作用边界。
- 只读取程序源码与公开格式资料；没有执行 Wake、访问真实 transcript、读取其 fixture 内容、启动真实 provider、读取个人会话库或远程镜像。
- 官方上游证据分别固定提交；即使定义可核实，也不等于全部用户安装版本可兼容。Cursor 私有格式没有取得官方版本承诺。

## 3. 架构对照

```text
Wake: adapters -> scanner/watcher -> Store(sessions/messages/FTS)
                                       -> tools::invoke -> CLI/MCP Markdown
                                       -> GPUI / exporter / terminal services

本项目: read-only sources -> provider ports -> staged canonical data
          -> outbox + generation/CAS -> catalog + rebuildable projections
          -> Application ADT -> CLI / Robot / MCP / Web / TUI
```

两者虽同用 Rust/SQLite，但数据权威不同。Wake 的展示行/seq 和 provider:native key 不能替代本项目 StableId、installation namespace、来源 membership 与可验证的原生字节证明。元数据先入库、随后补正文的 GUI 策略也不能绕过本项目原子提交与关系完整性约束。

## 4. 搜索：借鉴局部，不替换契约

### 4.1 实际实现

Wake `db.rs`：FTS schema L68/L131/L171 使用 trigram；`search/search_with` 约 L2543-L2780；`fts_terms` L3255、`needs_like_fallback` L3260、`fts_match_expr` L3266、`make_like_snippet` L3307。

- 查询 token 加引号并 AND 组合；短于三个 code point 的词触发 LIKE 分支。
- 标题和正文分开搜；标题命中具有自己的合并顺序，正文使用 BM25 和会话更新时间信号。
- LIKE 摘要通过大小写展开到原字符位置的映射定位命中，避免将 lower-case 后的偏移直接用于原文本。
- Store 多处使用 prepare_cached；会话元数据复用减少同轮重复读取。

### 4.2 本项目已有的能力

基线 SQLite `query_with_policy` L7960-L8032：先过滤再 limit；独立语料库排名通过 RRF k=60 融合；以 wire ID 确定性破平。`safe_fts_query` L8490 起支持引号外末尾星号，之前把查询概括为“完全不支持前缀”的旧记录不能作为当前事实。

Application `assemble_search_hit` L970-L1009 仍以正文前缀做显示摘要；完整 payload 则参与 guidance，所以“命中正确但摘要看不到命中”是可验证的体验缺口。CJK 索引/查询共用 `fts_tokens_cjk`；guidance 的证据词另有约束，不能把 FTS 转换串暴露为匹配解释。

### 4.3 采用条件

- 命中摘要：放在 Application 投影层改造，先验证 Unicode、前缀、语义-only、JSON 字节预算，保留 ranking/cursor/StableId；本轮仅原型。
- 语句缓存：当前项目有意禁用 rusqlite cache feature 以减少依赖；只有长连接重复 SQL 存在合理收益窗口。必须比较连接生命周期、内存、依赖和制品体积，而非只贴热循环数字。
- Trigram：可作为对照，不默认替换 CJK。短词 LIKE 可能线性扫描；句内子串与 token/prefix 语义也不同，不能只看 recall 或耗时的单一汇总。

## 5. 扫描与 watcher

Wake `scanner.rs` L315-L633：refresh claims、路径归属、同 native ID 副本排序、quick metadata、mtime/size 跳过、删除检测、最近修改优先及失败副本尝试。

Wake `watcher.rs`：L46-L52 最长组件根匹配；L123-L199 合并事件后按当前文件存在性判定；L141-L143 处理 need_rescan；L202-L255 使用 800ms 收集窗。按实例而非仅 AgentId 归属、事件溢出重扫、重命名事件按处理时状态裁决值得加入未来测试清单。

限制：根最长匹配不是 installation 身份证明；mtime/size 不足以替代内容指纹；静默忽略 watch/remove 错误、不有界的事件队列和展示 key 去重不能整体移植。本项目已有内容指纹、截断尾保留、变更源推迟、完整性与 outbox 路径，不能倒退到更弱的猜测。

## 6. Provider 覆盖增量

| Provider | Wake 证据 | 当前本项目差异 | 判定/未决事项 |
|---|---|---|---|
| Cursor IDE | `cursor_ide.rs` L189-L236/L348-L384：cursorDiskKV、composerData、bubbleId、headers 顺序 | 当前 Cursor 主要读取 ItemTable 两种 key | adapt；缺 bubble/坏行不能返回“零未知”；需独立 storage variant 和上游版本证据 |
| Hermes | `hermes.rs` L174-L303/L409-L435：state.db、profile、REAL 秒时间、两种 tool_calls | 当前本项目 Hermes 解析 session JSON | adapt；保留 profile 命名空间、精确 ID、歧义关联；不能用 FIFO/合成 call ID 冒充原生证据 |
| DSH | `dsh.rs` L169-L211/L224-L495：代际 JSONL/zstd、header 原生 ID、事件词汇 | 新增 provider 候选 | adapt；解压/记录总量须限额；过滤 subagent/忽略 replace 不是规范语义 |
| ZCode | `zcode.rs` L103-L138/L322-L435：session/message/part、可选列、隐藏状态 | OpenCode 的相似三表 probe 不能认证 ZCode | adapt；独立判别 variant，不能默认继承 OpenCode resume |

补读 `opencode.rs` L346-L404 后确认，ZCode 复用的 v1 解析器会先把 part 按 message_id 收集，再处理 message；其中 rows.flatten 会吞掉读取错误，坏 part 的统计也并不等价于完整性证明。这个结构不能逐段复制为本项目的有界、可审计解析器。

这些能力的生产落地都需经过 Probe -> bounded parse -> staged canonical data -> 来源证明/命名空间 -> 原子提交路径。本轮最小探针只验证字段与顺序，绝不注册新 provider 或宣称 resume 可用。

## 7. CLI/MCP、上下文与恢复

- `cli.rs` L219-L255、`mcp/tools.rs` L248-L264：查询共用 tools::invoke；`bin/wake_cli.rs` L76-L86 和 MCP 调同一层。它统一的是 Markdown 工具输出，不等于所有服务共享完整 Application ADT。
- `mcp/tools.rs` L902-L924/L1116-L1132/L1342-L1346：列表 offset=0 与裸 from_seq/next_seq 不是绑定 generation、查询与 TTL 的游标。
- `services/exporter.rs` L223-L232/L261-L394：默认转录 60 条、20,000 字符、单条 4,000 字符；标题/页脚/标记等另占空间，不能替代最终序列化字节硬闸。
- CLI 参数与 MCP schema 的双向覆盖测试（`cli.rs` L960-L1011）值得适配到现有测试，而不是另做一套业务入口。
- `services/context.rs` L17-L68 做目录范围选择，不能证明 junction/alias 与 installation 身份等价。
- `terminal/mod.rs` L163-L175/L359-L405 的 ID 检查有启发，但之后进入 shell、GUI、剪贴板以及平台脚本链；安装探测、进程投递成功和真正 resume 成功须分开。
- 本项目已有结构化 executable/args/current_dir、首次预览、明确 ACK、原始 CWD 与原生元数据；不借鉴 shell 字符串恢复或剪贴板 fallback。

完整导出另有全部 thinking/tool I/O、当前时间与子会话失败被吞掉等差异，不能作为本项目确定性、有界且受隐私约束的 handoff 实现。

## 8. 明确拒绝的反模式

1. **索引打开失败就重建**：`db.rs` L359-L382 实际删除既有 .corrupt、rename 主库并删除 WAL/SHM，且忽略这些文件操作错误；并非安全地保存三件套。对任何 open 失败都走此路径，不能移植到 catalog 权威数据。
2. **只读名义下的副作用**：`adapters/sqlite_ro.rs` L21-L60 的临时复制方案忽略边车复制错误，不证明一致快照；查询启动的路径选择也可能迁移旧库。只读 annotation 不是审计结论。
3. **源 cleanup/delete**：适配器 cleanup 归属接口与 source-read-only 冲突，不能混入扫描路径。
4. **错误即空成功**：rows.flatten、unwrap_or_default、静默 clamp/跳过必须按本项目错误、完整性和预算契约重新实现。
5. **展示身份替代原生身份**：重排 seq、合成 tool IDs、冒号拼接 key 和有损路径显示只能用于展示，不可成为 StableId 或 resume proof。
6. **扩大产品范围**：GPUI、SSH mirror、源回收站、团队同步、后台联网、默认模型下载均不在本轮范围。

## 9. 隔离实验结果

全部实验在本机合成语料上完成，原始 JSON 与校验器在 `results/`；环境见 environment.json（Windows/NTFS、Ultra 7 155H、31.5 GiB、NVMe、Defender 实时防护开启；未清系统缓存、未关闭杀毒）。基线二进制为 `2b8f895` 未修改 release 构建（SHA-256 `605bd1b3…`，见 baseline-build.json），产品范围 diff 为空。

### A. 命中摘要（49 个合成用例，49/49 通过）

- 当前 prefix 策略在 6/49 用例超出局部 `{"text":...}` JSON 字节上限（实验本地预算控制组，非完整产品信封），长消息/转义场景确有显示层缺口。
- 命中窗口候选在 25/49 用例可见字面命中（prefix 对照为 16/49）；无匹配、语义-only 与预算拒绝场景返回诚实空窗，不伪造高亮。
- Unicode 小写展开、末位星号前缀边界、emoji/组合字符/CJK/控制字符与 2,744 组预算组合通过；只保证 Unicode 标量边界，不保证字素簇完整。
- 结论：值得以独立 Application 任务实现“命中窗口摘要 + 最终响应字节闸”，不改排名、游标、id 与证据契约。

### B. 检索与语句缓存

微基准（同库、同 SQL、同数据；release；100 万消息）：

| 指标 | CJK 单字+bigram | trigram / 短词 LIKE |
|---|---|---|
| 索引构建 | 3.67 s | 10.09 s |
| 单字「界」p50 | 0.049 ms | 120.1 ms（LIKE 全扫） |
| 双字「配置」p50 | 0.086 ms | 118.4 ms（LIKE 全扫） |
| 三字「数据库」p50 | 0.115 ms | 0.054 ms |
| `qarzneedle` 召回@10 | 0.5 | 1.0 |
| `OR` 召回@10 | 1.0 | 0.0 |
| 纯标点 `%` `_` `\` 召回@10 | 0（当前被跳过） | 1.0 |

- 短词 LIKE 从 10 万的 11.8 ms 增至 100 万的 120 ms，近似线性——证实短词降级不能作为默认路径。
- trigram 的语义收益是真实子串匹配（`qarzneedle` 命中 `xxqarzneedleyy`），代价是短词精度下降（`OR` 命中无关子串）、构建时间约 2.7 倍、短词延迟两到三个数量级。
- 语句缓存（100 万、16 条容量）：长连接下廉价语句 p50 0.021→0.016 ms（约 24%），CJK FTS 0.047→0.040 ms（约 15%）；每次重开的连接无收益（0.083→0.104 ms）；127 ms 级 LIKE 扫描与准备开销无关。
- 结论：CLI 一进程一查询难以受益；缓存候选只在长驻 MCP/Web 进程且只对廉价语句有效，须以依赖/制品/内存实测作为准入条件。

产品基线（未修改 release 二进制，进程级 warm 采样，n=100/scale）：

| 规模 | 初始 sync | 空转 sync p50 / p95 | search p50 / p95 | 库+sidecar | 峰值 RSS |
|---|---|---|---|---|---|
| 1 万 | 2.42 s | 22.0 / 24.1 ms | 149.8 / 181.5 ms | 51.2 MB | 154 MB |
| 10 万 | 35.0 s | 59.2 / 78.8 ms | 144.2 / 186.4 ms | 512.1 MB | 1.44 GB |
| 100 万 | 482.6 s | 645.6 / 2920.1 ms | 118.6 / 175.3 ms | 4.34 GB | 4.58 GB |

- 初始同步随规模超线性（10k→100k 与 100k→1M 均约 14 倍），100 万条约 8 分钟，为主要瓶颈。
- search 进程级 p50 稳定在 119–150 ms、无随规模恶化趋势；进程启动 p50 10.98 ms / p95 14.65 ms 已包含在内，查询本身远低于该值。
- 空转 sync 随文件数增长（200 文件时 p95 2.92 s），“无变化扫描”仍需按文件数优化。
- 产品召回与 CJK 微基准一致（`qarzneedle` 0.5），两次实验交叉验证同一语义：嵌入式 ASCII 子串当前不匹配。
- 局限：未清系统缓存，不构成冷盘结论；search 采样含进程启动与渲染；初始 sync 为分批总计；基线二进制由调用方提供并锁定哈希，未独立证明源码到二进制链路；Windows 计时曾出现 15.6 ms 量化，已改用高精度时钟后重跑。

### C. Provider 结构探针（35 个合成用例，35/35 通过）

- Cursor IDE：header 顺序（不按 KV/插入序）、缺失/NULL/坏 JSON/错形状/坏 UTF-8 的显式槽位与状态、`rawArgs→params→文本` 输入编码、`:`/`#`/Unicode 原生 ID、重复 bubble 独立序位、7 类负例（含巨大 cell、行数/字节/头部预算）；并发提交下源 DB/WAL/SHM 字节与元数据保持不变，读事务固定。
- Hermes：`timestamp, id` 排序、主库/命名/Unicode profile 命名空间隔离、两种 tool-call 形状、歧义与未匹配关联一律 `authoritative: false`、NULL 时间戳/正文显式保留、7 类负例。
- 独立上游证据：Hermes schema v30（sessions/messages 列）与 ZCode session/message/part + task_type/title_source/sequence 已与官方固定快照核对；Cursor 私有存储无官方版本契约，仅标“结构可复现”。
- 结论：Cursor IDE 与 Hermes SQLite 是真实格式增量，应各自开启独立 provider 证据任务；探针通过不等于兼容认证、身份证明或 resume 支持。

### 校验与可复现

- snippet：`validate-report` 确定性重放通过（49/49）。
- providers：`report.py validate-report` 重放通过（35/35）。
- 微基准：`run_search.py validate` 通过（含 DB 哈希与 query plan）；no-cache 与 cache 两份报告均 complete。
- 产品基线：封存与结构校验通过；三规模 complete。
- 任务级：`validate_study.py --mode full --wake-checkout <Wake> --workspace <repo>` 校验固定提交、27 个源文件哈希/blob、21 条复用矩阵、全部结果报告与产品范围 diff；5 项单测覆盖篡改拒绝与隐私扫描。

## 10. 后续顺序与治理

1. **初始索引吞吐专项**（实测第一瓶颈）：100 万条 8 分钟、RSS 4.58 GB、库 4.34 GB；先做 profile/写放大定位，再决定批量事务、索引投影或分片策略，不引入新依赖。
2. **无变化扫描按文件数优化**：200 文件 p95 2.92 s；对齐现有指纹缓存的 O(文件) 成本，不加后台 watcher。
3. **Application 命中窗口摘要**：按实验 A 的边界单独开任务（含最终信封字节闸与语义-only 诚实空窗）。
4. 格式扩展优先 Cursor IDE/Hermes，DSH/ZCode 后续独立证据任务；每项都要完整性、资源预算、native proof、source immutability、版本矩阵和 golden 测试。
5. 语句缓存只在长驻进程廉价语句被证实为主要瓶颈时再评估，且必须先量依赖/制品/内存增量；短词 LIKE 与默认 trigram 不进入候选。
6. watcher、CLI/MCP schema 一致性作为后续测试增强候选，不新增无需求的功能。

本轮不修改 `.trellis/spec/` 或正式 REUSE-LICENSE-AUDIT/发布矩阵，不冻结 SLO；研究发现先保存在任务内。后续实质复用需单独许可复核、正式任务与相应规范审查。
