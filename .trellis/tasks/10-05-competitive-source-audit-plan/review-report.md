# agent-session-grep 竞品源码对照与整改评审（覆盖收口稿）

> 日期：2026-10-05 起草，2026-10-06 覆盖收口；ASG HEAD：`5b232cdedbff33a251e7fb266558be7af8496e1a`。
> **范围不缩水，完成度不造假：15/15 项目已进入源码级对照；覆盖分三层且逐文件记账。** T1=有精确行区间回执+SHA256 的逐段全文精读；T2=14 个探针对 9,421 个非二进制文本文件的逐文件机械扫描（`research/sweep-*.json`）；T3=显式排除（二进制/超大非源文件，记录原因与哈希）。全量清点 9,820 文件、约 231 万候选源码行（含测试与可能的生成代码），不等于全部人工读过。中央账本 `coverage-index.json` 记录每个文件的 state/区间/哈希复核；覆盖收口时 **15/15 项目均已有 T1 回执并入**（cass/agentsview/agent-sessions/cc-switch/cc-sessions-viewer/ctx 为多分片）：282 个文件全文 + 66 个文件有区间回执（合计 ≈25.1 万行带精确区间），9,421 个非二进制文件有 14 类探针逐文件扫描记录（其中 348 个已升级为 T1 回执）；未读/未纳入行区间者与残余边界在各项目 receipt 与 §三·B 明确列出，不冒充已读。本报告不是全量安全审计证书，任务不标 completed。
> 本轮只写本任务的研究/计划，运行隔离合成验证；没有修改产品代码、执行真实会话恢复或发布仓库。未使用旧 deep-read 报告代替源码。

## 一、先给判决

**ASG 不是一个没技术含量的玩具；它是一个有相当工程底子，却把“工程体系齐全”误当成“产品竞争力已经成立”的未交付产品。**

最该挨骂的不是 Rust、SQLite 或分层架构，而是下面这条链：

> 对手有些是什么技术栈都没写对 → 给自己立“无人做到”的差异化 → 扩接口、扩矩阵、扩规范 → 用户首次使用仍要自己构建、挑 DB、理解 wire-id → 普通搜索还为重复的 Git 探测白花时间。

**一个号称 evidence-first 的项目，先把自己的竞品事实账记对，再谈“登顶”。**

但也不能为了骂而胡说：当前独立构建的默认 workspace 测试 1758 通过，Clippy、格式检查、Web 交互和可选语义模块测试都通过。源只读、身份/Placement、事务与 generation、过滤分页和输出脱敏确实有实现与测试，不该全部推倒。

## 二、审查方法与可信度

- 主项目当前源码与隔离运行结果是直接证据；历史 benchmark 只作带 commit 的历史锚点。
- 14 个对标目录有 Git 元数据，tracked 产品代码无修改；本地 `.codegraph` 和 memex 的额外 Cargo.lock 不作为产品变更。cc-sessions-viewer 无 Git 元数据，按 package 0.3.25 的目录快照评价。
- 没有运行竞品安装器、模型、native resume 或同语料 benchmark，因此不编造竞品胜出几倍、三平台稳定或“绝对安全”的结论。
- `full` benchmark 指本仓库 harness 的 4000 条合成消息 full profile，不是 50GB/1000万消息压测，更不表示完整项目审计完成。
- 子代理两次尝试均受模型 token 限流影响，留下的清点不是结论；主线程已接管并补上各组实际源码证据。限流只改变顺序，不删除任何项目。
- 覆盖模型（2026-10-06 起）：T1 全文精读要求精确行区间+SHA256 前后校验，回执在 `coverage-<project>*.json`；T2 机械扫描由 `research/sweep-probes.mjs` 对 `coverage-index.json` 全部非二进制文件执行 14 类探针（shell/SQL/路径/监视/吞错/截断/并发/索引/恢复/脱敏/测试/接口等），逐文件命中计数存于 `sweep-<project>.json`；T3 排除项记录原因与哈希。T1 与 T2 均是可复核的静态覆盖，不等于运行时验证。

## 三、15 个项目逐项对照：该学什么，别抄什么

| 项目 / 本地版本点 | 实际形态与核心能力 | 给 ASG 的压力 | 不应照抄 / 口径限制 |
|---|---|---|---|
| agent-sessions / af4793d | Swift/SwiftUI macOS 桌面；FTS、深扫/缓存、会话与 quota/usage 工作台 | 冷启动、标题/sidecar 更新、命中到详情与恢复的连贯性 | 不是 Go CLI；不必为了对齐它而先做 macOS GUI |
| AgentRecall / 3c7bfa5 | Electron + TypeScript + Node SQLite；会话管理、FTS/规则排序、MCP | 会话标题/收藏/项目/下一步的实际可用性 | 不是 Python CLI；桌面和 MCP 排名并非同一函数，query 候选全量 hydrate 有成本 |
| agentsview / a84564a | Go + Svelte；桌面/Web/MCP、FTS、vector generation 与结构化会话 | 层级/状态、索引 readiness、消息定位 | 不是 TS CLI；不能照搬团队/服务全部功能，部分 UI 操作吞错 |
| agf / 44be9cf，0.12.0 | Rust 快速定位/恢复；摘要/目录/分支模糊查找 | 使用门槛低，目标单纯 | 不是 Go；头尾/摘要定位不等价全文证据检索，scanner 失败会转空 |
| cc-sessions-viewer / 无 Git，0.3.25 | Vue/Tauri 会话工作台；并行扫描用户消息，带编辑/fork/恢复 | 跳到正确消息与分支的细节 | 不搜所有 assistant/tool 文本；整个应用不是永远只读；无 LICENSE 文件需授权核验 |
| cc-switch / f748f3a，3.19.2 | Tauri/React/Rust 配置 + 会话管理；7 家 scanner，FlexSearch metadata | 原有工具顺手就能找会话，不能当成完全无竞争 | 不是“无检索”；metadata 搜索不等价全文；renderer 信任边界不可直接搬给 MCP |
| claude-historian-mcp / b627cdf | TypeScript/Node MCP；文件/错误/工具模式等任务工具 | 可发现的 agent 工具语义 | 不是 Python；规则 semanticBoosts 不是神经语义，短 query/错误转空有局限 |
| cass / aa92a45，0.6.22 | Rust CLI/TUI；Tantivy/独立搜索生态、pack 预算/引用/遗漏理由 | ASG 的“无人 pack”主张被实际 CLI 路径反证 | 非普通 MIT；附加条款包括 analysis/benchmark 等，不能直接复用；不少核心在外部依赖 |
| ctx / 06bc5ed1 | Rust SQLite、带 citation/visibility/pagination 的事件检索、SDK/CLI transport | agent 可消费的任务级契约、setup、来源定位 | hosted SDK 明确 placeholder；repo substring 与 ASG slug 语义不同，宣传效率数字未复现 |
| fast-resume / 66e42cf，2.5.0 | Rust + Tantivy；默认索引、刷新、TUI、拼写容错、恢复 | 第一次成功路径与搜索—预览—继续工作 | 某些 API 吞查询错误；索引当缓存可删除，不可照搬到权威 catalog |
| hstry / 88b78b1 | Rust core + TS adapters；双 FTS、批量事务、source/remote/export | code-aware query、source 同步和结构化消息 | 不能凭 UUID v5 判断全部身份；conversation 从 v4 + external 映射起步，跨格式恢复有状态丢失边界 |
| memex / 9a88a5c | Lite 是 Regex 现场扫；完整服务是 FTS/vector/RRF + 多层 compact/RAG | raw/talk/session 层次组织 | 不可将 Lite 与重服务混写成一个能力标签；不要照抄隐藏降级/N+1/功能全家桶 |
| Recall / 22625bf | Rust CLI/TUI、FTS + sqlite-vec、4 目标 handoff | 会话导向的结果和明确下一步 | 自己已区分 handoff 与 native resume；原文直塞 prompt 不等于有严格预算的 evidence pack |
| sessiongrep / c5c1874 | Rust SQLite CLI/TUI/MCP、自动刷新、FTS + fuzzy | 简单的任务入口、timeline、恢复命令 | 预截断后过滤/故障吞掉等不能照抄，近期加分可能引入无文字命中候选 |
| Wake / 71aeca6，0.8.5 | Rust + GPUI，FTS5 trigram、CLI/MCP、source/sidecar/remote | CJK/substr、命中定位、明确 freshness 与降级 | 短词走 LIKE，不证明短中文性能最优；原生桌面/SSH/管理功能不必全追 |

逐项源码与阅读边界见 research 下六份竞品报告；矩阵不是根据 README 功能数量打分。

## 三·B 竞品侧源码级新证据（T1 回执支撑，2026-10-06）

> 口径：以下是**竞品自身的缺陷/教训**，用途只有三个：修正竞争事实表、找出 ASG 可能共病的坑、校准“对手也不是成品”的预期。**不得**用它宣布 ASG 优势——ASG 是否犯同类错误需 ASG 侧独立验证（本轮未做）。

### agf（33/34 文件 T1 全文回执 + TUI 2686/2686 行；receipt: `research/agf-file-coverage.json`、`research/coverage-agf-tui.json`）

