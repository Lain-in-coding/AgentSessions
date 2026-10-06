# Research: agf 0.12.0 本地快照审计——协调停止后的部分交付

- Query: 全文件审计 agf 的扫描、源生命周期、缓存、恢复命令、检索/TUI、安装发布和测试证据，提炼 ASG 规划教训。本分支依主线程协调指令停止继续源扫描，保留实读结果。
- Scope: internal；仅 `Github_src/agf` 的记录快照；本轮没有修改竞品或 ASG 产品。
- Date: 2026-10-06
- Recorded snapshot: `44be9cffd3b3eeb44ff1f31a751ec8954adf14cf`，由派发提供；未执行 Git 验证。
- Snapshot version: `0.12.0`（`Cargo.toml:3`）；CHANGELOG 为该版本记载的日期是 **2026-06-11**（`CHANGELOG.md:3`）。这不是对 2026-10-06 最新上游版本的查询结论。
- State: **partial / coordination stop**，不是全仓审计完成，更不是总任务或 15 个项目范围收缩。

## Findings

### 1. 停止点与真实覆盖

主线程核实另一个 MAIN Codex 会话正在继续完整 agf 审计；主任务 owner 为 `01a10bc1-d181-77e2-ab19-5c3264cffb76`（主线程提供；本子代理未额外检查该会话）。收到停止指令后，本分支只整理两个指定产物，没有继续扫描竞品源码。

| 口径 | 实际覆盖 |
|---|---|
| 快照文件清点 | 34 个；排除本地 `.git/`、`.codegraph/`、`target/` 生成树 |
| 全文已读 | **32 文件 full** |
| 部分已读 | **1 文件 partial**：`src/tui/mod.rs` |
| 整文件未读 | **0 文件 not_read**；不代表没有未读行 |
| 非源码排除 | **1 文件 excluded**：`assets/demo.gif`，仅清点/哈希，未作视觉审查 |
| Rust 候选源 | 24 文件 / 9,283 行；23 full + 1 partial |
| Rust 实读 | **8,778 / 9,283 行**；剩余 **505 行** |
| 全部文本实读 | **10,808 / 11,313 行**；包括完整 Cargo.lock、文档、配置和许可文本 |
| 精确剩余 | **`src/tui/mod.rs:2182–2686`**，必须交给 owner 继续，不得标 full |

最后一份完成回执是 `src/tui/mod.rs:1978–2181`，exec 输出 `1b2569` 在主线程协调停止前完整返回 exit 0。它只读到 `ui_help` 中部；后续行内容及可能存在的渲染、辅助函数、测试，均没有阅读信用。

**回执规则**：伴随 `agf-file-coverage.json` 含每文件行数、初始 SHA256、合并已读区间、逐工具回执、状态和精确剩余区间。CodeGraph 只计入实际显示的 `src/main.rs:83–277`；导航图和符号目录均不计全文。一次 model/config 合并输出被截断，其缺失区间已单独补读；JSON 记录了具体回执，未给截断内容虚假信用。首次粗清点触及预存 target 生成文件以统计行数，后续已排除；未采用其中任何产物作为测试结果。

**哈希边界**：SHA256 来自本分支初始清点，不是 Git 对象校验，也不是读完后的第二次哈希。协调停止后未为补验证而重新读取源码；并发 owner 后续如改动本地源码，应自行复核行号与哈希。

### 2. 关键判断摘要

这版 agf 的优点是入口短、会话卡片与恢复路径直接，且已经处理过多项实际格式和跨平台回归。不能继续把旧的 Cursor TXT-only、Pi 只留最新会话、Windows 一律走 sh、版本升级不失效缓存等已修问题冒充现存缺陷。

但它不是“天然只读的会话定位器”：**普通 Codex 扫描会写源数据库；部分读取失败可能被当成源消失。** 另外，源 session ID 直接进入 shell 字符串、异步刷新与批量删除用数组索引维持选择、缓存把展示裁剪与新鲜度混为一谈，都是 ASG 不应照搬的边界。以下均是实际可达代码路径的静态判断，不是动态复现或最新上游漏洞公告。

### 3. 实际调用入口与可达范围

