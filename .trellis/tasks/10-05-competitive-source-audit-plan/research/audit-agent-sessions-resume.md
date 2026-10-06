# Research: agent-sessions 分片 B 静态审计（终端 / 恢复 / 列表连贯性）

- Query: 对 `Github_src/agent-sessions`（commit af4793d317ab83e73ea205ff3f91feee32a3f0dd）逐段精读：终端启动/恢复链路（命令构造、转义、环境、cwd、错误处理）、列表→命中→详情→恢复连贯性、Codex active/status 探测的刷新频率/误判/资源成本，并与 ASG 对照。
- Scope: internal；只读四个文件：`AgentSessions/Views/SessionTerminalView.swift`、`AgentSessions/Views/UnifiedSessionsView.swift`、`AgentSessions/CodexStatus/CodexStatusService.swift`、`AgentSessions/Services/CodexActiveSessionsModel.swift`（任务书列的是 `CodexStatus/`，实际路径在 `Services/`，由文件系统检索确认）。分片 A（DB/Session/SearchCoordinator/索引）未读；T2 `sweep-agent-sessions.json` 只用过滤查询，未整读，不作为行级证据。
- Date: 2026-10-06
- Status: **阶段性静态审计完成；4 个文件 = 1 full（CodexStatusService）+ 3 partial（missing_ranges 精确列账见 `coverage-agent-sessions-resume.json`）。未 build、未 test、未启动任何 app、未联网、未做 git 写。** 所有"行为"均为静态推导，不代表已运行复现。

## 0. 阅读凭证与边界

| 文件 | 总行数 | 状态 | 实读行数 | 阅读区间（1-based，闭区间） |
|---|---:|---|---:|---|
| `AgentSessions/Views/SessionTerminalView.swift` | 5861 | partial | 1020 | 1-120; 300-449; 950-1249; 3760-3909; 4290-4589 |
| `AgentSessions/Views/UnifiedSessionsView.swift` | 4455 | partial | 1866 | 330-479; 560-709; 1130-1279; 1294-1403; 1490-1715; 2068-2777; 3013-3292; 4190-4279 |
| `AgentSessions/CodexStatus/CodexStatusService.swift` | 3869 | **full** | 3869 | 1-3869 连续 26 个窗口（每窗 ≤150 行，另补 671-673 显示截断重读） |
| `AgentSessions/Services/CodexActiveSessionsModel.swift` | 3490 | partial | 1843 | 1-599; 1420-1983; 1985-2284; 2290-2369; 2543-2692; 2750-2899 |

- 每个窗口 ≤150 行，均为编号源文显示；文件在读前/读后各做一次 SHA-256，四份哈希一致（`content_unchanged=true`），但这只证明审计期间内容未变。
- 本分片四文件之外的内容（尤其 `AgentSessions/Resume/AgentTerminalLauncher.swift`、各 `*ResumeCoordinator/CommandBuilder/TerminalLauncher`、`AgentSessions/Services/PresenceEngine.swift`、`CodexOAuth/*`、`Resources/codex_status_capture.sh`）**未读**；凡引用只以本分片文件内的调用点为准，并在文中标注"调用点证据"。
- 每处 grep 命中或 CodeGraph 片段都不等于读完；未读区间一律列入 missing_ranges，不据此下结论。

## 1. 必须回答

### 1.1 终端 / 恢复如何启动（PTY？Process？argv vs shell 字符串；转义与注入面；cwd/env）

**结论（以本分片可见的调用链为限）：没有嵌入式 PTY 的证据；恢复走"外部终端 App + shell 字符串"路线。**

