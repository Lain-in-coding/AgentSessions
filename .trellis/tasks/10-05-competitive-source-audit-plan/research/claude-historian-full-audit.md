# Research: claude-historian-mcp 全文件审读凭证与静态源码审计

- Query: 对 `Github_src/claude-historian-mcp`（commit `b627cdfbfe46040368ed9c3f5bb2b531c65a8988`）逐文件审读，核查检索/排序/输出、MCP 契约、打包发布与文档声明一致性。
- Scope: internal；仅该本地快照；只读审计，不运行 build/test/install/network，不执行 git 写操作。
- Date: 2026-10-06
- Status: **静态审读完成；未做任何运行验证；未修改任何被审文件。**

## 1. 阅读边界与覆盖凭证

| 口径 | 数量 |
|---|---|
| 清单文件（coverage-index, project=claude-historian-mcp） | 46 |
| full（精确行区间被逐段实际显示） | 41 |
| partial | 1：`package-lock.json`，320 / 8978 行（生成文件，按关键包抽样） |
| excluded（仅 SHA256 + 字节数 + 文件头） | 4 个二进制 |
| source 文件 | 12 / 12 full，8,406 行 |
| 第一方文本（README/manifest/配置/文档，含 SKILL.md） | 29 / 29 full，3,195 行 |
| full 文本行合计 | 11,601 |
| 实际阅读行合计（full + lock 抽样） | 11,921 / 20,579 行（不含 4 个二进制） |
| 快照基线 | 46 个文件 SHA256/字节/行数与中央清单 100% 一致；审读前后两次全量 SHA256 均 0 mismatch |

方法：全部文本用 PowerShell 带行号有界读取（每次 ≤150 行；大文件多段拼接覆盖 1..EOF），**逐段实际显示后才计入 read_ranges**，哈希不计为阅读。两次单行文件（`.nvmrc`、`.husky/pre-commit`）因 PowerShell 标量索引缺陷只显示首字符，已用 `@()` 重读并以 Format-Hex 校验实际内容（`lts/*`、`npx lint-staged`）。完整区间见同目录 `coverage-claude-historian.json`。

- commit 校验：读取 `.git/HEAD`（`ref: refs/heads/master`）与 `.git/refs/heads/master`，与记录 hash 一致；未运行任何 git 命令、未做写操作。
- 文本侧 29 个 full 文件 = 46 − 12 source − 1 lock − 4 binary；其中根 `manifest.json`、`dxt/manifest.json`、`dxt/package.json`、`manifest.json.backup`、`tsconfig.json`、workflows、husky、`.claude/skills/claude-historian/SKILL.md` 等全部逐行读完。

## 2. 架构与数据流（读码实证）

- 入口：`src/index.ts:54-65` 构造 McpServer（name=claude-historian，version 取 package.json）；注册 `search`/`inspect`（index.ts:90-203），另有 10 个旧工具名迁移桩（index.ts:32-43,75-88，恒返回 isError 提示）。
- 检索：`HistorySearchEngine` 全量扫描 `~/.claude/projects/*/*.jsonl`（search.ts:121-158,290-334）；解析器两阶段（原始行子串预过滤 → JSON.parse → 评分，parser.ts:84-137），<400 KB 走 readFile+split、否则 readline 流（parser.ts:31,139-170）。
- 排序：utils.calculateRelevanceScore 词集匹配 + 软惩罚（utils.ts:397-453）→ selectTopRelevantResults 乘子/语义/意图软加分（search.ts:431-509）；进程内查询缓存 Map 200 条 60s（search.ts:60-88）、目录缓存 30s（utils.ts:17-23）。
- 输出：formatter 统一「📜 半框 + JSON + 0-100 归一化 + tokens≈chars/4」（formatter.ts:39-73,295-338），空结果带 hint（如 formatter.ts:302-313）。
- 只读性：12 个 src 文件逐一核对，无 writeFile/mkdir/rename 等写调用；唯一子进程调用在 `--doctor` 自检（index.ts:605-611）。
- Desktop/DXT：universal-engine 为纯代理 façade（universal-engine.ts:53-104）；Desktop 代码已是注释块（universal-engine.ts:548-628；utils.ts:598-650）；`dxt/server.js` 是另一套 7 工具旧服务（dxt/server.js:20-490），TODO.md:5 自认死代码。