| 入口 | 实际路径与边界 |
|---|---|
| `resume` / `list` / `stats` | `main.rs:104–185` → `scanner/mod.rs:176–202` → 八个 scanner；普通扫描可进入 Codex prune |
| `watch` | `main.rs:187–189` → `watch.rs:16–58` → 初始及周期 `scan_all`；同样可达 Codex prune |
| 默认 TUI | `main.rs:194–265` → `cache::load_cache/start_stale_scan` → `App::run/ingest_scan_results`（`tui/mod.rs:462–537`）；仅 cache 刷新侧按已安装代理过滤 |
| 原生恢复 | CLI `main.rs:149–163`，TUI `tui/mod.rs:1645–1651` → `action.rs:67–71` → `model.rs:76–87` → `main.rs:319–357` 的文件交付/TTY shell/管道输出 |
| 用户删除 | `tui/mod.rs:1253–1270,1655–1784` → `delete::delete_session` → provider 删除实现；确认默认 Cancel，且成功返回才删 UI row |
| plugin trait | `plugin.rs:99–124` 的 data_sources 被 cache 真实调用；`scan_all` 和 stale worker 直接 match scanner，不经 `PluginAdapter::scan`。不能把 trait 的备用方法当作唯一运行链 |

### 4. 逐文件覆盖与职责

下表和 JSON 都只代表本分支，不合并或覆盖 owner 的账本。first-party 测试与源码同文件内联；Cargo.lock 全文只按依赖元数据审读，不意味着审了第三方实现。

| 路径（相对 agf 根目录） | 总行数 | 状态 / 已读范围 | 职责与注意点 |
|---|---:|---|---|
| `.github/workflows/ci.yml` | 58 | full / 1–58 | Linux/Windows lint、fmt、测试的工作流定义。 |
| `.github/workflows/release.yml` | 135 | full / 1–135 | 版本标签校验、四平台构建打包、校验和及可跳过的 crates.io 发布。 |
| `.gitignore` | 11 | full / 1–11 | 构建、编辑器及本地代理状态排除项。 |
| `assets/demo.gif` | — | excluded / 无源码行 | 二进制演示图；无可计源码行，仅初始哈希。 |
| `Cargo.lock` | 1278 | full / 1–1278 | 完整生成锁文件；固定依赖版本与校验和，不审依赖源码。 |
| `Cargo.toml` | 36 | full / 1–36 | 包版本、Rust 最低版本、依赖和 release panic 策略。 |
| `CHANGELOG.md` | 264 | full / 1–264 | 历史功能/回归/性能声明；与现有代码区分，不采作实测。 |
| `homebrew/agf.rb` | 29 | full / 1–29 | 固定版本与下载哈希、架构选择、仅 help 的 formula test。 |
| `LICENSE` | 21 | full / 1–21 | MIT 许可文本；本研究未复制竞品代码。 |
| `README.md` | 198 | full / 1–198 | 安装首用、provider/路径声明、按键、配置与 shell 说明。 |
| `src/action.rs` | 79 | full / 1–79 | 操作/恢复/新建命令组合与 editor 选择。 |
| `src/cache.rs` | 440 | full / 1–440 | 每代理缓存、版本与 mtime、流式扫描、持久化及测试。 |
| `src/config.rs` | 158 | full / 1–158 | provider 默认目录、PATH/PATHEXT 发现与平台测试。 |
| `src/delete.rs` | 616 | full / 1–616 | provider 原生删除、JSONL 重写和局部回归测试。 |
| `src/error.rs` | 16 | full / 1–16 | IO/JSON/SQLite/home 错误类型；无 partial-scan 结果模型。 |
| `src/fuzzy.rs` | 85 | full / 1–85 | 对预筛选索引做 nucleo 匹配与可选摘要拼接。 |
| `src/list.rs` | 215 | full / 1–215 | 表格/JSON/CSV 输出、agent 筛选、字符截断。 |
| `src/main.rs` | 362 | full / 1–362 | 全部 CLI 入口、TUI 装配、终端清理与命令交付。 |
| `src/model.rs` | 290 | full / 1–290 | provider/session 模型、恢复命令和权限模式、search_text。 |
| `src/plugin.rs` | 126 | full / 1–126 | 注册适配器和缓存源路径；部分 trait 方法为备用。 |
| `src/scanner/claude.rs` | 392 | full / 1–392 | history、transcript 存在性、recap/worktree/current branch 与测试。 |
| `src/scanner/codex.rs` | 612 | full / 1–612 | live-set、SQLite 主源、JSONL fallback、源 DB prune 与测试。 |
| `src/scanner/cursor_agent.rs` | 786 | full / 1–786 | 双布局、store.db、现存路径解码、prompt 抽取与回归测试。 |
| `src/scanner/gemini.rs` | 311 | full / 1–311 | 项目映射、ID 去重、64 KiB 读取与字符串 fallback。 |
| `src/scanner/hermes.rs` | 242 | full / 1–242 | 根会话、活动时间/标题/用户预览聚合、空 cwd 与测试。 |
| `src/scanner/kiro.rs` | 93 | full / 1–93 | conversations_v2 只读查询与首条用户摘要。 |
| `src/scanner/mod.rs` | 321 | full / 1–321 | 八 provider 调度、共享有界读取和字符处理测试。 |
| `src/scanner/opencode.rs` | 70 | full / 1–70 | 未归档 root session 及 child title 的只读聚合。 |
| `src/scanner/pi.rs` | 319 | full / 1–319 | header/user-message、累计预算、坏行恢复与多会话测试。 |
| `src/settings.rs` | 122 | full / 1–122 | 默认值、配置诊断与 merge-save。 |
| `src/shell.rs` | 465 | full / 1–465 | 两类 shell 编码、setup/profile、wrapper 与平台选择测试。 |
| `src/stats.rs` | 276 | full / 1–276 | 按 provider/项目名称计数与互斥活动时间桶。 |
| `src/tui/mod.rs` | 2686 | partial / 1–2181 | 仅 1–2181：状态/filter/sort/ingest、操作/删除/预览、部分 help。 |
| `src/watch.rs` | 201 | full / 1–201 | 周期扫描互斥、游标夹取、provider 级 pgrep 状态与渲染。 |