1. **入口与分派。** 右键菜单（`canResumeSession` 预检禁用，`:1162-1167`）与工具栏（`.disabled(!canResumeSelectedSession)`，`:1827-1834`）调用 `resume(s)`；`resume(s)` 按 provider 分派：Codex 走 `CodexResumeCoordinator.shared.quickLaunchInTerminal(session:)`，其余 provider 各自构造 `*ResumeInput`（sessionID/workingDirectory/binaryOverride）并组合 `*ResumeCoordinator(env:builder:launcher:)`（`UnifiedSessionsView.swift:3148-3285`）。launcher 由 `ResumePreferenceHelpers.resolveTerminalKind()` 在 iTerm2 / Warp / WarpPreview / Terminal.app(unknown) 之间选择（`:3163-3167`、`:3181-3185`、`:3236-3240` 等，七处同构）。
2. **argv vs shell 字符串。** 本分片可见的"恢复命令构造"只有**剪贴板路径**：`copyResumeCommand` 把命令拼成 `cd <quoted wd> && <quoted binary> --resume <quoted sid>` 形态的**一个 shell 字符串**（`:1527-1633`，Codex 分支 `:1547-1555`）。`resume(s)` 本身把结构化字段（sessionID、workingDirectory、binaryOverride）传给 coordinator（`:3160`、`:3178` 等），是否在下游变回 shell 字符串取决于未读的 builder/launcher。
3. **转义与注入面。** 所有拼接点都经过各自的 `builder.shellQuoteIfNeeded(...)` 逐段引用（`:1540`、`:1553`、`:1565`、`:1579`、`:1592`、`:1606`、`:1615-1617`、`:1627`）；`binary` 可来自用户设置（`binaryPath` / `binaryOverride`），`sid` 主要来自会话元数据（如 `session.codexInternalSessionID ?? codexFilenameUUID`，`:1549`）。**注入面结论是条件性的**：调用点形式上是"先引用再拼接"，但 `shellQuoteIfNeeded` 的实现本体（各 `*ResumeCommandBuilder`）不在本分片读取范围，无法验证其对单引号/换行/NUL/组合字符的处理；在下游实现未验之前，不能宣称该面已闭合，也不能宣称存在实际注入漏洞。
4. **cwd。** `effectiveWorkingDirectoryURL(for:)`（`:3069-3094`）按 provider 取工作目录（Claude 用 `projectRoot`，Codex 用 `CodexResumeSettings.effectiveWorkingDirectory`，其余走各自 settings）；复制命令用 `cd` 前缀表达，恢复时作为 `input.workingDirectory` 传入。菜单还有独立的 "Open Working Directory"（`:3096-3101`，存在性检查后交给 Finder）。
5. **env。** 本分片在恢复链路上没有设置子进程环境变量的代码；与环境相关的是**状态探测**而非恢复：REPL 探测用 `/usr/bin/env bash -lc codex` 并注入登录 shell PATH（`CodexStatusService.swift:2073-2085`），tmux /status 捕获脚本注入 `CODEX_BIN`/`TMUX_BIN`/`TIMEOUT_SECS`/`PATH`（`:2673-2695`）。
6. **外部进程清单（本分片内全部 Process 调用点，供 PTY 判断）。** `SessionTerminalView.swift:4369-4372`（`/usr/bin/open -a Preview`，`try?` 静默）；`CodexStatusService.swift:2079-2081`（env+bash -lc）、`:2673-2675`（/bin/bash + 脚本路径）、`:3198-3206`（ps/lsof/tmux 等，带超时）、`:3835-3852` 与 `:3855-3868`（登录 shell PATH/tmux 解析，`waitForExit()` 无超时）；`CodexActiveSessionsModel.swift:1809-1823`（osascript focus，无超时）、`:2783-2795`（ps/lsof/iTerm 列表，带超时）。**四文件内没有任何 forkpty/openpty/伪终端分配符号**；launcher 真身未读，故此结论限定为"本分片未见 PTY"。

### 1.2 列表→命中→详情→恢复的连贯性