## 3. 高置信发现（每条含严重级别 / file:line 锚点 / 证据）

### F1 [中] 项目路径解码有损，污染展示并击穿 config 搜索 — utils.ts:199-202
证据：`decodeProjectPath` 用 `replace(/-/g, '/')` 把所有连字符还原为斜杠，而编码只替换了 `/`，无法区分真实连字符。调用点：parser.ts:124（每条消息 projectPath）、universal-engine.ts:310（inspect 的 project_path）、search.ts:1597（sessions 的 project_path/name）、search.ts:2247（memories 项目名）、utils.ts:133-137（按解码路径探测项目 `.claude` 目录）。search.ts:233-234 注释自己承认 "lossy decodeProjectPath"。
影响：`claude-historian-mcp` 这类项目显示为 `…/claude/historian/mcp`；`search_config` 对含连字符项目的项目级 `.claude` 目录探测全错（recall 丢失）。结构性缺陷，非显示瑕疵。

### F2 [中] 用户查询词未转义直接构造 RegExp，特定查询静默全空 — search.ts:2099,2137,2240
证据：三处 `new RegExp(term, 'g')`（plans 2099、config 2137、memories 2240）。含正则元字符的查询（典型 `c++`、`foo[`、`a{2`）在构造函数处抛 SyntaxError，且三处都被静默吞掉（config 按文件跳过导致全部文件被跳过；plans 经 Promise.allSettled 丢弃；memories 逐文件跳过）。任务/会话/错误等 scope 用 `includes()`，不受影响。
影响：同查询跨 scope 行为不一致，且空结果不解释原因。

### F3 [中] 无任何自动化行为测试，`npm test` 名不符实 — package.json:30；.github/workflows/test.yml:18-22
证据：`"test": "npm run typecheck && npm run lint"`；CI 仅 clean-install/build/typecheck/lint/format:check；仓库无 `test/`、无 `*.test.*`、无 fixture。PERFORMANCE.md 的 24-43 条“benchmark”是人工命令清单（PERFORMANCE.md:1220-1321），质量分 4.x/5 为自评。
影响：回归（F1/F2/F6 类）无自动拦截；与 ASG 的单测/CI 护栏对比鲜明。

### F4 [中] `inspect` 的 `detail_level` 被声明但完全忽略 — index.ts:166-170 vs index.ts:437-463
证据：schema 暴露 `detail_level`（summary/detailed/raw，默认 summary）；handleInspect 只读 session_id/max_messages/focus（437-439）；formatter.formatCompactSummary 不接 detail 参数（formatter.ts:935-959）。SKILL.md:31 亦推荐 inspect 深挖。
影响：契约漂移；调用方按 schema 传 `raw` 得不到任何差异，与 search 的 detail_level 行为不对称。

### F5 [中] 迁移桩污染 tools/list，并直接击穿自带 doctor 断言 — index.ts:75-88 vs index.ts:656-664；README.md:94
证据：MIGRATION_HINTS 10 条（index.ts:32-43）逐条 registerTool → tools/list 实际 12 个（10 桩 + search + inspect）；`testMCPServer` 却断言 `tools.length === 2`（661-664）。README:94 宣称 “Two tools, 11 scopes”。
影响：客户端每轮会话都为 10 个必然报错的工具描述付出上下文；`--doctor` 把正确实现判失败。兼容层正确做法应是 alias/initialize 提示而不扩大注册面。

### F6 [中] 词界修正被后续版本回退，ReAct→react 类误报重新存在 — utils.ts:379-381,409-429；search.ts:456
证据：utils.ts 注释明示 “v1.0.4 mixed-case rejection … v1.0.5 fixed to simple case-insensitive matching”；contentWordSet 由 lower 化词构成（412-416）；`anyTermMatched` 用 `lowerContent.includes(w)`（428-429）；search.ts:456 同样 `contentLower.includes(t)`。PERFORMANCE.md:445-484 记录过该误报族并宣称修复。
影响：`ReAct` 内容会命中 `react` 查询；`matchesTechTerm` 已删除（utils.ts:374-377），仅注释保留历史。属“已知回退的复现”。