## 已审 provider 主链与关键结论（静态证据）

### AGF-01 — 高：普通扫描主动删除 Codex 源数据库，且“不完整枚举”保护不充分

**实际可达**：main 的 list/stats/resume → scanner::scan_all → codex::scan（src/main.rs:111,170,183；src/scanner/mod.rs:179）；默认 TUI 刷新 → start_stale_scan → codex::scan（src/cache.rs:299–327）。`scan` 先 collect_live_session_ids，再 scan_sqlite（src/scanner/codex.rs:25–38）；发现未进入 live 集的 active/nonempty-cwd row 就收集 ID（156–160,180–195），关闭只读连接后调用 prune（228–234）。prune 会以可写连接打开每一个 state_*.sqlite，事务内按参数化 ID 删除 threads 并提交（100–127）。这不是不可达 helper，也不是只有用户点击 Delete 才发生。

**具体缺口**：逐文件 read_first_line/JSON 失败被直接跳过（75–87）；有遍历错误但至少读到一个 ID 时依然返回 Some(部分集合)，仅 `had_walk_error && ids.is_empty()` 才返回 None（88–95）；缺 sessions 目录视作可信空集合（60–63）。因此“一个文件无法读/首行不完整/超过读取上限”或“部分子目录无法遍历但其他文件读成功”均可把本来存在的源 row 当孤儿删除；无需假设目录彻底不可读。删除跨多个 DB 且逐行 execute/commit 错误静默忽略，没有跨文件 rollback，也没有用户确认。仅静态可达推论，未对真实 Codex 数据执行。

**反证/已有保护**：SQL 参数化；每库事务；全局遍历失败且集合空时保留 row；只读主查询；正常孤儿过滤目的合理。测试只覆盖正确指定 ID、多 DB、缺表、目录缺失、正常 payload 和 history 预过滤（462–610）；该文件测试未覆盖部分 walk/read/parse 失败引发的误判；未审 TUI 尾段不作否定推断。

**披露反证**：`CHANGELOG.md:136` 已明确说明 Codex 扫描会硬删孤儿行；本发现不是指控该行为完全未披露，而是指出 live-set 完整性判断与普通读取权限边界不足。

**ASG 教训**：发现/检索绝不能隐式写 provider 源；完整扫描与 partial/error 必须是不同结果。即使清理自有索引也要依赖完整代际/源证据，而非“没看到即删除”。源码只证明这版 agf 的问题，不宣称当前上游/provider 格式仍相同。

### AGF-02 — 高（有前提）：会话 ID 到 shell 的边界没有编码

**可达**：Pi parse_session 把 header.id 原样给 Session（src/scanner/pi.rs:104–128），Gemini 的 JSON sessionId 同理（src/scanner/gemini.rs:129–143），OpenCode/Hermes 的 DB id 同理（src/scanner/opencode.rs:33–61；src/scanner/hermes.rs:111–137,185–193）。main resume → action::resume_with_flags → Agent::resume_cmd → deliver_command（src/main.rs:149–163,319–357；src/action.rs:67–71；src/model.rs:76–87）。七个 provider ID 插进原始单引号模板，无转义/类型校验；单引号可破坏参数边界。项目路径已有 POSIX/PowerShell 分别转义的 quote 函数（src/shell.rs:64–94），并不覆盖 ID。

**影响与前提**：需要源元数据/导入会话/缓存中出现包含特殊字符的 ID，用户再选中恢复；能造成命令解析错误，恶意 ID 有 shell 注入风险。不是“输入任意搜索词立即远程执行”。普通生成 UUID 降低发生概率，不构成代码上的约束。Kiro 特例完全不使用 ID（src/model.rs:82–84）。$EDITOR/配置 editor 为用户显式可信命令，不将它和源 ID 混为一谈。