1. **选择身份。** 表格选择是 `String`（session id）单值绑定（`UnifiedSessionsView.swift:2068-2106`）；权威查找用 `cachedRowByID` O(1)（`:380-383`）。行高亮绑定原始 `selection`（即时），转写面板绑定 150ms 防抖后的 `settledSelection`（`:361-366`、`:2113-2139`、`:2235-2269`）——键重复滚动时列表跟手、详情不抖动。
2. **过滤与"命中"。** 行集合 = 搜索态用 `searchCoordinator.results` + `applyFiltersAndSort`，否则用 `unified.sessions`，再叠加 active-only（`:462-474`）；搜索出结果且无有效选择时自动选第一条但不抢焦点（`:1358-1368`）；搜索运行期按策略 hold 旧行（`:2525-2543`）；数据抖动期选择缺失不立即替换，抖动结束后补一次（`:2583-2601`、`:1321-1343`）。
3. **命中→详情（Cockpit 导航）。** 通知或待处理桥进入 `handleCockpitNavigation`：解析目标 → 若行不可见则强制移除过滤器（含 provider 开关、query、日期、模型、kinds；`:2382-2410`）→ 重新构建行 → `setActiveSelection` → 折叠搜索 UI → 激活窗口置前（`:2295-2328`）。**身份解析顺序：unifiedSessionID → 规范化 logPath → liveSessionIDCandidates；cwd-only 猜测被显式拒绝**（`:2348-2379`，注释原文 "prefer 'no navigation' over navigating to a potentially wrong session from the same directory"）——这是本分片最值得肯定的 fail-closed 设计。跨进程待处理导航 45 秒过期（`:2330-2346`）。
4. **命中→详情（搜索自动跳转）。** `scheduleAutoJump` 只在 150ms settle 后触发，与转写面板的 `autoJumpSessionID == session.id` 门锁步，避免"面板还在旧会话时就跳跃"（`:2249-2256`、`:2742-2753`）。转写面板内部对深链接/搜索跳转用 pending + widenWindowForJump（`SessionTerminalView.swift:1093-1159`）处理大会话离线窗口。
5. **失败反馈。** 焦点路径有反馈：`focusActiveTerminal` 失败 `showActionAlert`（`:3040-3058`）。**但恢复路径没有**：七处 `resume` 分派全部写 `_ = await coord.resumeInTerminal(...)` / `_ = await CodexResumeCoordinator...quickLaunchInTerminal(...)`（`:3152-3154`、`:3171`、`:3189`、`:3207`、`:3225`、`:3244`、`:3262`、`:3280`），返回值被丢弃，失败只停留在未读的下游里。预检禁用（`canResumeSession`/`canCopyResumeCommand`，`:1503-1525`、`:3129-3146`）能挡住"明显不可能"的项，挡不住运行时失败（binary 缺失、终端拒绝 AppleScript、Warp 未安装等）。
6. **滚动/定位的残余。** 列表侧整表重建由 `.id(tableIdentity(columnLayoutID:reorderGeneration:))` 触发（`:1145`）；行滚动定位本体应在 AppKit 表格协调器（`AppKitOutlineTableCoordinator` 注释见 `:399`），不在本分片已读区间，列为残余未决。

### 1.3 Codex active/status 探测：刷新频率、误判与资源成本

**A. 用量/鉴权探测（CodexStatusService，full 读）。**