| ID | 级别 | 结论 | 关键锚点 |
|---|---|---|---|
| AGF-01 | 高 | 普通扫描（list/stats/resume/默认 TUI 刷新）会以“孤儿清理”名义**删除 Codex 源库 rows**；逐文件读失败被跳过、遍历有错但非空即返回部分集合、缺目录视为可信空集，均可把存在的 row 误判为孤儿删除；删除跨多库无 rollback/无确认 | `src/main.rs:111,170,183`；`src/scanner/codex.rs:25-38,75-95,100-127,156-195,228-234`；`src/cache.rs:299-327` |
| AGF-02 | 高（有前提） | provider 会话 ID 原样插入单引号 shell 模板、无转义；项目路径有转义函数但不覆盖 ID → 命令解析错误/注入风险（需特殊字符 ID 落到缓存或导入） | `src/scanner/pi.rs:104-128`；`src/model.rs:76-87`；`src/shell.rs:64-94` |
| AGF-03 | 中高 | “新鲜”只看监听源 mtime（深度 4 最大秒级），不覆盖实际扫描输入（projects JSONL/.git HEAD/DB WAL）；刷新后写缓存才重采 mtime；`max_sessions` 裁剪发生在写缓存前 → 截断视图被持久化为“新鲜全量” | `src/cache.rs:119-140,201-208,258-269,319-326`；`src/main.rs:223-227,261-265`；`src/plugin.rs:102-123` |
| AGF-04 | 高（竞态） | 异步刷新后，批量删除/勾选仍按旧数组下标解析目标；刷新排序变化后可指向另一场会话；选中身份在刷新之后才捕获 | `src/tui/mod.rs:194-197,471-475,488-511,1681-1684,1759-1769` |
| AGF-05~07 | 中 | provider 删除非原子；Hermes 无 wrapper 的恢复缺口；新鲜度/错误/统计口径需向用户说清 | 见 `research/agf-full-audit.md` |
| 反证 | — | watch 有 AtomicBool 防重叠扫描、刷新后游标夹取；SQL 参数化删除、每库事务 | `src/watch.rs:35-58`；`src/scanner/codex.rs:100-127` |

### sessiongrep（25/26 文件 T1 全文回执 + 1 显式排除；receipt: `research/coverage-sessiongrep.json`）

| ID | 级别 | 结论 | 关键锚点 |
|---|---|---|---|
| SG-01 | P1 | FTS 先全库取 limit×5 候选，再做 provider/path/since 过滤 → 全局截断可把域内合法匹配挤没，过滤后甚至返回 0 条 | `src/db.rs:287-295,395-423,427-477` |
| SG-02 | P2 | 文字分为 0 的候选仍可因近因分/当前 repo 分被接纳为正分“相关”结果（零证据升格） | `src/db.rs:318-381` |
| SG-03 | P2 | 不完整解析/读取失败会被固化为 current；失败身份可能与正常身份分叉或争用 | 见 `research/sessiongrep-full-audit.md` |
| SG-04 | P2 | 单会话写事务可靠，但 full 重建“先 clear 再逐条提交”无原子切换；mtime/size-only 增量、content_hash 恒 null、无源快照比较 | `src/db.rs:144-242`；`src/indexer.rs:28-30` |
| SG-05~09 | P2 | LIKE 字面量语义不一致；Cursor 恢复穷举启发式；清理标记/扁平正文丢消息身份；MCP freshness/预算边界；TUI 状态/分隔符问题 | 见回执报告 |
| SG-10 | P3 | 首用直观，但配置/安装/发布声明缺闭环证据 | `README.md` + CI 配置 |
| 反证 | — | UTF-8 截断修复字符边界、repo 检测覆盖 worktree/submodule、FTS 错误以 Result 返回（非全盘吞错） | `src/util.rs:26-79,225-282,464-497` |

### fast-resume（T1 10+10 文件 / 5,595 行回执；receipt: `research/coverage-fast-resume.json`）

| ID | 级别 | 结论 | 关键锚点 |
|---|---|---|---|
| FR-01 | P1 | search 六个方法用 `unwrap_or_default()/unwrap_or(0)` 把查询错误吞成空结果/0；CLI 与 TUI 初始搜索受影响（TUI 另一路径会显示 "search failed"） | `src/search.rs:27-63`；`src/main.rs:113-119`；`src/tui/state.rs:112,116-141`；反证 `src/tui.rs:204-217` |
| FR-02 | P2 | 全量 rebuild 先 delete_all 再补写、无 last-good；provider 暂时不可读即返回空集 → 窗口期静默缩水（可自愈） | `src/index.rs:83-97`；`src/main.rs:80-96,153-157`；`src/adapters/opencode.rs:109-130` |
| FR-03 | P2 | Codex 正文双源双写（response_item + event_msg）导致 prompt 重复；`turns/user_prompts` 只认 event_msg，为空即整体丢弃 | `src/adapters/codex.rs:85-106,122-124` |
| FR-04 | P2 | TUI 新鲜度只有启动时单次增量：无文件监听（sweep fs_watch=0）、无手动刷新键 | `src/tui.rs:54-81`；`src/tui/input.rs:16-58` |
| FR-05 | P2 | 增量只信 mtime（容差 0.001s）、schema 无 content hash；复制保时间戳场景会漏更新 | `src/adapters/shared.rs:27-36`；`src/index/schema.rs:44-63` |
| 正面 | — | resume/launch 用 argv 数组直执行、无 shell 拼接；last-good 屏障（JSONL 健康分级+不完整扫描零删除）；容错查询解析 | `src/main.rs:214-226`；`src/adapters/shared.rs:38-57,158-185`；`src/index/queries.rs:56-89` |

> 口径：fast-resume 自我定位就是可重建缓存（README:72-78），FR-02/FR-05 在其定位内是边界而非缺陷断言；对 ASG 的意义恰是反例——权威 catalog 不能套用“可删缓存”语义。

### cass · 分片 A（pack/证据捆绑/资产状态；5 文件 / 8,421 行全文回执；receipt: `research/coverage-cass-pack.json`）

| ID | 级别 | 结论 | 关键锚点 |
|---|---|---|---|
| CASS-A-01 | P1 | 证据锚定弱于字面语义：`verified` 只查锚点存在；`message_index` 恒空、`line_end=line_start` → 可回文件+行+conversation_id+哈希，但回不到稳定消息身份 | `src/search/pack_planner.rs:280,282,1927` |
| CASS-A-02 | P2 | 预算只是“选材预算”：仅 evidence 段被强制；`max_output_tokens_with_overflow` 全库无人读取（机械验证）；omitted/metadata/outline 渲染无界 | `pack_planner.rs:766,897-905,1527-1530` |
| CASS-A-03 | P2 | `context_lines` 通过校验但零消费（全文无上下文扩展逻辑，仅回显）→ 调参无行为变化 | `pack_planner.rs:62,72,83,1482` |
| CASS-A-04 | P2 | 契约死条目与失真字段：`SameSessionLowerRank`/`FieldMaskExcluded` 永不产出；硬淘汰候选 `estimated_tokens` 恒 0 | `pack_planner.rs:401,405,824-836,1313-1327` |
| CASS-A-05 | P2 | 资产锁读取静默降级（不可读锁 → Default → Idle → Launch）；fail-open 仅在 lexical 可用时成立；单飞互斥依赖下游 flock（本文件无证明） | `asset_state.rs:223,310,1785,1815,1808-1855` |
| CASS-A-06 | P3 | 事件日志非原子截断；脱敏区间恒为整串 | `asset_state.rs:1977-1996,1777-1792` |
| 正面 | — | 成文 `cass.pack.v1` schema+limits/validate；七级确定性决胜链+测试；漏因账本；三态新鲜度+4×窗口衰减；health/warnings/recommended_action | `pack_planner.rs:58-87,1463,1264-1293,3413-3456,384-406,1146-1168,1556-1656` |

> 双重含义：① 竞争事实表必须承认 cass 已有 pack 级契约（“无人做到”措辞不成立）；② cass 的 pack 本身也是半成品——消息身份锚定缺失、预算/契约存在哑火位，ASG 的 Stable Message/placement/cursor generation 正是它没有的。**两边都不该吹。**

### claude-historian-mcp（41/46 文件 full，12/12 源码 8,406 行；receipt: `research/coverage-claude-historian.json`）