**已有保护**：项目路径引号的两类回归测试（src/shell.rs:333–354）；非交互直接调用不执行、只输出；shell wrapper 要求子进程退出码零才 eval（246–327）。但使用 wrapper 的正常交互路径会 eval/Invoke-Expression，裸 TTY 会 shell exec，故漏洞没有被“只输出字符串”完全隔断。ASG 应采用 typed intent + argv/cwd 分离；需要 shell 集成时统一编码每个不可信参数。

### AGF-03 — 中高：缓存新鲜度不是源状态，且剪裁后的视图会成为缓存

src/cache.rs:119–140 使用深度 4 的全局最大秒级 mtime；201–208 的 fresh 条件是旧值 >= 当前值且当前 > 0。plugin 的源集合并不覆盖所有扫描输入：Claude 只有 history.jsonl（src/plugin.rs:102–104），而 scanner 同时读 projects JSONL 和实际项目 .git/HEAD（src/scanner/claude.rs:223–225,297–322）；Gemini 只有 tmp（src/plugin.rs:118–120），但还依赖 projects.json（src/scanner/gemini.rs:78–101）；OpenCode/Kiro/Hermes 只列主 DB（src/plugin.rs:106–114,121–123），未列旁边的 WAL。故“项目日志/映射更新但监听源 mtime 不变”的过期显示在代码上成立；源消失/时间回退、同秒更新也不保证触发。不能笼统说所有删除都漏：目录 mtime 或其他监听文件增加仍会触发。

刷新后写缓存时才重新采 mtime（src/cache.rs:258–269），不是扫描时的同一指纹；TUI 运行期间后续变化可被旧 Session 数据盖上新鲜时间。scan Err 被 unwrap_or_default 转空（319–326）；无 Result/freshness 字段的 ScanResult（287–291）不能告诉 UI 是空还是失败。write_cache 只保留 10 个 summary（91–102），main 对 max_sessions 剪裁后才传给它（src/main.rs:223–227,261–265），因此会把展示限制持久化为“全量且新鲜”。

**反证**：版本+schema失效与未结束 worker 保留旧缓存都已修；成功 scan 的重建可清除真正孤儿；cache tmp+rename 比直接覆盖好，但固定 tmp 名没有多进程并发协调，写失败静默。ASG 学异步可感知首屏，不学按秒最大 mtime、混淆失败为空、把视图当完整缓存。

### 其余 provider 结构与边界

- Claude：history.jsonl 有真正 transcript ID 才进入列表，孤儿过滤已存在（src/scanner/claude.rs:223–254,288–295）。元数据读取上限 16 KiB head + 256 KiB tail（23–24,97–129），摘要来自 history，全量保留已存在 ID 的记录并按新到旧排（273–277,317–320）。不能称全文消息索引。分支是当前根项目分支而不是会话时点，worktree 标记只识别 POSIX `/.claude/worktrees/`（148–156,200–213,305–315）。
- Codex：主 DB 只选字典序最大的 state_*.sqlite（239–252），不是数字版本或 mtime；到两位后如 state_9/state_10 的顺序会反直觉。SQLite 非空即不补 JSONL（37–43），混合存储中未进 DB 的日志不会合并；JSONL fallback 不包含 archived 过滤（255–349），DB 合法空集合也会启动 fallback。边界不能等同于所有 provider 版本上的确定故障。
- Cursor：深度 3 txt + 深度 4 同名目录/jsonl；JSONL 要求存在 matching store.db，TXT 不要求（src/scanner/cursor_agent.rs:40–137）。store.db 存在但不可解析依然保留会话、可用第一 prompt 补预览（139–155；测试618–647），所以“存在”不是经过验证的可恢复性。timestamp 优先 createdAt 而非最后活跃（218–224）。dash 路径从 `/` 做存在性回溯，Windows drive/UNC、非现存目录没有完整证据；相关真实目录 fixture 大量仅 Unix（322–398,423–555,716–754）。
- Pi：保留同项目多个 ID 的修复与测试已存在（src/scanner/pi.rs:53–55,274–318）；summary 是读取顺序中最初的若干用户内容，并非 newest-first（67–91,185–204）；timestamp 首选 header timestamp（104–119）。512 KiB 是读完整行后的累计阈值（67–95），不能限制单条超大/无换行记录的读取。Cursor 同样先由 lines() 产出行才检查上限（src/scanner/cursor_agent.rs:254–268）；不要误称其为严格字节预算。共享 read_first_line 真用了 take(512 KiB)（src/scanner/mod.rs:25–44），是有效反证。
- Gemini：projects.json 映射缺失/未知项目跳过（src/scanner/gemini.rs:46–50,78–117）；同 ID 以 lastUpdated 合并命名/哈希目录（29–67）。64 KiB 的大文件路径有真实读取上限和 UTF-8 处理（172–190），但 fallback 只接受紧凑无空格的键值字符串、不做 JSON 字符串反转义（224–267），小文件完整 JSON 解析兼容性更好；不是整个 Gemini parser 都不支持 whitespace。最后更新时间仅在字段可解析时生效。
- OpenCode：SQLite 只读，未归档 root session 与直接 child titles 聚合（src/scanner/opencode.rs:15–31）；不是消息全文，也没有本文件内测试。Hermes：root sessions + message last_active + child titles + 至多 4 个最初 user messages（src/scanner/hermes.rs:85–108）；CLI ID 前缀只决定是否展示预览，不过滤其他 session，也不校验用于 shell 的 ID（34–47,154–164,185–193）。
- Kiro：只读 conversations_v2 表，首 user 内容截为100字符（src/scanner/kiro.rs:15–63,66–93），但 Resume 会恢复 cwd 的 latest 而不使用被选 ID（src/model.rs:82–84）。列表选择语义和“特定会话恢复”不是一回事；本快照未连接 native CLI验证。