- **主循环**：`refreshTick()` → `nextInterval()` → sleep（`:1948-1977`）。间隔策略（`:3223-3258`）：可见时 60-180 秒（用户值缺省 60，clamp `[60,180]`，`:3255-3258`）；电池供电 180 秒；**menu-bar 后台**电池 180 秒、AC 用可见间隔；**完全隐藏且不紧急 = 1 小时（近似停摆）**；"紧急"= 5h 用量 ≥80% 或 15 分钟内重置（`:3266-3273`），紧急时不再落入 1 小时分支。
- **三层数据源成本**：被动 JSONL 解析有 12 分钟未变跳过 + mtime 守卫（`:1922-1923`、`:3335-3342`），尾读上限 192 KiB（`:1926`、`:3439-3440`），候选文件上限 8/32（`:1924-1925`）；OAuth/CLI-RPC 权威取数成功冷却 60 秒（`:2382-2408`）；tmux `/status` 探测常规需用户 opt-in + 4 小时冷却（`:2568-2572`、`:2608-2610`），并先过"权威 login-status"闸（120 秒节流、5 秒有界子进程，`:1882`、`:1908-1916`、`:2580-2598`）。后台 menuBackground 明确"不做目录扫描"，每跳只检查已种子文件 + 1 天 3 个候选（`:2293-2345`）。
- **override 路径（值得注意的边界）**：当两个窗口完全缺失数据时，自动探测**绕过 opt-in 与可见性门**，只保留 30 分钟最低间隔（`:2560-2577`）；仍在 `FeatureFlags.disableCodexProbes`（`:2288`、`:2634`）与权威 login-status 闸之后，且失败也会写冷却时间戳防重试风暴（`:2600-2603`）。成本上界约 1-2 条消息 / 30 分钟。
- **文档与代码不一致（P3）**：文件头自述 tmux 探测是 "`CodexAllowStatusProbe` preference + 10min cooldown"（`:30`），代码实际是用户 opt-in + 4 小时冷却（`:1923`、`:2610`），override 路径才接近 30 分钟（`:2576`）。头部文档会误导对 token 成本的评估。
- **清理成本**：孤儿探测清理（ps 全表 + 逐 PID `ps eww` + lsof + tmux socket 扫描）以 1 小时最小间隔运行（`:1932`、`:2859-2953`、`:3275-3293`），且延迟到有可见消费方才做（`:1956-1958`、`:2008-2030`）。

**B. live presence 探测（CodexActiveSessionsModel，partial 读）。**

- **轮询间隔**：前台有可见消费者 2 秒；后台 15 秒；pinned cockpit 背景 3 秒；连续 3 个稳定周期后退到 5 秒；`appIsActive=false` 且无 cockpit 时 15 秒（`CodexActiveSessionsModel.swift:128-135`、`:1424-1435`、`:1445-1456`）。
- **子进程探测最小间隔**：进程探测按状态取 6 / 45 / 30 / 120 秒（前台 registry 空 6s，前台 registry 非空 30s，后台 45/120s，`:139-142`、`:1600-1623`）；iTerm 尾探后台 9-15 秒（`:133`、`:1625-1633`）；iTerm 尾探 round-robin 预算 4，resume 平滑 `[1,2,4]`（`:143-144`、`:1464-1493`）。选择打开时昂贵探测延后 2.5 秒（`:575-578`），这是"命中→详情"侧的降载协作。
- **误判面与抑制机制**：live 状态是"iTerm 尾文本词法 marker + 日志 mtime"启发式——busy marker 只在近底部 8/12/16 行窗口匹配（`:1874-1909`、`:1912-1960`、`:1964-2008`）；探针拿不到尾文本时**倾向 `.openIdle`** 而非 false-active（`:2195-2203`）；inconclusive 时给 6 秒 active 宽限防闪烁（`:136`、`:2226-2233`）；mtime 回退窗口 Codex 2.5s / Claude 15s / OpenCode 30s（`:2158-2170`、`:2341-2352`）；空转换期最多抑制 3 个周期（`:1548-1559`），iTerm 标题探测空结果时回退缓存映射（`:1561-1576`）。
- **成本形态**：常在成本 = 2 秒轮询 × (ps/lsof/osascript 子进程按上述间隔节流) + iTerm 尾探预算；有界（`runCommand` 带超时+SIGKILL，`:2755-2819`），但**没有实测数据**——本分片未运行，`PresenceEngine.swift`（真正 loop）未读，`debugPerformanceSnapshot` 未采。

### 1.4 与 ASG 对照（命中→恢复连贯性、冷启动）

**agent-sessions 强在哪（可学）**