| ID | 级别 | 结论 | 关键锚点 |
|---|---|---|---|
| CH-01 | 中 | 查询词未转义进 RegExp：`c++`、`foo[` 等在 config/memories/plans 三处抛 SyntaxError 被 catch 吞掉 → 静默全空；其它 scope 用 includes 不受影响，同查询跨 scope 行为不一致 | `search.ts:2099,2137,2240` |
| CH-02 | 中 | 项目路径解码有损：`replace(/-/g,'/')` 把含连字符项目解错；`search_config` 对 `.claude` 目录探测全错；代码注释自认 lossy | `utils.ts:199-202`；`parser.ts:124`；`search.ts:1597,2247,233-234` |
| CH-03 | 中 | 无任何自动化行为测试：`npm test`=typecheck+lint；PERFORMANCE.md 的 benchmark 是人工命令 → 回归无护栏 | `package.json:30`；`test.yml:18-22`；`PERFORMANCE.md:1220-1321` |
| CH-04 | 中 | 迁移桩污染 tools/list：10 个旧工具桩 + search/inspect = 12 个工具，而自带 doctor 断言 `tools.length===2` → 自检必失败；README 仍称 "Two tools" | `index.ts:75-88,656-664`；`README.md:94` |
| CH-05 | 中 | MCP 契约漂移：`detail_level` 声明但被忽略；`message_count` 实为默认 10 的截断窗口，README 示例不可达 | `index.ts:166-170,437-463`；`formatter.ts:935-959`；`universal-engine.ts:311,327`；`README.md:180` |
| CH-06 | 低 | release.yml 手动触发 + `if: push` 永不执行（空转）；.dxt 实为 ZIP 而脚本产 tar.gz；icon.png 实为 JPEG | 见 `research/claude-historian-full-audit.md` F11-F14 |

### memex（9/9 T1 文件 full、7,133 行；receipt: `research/coverage-memex.json`）

| ID | 级别 | 结论 | 关键锚点 |
|---|---|---|---|
| MX-01 | P2 | 注入丢锚：只有 8 字符会话前缀+层标签，无 message id/时间戳/生成标注；L0 原文与 L1-L3 摘要同栏混排；消息静默截 500 字符。反证：底层有 `at`→`around` 回原文链路，缺的是接进注入格式 | `inject/mod.rs:670-676,802-806,845-852`；`mcp/mod.rs:2050-2134` |
| MX-02 | P2 | 时间排序静默换引擎：`order_by≠Score` 强制 FTS-only 丢弃向量结果，仅 info 日志，响应无 effective_mode | `search/mod.rs:287-296,366-394` |
| MX-03 | P2 | 检索失败与零命中不可区分：FTS/向量出错仅 warn 后继续，皆空即 `Ok([])` | `search/mod.rs:335-364`；`vector/mod.rs:215-218`；对照 `mcp/mod.rs:801-813` |
| MX-04 | P2 | 回退后 level 不更新：L3→L2→L0 逐级回退但响应 `level` 仍写请求层 | `mcp/mod.rs:571-575,712-730,772-777` |
| MX-05..11 | P3 | 预算硬 break+chars/4 估算；time_decay 无效/Dot 复用 cosine；注释工具数失真；Lite mtime 过滤；"只读"封装含写方法；DTO 硬编码 source=claude；`source=local` 本地不足时仍补查 sync server（隐私张力） | 见 `research/audit-memex.md` §4 |
| 能力分离 | — | Lite=无索引现场 regex grep 本地 CLI；完整服务=FTS+LanceDB+RRF+LLM compact+inject+HTTP/MCP+remote。"Rust CLI hybrid"单标签作废 | — |
| 正面 | — | 多层记忆落地；RRF 保留 sources/fts_rank/vector_distance 归因；搜索命中→原文 at 锚闭环（有测试） | `mcp/mod.rs:2050-2134` |

### hstry（8 必读文件 7 full + main.rs 86.8%；合计 15,316 行阅读；receipt: `research/coverage-hstry.json`）

| ID | 级别 | 结论 | 关键锚点 |
|---|---|---|---|
| H-01 | P1 | 长消息正文投影截断（每 part 500 字符/累计 4000/>2000 字触发）且 **FTS 索引的正是截断后 content**，show/导出同读；parts_json 保留全文 | `crates/hstry-core/src/db.rs:3322-3354,1468-1470`；`migrations/001:98-104`；`main.rs:2589-2593,4410-4416` |
| H-02 | P2 | `resume --json` 谎报 `launched:true`（json 分支直接返回，spawn 在其外） | `main.rs:5409-5431,5095-5097,5203-5205` |
| H-03 | P2 | adapter 抛错信息丢失（TS 写 stdout 后 exit(1)；Rust 只读 stderr） | `adapters/types/index.ts:375-378`；`runner.rs:329-332` |
| H-04 | P2 | `external_id` NULL 时每次同步重复建会话（UNIQUE 对 NULL 不去重） | `migrations/001:27`；`db.rs:577-598`；`ingest.rs:63-93` |
| H-05 | P2 | 后过滤在 limit×4 截断之后；但单角色/source/时间已在 SQL 内先于 LIMIT（优于 SG-01） | `main.rs:2003,2064-2097,2139-2141`；`db.rs:2135-2167` |
| H-06/07 | P2 | 磁盘 migrations 目录遮蔽内嵌集合；web sync 仅 ChatGPT 实现 | `db.rs:128-150`；`web-runner.ts:73-91` |
| 正面 | — | 16 adapters 含 ASG 没有的来源（ChatGPT/Claude.ai/Gemini 导出、Jan/LM Studio/Open WebUI/Goose）；SQL 先过滤再 cap；batch 单事务+单写者；UUID v5 幂等；verify/reseed；peek 预算 | 见回执 |

> 广度压力点：hstry 用“一协议 × 16 目录”换来源覆盖——ASG 的 14 家矩阵在“导入历史（ChatGPT/Claude.ai/Gemini 导出文件）”这类来源上确实缺席，这是真实竞争压力，但不应用追 adapter 数量的方式回应。

### Recall（35/37 full + 2 partial、13,244 行；receipt: `research/coverage-Recall.json`）

| ID | 级别 | 结论 | 关键锚点 |
|---|---|---|---|
| R-01 | P1 | handoff 把全量 transcript 直塞 argv、无预算：无 token/byte 上限、无证据标记；超长会话触 OS 参数上限 | `handoff.rs:20-25`；`transcript.rs:3-28`；`session.rs:651-660`；`session_action.rs:40-54` |
| R-02 | P2 | sqlite-vec 的 k 截断先于过滤（CLI k=300 全局 KNN 后 JOIN 过滤；FTS 反而是前置过滤）→ 强过滤下向量召回漏失且不报错 | `search.rs:158-175` vs `:133` |
| R-03 | P2 | CJK 无分词：unicode61 使连续中文成为整串 token，查询按空白切词 OR 连接，无 bigram/trigram/fuzzy | `schema.rs:80-85`；`search.rs:295-308` |
| R-04 | P2 | 无孤儿对账：prune 默认空 → 源文件删除后索引永久陈旧 | `sync.rs:196-246`；`adapters/mod.rs:40-42` |
| R-05 | P2 | 嵌入失败无重试/崩溃恢复；processing 中途崩溃无接管；TUI 单次失败即进程内永久降级 | `semantic_store.rs:73-95`；`semantic.rs:65-82`；`search_worker.rs:121-136` |
| R-06 | P2 | RRF 等分无 tiebreak + offset 分页 | `search.rs:291`；`session.rs:353-359` |
| 正面 | — | 11 家 provider；任务导向动作+确认流；后台嵌入队列状态机；内置评测 harness（Hit@5/10+MRR）；单一 SearchEngine；imported 禁 resume/open 但允许 handoff 的边界 | `bench.rs:305-360`；`session.rs:608-610`；`app.rs:3372-3390` |

### cass · 分片 B（检索管线；query.rs partial 6,525/20,986 + 3 文件全文；receipt: `research/coverage-cass-query.json`）

| ID | 级别 | 结论 | 关键锚点 |
|---|---|---|---|
| CQ-1 | P1（有界） | 过滤 vs cap：Tantivy 主路径过滤先于 cap；但 **SQLite 降级 cap 先于过滤**（1024/批、≤30k 补偿），message-scan 更是 `LIMIT 30000` 前缀截断再过滤，“过滤后非空但不足 limit”不重扫 → SG-01 同族、被 30k 窗口有界 | `query.rs:6201-6218`；`6605-6628,6710-6747,6945-6962,7442-7478` |
| CQ-2 | P2 | 零证据升格变体：无 boost（grep 0 命中），但语义 top-k 无相似度门槛 + RRF“名次即正分” → 低语义证据可入混合结果；文字路径必须有 MATCH 证据 | `query.rs:4082-4096,4243-4401,1810-1867,1995,2018-2025` |
| CQ-3 | P2 | 模式降级：默认 Hybrid fail-open→lexical（realized mode+reason 进 robot 元数据）；`--mode semantic` fail-closed（code 15）；tier 降级仅 debug 日志 | `lib.rs:22726-22732,25211-25219,22860-22868`；`query.rs:4311-4314` |
| CQ-4 | P2/信息 | 排序=BM25+RRF（联邦 k=60；混合走外部依赖 frankensearch 默认配置）；无 recency/path boost；过滤后才分页且有跨页测试；cursor 编解码未审 | `query.rs:1598-1610,1861-1867,5591-5606,10337-10412` |
| CQ-5 | P3 | 文档-行为漂移：`MatchType/quality_factor` 声称用于排序、实际不参与打分；wildcard 回退不降权 | `query.rs:1117-1148,5714-5718` |
| 正面 | — | 过滤 pushdown；降级分批扫描补偿；去重/过滤后分页+跨页测试；确定性 RRF tie-break；provenance/line_number/content_hash；fail-open 元数据化；manifest 原子发布+保守恢复+cleanup 指纹审批 | `lexical_generation.rs` 等 |