## TUI、删除、命令交付及产品接口补充

### AGF-04 — 高（竞态条件）：异步刷新会让批量删除的索引指向另一场会话

**可达路径**：每帧在所有模式的输入处理前，`App::run` 都调用 ingest（`src/tui/mod.rs:514–534`）；新扫描结果被 merge，然后 apply_sort 重排 sessions（462–510）。BulkDelete 的 Space 保存的是数组 `usize` 索引，不是 `(agent, session_id)`（1680–1685）；确认时又直接用这些旧索引访问并调用原生删除（1756–1770）。期间没有重建 selected_set 的身份绑定，也没有在删除确认模式暂停 ingest。正常条件是：先从已显示会话中勾选，另一个 stale provider 随后完成并插入更靠前的会话，然后用户确认。

**影响**：勾选目标可以静态推导为漂移到别的真实 Session，风险不止是显示抖动。分组也保存数组索引（349–369），仅进入 GroupedBrowse 时 build_groups（642–646）；后台排序后仍沿旧组索引取会话/开启菜单（835–865,911–979），会把行归到旧项目。没有运行时复现；不宣称所有刷新或所有删除都会错。

**反证/已有防护**：确认默认 Cancel（1267–1269,1692–1694）；批量按倒序删，且有 idx<len 边界与 is_ok 检查（1759–1769）；apply_sort 会试图恢复单选 pivot（194–240）。这些能避免常见越界和失败假消失，但不能保持批量索引身份；pivot 只按裸 session_id 恢复，pin/summary offset 也没有 provider namespace（223–237,296–305,1271–1280）。此处不猜测未读 TUI 尾段是否有相关间接测试。

**ASG 教训**：异步列表的选择/确认必须绑定稳定、带 provider 域的 ID 与代际；执行前重新核对，不能把可变数组位置当授权对象。建议与恢复 intent、freshness 契约一起设计，不是本轮实施授权。

### AGF-05 — 中高：UI 的“成功才移除”已修，但 provider 删除不等于完整/原子删除

**入口**：TUI `ui_delete_confirm` → `delete::delete_session`；UI 仅用 is_ok 判断成功（`src/tui/mod.rs:1756–1784`）。这个修复真实存在（也见 `CHANGELOG.md:14`），不能再次报告成“所有失败都无条件隐藏”。问题在于部分 helper 仍可能 no-op/部分成功后返回 Ok：

| 子路径 | 具体静态证据 / 后果 |
|---|---|
| Claude | scanner 接受 `projects/*/<id>.jsonl`（`scanner/claude.rs:47–78`），delete 只改 history 并删除同名目录（`delete.rs:80–108`），不删除该普通 JSONL 文件。列表消失不代表原始 transcript 被清除 |
| Pi | scanner 可以越过坏 UTF-8 行、在后面捕获 header（`scanner/pi.rs:67–99,237–271`）；delete 却全文件 read_to_string 并只检查字面第一行（`delete.rs:266–290`）。存在可列出但删除跳过、最终仍 Ok 的文件形态 |
| Gemini | scanner 合并命名/哈希目录同 ID（`scanner/gemini.rs:29–67`）；delete 找到第一份就返回（`delete.rs:368–396`）。另一个副本仍可在重扫后出现 |
| Codex | SQLite 删除的 open/execute 错误被吞（`delete.rs:164–184`），随后依次删首个匹配 rollout、重写 history（137–155,188–216），没有跨三种源的事务；未找到文件也返回 Ok |
| 共享 history 重写 | `delete.rs:36–59` 先读全文件后直接覆盖；没有锁/源指纹/临时文件原子替换。若 provider 同时 append，存在丢掉新写内容的条件性风险 |