1. **命中→恢复是 UI 内闭环**：选中行右键/⌃⌘R 直接拉起原始 CLI 终端；ASG 现状要先构建/PATH/`--db`，再执行 resume（`research/asg-main.md` ASG-02；SG-08 教训）。冷启动摩擦面上 agent-sessions 明显更短（GUI 常驻、列表即用）。
2. **身份解析 fail-closed**：id → logPath → runtime id 三级，显式拒绝 cwd-only 猜测（`UnifiedSessionsView.swift:2348-2379`）。这正对应 SG-03/SG-08 的"错误身份分叉/模糊解析"教训，值得 ASG 在会话解析/导航里照做。
3. **选择传播的稳定性工程**：行高亮即时 + 详情 150ms settle + 自动跳转与面板选择锁步（`:361-366`、`:2249-2256`、`:2742-2753`）；抖动期选择替换延迟 + 补跑（`:1321-1343`、`:2583-2601`）。对应到 ASG 的等价物是"大输出两段式渲染 + 稳定门控"，不是 UI 细节。
4. **冷启动渲染结构**：转写两段式窗口构建——stage-1 先画最后窗口、stage-2 按字符阈值决定是否换全量，且滚动中（ScrubSignal）不落大 apply（`SessionTerminalView.swift:997-1090`、`:1032/1070/1143`）；重建有签名去重防双跑（`:952-980`）。ASG 若做 TUI/Web 大结果首屏可借鉴"尾窗先画 + 稳定门控"。
5. **探测分层与"权威前置闸"**：被动日志 → 权威 OAuth/RPC（60s 冷却）→ opt-in tmux 探测（4h）+ pre-spawn authoritative login-status 闸（防挂起子进程）（`CodexStatusService.swift:2382-2408`、`:2580-2598`）。ASG 的 freshness 设计（SG-08.1 教训）可直接复用"被动优先、主动有闸、失败不推进水位"的骨架。

**agent-sessions 弱在哪（不学 / 需补）**

1. **恢复结果被丢弃**：`_ = await ...resumeInTerminal(...)`（七处）没有回执、没有 UI 反馈；对比 ASG 教训"机器错误分类 + 结构化结果"。应返回结构化结果（launched/terminalUnavailable/binaryMissing/...) 并驱动反馈。
2. **恢复命令的 shell 字符串出口**：剪贴板路径直接产出 `cd ... && ...` 字符串（`:1527-1633`），安全完全押在 `shellQuoteIfNeeded` 单点实现上；与 SG-08 教训"优先返回结构化 argv/cwd，提示字符串单独转义"相反。至少要补 quoting 边界专项验收，并把"可执行语义"和"展示字符串"分开。
3. **终端集成绑定 macOS 特定 App + AppleScript**：iTerm2/Warp/Terminal.app 逐家适配（`:3163-3167` 等）+ osascript（`CodexActiveSessionsModel.swift:1746-1832`），不可移植且 `waitForExit()` 无超时（用户手势触发时可卡主调用栈）。
4. **"抑制"掩盖了 freshness 语义**：空转换最多抑制 3 周期（`:1548-1559`）、探测失败倾向 openIdle（`:2200-2203`）——对"会话是否在跑"这是合理的防抖，但 presence 没有 last-good/freshness/partial 字段供消费者区分"确认空闲"与"没探到"。ASG 应坚持把这类状态显式化（SG-08.1）。

**不该学（清单）**：丢弃 resume 结果；把 shell 字符串当唯一命令出口；用 AppleScript 控制外部终端并做无超时等待；用隐藏抑制代替 freshness 表达。
**该学（清单）**：settled 选择 + 锁步跳转；fail-closed 多级身份解析；尾窗先画 + 稳定门控；被动优先 + 权威前置闸的探测分层；把冷却/节拍写成常量矩阵并给出成本上界（agent-sessions 这点做得好：CASM:128-146 与 CSS:3223-3263 两处都是常量矩阵）。

## 2. 发现（Top 5，含反证）

严重度沿用本项目口径：P1=核心正确性；P2=有条件的正确性/可用性/边界风险；P3=低优先或证据欠缺；I=已知取舍。

### RS-01 · P2 · 恢复执行没有任何失败反馈路径