> 关键警告：cass 的核心排序/过滤语义（fs_rrf_fuse / fs_cass_build_tantivy_query / fs_candidate_count）在 **外部 git 依赖 frankensearch**（rev f7fa7a02，本机无 checkout）——可验证性打折。契约在内、排序在别人手里，这也定义了它“pack 已完成”说法的边界。

### agentsview · 分片 A（5 文件 12,138/12,544 行=96.8%；receipt: `research/coverage-agentsview-core.json`）

| ID | 级别 | 结论 | 关键锚点 |
|---|---|---|---|
| AGV-01 | P2 | semantic/hybrid 候选 k 先于元数据过滤（取回后才按 allowed-session/Scope 过滤；hybrid FTS 腿 4×k 批上限自认 under-fill）→ SG-01 有界变体；词法模式过滤在 LIMIT 前（正面对照） | `search_content.go:716-720,801-821,1043-1067` |
| AGV-02 | P2 | Recall 每腿 500 截断先于 Rank；FTS→LIKE 静默回退，API 无 engine/degraded 标志 | `recall.go:995-1007,612-636,855-866,1373-1385` |
| AGV-03 | P2 | 侧栏索引无分页路径整表物化（仅 Limit>0/Cursor/Starred 才分页） | `sessions.go:711-795` |
| AGV-04 | P2 | 旧 Search OFFSET 深分页（每页重跑 FTS+name UNION）；会话列表已是 HMAC keyset | `search.go:342-546`；对照 `sessions.go:401-464` |
| AGV-05 | P3 | FTS 缺失失败语义三态不一致（静默容忍/显式 errFTSUnavailable/静默 LIKE）；判定靠错误串匹配 | `db.go:3342-3347`；`search_content.go:617-634`；`recall.go:1380-1385` |
| AGV-06..08 | P3 | sync_marker 坏 created_at 盲窗；MAX(id)+1 依赖单写者；跳过缓存全表重写 | `db.go:2260-2270`；`messages.go:762-771`；`skipped.go:32-72` |
| 正面 | — | user_version+行级 data_version“过期不盖章”；删除后写排除+幽灵防护；keyset+HMAC 游标与 sort 误配拒绝；FTS 触发器批量删重协议；删除日志 tombstone；失败分类学 | `sessions.go:2341-2536,1221-1268,401-464`；`messages.go:1277-1313` |

### cc-switch · 分片 A（会话管理/检索/用量；20 文件 11,675 行，T1 主文件 10/10 full；receipt: `research/coverage-cc-switch-sessions.json`）

| ID | 级别 | 结论 | 关键锚点 |
|---|---|---|---|
| CS-1 | P1（事实表） | **“无检索”被推翻**：存在 FlexSearch 元数据检索（字段 sessionId/title/summary/projectDir/sourcePath，tokenize=full，内存态不含正文） | `src/hooks/useSessionSearch.ts:27-48`；`SessionManagerPage.tsx:229-237,997-1017`；`SessionItem.tsx:88-90` |
| CS-2 | P2 | 检索是“会话说”不是证据级：结果只能打开会话；正文命中词需打开后高亮、不自动跳转命中；TOC 只跳 user 消息 | `SessionMessageItem.tsx:39-48,89-93`；`SessionManagerPage.tsx:366-391` |
| CS-3 | P2 | resume：5/7 家可构造（codex/claude/opencode/gemini/grok），实际拉起仅 macOS；其余复制命令；cwd 有单引号转义 | `codex.rs:414`；`claude.rs:251`；`opencode.rs:476`；`gemini.rs:172`；`grok.rs:192`；`SessionManagerPage.tsx:418-427`；`terminal/mod.rs:328-338` |
| CS-4 | P2 | 命令字符串直通 shell 已在源码记录为接受风险 | `commands/session_manager.rs:27-60` |
| CS-5 | P2/P3 | 静默截断/一致性：Hermes LIMIT 500、JSONL 浅扫、messages 固定列、JSONL 删除不校验 ID；Codex 递归无深度上限 | `hermes.rs:84,291-307,488-496`；`codex.rs:504-522` |
| CS-6 | P2 | 无持久索引、无 watcher，list_sessions 每次 7 线程全量重扫；UI 30s staleTime。加分：用量子系统游标+去重+重放防护+批事务 | `mod.rs:58-94`；`commands/session_manager.rs:6-11`；`queries.ts:307-324`；`session_usage_codex.rs:1093-1125,1295-1346` |

> 事实表修正（B0 直接输入）：L30/L49 的“无检索/不可比”应改为“元数据检索（FlexSearch 内存索引：无正文/无持久化/无 source span）+ 7 家会话管理与 resume 命令生成”；“不可比”限定为“证据级检索不可比”。

### Wake（13 文件 11,388 行；核心 6 件全 full + db.rs 选区 31.6%；receipt: `research/coverage-Wake.json`）

| ID | 级别 | 结论 | 关键锚点 |
|---|---|---|---|
| WK-01 | P2 | 任意 <3 码点词项使整条查询退化为全表 LIKE（含常见 CJK 双字）；degraded 路径丢 bm25 | `db.rs:3259-3262,2618-2688,2250-2258,1121-1122`；反证 `mcp/tools.rs:658-681` |
| WK-02 | P2 | ≥3 码点 trigram 零命中无 LIKE/提示兜底；fts_reindex 旗一轮后无条件清 → 重索引失败窗口静默空结果 | `db.rs:2565-2571,2618-2653`；`scanner.rs:328-333,625-630` |
| WK-03 | P2 | 新鲜度仅 mtime+size，无内容哈希（sidecar 有独立通道） | `scanner.rs:461-464,485-488,447-451` |
| WK-04 | P2 | 解析/写库失败仅 eprintln，MCP/CLI 无健康度面；index_note 只报新鲜度 | `scanner.rs:558,562,594-597,1153`；`mcp/tools.rs:471-480` |
| WK-05 | P3 | 消息身份只有 seq、索引文本 32KiB 截断；无原生消息 id/父边/字节锚 | `models.rs:324-340,372-380,712-720` |
| 正面 | — | 过滤在 FTS SQL 内 LIMIT 前完成；`None≠Some(空)` 冻结纪律；写事务内副本裁决+last-good；删除三段式（无链接检查→revalidate→sha256 journal→trash→内容证据恢复+墓碑）；watcher 溢出 rescan；CLI↔MCP 双射契约测试 | `db.rs:2583-2598`；`scanner.rs:389-423,546-605,642-755`；`cleanup.rs:291-545,810-1028`；`watcher.rs:114-200`；`cli.rs:959-1012` |

> Wake 是目前竞品中“防错纪律”最接近 ASG 方向的一家（过滤前置、None 语义、last-good、删除内容证据）；弱点在短词全表 LIKE 与 seq-only 身份。

### ctx · 分片 A（capture/store/search packet；25 文件 11,333 行，7 个 T1 必读全 full；receipt: `research/coverage-ctx-core.json`）

| ID | 级别 | 结论 | 关键锚点 |
|---|---|---|---|
| CTX-01 | P2 | pagination 只写不读且三形状分裂：packet 生成 `offset:{n}`，但 query/main/MCP 均无 cursor/offset 输入；契约 fixture `{limit}` 与 JVM(limit/offset/nextCursor/hasMore) 对不上；>200 命中无取回路径 | `packet.rs:114-123`；`query.rs:24-30`；`main.rs:317-434`；`search.results.json:85-87`；`SearchPagination.java:18-55` |
| CTX-02 | P2 | 全量索引重建无事务快照：裸 DELETE+逐表重建、无外层事务；自愈只在投影计数为 0 时触发 → 半重建可持久、缺表时搜索静默空 | `projections.rs:580-660,757-799,134-136`；`import.rs:830`；`import/native.rs:66,91` |
| CTX-03 | P2 | citation 仅事件粒度：有 id/seq/cursor/raw path/exists，但无 span/内容哈希；snippet 来自索引期裁到 2048 字符的预览；完整 payload 不参与检索 | `dtos.rs:742-760`；`projections.rs:1639-1666`；`results.rs:284`；`events.rs:526` |
| CTX-04 | P2/P3 | withheld 三层不一致：枚举+过滤谓词存在，但 DDL CHECK 不含 withheld → 写不进；packet visibility 恒为 LocalOnly | `sync.rs:9-18`；`projections.rs:1600-1622,1005-1020`；`ddl.rs:371-373`；`results.rs:195,304` |
| CTX-05 | P3 | 批量半写：64 单位/8MiB 轮转提交，错误只回滚当前批；无“已提交到哪”的回执 | `batches.rs:341-387,199-202,180-188,235-245` |
| 正面 | — | citation 字段骨架（事件/会话双引用+外部 id+cursor+源存在性）；truncation reason 机器可读；indexed/last_imported 双水位与 pending SQL；错误分类；显式 retention 元数据 | 见回执 |