**反证**：SQL ID 使用参数化；Cursor 已覆盖 JSONL 目录与 legacy TXT 两种删除且有 sibling 保留测试（`delete.rs:335–350,563–615`）；Hermes 的四条 DB 删除在一个事务内并传播错误（409–449）。Hermes dump 清理为显式 best-effort，且按 ID 前缀匹配（451–465），不能宣称同目录备份全部精确清除。总删除入口只有 debug_assert 而非 release 输入校验（12–18）；本轮不把这单独夸大成任意路径删除漏洞。

**ASG 教训**：发现/索引与破坏性源操作分权。若以后提供原生删除，要单独授权、明确“移除索引/移到回收/删除所有副本”的语义，维护源位置与成功/失败/跳过清单；保留失败目标和可恢复边界，而不是只看 bool。

### AGF-06 — 中：没有 wrapper 的 Hermes 恢复与 shell/path 支持有具体缺口

- Hermes scanner 真实返回空 project_path（`src/scanner/hermes.rs:185–193`）；`cd_and` 对空路径返回纯 agent 命令（`src/shell.rs:82–94`），但 `is_cd_only` 只看有没有连接分隔符（104–111），将这个真实恢复命令误判为 cd-only。`deliver_command` 在 TTY 检查之前走该分支、告警并打印而不执行（`src/main.rs:319–338`）。**默认已装 wrapper 时不受该分支影响**：AGF_CMD_FILE 优先写文件并由 wrapper 执行；不是“所有 Hermes 无法恢复”。
- project_path 单引号已按 POSIX/PowerShell 编码（`shell.rs:64–94`），但 PowerShell 的生成文本为未带 `-LiteralPath` 的 Set-Location（92,100）。字面路径合同对 `[]` 等特殊路径不完整；本轮不运行 shell 验证其在各平台的实际结果。
- Windows 默认 shell、MSYSTEM/SHELL 分类以及路径 basename/大小写处理有实现和测试（`shell.rs:21–62,368–449`），不能重报“Windows 一律 sh”。但 setup 的自动识别只匹配 zsh/bash/fish，PowerShell 仅是 Windows 且 SHELL 为空时的分支（129–170）；手工 `init powershell/pwsh` 是另一条支持路径（234–243）。README:148–160 的“自动检测”与“手工支持”需要区分。
- POSIX wrapper 最后执行临时文件清理且没有显式 return 保存的 ret（`shell.rs:246–287`）；主命令错误状态可能被清理状态掩盖。PowerShell wrapper finally 会删除两个环境变量而非恢复调用前值（302–327）。这些是 wrapper 生命周期边界，不是对配置中可信 editor 字符串的注入指控。
- provider 根路径由 `config.rs:8–46` 固定，未读取相应 provider 自定义 home 环境变量；OpenCode 直接拼 `.local/share/opencode`。这只说明快照没有可配置数据根，不等于本轮验证了每款原生 CLI 的目录协议。

**ASG 教训**：用明确的动作类型区分 cd-only/launch；统一 argv/cwd 与 shell 输出编码；平台和自定义数据根契约分别验收。对于不带 ID 的 Kiro，README:51 已诚实披露“cwd 最新”，应将其作为能力降级而非精确恢复承诺。

### AGF-07 — 中：新鲜度、错误、搜索范围和统计口径需向用户讲清楚

**缓存与进度补证**：`load_cache` 只加载 fresh bucket，stale bucket 被排队而未送入首屏（`src/cache.rs:195–213`），与 `main.rs:203–205` 的“即使 stale 也立即展示”注释不一致。冷缓存/无效版本返回全部八个 stale agent（cache.rs:174–185），worker 再按 installed 过滤（305–311），而 main 的 scanning_agents 仍取过滤前集合（main.rs:215–221）。TUI 只在收到结果时 remove agent，Disconnected 不清空剩余集合（tui/mod.rs:462–487）；因此未安装代理也可永远留在 scanning 指示（743–746）。这是冷缓存可达路径，不是必须 worker panic 才发生。