- **证据**：七处恢复分派把 coordinator 的异步结果整体丢弃：`UnifiedSessionsView.swift:3152-3154`（Codex quickLaunch）、`:3171`、`:3189`、`:3207`、`:3225`、`:3244`、`:3262`、`:3280`（各 provider `resumeInTerminal`）。本文件里没有任何基于返回值的状态更新、alert 或日志。
- **可达**：binary 未被安装/被改名（settings `binaryPath` 指向失效路径）、终端 App 拒绝 AppleScript、Warp/iTerm 未运行时，菜单项仍可能是启用的（`canResumeSession` 只做静态判断，`:3129-3146`），用户点击后无任何反馈。
- **反证/界限**：焦点路径（Focus in iTerm2）有 `showActionAlert`（`:3040-3058`）；"明显不可能"的项会禁用菜单/工具条（`:1162-1167`、`:1191-1193`、工具栏 `.disabled(!canResumeSelectedSession)` `:1833`）；失败的具体处理逻辑可能在下游 coordinator/launcher（未读），不能断言"整个产品没有反馈"，只能说"本文件这一层把结果丢了"。

### RS-02 · P2 · 剪贴板恢复命令是 shell 字符串，转义正确性押在单点 helper

- **证据**：`copyResumeCommand` 对八个 provider 生成 `cd <quoted> && <quoted binary> --resume <quoted sid>` 类字符串（`:1527-1633`）；每段引用都经 `builder.shellQuoteIfNeeded`，但 helper 本体不在本分片范围。session id 源数据形态多样（如 `codexInternalSessionID ?? codexFilenameUUID`，`:1549`；Antigravity 从文件推导，`:1621-1623`）。
- **可达**：若源数据（本地会话文件/目录名）含换行、单引号等在 quoting 实现中未覆盖的字符，粘贴执行时会改变命令语义；`binary` 若来自含空格/引号的自定义路径同样受影响。
- **反证/界限**：调用点形式上是"先引用再拼接"，且这是**剪贴板**路径——执行与否由用户决定；应用内 `resume(s)` 把结构化字段传给 coordinator，可能完全不经此字符串路径。要定性需先读 `*ResumeCommandBuilder.shellQuoteIfNeeded` 并做边界用例（本分片不做结论）。

### RS-03 · P2 · tmux /status 探测在"双窗口缺失"时绕过敏 opt-in 与可见性门（成本边界）

- **证据**：`maybeProbeStatusViaTMUX` 的 override 分支：`needsProbeOverride = missingRateLimits || (stale5h && staleWeek)` 时跳过 `CodexAllowStatusProbe` 与 `visible` 检查，只保留 30 分钟内存冷却（`CodexStatusService.swift:2560-2577`）；menu-background tick 也会调用该探测（`:2367-2370`）。文件头自述的 "10min cooldown"（`:30`）与代码 4 小时常规冷却（`:1923`、`:2610`）不一致，会误导成本评估。
- **反证/界限**：仍受 `FeatureFlags.disableCodexProbes`（`:2288`、`:2634`）、权威 login-status 前置闸（`:2580-2598`）、30 分钟下限与失败写冷却（`:2600-2603`）约束，成本上界 1-2 条消息/30 分钟；这是注释明示的"last resort"设计（`:2561-2563`），不是无闸轮询。

### RS-04 · P2 / I · live 活跃状态是启发式，且"探不到"与"确认空闲"被混同表达

- **证据**：状态=词法 marker（近底部 8-16 行，`CodexActiveSessionsModel.swift:1874-2008`）+ mtime 回退（2.5/15/30s，`:2158-2170`、`:2341-2352`）；尾探失败→`openIdle`（`:2200-2203`）；空转换抑制最多 3 周期（`:1548-1559`）；稳定后降频 5s（`:132`、`:1445-1456`）。`presences` 数据模型没有 freshness/partial 字段（`:24-68`）。
- **可达**：iTerm 尾文本历史里残留 busy 词、或探针被节流时，UI 的"working/idle"与真实状态可短暂不一致；对依赖该状态做恢复/提醒的消费者无法区分"确认空闲"和"最近没探到"。
- **反证/界限**：这些抑制/宽限均为显式防抖设计，方向是"宁可 openIdle 不要 false-active"；轮询/探测频率有界（2s/6-120s/预算 4），长时间错误活跃的概率被压低。本项是设计取舍 + 可观测性缺口（I），不是已复现的误报。