> 对 ASG handoff-pack 的底线（来自 ctx 教训）：证据必须带 span+强哈希并从权威 payload 渲染；cursor 必须有消费者和“翻页无重无漏”验收；有损归一化必须落 omission 元数据。

### AgentRecall · 分片 A（9/9 full、10,114 行；receipt: `research/coverage-AgentRecall-core.json`）

| ID | 级别 | 结论 | 关键锚点 |
|---|---|---|---|
| AR-01 | P2 | MCP 与桌面检索**确为两套实现、两套排序**：MCP 独立 raw SQL `ORDER BY rank`（自认 reimplemented）；桌面为 FTS 闸门+JS smartScore；过滤/title 优先级/source 映射均不同 | `bin/agent-recall-mcp.mjs:96-107,178-181`；`sessions.ts:1058-1093,1729-1737,1422-1425,1440-1447,1661` |
| AR-02 | P2 | 桌面 query 路径全量 hydrate + JS 全量排序后截断（候选无 LIMIT） | `sessions.ts:1407-1409,1079-1086` |
| AR-03 | P2 | 元数据链路：标题经 IPC+FTS 刷新；收藏/置顶/隐藏纯 UPDATE；摘要以 `file_mtime_ms` 版本化 stale；**无“下一步”字段**（=resume 记 `last_resumed_at`） | `sessions.ts:461-477,890-917,1698,668-670` |
| AR-04 | 信息 | 存储真实实现：FTS5 **trigram** 表（unicode61→trigram 重建）、索引含全量消息+摘要 → 内容级全文检索；session_fts 非 contentless（正文双份存储） | `schema.ts:184-191,304-322`；`sessions.ts:1132-1150` |
| AR-05 | 信息 | 远程边界：远程与本地同库（environment_id），未 hydrate 时桌面直读 SSH；**MCP 无远程逻辑** → 未 hydrate 会话在 MCP 只见 0 条本地消息（推演，未运行验证） | `main/index.ts:463-486,1206-1218` |
| 形态修正 | B0 | “Python CLI”错误 → TypeScript/Electron/React 桌面 + node:sqlite（Node ≥22.13）+ 独立 Node stdio MCP server；全仓 0 个 .py（Python 仅出现在可选运行时助手） | `package.json`；`main/index.ts:1559-1562`；`session-loader.ts:24-25` |

### cc-sessions-viewer · 分片 A（T1 9,252/9,812 行；receipt: `research/coverage-cc-sessions-viewer-core.json`）

| ID | 级别 | 结论 | 关键锚点 |
|---|---|---|---|
| CCV-01 | P2 安全 | 前端 `extra_args` 未转义直拼 shell（program/args 有引号、extra_args 原样 append）；PTY/外部终端 resume/new 透传；session id 有白名单而 extra_args 无校验。反证：值来自本地前端设置 launchArgs | `agent_command.rs:52-54,73-75`；`lib.rs:845-885,1225-1265,859-865`；`App.vue:3277,3352` |
| CCV-02 | P2 可用性 | `match_snippet` 用小写串算 char 下标却切原串 → İ 类膨胀字符可 panic；被 spawn_blocking 接住转 Err，整个搜索请求失败 | `agents/mod.rs:1089-1097`；`lib.rs:687-688` |
| CCV-03 | P2 状态 | `send()` 先记用户消息+置 turn started 再 spawn/写 stdin；早退不清 started → 幽灵消息+重连永显 running | `agent_chat.rs:3256-3299,3446-3461,3471-3492,4058-4069` |
| CCV-04 | P2 合规 | README 声明 MIT 并链接 LICENSE，但本快照无 LICENSE/COPYING/NOTICE；package.json/Cargo.toml 无 license 字段（只限快照断言） | `README.md:10,186-188` |
| CCV-05 | P3 资源 | USER_TEXT_CACHE/USAGE_CACHE 无界无淘汰、命中整段 clone；冷路径整文件读入 | `agents/mod.rs:39-80,637-671,1056-1085` |
| 检索语义 | 信息 | 真·全量扫描（`list_sessions(0,usize::MAX)`、rayon 4 线程）+ (path,mtime) 缓存；硬约束只匹配 `role=="user"` 的 text 块；代际取消+200 条上限；keyword scope 先匹配标题 | `agents/mod.rs:988-1034` |
| 正面 | — | 命中自带 msg_index/uuid；Pi 返回 pi_leaf_id 并按 terminal lineage 搜索；写租约防并发 resume；能力缺失 fail-closed（搜索/统计 fail-open 无 partial 标记） | `agents/mod.rs:895-902,956-986` |

> ASG 可学：leaf/placement 坐标随命中返回、双层检索+mtime 缓存、取消代际与上限、写租约。不该学：index-only 跳转、provider 特例分支、无界缓存、extra_args 裸拼、无 freshness 的静默 partial。
> 快照口径说明：本仓库无 Git 元数据，coverage-index 记录树哈希 `0ee8993c…`；其引用的 src/settings.ts、README.ja.md 在快照中缺失，“无 LICENSE”类结论仅限该快照。

### cass · 分片 C（storage/indexer；sqlite.rs 6,610/30,047 + indexer/mod.rs 4,762/52,111 + semantic.rs 全 6,229；receipt: `research/coverage-cass-storage.json`）

| ID | 级别 | 结论 | 关键锚点 |
|---|---|---|---|
| CS-1 | P2 | “内容指纹”实为 `content-v1:{会话数}:{max会话ID}:{max消息ID}`（基数+ID）→ 保量改写不使 checkpoint 失效，可能跳过重建/续传错位 | `indexer/mod.rs:8695-8747,2801-2808,14093-14128`；`semantic.rs:3156-3161,4920-4983` |
| CS-2 | P2 | begin-concurrent 写者 `foreign_keys=OFF`（失败仅 debug）+ 孤儿清理 preflight 默认跳过（代码注释自认断连留孤儿） | `indexer/mod.rs:26128-26135,25978-25984,13459-13501`；`sqlite.rs:4639-4656,5923-6222` |
| CS-3 | P2 | in-DB FTS 影子写入 best-effort（错误吞掉仅 warn）；routine preflight 校验默认关闭 | `sqlite.rs:14102-14236,11127-11167`；`indexer/mod.rs:13342-13419,14842-14911` |
| CS-4 | P3 | 批量消息插入用 `last_rowid-(n-1)` 反推 message id，snippets/FTS 依赖推断 id；rowid 非连续会错绑 | `sqlite.rs:13577-13586,13713-13722,10031-10050` |
| CS-5 | P3 | `forget_conversations_by_source_glob` 拼 SQL（无绑定无上限）；`delete_source` 的 cascade 参数被忽略 | `sqlite.rs:7723-7764,9833-9844` |
| 正面 | — | staged swap+校验后才发布；“无法证明可恢复就拒绝原地重建”（FTS Unqueryable/Excess/Divergent fail-closed）；全局水位只在扫描成功且无排除时推进；OOM 二分删除+staging 残骸回收；重试白名单+串行 fallback；full rebuild 不 eager 删除 | 见回执 |

### agent-sessions · 分片 A（T1 9,721/9,721 行 = 100%；receipt: `research/coverage-agent-sessions-core.json`）

| ID | 级别 | 结论 | 关键锚点 |
|---|---|---|---|
| AF-01 | P2 | 解析失败=成功且推进水位：Codex 读失败产出 1 个 error 事件仍算成功并可覆盖旧事件；Claude nil 被静默丢弃但水位照常前进 → 失败不再重试、无错误面（SG-03 教训的活例） | `SessionIndexer.swift:2056-2060,910-947,1308-1324`；`ClaudeSessionParser.swift:115-120`；`SessionIndexingEngine.swift:143-165`；`ClaudeSessionIndexer.swift:412` |
| AF-02 | P2 | 重索引窗口该 source 搜索/列表静默归零（guardrail 注释自认 silent zero；实测 5GB/3363 会话 ~149s）；保语料原语仍清 meta；两个历史标记整体清 FTS 语料 | `DB.swift:388-396,1334-1383,406-427` |
| AF-03 | P3 | sideChatsOnly 自由文本可被 FTS 截断饿死（限额提升只覆盖 repo/archived；FTS 先全库 bm25 序 LIMIT 2000） | `SearchCoordinator.swift:735-742`；`DB.swift:1901-1909` |
| AF-04 | P3 | FTS 降级无健康面（DDL 失败空 catch；查询 `try? … ?? []` 静默走 legacy） | `DB.swift:325-327`；`SearchCoordinator.swift:319-329` |
| AF-05~07 | P3 | 结果序无统一契约（bm25→toolIO→后台扫描→legacy 分段拼接）；删除/重算无事务；ID 冲突键不对称 | `SearchCoordinator.swift:333-374,483-527,793-973`；见回执 |
| 正面 | — | meta-only 水合→先发布→增量+gap 差集重扫→≥8MB tail-first 预绘；mtime+size+format_version 三元组“当前性”资格；stale 绕体积门槛；COALESCE 保留；rollups 最大余数法守恒+meta_mtime 增量 | `SessionIndexer.swift:743-848,1326-1334,446-501`；`DB.swift:1791-1848,1633-1649,2318-2387` |