**错误边界**：ScanResult 无错误或完整性状态，scanner Err 转空结果（cache.rs:287–291,318–327）；scan_all 的 join panic 仅 debug 日志（scanner/mod.rs:189–196）。release 明确 `panic="abort"`（Cargo.toml:31–36），不能把 join 错误分支或 RAII 正常退出清理当作 release panic 隔离保证。CHANGELOG:174 的“非致命”仅是作者历史陈述。cache/config 的固定 tmp+rename 与错误忽略（cache.rs:279–284；settings.rs:109–113）有正常路径保护，但没有证明多进程并发不会互相覆盖。

**检索与预览**：CLI resume 帮助含 summary（main.rs:46），实际固定 `include_summaries=false`（117），不加载 search_scope 配置；TUI 则在 filter 时用设置值（tui/mod.rs:244–268）。search_text 仅项目名/路径、可选前 N summary、分支，没有 ID/recap/完整消息（model.rs:227–239）。agent 预筛选在 fuzzy 之前，是正面证据；匹配采用 UTF32，不应据此虚构中文质量 benchmark。Preview 的 History 只是已有 summaries 的逐条宽度截断（tui/mod.rs:2015–2025），不是可追溯全文消息/命中上下文；控制台 summary 数量上限和缓存只保存10项会改变跨启动可见范围（cache.rs:97）。help 中修改数量后只 save、不 update_filter（tui/mod.rs:2075–2083），需后续触发重新过滤。

**输出与统计**：未知 list format 静默回退表格（list.rs:13–20）；CSV 只对 project/path 统一 escape，ID/branch 直接插入（168–188），对源中含 CSV 特殊字符的值缺少一致编码。JSON 有 serializer 保障但不是 ASG 的版本化分页/错误协议。stats 按 project_name 合并，而非完整路径（stats.rs:119–135,224–227）；“Last 7d / Last 30d”代码实际是排除更近桶后的互斥区间（173–199,239–260），并非累计窗口。Cursor/Pi/Claude/Codex 的 timestamp 含义也不一致，不能把所有时间排序称为“最后消息活跃度”。

**watch 反证及限制**：AtomicBool 防重叠扫描、刷新后游标夹取均已实现（watch.rs:35–58），不是旧 thread leak。状态探测统一运行 `pgrep -x`（180–191），对缺该工具的系统直接得到空集合；同 agent 全部会话使用同一个“运行中”标记（131–139），没有验证某一 Session 正在运行。它的周期扫描也会触发 AGF-01 的写源行为。

**ASG 教训**：明确 metadata finder 与全文证据检索两种工作量；将 stale/partial/error 与零命中分开，窗口统计和 provider 时间字段标注语义。沿用既有规范化错误合同，不为“好上手”丢掉机器可判定性。

## 安装、发布与测试证据

- **已有可取机制**：README:15–29 首用链短；Cargo.toml:3–6 同时声明包版本、edition 2024、Rust 1.88。CI 有 Linux clippy/fmt/test 和 Windows clippy/test（`.github/workflows/ci.yml:14–58`），并用 --locked、缓存和取消过期 run。release 校验 tag/package 一致（release.yml:12–24）、构建四个平台并生成四份归档的 SHA256（26–116）。homebrew 版本也是 0.12.0，并固定下载哈希（homebrew/agf.rb:4–19）。
- **仍需 owner/后续验证**：release 的 needs 链只要求 verify→build→release→publish，未直接依赖 lint/test jobs；这不证明实际发布绕过了所有分支保护。CI 用 stable 而非显式 1.88，最低版本声明未在所见 workflow 中单独验收。没有观察实际 CI run、下载归档验证哈希、试装或跨 OS 恢复。
- **发行口径**：矩阵是 macOS ARM/x86_64、Linux x86_64 GNU、Windows x86_64 MSVC（release.yml:32–44）；不是所有 OS/架构。Homebrew Linux 分支没有 ARM 路径（agf.rb:17–20）。crates.io token 缺失会明确 exit 0 跳过 publish（release.yml:125–135）；因此绿色 release 不能单独证明 registry 上线。
- **测试不是零，但覆盖不能夸大**：已全文读到 cache 版本/序列化、shell 选择/引用、共享 reader 边界、Codex prune/history、Cursor 双布局/错误文本/路径、Pi 坏行/多会话、Hermes helper、delete Codex/Cursor 等内联测试。Windows 真实目录类 fixture 有 Unix gates（如 scanner/cursor_agent.rs:373–398；scanner/pi.rs:274–318）。Homebrew test 只查 --help（agf.rb:26–28）。CHANGELOG:31 宣称 75 tests，是历史文本，不是本轮执行/计数结果；TUI:2182–2686 未读，**本分支不能给全仓测试总数、全仓“没有某测试”、全部通过或完整 UI 覆盖的结论**。
- **性能声明不采作本轮实测**：CHANGELOG:98–103、148、191 的倍数/耗时来自作者或依赖升级叙述。源码确认存在少 clone、bounded read、后台按 agent 流式刷新等机制，但没有同机同负载 benchmark，更不能与 ASG 全文索引进行数字排名。