### F7 [中] `scope:all` 对其余子源丢弃 project/timeframe，跨源分数不可比 — index.ts:337-345,379
证据：scope 专向分支把 project/timeframe 传入 errors/sessions/tools（255-296），all 分支的 Promise.allSettled 只传 (query,limit)/(limit,project)（337-345）；错误解另加 `relevanceScore + frequency`（379），而对话分数在 formatter 层才归一化（formatter.ts:334）。
影响：all 的过滤语义与其他 scope 不一致；错误频率可压过相关性分数，排序可解释性差。

### F8 [低] “yesterday” 时间过滤器无上界 — utils.ts:576-579,592-595
证据：cutoff 仅设为昨日 00:00，谓词只有 `messageDate >= cutoff`，没有 `< 今日 00:00` 上界；实现上 yesterday 包含今天（也包含任何未来时间戳）。today/week/month 的起点式语义可接受，yesterday 明显偏离命名。

### F9 [低] inspect 的 message_count 是“截断窗口数”，README 示例不可达 — universal-engine.ts:311,327；index.ts:176-180,438；README.md:180
证据：先 `foundMessages.slice(0, maxMessages || 100)`（311），message_count 取该数组长度（327）；MCP 侧默认 `max_messages=10`（index.ts:179,438），README:173-187 示例展示 `"messages": 128`；PERFORMANCE.md:1190 贴出的真实输出是 “Smart Summary (10 msgs)”。
影响：字段名暗示总数，实为分析窗口；README 与实现互相矛盾。

### F10 [低] 版本与元数据多源漂移 — package.json:3；CHANGELOG.md:1,22；manifest.json:4；dxt/manifest.json:5；dxt/server.js:25
证据：package.json=1.0.6-beta.0；CHANGELOG 最新节 1.1.0（2025-12-09）；根 manifest.json 与 dxt/manifest.json 均 1.0.1；dxt/server.js 自报 1.0.1；制品名 `v1.0.1-fixed`。两个 manifest 的 tools 数组仍是 7 个旧工具名（manifest.json:32-61、dxt/manifest.json:43-72），与现服务不符。
影响：DXT 渠道元数据无法反映真实工具面/版本；自动化比对与发布流水线噪声。

### F11 [中] 发布工作流自相矛盾：手动触发 + `if: push` = 永不执行 — .github/workflows/release.yml:9,18
证据：trigger 仅 workflow_dispatch（release.yml:9），job 却 `if: github.event_name == 'push'`（18），push 触发被注释（4-8）；security.yml 同样仅手动（security.yml:3-12）；pre-push 的 npm audit 不阻塞（.husky/pre-push:15-16）。
影响：仓库内 release/security 自动化事实上关闭，与 CHANGELOG 自动生成的形象不一致。

### F12 [低] DXT 制品/构建脚本/图标互相不一致 — build-dxt.js:23,33,52,59；dxt/manifest.json:21
证据（字节头级）：已提交 `claude-historian-v1.0.1-fixed.dxt` 头部 `50 4B 03 04`（ZIP），而 build-dxt.js:57-59 自述生成 “tar.gz with .dxt extension”；build-dxt.js:23 拷贝根 manifest.json（1.0.1，entry `./dist/index.js`），不是 dxt/manifest.json（server.js 入口）——两套 DXT 管线并存；脚本用 POSIX 专有 `cp -r`/`tar` 与 `npm install --production --no-optional`（33,52,59），后者与根 package.json:85-87 的 optionalDependencies(level) 冲突；dxt/icon.png 实际字节头 `FF D8 FF DB`（JPEG）而非 PNG（dxt/manifest.json:21 声称 icon.png；.gitattributes:13 声明 `*.png binary`）。
影响：已提交 .dxt 无法由仓库脚本复现；Windows 必失败；图标格式错标。属打包资产问题，不影响 MCP 运行。

### F13 [低] README 策略清单与代码锚点过期 — README.md:227,234,237,256
证据：messageCache 已删除（search.ts:56-58；PERFORMANCE.md:40），README:234 仍把它列为搜索策略并链 search.ts#L27；README:227 声称去重在 search-helpers.ts#L40（现为 expandQuery 中段，实际在 58-100）；README:237 把 calculateQuerySimilarity 标为 “Edit distance” 并链 L157（现为 calculateImportanceScore），实际实现是位置字符比较 isWordSimilar（search-helpers.ts:518-532）；README:256 写 “Reads from: ~/.claude/conversations/”，而代码唯一数据根是 `~/.claude/projects/`（utils.ts:32-35）。
影响：文档可信度下降，对外部读者是误导。