### cc-switch · 分片 B（proxy/transform；11,094/20,747 行=53.5%；receipt: `research/coverage-cc-switch-proxy.json`）

| ID | 级别 | 结论 | 关键锚点 |
|---|---|---|---|
| CSW-B1 | P2 | 主代理端点无入站鉴权：健康/状态端点裸奔，唯一 Bearer 校验仅 Claude Desktop 网关；/v1/messages、/v1/responses、/v1beta/* 未校验；监听地址可配 0.0.0.0（反证：默认回环，server.rs 仅 grep 级） | `handlers.rs:57-71,129-149,261-286`；`server.rs:100-117`；`services/proxy.rs:1484-1490` |
| CSW-B2 | P2 | Token 同步“写库失败仅 warn 继续”（注释要求 fail-fast，实际 DB 写失败仅 warn 且外层仍 Ok）→ Live 已占位、DB 陈旧 token 双端失联风险（反证：Live 备份可恢复） | `services/proxy.rs:620-627,1068-1078,1129-1137,1181-1191,1245-1268` |
| CSW-B3 | 正面 | failover-safe 流式提交协议：非流式全量缓冲、流式首包预热、Responses 语义起始校验、2xx 错误包络参与 failover；提交后中途失败有意不换家 | `forwarder.rs:2285-2312,2341-2373,2433-2513,3090-3093` |
| CSW-B4 | 正面+边界 | 日志脱敏与占位符不变量成体系（请求体只记 bytes+hash、URL 双模脱敏、错误摘要最小化、PROXY_MANAGED 拒绝出站）；边界：DB/Live 明文存 token，加密未验证 | `forwarder.rs:2195-2199,2177-2186,3520-3521`；`services/proxy.rs:1068-1071,1392-1397` |
| CSW-B5 | P3 | 静默降级+超时零值语义：models 解析失败无日志空表；Copilot debug-only；non_streaming_timeout=0 无显式超时且不缓冲 | `handlers.rs:95-101,517-524`；`forwarder.rs:2600-2605,2237-2239,2354-2356` |
| 关系 | — | 代理只会话身份与归因（session_id 提取、用量入库、工具缓存），不索引正文、无检索/恢复面；真实压力=流量咽喉的用量/成本视图+账号池+日常入口；无压力=检索质量/召回/语义/CLI-MCP 深度 | `forwarder.rs:1255-1301,1328-1339`；`handlers.rs:2575-2660` |

### agent-sessions · 分片 B（实读 8,598/17,675 行；receipt: `research/coverage-agent-sessions-resume.json`）

| ID | 级别 | 结论 | 关键锚点 |
|---|---|---|---|
| RS-01 | P2 | 恢复执行零反馈：七处 `_ = await …resumeInTerminal` 丢弃结果；canResume 预检+焦点 alert 存在，但运行时失败无出口 | `UnifiedSessionsView.swift:3152-3154,3171,3189,3207,3225,3244,3262,3280,1162-1167,3040-3058` |
| RS-02 | P2 | 剪贴板恢复命令是 shell 字符串（`cd <q wd> && <q binary> --resume <q sid>`），转义全押在各自 shellQuoteIfNeeded（本体未读）→ 条件性结论 | `UnifiedSessionsView.swift:1527-1633` |
| RS-03 | P2 | /status 探测 override 绕过敏 opt-in 与可见性（双窗口缺失只保留 30 分钟下限）；文件头 10min 冷却与代码 4h 不一致 | `CodexStatusService.swift:2560-2577,30,1923` |
| RS-04 | P2 | live 状态启发式且“探不到=空闲”（词法 marker+mtime 2.5/15/30s+尾探失败→openIdle）；presences 无 freshness 字段 | `CodexActiveSessionsModel.swift:1874-2008,2158-2170,2341-2352,2200-2203,1548-1559` |
| RS-05 | P3 | 两处无超时 waitForExit（osascript 主线程手势路径；登录 shell PATH 解析） | `CodexActiveSessionsModel.swift:1809-1823`；`CodexStatusService.swift:3835-3868` |
| 正面+口径 | — | 恢复=外部终端 App（iTerm2/Warp/Terminal）非嵌入式 PTY；身份=String id+cachedRowByID O(1)；命中→详情三级解析、显式拒绝 cwd-only 猜测；settled 选择+锁步跳转；尾窗先画+稳定门控 | `UnifiedSessionsView.swift:361-366,2235-2269,2348-2379,2742-2753`；`SessionTerminalView.swift:997-1090` |

> 口径修正：SessionTerminalView.swift 实为转写渲染器、不是启动/恢复链路（分片 B 如实纠正）；真实 launcher 在 AgentSessions/Resume/*（未读，列为残余）。

### agentsview · 分片 B（sync/parser；T1 9,804 行，codex/claude full + engine partial；receipt: `research/coverage-agentsview-sync.json`）

| ID | 级别 | 结论 | 关键锚点 |
|---|---|---|---|
| AGV-B1 | P1 正面 | 全量扫描不会因“源文件未发现”删档：主循环只处理发现集、无 presence 清扫；presence 漂移只在只读 parse-diff 报告；重扫用 orphan copy 保档 | `engine.go:2465-2757,213-235,1753-1782`；`parsediff.go:917-967` |
| AGV-B2 | P2 | 显式删除例外族：容器/成员消失 → SkipNoSession+ForceReplace → DeleteParserExcludedSessions 真删；同路径 ID 换代有 stale-row cleanup；护栏=可达性检查/cwd 冻结/resurrection guard | `engine.go:4461-4469,4685-4701,4016-4028,4879-4982,2997-3015,4935-4969`；`cwd_filter.go:74-96` |
| AGV-B3 | P2 | 失败分类=noCacheSkip（瞬时错误不落负缓存）+ 按结果 data_version 降版重试；弱点：无退避/重试计数/告警面，重试依赖下一次同步 | `engine.go:4227-4232,4660-4682,4722-4728,6847-6858` |
| AGV-B4 | P2 | resync 换库守卫充分（取消/空发现/synced=0/failed>ok 拒绝 swap），但 rename 成功后 reopen 失败不回滚（降级非丢数据） | `engine.go:1195-1229,1618-1641,1906-1949` |
| AGV-B5 | P3 | parser 保真边界：codex fork gate fail-open；claude DAG 分支阈值启发式；半解析处置到位（截断识别、完整行才推进 offset）；provider 注册表 53 case+2 import-only | `codex.go:97-145,1769-1851,1951-1963`；`claude.go:997-1103,269-276`；`provider.go:402-521` |

> agentsview 的“非破坏重扫 + 显式删除契约”是竞品中最接近 ASG 纪律的实现之一；它与 sessiongrep/Recall 形成对照：同族产品里“扫描删数据”“失败固化为 current”各有踩坑者。

### cass · 分片 D（lib.rs 脊柱 + sources；lib partial 2,699/100,119 + sync/config full；receipt: `research/coverage-cass-lib.json`）

| ID | 级别 | 结论 | 关键锚点 |
|---|---|---|---|
| D-01 | P2 | pack readiness 注入缩水：字面常量——lexical 恒 Ready、source_sync_gaps 恒空、recommended_action 恒 None → health 块在 CLI 路径失去信息量 | `lib.rs:23848-23867`；对照 `pack_planner.rs:561-573` |
| D-02 | P2 | cursor 无 generation 绑定/无过期（base64{offset,limit}；decode 静默合并；manifest 暴露 generation 但不校验）→ 对照 ASG 的 cursor_invalid/expired/generation_mismatch 缺一档 | `lib.rs:25308-25318,22542-22570,25349-25361,25430-25441` |
| D-03 | P2 | sync 软失败：全部路径失败仍 `Ok(SyncReport)`；additive-only 镜像不反映远端删除。反证：scheduler 转 Flapping/BackingOff，风险在调用方/退出码 | `sync.rs:1019-1104,798-813,6-11,1185`；`lib.rs:2882-2894` |
| D-04 | P2 | robot `_meta` 多分支平行手写（JSON/JSONL/compact/toon/sessions 各自拼装），schema 有但分支等价性未证 | `lib.rs:26117-26149,26250,26413,26504,26547,26637,82009-82059` |
| D-05 | P3 | 100K 行单文件脊柱（Cli 1,311 行；单函数 run_doctor_impl 2,834 行；json! 宏体 9,754 行）；19% 为内联测试（33 模块/471 个 #[test]） | `lib.rs:226-1536,73428-76261,25553-26679` |
| 遗留答复 | — | **pack CLI 确实注入 limits**（dispatch→PackPlannerLimits+validate→candidate_fetch_limit→PackPlanRequest）；默认 12000/8/24/3/1600 与 planner 契约一致；遗留只在 readiness（D-01） | `lib.rs:6762-6803,23629-23636,23750-23754,23811-23819` |
| 正面 | — | 同步“可解释账本”（SyncTransportDecision+failure reason+scheduler）；config 写前校验+round-trip+原子替换+备份；44 命令/65 robot 别名结构化面；cursor manifest 机器可读 continuation 理由 | `sync.rs:522-659,686-718,2646-2812`；`config.rs:473-503,1158-1189` |

> 过程更正：分片 D 回执中 config.rs 哈希有 63 位转录笔误，主会话已按磁盘实测 64 位哈希更正并在回执中留痕（content-unchanged 结论不变）。

### ctx · 分片 B（CLI/MCP/daemon/install；14 文件 8,921 行；receipt: `research/coverage-ctx-cli.json`）

| ID | 级别 | 结论 | 关键锚点 |
|---|---|---|---|
| CTXCLI-01 | P2 | packet cursor 只写不读（CLI/MCP 侧确认）：search 输入无 cursor/offset（全文零命中）；但仍输出 per-result cursor 与 packet.pagination/truncation；show_session 截断无续取游标 | `mcp.rs:717-735,612-628`；`search_render.rs:60,69-70` |
| CTXCLI-02 | P2 | agent 配置写入非原子 + JSONC 丢注释 + 多目标半成功：fs::write 无 temp/rename/备份；jsonc 解析后按纯 JSON 重写；failed>0 不回滚 | `integrations/mcp.rs:1050-1073,1136-1152,804-841`；`slash_commands.rs:684-707` |
| CTXCLI-03 | P2 | Windows 升级无 journal 恢复（非 Unix 恢复恒 false；分离 PowerShell helper+ExecutionPolicy Bypass；版本探测用 contains）；反证：下载先验 SHA-256+元数据验签 | `install.rs:638-658,1441-1451,1303-1311,519-528`；`upgrade/command.rs:526-541`；`metadata.rs:183-198` |
| CTXCLI-04 | P2 | 模型“谁能下载”不齐 + 语义首用强依赖 daemon/网络（worker 无缓存拒绝下载；语义开启+daemon 关闭直接 bail）。反证：缓存后可完全离线 | `embedding_backend.rs:4-11,228-231`；`daemon.rs:1944-1950,846-851`；`setup.rs:40-44` |
| CTXCLI-05 | P3 | 错误契约碎片化：CLI anyhow→exit 1；doctor findings 非空仍 exit 0；MCP 未知参数键 -32602 vs 类型错误 isError | `main.rs:663-757`；`doctor.rs:53-77`；`mcp.rs:890-909,370-373` |
| 正面 | — | 逐目标集成状态机（15 MCP 目标五态+冲突默认拒绝+slash 哈希 Modified）；升级信任链（签名→SHA→暂存探测→journal 发布/回滚/重启恢复）；MCP 最小正确性面（initialize 前置、1MiB 行限、read-only open、截断元数据）；daemon 资源状态机（retryable defer、token+0600+2s 超时） | `integrations/mcp.rs:271-301,719-738,967-980`；`metadata.rs:183-198`；`mcp.rs:220-227,40,370-373,656-666`；`daemon.rs:1754-1772,137-138,320-358` |

### cc-sessions-viewer · 分片 B（7 家 parser；20,148 行中实读 11,438 行，3 full + 4 partial；receipt: `research/coverage-cc-sessions-viewer-parsers.json`）

| ID | 级别 | 结论 | 关键锚点 |
|---|---|---|---|
| CCVP-01 | P2 | 检索语义**宽于**“只索引用户消息”：精确层=`role=="user"` 且 `block.kind=="text"`，无 meta_kind 过滤 → 伪用户系统文本可被检索；反证：tool 结果 7 家全部不参与（均落 `kind=tool_result`）、kimi 两层最严格 | `agents/mod.rs:1008-1019,988-991`；`grok.rs:880-905`；`agy.rs:695-713`；`pi.rs:889-904`；`claude.rs:1574-1599`；`codex.rs:1217-1223`；`kimi.rs:506-515,2006-2014` |
| CCVP-02 | P2 | agy 早期历史恢复依赖每会话 .git + 读路径内启动 git 子进程 + 失败静默；`preferred_transcript` 以“更大文件”选完整度 | `agy.rs:434-546,95-109` |
| CCVP-03 | P2 | codex 发现层静默丢会话/降级：首行非 session_meta → 整个文件无声消失；state sqlite 不可用 → flags 全默认（internal 子代理可能不被识别） | `codex.rs:450-475,2123-2125,2168-2178,113-154,172-183` |
| CCVP-04 | P2 | 快照/并发保护不对称：pi 3 次 revision 重试、kimi 4 次+部分写入拒绝 vs claude/codex/grok 直读容忍坏行 | `pi.rs:330-343`；`kimi.rs:46-47,133-206`；`claude.rs:1458-1468`；`codex.rs:1443-1450`；`grok.rs:996-1003` |
| CCVP-05 | 信息 | 能力矩阵：树/导出/leaf 唯一 Pi；磁盘 fork 唯一 Claude；resume 逐家（pi --session / opencode --session / agy --conversation / claude --resume / codex resume / grok --resume / kimi --session） | `pi.rs:1526-1562,439-522`；`claude.rs:1082-1239`；`mod.rs:251-253,257-264,274-289,438-447` |
| 对照结论 | — | **广度：ASG 领先**（14 活跃 provider vs 7）；**保真深度：CCV 领先**（7 家全有工具结构化/diff、注入分类、写路径、resume 构造、过半家 usage 解码，而 ASG 自报 tool_activity 仅 claude/codex=partial） | `asg-provider-matrix.json` 对照 |

> 事实表注解：L31“只匹配用户消息（工具调用/结果/文件改动不参与匹配）”应改为“工具结果与文件改动不参与；但无 meta_kind 过滤，伪用户系统文本可被检索”。这条修正同样进入 B0 清单。

**对 ASG 的直接含义**：SG-01（cap 先于过滤）、SG-02（零证据升格）、SG-03（失败固化为 current）、AGF-03（裁剪视图进缓存）、AGF-02（shell 参数拼接）这五类是 ASG 必须拿自己的过滤/缓存/水位/resume 契约逐条自查的坑；ASG 侧已有 filter-before-topk 与 snapshot/generation 机制（见 §五），但本轮未按这些场景做专项验证，resume 是否全程参数化也需自查。另加两条同族教训：fast-resume FR-01 与 claude-historian CH-01/CH-05 表明「错误被吞成空结果 / 契约字段哑火」是普遍病——ASG 的跨入口错误分类与预算/截断契约需要一次专项自查。hstry H-01 再补一刀：索引与展示共用截断投影会让「搜不到」与「看不到」同时发生——ASG 的投影/索引/导出是否共用同一截断版本需要自查。Recall R-02 又把 SG-01 的“cap 先于过滤”扩展到了向量路径——ASG 若做 ANN/向量，必须同时证明「先过滤后 top-k」与过滤选择性阶梯。对应自查与整改并入 `implement.md` B 线。

## 四、必须正视的发现

### P0-01：竞品事实和差异化论证先返工

`docs/product/COMPETITOR-COMPARISON.md:20-33` 至少六处技术栈/形态错误；cc-switch 的“无检索”也被 `Github_src/cc-switch/src/hooks/useSessionSearch.ts:14-69` 反证。

`docs/product/OPEN-SOURCE-ROADMAP.md:88-94` 的“无人做到 pack 级证据契约”等绝对化措辞不能成立：

- cass `src/lib.rs:23592-23609,23794-23863` 实际接入 `cass pack`，不是空壳计划。
- cass `src/search/pack_planner.rs:58-85,372-469` 已有预算、引用选择、去重、遗漏原因、来源/脱敏。
- ctx `crates/ctx-history-search/src/packet.rs:14-74` 有 citations、links、visibility、分页/截断。
- Wake `crates/wake-core/src/db.rs:2542-2687` 明确处理 CJK/substr 与短词退化。

**狠话：对手被写成纸老虎，自己当然显得天下无敌。但那叫比较表失真，不叫竞争壁垒。**

整改不是删除所有差异化，而是把“独占”改成可测的承诺：证据验证成功率、出处失效时的行为、过滤分页一致性、同任务的输出 token 成本、用户多久能恢复工作。

### P1-02：普通搜索四次 Git，热路径白花钱

代码链：`crates/agent-session-grep-cli/src/lib.rs:2082-2089` 为读时钟建 App；`2145-2161` 真正检索又建一次；`3058-3099` 每个 App 都临时解析当前 repo；`repo_identity.rs:31-54` 通常两次 Git 子进程。get/show（2365-2390）也承担不需要的探测。

同一当前 release、同一合成条目、配对 20 次，data 完全一致：

| 命令 | 正常 Git 探测中位数 | 既有测试注入关闭该信号 | 差额 |
|---|---:|---:|---:|
| get | 53.67ms | 9.15ms | 44.51ms |
| show | 52.44ms | 8.97ms | 43.47ms |
| search | 97.11ms | 9.98ms | 87.13ms |

**狠话：别急着换引擎、上 ANN。你先别让一次本地查询替 Git 开四次门。**

整改：按请求只构造一次必要上下文；读 clock 不建全功能 App；get/show 不解析 repo；检索复用一次 repo 结果。不能简单让用户永久设置测试变量来掩盖问题。

注意：search 的全部约 87ms 差额包含一轮为 repo-aware 排序所需的合法成本；保留该功能时不能无条件全部删掉。get/show 不需要该信号，可以移除整轮无关探测。

### P1-03：durable journal 没有免费午餐

单文件、200 条消息、源 56,320 bytes，每次只改最后一条固定长度 revision，sync 21 次：

- catalog 始终 202 行；
- activated batch 1 → 21；
- journal manifest 194,315 → 4,739,455 字符；
- store（含存在侧车）1.37 MB → 6.07 MB。

这不是乱猜 SQLite freelist：具体的 activated manifest 正在累积。源码 `adapters-sqlite/src/lib.rs:969-1061,6972-7027` 明确存储/读取完整批次明细；当前实验见 `research/journal-growth.json`。

**狠话：可靠性是价值，但不是给每次同步永久背一大包操作明细免算账的理由。**

整改前必须决定保留政策：building/未决操作不能删；terminal batch 哪些字段为恢复、幂等、诊断必须，哪些可 compact/过期。不能为了省空间把 outbox 整个砍掉。

### P1-04：默认向量模式不是语义优势

当前冻结合成 benchmark：2000 消息、200 会话、100 query。

| 默认构建模式 | Recall@10 | 中文 46 查询 Recall@10 | 代码 25 查询 Recall@10 |
|---|---:|---:|---:|
| lexical | 0.750 | 0.641 | 1.000 |
| semantic（bigram-hash） | 0.475 | 0.478 | 0.360 |
| hybrid | 0.755 | 0.641 | 1.000 |

库增加约 2.14 MB 向量投影，hybrid 在此集合相对 lexical 只多 0.5 个百分点。报告自己诚实标明 `is_real_embedding_model=false`、`threshold_pending=true`、`promotion_claim=none`。

**狠话：把向量算出来，不等于把语义做出来；让 RRF 跑通，更不等于用户的问题被解决。**

不能用此结果否定没有加载权重实测的 Candle E5。应保留 lexical 默认，把 fuzzy/model-kind 直接展示到使用入口；用真实模型、独立 holdout、长消息和 CJK/code/path/error 查询再定质量门。Candle 当前 512-token 截断、单消息单向量（`candle_embedding.rs:124-129,221-227`；SQLite `lib.rs:2417-2475`）也需要长文测试，不要仅在短合成句子上自我满足。

### P1-05：首次使用仍是一张开发者入门试卷

README:28-61 要 checkout、本机构建、手动 PATH、手动 DB；`parse_db_flag`（CLI lib.rs:1680-1718）甚至对合法 search 说“可能不是有效命令”。

这不是严重安全 bug，却是产品真实门槛。对手 agf/fr 不需要先学 canonical catalog 才能定位会话。默认数据路径、明确的首次发现/确认/同步、可直接读懂的会话结果，应优先于继续扩接口数量。

不得用便利作借口静默扫描所有私人目录、自动上传或替用户执行 resume。

### P1-06：支持矩阵要解释到闭环，而不是停在行数

实测 `asg --robot providers`：14 implemented 仍 experimental；resume 为 8 derived / 3 unknown / 3 unsupported；Aider/Cursor discovery unsupported；tool_activity 只有 Claude/Codex partial，其余已实现项 unsupported。

这不是“14 个 parser 全废了”。context 字段特指上下文图，不能偷换成“不能看平面会话”。问题是用户需要知道自己的 agent 到底能走到哪一步，而不是只记住 14/16。

先把高频 core provider 的版本/样本/错误/恢复合同稳定下来，长尾保持诚实等级；不要为了宣传数字乱升成熟度。

### P2-07：大文件和多处参数元数据，是可测的维护债，不是重写理由

SQLite lib.rs 19287 行，其中约前 8500 行属于生产部分，不能把测试都算成生产怪物；但 schema、迁移、关系投影、查询、向量等确实聚集。CLI 的同步/参数/装配/投影也集中。

规范还要求每新增一个 flag，多个 prefix scanner 都要同步注册。这是同步风险，而不是值得骄傲的“零依赖”。

正确顺序：先修上述小而确定的浪费，再按职责提取模块、集中参数元数据、建立跨入口契约校验。不要先抛出“换 clap / 换数据库 / 全部重写”拖延交付；当前规范还明确禁止直接引入 clap，变更须先改契约并评审。

### P0/P1-08：发布与指标说法要兑现

README 明确没有公开 tag/产物。现有三平台 CI 配置、安装/发布脚本、签署清单是资产，但不是可下载且已验证的交付物。历史 billing blocker 不代表现在仍阻塞，必须现场核验，不能永久引用过去的理由。

`resume_handoff_success`（gate 脚本:284-335）只测 **dry-run preview 生成**，不是实际 native resume 成功；脚本注释诚实，对外也必须保留限定。

## 五、哪些成绩必须保留

1. 只读源快照、metadata/fingerprint 前后验证，不能换成不安全的裸拷贝 SQLite 主文件。
2. Stable Message / contextual Placement、显式 relocation、generation/cursor 绑定，是有实际价值的域契约。
3. 当前语义查询先过滤再 top-k、catalog live JOIN、坏向量拒绝；不能为了跑分退回“先截断再过滤”。
4. 多入口共享 Application，跨边界默认脱敏、预算/截断、错误状态必须保留。
5. **实测进步要承认**：当前 4000 消息 noop sync P95 16.22ms；旧基线约 1809.78ms，不能继续用旧数字攻击当前版本。
6. 不向失败数据写回、显式 resume 预览/确认，属于产品信任基础，不能为了“顺滑”全部取消。

## 六、验证结果总表

| 项目 | 本轮实际结果 |
|---|---|
| workspace 默认测试 | 1758 passed / 0 failed / 20 ignored |
| Clippy / fmt | 均 exit 0 |
| Web DOM 交互 | 11 passed |
| semantic-candle application feature | 287 passed；不是模型质量证明 |
| Python 顶层 / release / evidence suite | 18（1 skipped）/9/58 tests，均 suite OK |
| SQLite WAL 既有 spike | A/B/C/D 全通过；仅 spike，不代替生产故障注入 |
| 当前 release build | 独立 target、locked/offline 成功 |
| Core full benchmark | validator 通过；4000 合成消息 |
| 默认检索质量 benchmark | validator 通过；2000 消息/100 查询，真实模型未加载 |
| Git 开销配对实验 | 20 次/变体，返回 data 一致 |
| Journal 重写实验 | 21 次同步，当前实体数固定，日志/存储增长有逐轮记录 |

当前 Core P95：search 162.90ms、show 59.78ms、get 62.90ms、initial sync 1059.32ms；store/source 17.83。后一个比例受合成消息很短影响，不能外推为真实大消息或 50GB 数据的必然比例。物理分表 dbstat 不可用，已记录限制，没有伪造页级分析。

第一次复用默认 target 出现与当前 trait 不符的 E0061；独立目标目录完整通过后，已排除将它作为 HEAD 编译缺陷。这就是为什么锐评也必须做反证。

## 七、整改顺序：先停止内耗，再证明优势

完整依赖、验收和回滚条件见 `design.md` / `implement.md`。

1. **事实与承诺**：修竞品表、移除无证据独占宣称；拆分 preview/native resume/pack validity/任务完成率。
2. **热路径**：消灭重复和无关 Git 探测，验证返回/排序/cursor 不变。
3. **存储生命周期**：先拍板 terminal journal 保留边界，再做 compact/GC 和反复编辑 soak tests。
4. **首用闭环**：默认路径/显式 override、发现确认、初始化、首条结果、context/恢复/交接。
5. **检索质量**：lexical 做扎实，真实 semantic 用有标签 holdout 证明增益，长文/CJK/code 分桶；不要先上 ANN。
6. **核心 provider 稳定性**：Claude/Codex 先形成版本化契约证据，长尾保持 explicit experimental；新增来源证据先核验，不凭竞品代码直接认证。
7. **交付**：当前 commit 三 OS 可追溯验证、签名/校验/安装/升级/卸载；发布权限和账单等 owner 状态单列。
8. **维护性与扩展**：按真实同步点拆职责，已有入口保留；云、图谱、自动总结、桌宠、更多模型或 provider 放到收益证据之后。

## 八、边界、剩余审查与 grill 决策

**尚未完成**：所有仓库逐文件全文审读、所有 provider 格式/版本逐契约审计、竞品安装/跨 OS/native resume/真实模型与同语料实跑、完整 supply-chain/GUI/远程模块安全审计。六份竞品研究均保留此边界，不能用“矩阵已经写完”取消这些工作。

对已经证实的问题，继续读更多文件不会让四次 Git 探测消失，也不会让 21 份 activated manifest 自动变小；可先把建议写完整，但未经用户决策和执行批准不实施。

当前最重要的新产品取舍：**已完成操作的完整 journal 明细是否允许限期/限量保留，而永久保留必要摘要、generation/digest 与未决操作？**推荐允许有约束的 compact/GC，以换取可控长期体积；代价是失去逐条历史完整明细，必须先说明哪些诊断/审计能力仍保留。该决策未拍板，不应擅自删除日志。