### RS-05 · P3 · 两处无超时子进程等待

- **证据**：`tryFocusITerm2` 以 osascript 执行 AppleScript 后 `process.waitForExit()`（`CodexActiveSessionsModel.swift:1809-1823`），无超时；由 `UnifiedSessionsView.focusActiveTerminal`（主线程路径，`:3040-3058`）直接调用。登录 shell PATH/tmux 解析同样 `p.waitForExit()` 无超时（`CodexStatusService.swift:3835-3852`、`:3855-3868`）。
- **可达**：iTerm 无响应/被自动化权限弹窗阻塞时，用户点击 "Focus in iTerm2" 可让 UI 卡住；路径解析在冷 shell 或权限异常时可拖慢服务启动。
- **反证/界限**：焦点是用户手势触发（不是后台自动），曝光面小；其余探测类子进程都有超时封装（`:2803-2834`、`:3198-3221`、`CodexActiveSessionsModel.swift:2755-2819`），说明工程上并非不知道要加超时——这是局部遗漏而非系统性缺陷。

## 3. 正面设计证据（不列为缺陷，供 ASG 取用）

- 选择防抖 + 锁步自动跳转（`UnifiedSessionsView.swift:2235-2269`、`:2742-2753`）。
- Cockpit 命中→详情的 fail-closed 身份解析与"宁可不导航"注释（`:2348-2379`）。
- 行重建的 generation 防过期落盘（`:2679-2718`）、大数据抖动时 hold rows（`:2525-2543`）。
- 转写两段式窗口构建 + scrub-quiet 门 + 签名去重（`SessionTerminalView.swift:952-1090`、`:1093-1159`）。
- 探测冷却矩阵与"失败也写冷却防重试风暴"（`CodexStatusService.swift:2544-2610`）；权威 login-status pre-spawn 闸（`:2580-2598`）。
- 防误报三件套：尾探失败→openIdle、空转换抑制、iTerm 标题缓存回退（`CodexActiveSessionsModel.swift:2195-2203`、`:1548-1576`）。

## 4. 覆盖统计与残余未决

**统计**：分片 B 四个文件共 17675 行；本轮实读 8598 行（`1020+1866+3869+1843`）。状态：1 full、3 partial；哈希 4/4 前后一致；未执行任何 build/test/app/网络/git 写；除两个交付文件外未改动任何文件。

**残余未决（需要下一轮或专项）：**

1. `AgentSessions/Resume/AgentTerminalLauncher.swift` 与各 `*ResumeCoordinator/*CommandBuilder/*TerminalLauncher`（约 40 个小文件）未读——"PTY？argv？env？cwd 注入点？"的最终形态、以及 `shellQuoteIfNeeded` 的真实边界（RS-02 的定性）必须在这里闭环。
2. `AgentSessions/Services/PresenceEngine.swift`（约 70 KiB）未读——presence 的真实 loop/探测调度/合并发布与 `debugPerformanceSnapshot` 数据未审计；RS-04 的资源成本数字仍是推导。
3. `CodexOAuthUsageFetcher`、`CodexCLIRPCProbe`、`Resources/codex_status_capture.sh`、`CLIAuthStatusProbe` 未读——RS-03 的网络/令牌成本只按调用点与冷却常量推导。
4. 列表侧滚动定位的 AppKit 协调器未读；`SessionTerminalView` 的搜索/图片其余区间未读（partial 已列账）。
5. 全部结论为静态推导；未运行任何复现。四文件哈希一致只说明审计期间未被修改。