### F14 [低] COMPARISON.md 自相矛盾地宣称 SQL/语义存储 — COMPARISON.md:291,296 vs README.md:251-257
证据：对比表把 claude-historian-mcp 标为 “SQL + semantic”“Storage: SQLite”，与 “Zero dependencies / no databases / JSONL on-demand” 设计及全部源码（无 DB 依赖，level 仅为 Desktop 死代码）矛盾；同文件 3-18 行恰有一段自我更正，说明旧结论残留。
影响：该表不可采信为产品事实。

### F15 [低-中] 无索引全量扫描 + 无并发上限的成本 — search.ts:296-334；PERFORMANCE.md:192
证据：每查询对所有项目 allSettled，项目内对全部 jsonl 再 allSettled（search.ts:300-304,332-334）；fileContext 预检整文件读一次（search.ts:95-108），随后 parser 再读一次（928）；缓存仅进程内 30s/60s。项目自测承认 97 项目 ~12s（PERFORMANCE.md:192，提速 42% 后仍慢）。
影响：大历史首查询延迟高、IO 尖峰；无跨进程增量索引（设计取舍）。

### F16 [低] search.ts 内约 9 个私有方法为死代码 — search.ts:582-663,666-735,767-790,792-881,883-890
证据：processProjectDirectory、prioritizeResultsForClaudeCode、deduplicateMessages、isSummaryMessage、isHighValueMessage、getOptimalLimit、enhanceQueryIntelligently、私有 calculateRelevanceScore、matchesTimeframe 均无调用点（`this.*` 全库无命中）；其中私有 calculateRelevanceScore 的 case-aware 词匹配（826-839）从未生效。另有 universal-engine.ts:548-628、utils.ts:598-650、index.ts:489-495 的大段死代码/注释；TODO.md:5-15 自认。
影响：维护面积虚增，易将死路径误读为现行行为（本次审读已显式排除）。

### F17 [低] 行为细节噪声（合并）
- `scoreFileReferences` 把 `.json` 视为含 `.js` 命中（utils.ts:529-535）→ 任何 JSON 提及 +3 分。
- 行级预过滤按原始 JSONL 整行子串（parser.ts:89-93），`auth` 会命中 `author` 类行：无假阴性但增加解析量，靠后续评分收敛。
- sessions 列表每项目仅取 `max(3, ceil(limit/2))` 个最新文件（search.ts:1603），单项目密集时较早会话不会出现。
- `getRecentSessions` 的 message_count 来自无查询全量解析（1607），准确但成本高。
- pre-push 的 npm audit 不阻塞（.husky/pre-push:15-16）；security.yml 停用（3-12）。

## 4. 对标 agent-session-grep（ASG）

### 4.1 historian 强于/不同于 ASG 的正面点
1. 零写入零索引运行时：12 个 src 文件逐一核对，全部只读；无 schema/迁移/worker/sidecar 负担；npx 即用，隐私边界清晰。
2. MCP 呈现契约：detail_level raw/detailed/summary 旁路、tokens 估算头、空结果 hint、readOnly/idempotent annotations（index.ts:136-139,182-185；formatter.ts:295-338）——可对照检验 ASG 自己的 raw 旁路与 hint 体验。
3. token 预算内保真：0-100 归一化、内容类型感知截断、代码块/错误保留（formatter.ts:54-229）。
4. 根目录覆盖：`CLAUDE_CONFIG_DIR`（utils.ts:28-30）——provider 数据根可覆盖的理念与 ASG 路径推导纪律同向（但解码必须无损，见 F1）。
5. 证据文化：PERFORMANCE.md 按版本记录评分、修复与“剩余缺口”（189-193），并给出可重跑命令（1220-1321）——竞品研究的一手材料。