## External References（仅记录快照声明，未联网核查）

- 项目来源：Cargo.toml:9 所声明的 `subinium/agf`；UI 来源：README:186–188 的 `subinium/SuperLightTUI`。README:44–67 含各 provider 文档/存储声明，未访问链接，也未执行对应 CLI。
- 锁定依赖：`superlighttui 0.20.1`（Cargo.lock:912–921）、`nucleo 0.5.0` / `nucleo-matcher 0.3.1`（570–588）、`rusqlite 0.40.1`（721–732）/ `libsqlite3-sys 0.38.1`（520–527）、`clap 4.6.1`（158–165）、`toml 1.1.2+spec-1.1.0`（955–966）、`unicode-width 0.2.2`（1012–1015）。这些是本地 lock facts，不是最新推荐或已知漏洞查询。
- Cargo.lock 全文已读只证明版本/依赖图声明；未审 vendored/transitive 实现、外部 API 兼容矩阵、安全公告或真实 runtime 行为。

## Related Specs

- `.trellis/workflow.md:145–190,309–380`：Phase 1 research，结果落本任务 research；不切实施。
- `.trellis/spec/agentsessions-cli/backend/error-handling.md`：ASG 规范化错误、stdout/stderr、版本合同。
- `.trellis/spec/agentsessions-adapters-sqlite/backend/error-handling.md`：busy/source change/schema/partial write 等既有合同。
- `.trellis/spec/agentsessions-provider-codex/backend/quality-guidelines.md` 与 `agentsessions-provider-claude/backend/quality-guidelines.md` 所读版本是占位模板，不能据此宣称已有具体 provider 验收规则。
- 活动任务 `prd.md` 与 `research/competitor-cli-resume.md` 已作为研究上下文读取；旧报告只是阶段链路结论。本轮不审 ASG 产品、不修改其规范，不读 implement.jsonl/check.jsonl。

## 给 ASG 的规划建议（未实施、未替用户批准）

1. **先守住源边界**：普通发现/list/search/watch/resume 生成不得删除 provider 数据；构造 partial walk、单文件读失败、truncated header、WAL 更新、源暂时缺失情形，要求不误清权威数据且向用户报告不完整性。
2. **身份与动作协议一起做**：provider+原生 ID+源定位组成稳定身份；恢复用 intent/argv/cwd，明确精确恢复与 Kiro 式 latest 降级；引号、Unicode、字面路径和错误出口都要可验收。
3. **异步 UX 不越过授权**：fresh/stale/error/result-generation 独立；批量选择和确认以稳定 ID/代际绑定，不让新扫描改变已选破坏性目标；cache 保存完整扫描结果而非 max_sessions 视图。
4. **借鉴短入口，不降低证据口径**：保持开箱可发现会话、可理解的卡片、筛选→预览→下一步；metadata 快捷层明确工作范围，完整检索层保留分页、命中位置、完整性和规范化错误。
5. **把发布/测试证据纳入计划**：最低工具链、真正支持的 OS/架构、provider 格式 fixture 与非破坏性场景分开验收；上游声明、静态判断、动态验证各有标签。

## Caveats / Not Found / Handoff

- **唯一未审源码区间**：`src/tui/mod.rs:2182–2686`（505 行）。停止不是降低完整审计目标；owner 必须接续，不能把此文标题/哈希/32 full 当作 34 文件全部审完。
- **二进制排除**：`assets/demo.gif` 未视觉审查。`.git/`、`.codegraph/`、预存 `target/` 是本地元数据/生成物，不属于 34 个快照文件的 first-party 覆盖；不从 target 推断测试通过。
- **静态限定**：未运行 build/install/test/cargo/resume/model/native agent 命令，没有操作真实会话，没有 Git 操作；旧已修问题已保留反证。尚缺实际 CI/安装、PowerShell/其他 OS 运行、每 provider 当下格式/权限模式、并发和故障动态复现、依赖实现审查。
- **并发与快照限定**：只存初始 SHA256，未做最终源重读/rehash；未验证 supplied commit 与工作树对象一致。后续 owner 引用前应核对本地变化。
- **协作边界**：本分支从未创建或修改 `research/coverage-index.json`、`research/coverage-sessiongrep.json` 或其他覆盖账本；唯一写入是 `research/agf-full-audit.md` 与 `research/agf-file-coverage.json`。未改变任务生命周期/runtime、产品、竞品或规格。owner 与整个 15 项目审计继续有效。