### 4.2 historian 弱于 ASG 的方面
1. 无索引/无 FTS：每查全量扫描 + 启发式打分 + 进程内 60s 缓存；ASG 的 SQLite+FTS/BM25+pack 契约在重复查询、增量更新、可解释排序上维度更高。12s/97 项目（自测）就是无索引成本上限。
2. 精度机制原始且多路径不一致：手写同义词表（search-helpers.ts:24-47）、位置字符比较冒充 Edit distance（518-532）、多处分词/子串回退（utils.ts:489-492；search.ts:456）。
3. 身份/来源模型薄：message 有 uuid，但对外 project_path/项目名经过有损解码（F1），sessions id 截断 8 位（formatter.ts:883,943-947），无父边/事件序/字节偏移等可追溯字段——ASG provider spec（原生身份/父边/严格形状）完全覆盖此维度。
4. 无行为测试（F3）；doctor 还会被自身工具表击穿（F5）——工程护栏差距最大处。
5. 元数据治理弱：版本四处漂移（F10）、发布/安全流程空转（F11）、DXT 资产不可复现（F12）。

### 4.3 ASG 可落地借鉴（建议级）
- 借鉴：MCP 输出加 raw/detail 旁路、空结果 hint、readOnly/idempotent 注解、token 估算头（若 ASG 尚未覆盖）。
- 借鉴：env 覆盖 provider 数据根；路径编解码必须无损——historian 的 decodeProjectPath 是反面教材。
- 借鉴（产品形态）：单入口 scope 化（conversations/plans/config/tasks/errors/tools/sessions）对统一检索体验友好；需与 ASG 的 cursor/分页与证据契约兼容。
- 避免：把“迁移提示工具”注册为真实工具（F5）；用 alias 或 initialize 提示替代。
- 避免：文档宣称超前于实现（版本/工具表/算法名/存储模型漂移）。
- 论据：其自测的 12s/97 projects 与无增量索引取舍，是 ASG 继续投资索引层的现成对照。

## 5. 外部参考与版本记录（仅本地记录）
- 快照：`.git/refs/heads/master` = `b627cdfbfe46040368ed9c3f5bb2b531c65a8988`（读文件校验，未运行 git）。
- 已解析依赖（package-lock 抽样，lockfileVersion 3）：@modelcontextprotocol/sdk 1.27.1（346-380）、zod 4.3.6（8958-8967）、level 10.0.0 optional（4020-4038）、eslint 10.0.3（2756-2785）、husky 9.1.7（3572-3580）、lint-staged 16.4.0 dev（4084-4105，engines node>=20.17）、prettier 3.8.1（7100-7112）、semantic-release 25.0.3（7443-7455）、typescript 5.9.3（8528-8542）、typescript-eslint 8.57.1（8543-8545）。
- 运行时要求 node>=20（package.json:78-80）；.nvmrc=`lts/*`；CI 用 node 22（test.yml:16）。
- LICENSE MIT（LICENSE:1-3），与 package.json:61、manifest.json:18 一致。
- 未联网、未安装、未构建；性能/质量数字均为项目自述，未复现。

## 6. 相关 specs / 已读上下文
- 活动任务 research/coverage-index.json 定义了本次回执格式与 46 文件清单（只读引用，未修改）。
- 格式基准：research/coverage-sessiongrep.json、research/sessiongrep-full-audit.md（只读学习，未修改）。
- ASG 背景（题述）：多 provider 本地会话检索、SQLite+FTS、pack/证据契约、resume 规划；本报告只做对标陈述，未重审 ASG 实现。

## Caveats / Not Found（精确剩余）
- package-lock.json 维持 partial：已读 1-110,346-380,2756-2785,3572-3580,4020-4060,4084-4105,7100-7112,7443-7455,8528-8545,8950-8978（320 行）；未读 [111-345],[381-2755],[2786-3571],[3581-4019],[4061-4083],[4106-7099],[7113-7442],[7456-8527],[8546-8949] 共 8658 行。依赖安全/许可证/MSRV 未审计。
- 二进制 4 个仅记录 SHA256/字节/文件头（.dxt=ZIP 头、demo.gif=GIF89a、icon.png=JPEG 头、logo=JFIF）；内容未做视觉/解包检查；38,980,078 B 的 .dxt 内部结构未展开。
- 未做任何运行验证：未 build/test/install/network；`--doctor`、MCP 协议交互、真实检索、性能与 DXT 安装均未执行；F5/F11 等由代码推演而非运行证实。
- TODO.md:7 “99 处 any”与现状不符：全 src `\bany\b` 计数 28（其中 universal-engine 17 处集中在大段死注释）；未运行 eslint/tsc 复核。
- 未审 `.git/`、`.codegraph/` 等非清单内容；未对第三方依赖源码审计。
- 覆盖回执：`coverage-claude-historian.json`（本报告配对文件）。
