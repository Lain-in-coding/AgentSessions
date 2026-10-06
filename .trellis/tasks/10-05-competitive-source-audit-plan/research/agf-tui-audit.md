# agf TUI 全量源码审计（单文件子审计）

## 1. 范围、证据等级与覆盖结论

- 日期：2026-10-06；父任务：`10-05-competitive-source-audit-plan`，保持 **planning**。
- 唯一归属源码：`Github_src/agf/src/tui/mod.rs`。已逐行阅读 **2686 / 2686 个实际源码行（100%）**，其中非空行 2484，包含末尾整个内联测试模块和 3 / 3 个测试。
- SHA256：`92E60605FDB51BFCBCC28D927DECE54118CEC3989F7D36A31E46525E90E10AE9`；文件 98266 字节。阅读前、阅读全文后及交付写入前哈希一致。
- 先调用 CodeGraph MCP，指定项目 `Github_src/agf`；该探索调用意外优先返回 `plugin.rs`，不把它算成本子审计的覆盖。随后使用 `codegraph node --file src/tui/mod.rs --offset <N> --limit 120 --path <repo>`，实际读取 23 个有界分段；全部源码分段输出均未截断。精确区间、调用输出标识及哈希见 `coverage-agf-tui.json`。
- CodeGraph 显示 2687 行，是把文件末尾换行后的空切片也编号为 2687；实际第 2686 行为结束花括号。最后一次输出为 2641–2687，本报告实际覆盖计数为 2641–2686，不以尾部空切片虚增源码行数。
- **证据分级**：“源码事实”是本文件直接可见的实现；“静态风险”是列明前提后的控制流/状态推导，均未运行复现；跨文件后果另行注明父审计提供的证据，不能冒充本子审计已读证据。
- **没有执行** cargo test、编译、TUI/native/GUI、resume、新会话、删除、剪贴板或生成的 shell 命令。没有视觉 QA，也没有宣称测试通过。只写本报告与覆盖 JSON，没有启动任务或修改产品/竞品源码。
- 本结论仅表示这个分工文件完整，不表示 agf 全仓或全部 15 个竞品已审完；父任务总体范围不变。

### 全文语义覆盖索引

| 源码范围 | 已审内容 |
|---|---|
| 1–240 | 导入、颜色、10 种 Mode、状态字段、构造器、排序和选择恢复 |
| 241–458 | 过滤映射、所选会话、摘要切换、设置保存、滚动、分组和 agent 过滤循环 |
| 459–547 | 扫描接收器、刷新合并、全局截断、逐帧调度与命令返回 |
| 548–789 | 普通浏览的键盘/鼠标、搜索框、刷新提示、输入同步 |
| 790–1073 | 分组展开、行映射、分组选择和渲染 |
| 1074–1654 | 动作菜单、新会话 agent/权限选择、resume 模式及命令交接 |
| 1655–1921 | 批量选择、删除确认、成功/失败后的本地状态 |
| 1922–2241 | 会话详情、帮助与可编辑设置 |
| 2242–2621 | 页脚、常规/紧凑列表、列宽、高亮、计数递减、Unicode 截断 |
| 2622–2686 | 测试夹具及全部 3 个内联测试 |

## 2. 已确认的能力与状态流

1. **模式和身份存储**：10 种模式覆盖普通/分组浏览、动作、新 agent、权限、resume、单/批量删除、详情和帮助。主数据为 `sessions: Vec<Session>`；过滤列表、分组子项、批量勾选均用数组下标，游标再指向过滤列表。`Github_src/agf/src/tui/mod.rs:24-43`、`:52-101`、`:290-294`。
2. **过滤映射是正确的，不应误报**：先生成 `agent_filtered`，再把 matcher 返回的 `r.index` 映射回 `agent_filtered[r.index]`；这与父任务给定的 FuzzyMatcher 契约一致。空查询保留当前数组顺序，非空查询直接采用 matcher 返回顺序；TUI 不在返回结果上再执行 time/name/agent 排序，实际模糊评分排序由父审计核验。`Github_src/agf/src/tui/mod.rs:244-268`。
3. **排序与 pin**：支持时间、项目名、agent 排序；随后稳定地把 pin 提前，再重新过滤和恢复单选。构造器设置 `Time` 标签，但本身不调用 `apply_sort()`，初始排序是否预先执行属于调用方证明范围。`Github_src/agf/src/tui/mod.rs:149-191`、`:194-240`。
4. **普通浏览**：Enter/点击进入动作；Right/Ctrl-L 进入详情；Ctrl-D 清空旧勾选后进入批删；Ctrl-G 构建分组；Tab/BackTab 按有会话的 agent 循环；Ctrl-S 改排序；Ctrl-U 清空查询；`[`/`]` 调整摘要偏移。保留了 `?`、`[`、`]` 和 Right 等快捷键，不能把它们一概当作可直接输入的查询字符/光标键。`Github_src/agf/src/tui/mod.rs:548-659`、`:769-788`。
5. **分组**：按 `project_path` 的精确字符串合组，可跨 agent；项目标题由路径末段派生。Enter 或 Space 都会展开标题或打开子项动作，Ctrl-L 打开子项详情。退出分组、详情或动作后主要回到普通浏览，并没有统一的“返回原分组模式”栈。`Github_src/agf/src/tui/mod.rs:349-383`、`:791-865`、`:1078-1079`、`:1922-1936`。
6. **resume 入口对齐**：动作菜单的数字键、鼠标和 Enter 都先进入 resume 模式选择，而不是其中某条捷径直接执行。新会话的数字键则直接使用默认 suffix，Enter 才进权限选择；这是可见的输入路径差异，不应把两者当成完全相同流程。`Github_src/agf/src/tui/mod.rs:1098-1145`、`:1318-1332`、`:1429-1436`。
7. **显式权限选择**：本文件内置 Claude Code、Codex、Gemini 的权限 label/flags，其他 agent 只有默认项；默认选项索引为 0。这里只确认字符串和交接存在，不背书这些 flags 对任意当前 CLI 版本均有效，也未执行绕过权限的选项。新会话依托当前所选会话路径，本文件没有无会话时的独立创建入口。`Github_src/agf/src/tui/mod.rs:1263-1265`、`:1326-1345`、`:1401-1426`、`:1538-1546`。
8. **删除的正向保护**：单删、批删进入确认时默认 `Cancel`；批删先降序处理下标，避免同一批删除自身导致后续下标平移；只在 `delete_session(...).is_ok()` 时移除行并递减计数，失败行不会仅因 UI 乐观更新而消失。单删确认显示 agent、项目、session ID 和路径。`Github_src/agf/src/tui/mod.rs:1267-1269`、`:1692-1694`、`:1756-1782`、`:1810-1824`、`:2569-2578`。
9. **详情边界**：展示 Session 已有字段、recap 和逐条截断的 summaries；Up/Down 切换会话而非滚动长正文。本文件没有载入完整 transcript 的逻辑，也没有详情正文滚动状态，不能等同于完整对话查看器。`Github_src/agf/src/tui/mod.rs:1922-2035`。
10. **绘制和静态成本**：普通列表按视口区间生成行；项目列宽在过滤后缓存；主要行构建与截断使用 Unicode 显示宽度，并规范化包含换行/回车/tab 的片段。但紧凑列表使用固定列宽，不显示摘要或 pin 标记；分组每帧仍遍历所有组及其 session 来生成 agent 集合，即使组不在可视区。没有性能 benchmark 或窄屏视觉验证。`Github_src/agf/src/tui/mod.rs:277-285`、`:904-924`、`:2258-2278`、`:2368-2418`、`:2421-2526`、`:2580-2619`。

## 3. 重点补充：max_sessions 对缓存完整性的跨层风险（TUI-12）

**优先级：P1 候选；TUI 数据损失路径已确认，缓存 freshness 后果待父审计合证。不是原始会话文件丢失。**

### 本文件独立证明的链路

- `ingest_scan_results()` 对每个成功收到的结果调用 `merge_agent_sessions(result.agent, result.sessions)`，随后把该 agent 从 `scanning_agents` 移除。`Github_src/agf/src/tui/mod.rs:462-475`。
- 合并先从 **主数据数组**删除该 agent 的旧记录、加入新记录，然后读取 `self.settings.max_sessions`；若为 Some，按时间降序排序并直接执行 **`self.sessions.truncate(max)`**。它不是 `filtered_indices` 上的视图限制，而且会影响所有 agent 的总集合。`Github_src/agf/src/tui/mod.rs:497-511`。
- 收到结果后的收尾仅是 `apply_sort()`，该函数重新排序/过滤现存数组，不会把被截断的尾部取回。`Github_src/agf/src/tui/mod.rs:488-492`、`:194-240`。
- App 没有另外保存一个“未截断的完整 sessions”数组：分组仅持有索引，其他选择/过滤状态也不能恢复被丢弃的 Session。即使单纯把 `settings.max_sessions` 改成 None 再排序，已丢弃的数据也不会凭空恢复；需要新的完整输入/扫描结果。`Github_src/agf/src/tui/mod.rs:38-43`、`:52-101`、`:244-268`。
- `scanning_agents` 跟踪“worker 是否已报告”，不跟踪“该 worker 的所有记录是否仍在数组里”。所以全部 worker 正常报告后，这个集合可以为空，同时 `app.sessions` 已被 cap 截断。`Github_src/agf/src/tui/mod.rs:471-475`、`:504-511`。

### 父审计提供、此子任务没有重读的外层证据

父代理/用户提供：`Github_src/agf/src/main.rs:223-226` 在 App 构造前截断；`:265` 把 `app.sessions` 和 `app.scanning_agents` 交给缓存写入；`Github_src/agf/src/cache.rs:251-269` 只持久化传入会话并记当前源 mtime，缓存版本判定不含 max_sessions。**这些是父审计输入，不计入本文件覆盖，也不冒充自行阅读/运行的结论。**

在该外层契约成立、有关 agent 已完成报告、后续源文件 mtime 没有改变的前提下，TUI 可以把一个“已完成但被截断”的集合交回调用方；缓存会把它保存成 fresh。之后提高/取消显示上限，旧缓存的完整性不会仅因 freshness 检查而恢复。保留未完成 agent 的 pending 机制不能解决“已完成却被 cap 截断”的这条路径。风险是 **缓存/检索结果持续不完整，直到重新全量扫描或缓存失效**，不是竞品在这里删除了原始会话文件。

### 测试证据与尚未执行的回归设计

本文件全部测试都在 `scroll_margin_tests`，仅测滚动偏移；夹具传入 `scan_rx=None`、空 pending 集合和默认设置，没有为合并或 max_sessions 写断言。`Github_src/agf/src/tui/mod.rs:2622-2686`。

建议父任务后续单独批准验证以下案例（**本次没有编写或执行测试**）：

1. cap=2、某 agent 完整扫描返回 3 条记录，调用 ingest 后验证主数组只剩 2 条且该 agent 已不在 pending；这是 TUI 侧风险的最小案例。
2. 随后取消 cap、仅重排/过滤，在没有新扫描输入时仍无法恢复第三条；验证完整模型与显示视图没有分离。
3. 两个 agent 的结果依次返回，验证全局 cap 可挤掉此前已报告 agent 的记录，而 pending 最终仍为空。
4. 缓存写回后只改变 cap、不改变源 mtime，再启动是否仍得到截断集合，由父审计在 cache/main 层验证；不要用上面三个 TUI 案例代替跨启动证明。

比较/设计启示：完整扫描数据与显示上限宜分离；缓存若接受有损子集，需要可检测的完整性契约。这是后续方案候选，不是本轮获批的实现改动。

## 4. 其他发现（均为静态审阅，未运行复现）

### TUI-01 · P1：刷新后才捕获 pivot，单选及动作目标可漂移

- **源码事实**：`ingest_scan_results` 先改 `sessions`，最后才调用 `apply_sort`；而 `apply_sort` 在入口通过旧 `filtered_indices` 和旧 `selected` 读取 pivot ID。这个 pivot 已不是刷新前的选中对象。`Github_src/agf/src/tui/mod.rs:194-197`、`:290-294`、`:471-491`、`:497-505`。
- **最小静态推演**：原数组 `[A(agent a), B(agent b)]`，选中 A，旧过滤索引为 `[0,1]`。a 的刷新仍包含同一 A，但 retain/extend 暂时变成 `[B,A]`；随后 pivot 从旧位置 0 读到 B，即使排序再把 A 放回首行，恢复的仍是 B。此例不需要 session ID 碰撞，也不需要被选会话真的消失。
- **影响**：每帧无论处于哪种模式，都先 ingest 再处理输入；删除确认没有被冻结的目标，`delete_index=Yes` 不因刷新而复位。用户上一帧确认的是 A，下一帧的 Enter 可落到 B。resume 模式选项是在进入时从原 agent 复制的，实际 dispatch 却重读当前 session，还可能出现旧 flags 配新 agent。`Github_src/agf/src/tui/mod.rs:520-533`、`:1105-1109`、`:1645-1651`、`:1729-1782`。
- **建议验证契约**：批量变更前捕获稳定身份；模态动作绑定目标快照，目标变化/消失时取消或重新确认，不能只保留一个数组游标。

### TUI-02 · P1：批量勾选保存数组下标，刷新/重排后可能删错会话

- **源码事实**：`selected_set: HashSet<usize>` 存的是 `sessions` 下标；勾选后后台仍可插入、retain、排序、截断，合并与排序都不重映射这个集合。`Github_src/agf/src/tui/mod.rs:71`、`:194-240`、`:497-511`、`:1680-1689`。
- **静态推演**：按时间原为 `[A,B]`，勾选 B 的下标 1；新 agent 带来更新的 C，排序后 `[C,A,B]`，下标 1 现在是 A。勾选渲染、确认名称与真正删除都重新解释这个旧下标。边界 `.get()` / `<len` 只能防越界，不能防删错有效行。`Github_src/agf/src/tui/mod.rs:1759-1769`、`:1862-1869`、`:2286-2288`。
- 降序删除是对单批处理自身移位的正确保护，但不解决进入确认之前发生的后台重排。建议把勾选集合和确认目标都绑定稳定身份，并覆盖“确认已打开时收到扫描结果”。

### TUI-03 · P1：分组缓存未随刷新重建，可错指甚至越界崩溃

- **源码事实**：进入分组时构建一次 `groups`；后台 ingest 只重排/过滤，不调用 `build_groups`。组里保存裸 session 下标。`Github_src/agf/src/tui/mod.rs:38-43`、`:349-369`、`:488-492`、`:642-646`。
- 渲染组标题的时间和 agent 集合直接访问 `app.sessions[i]` / `app.sessions[idx]`，而且这发生在可见性判断之前。数组缩短后，旧组索引可越界；即使组折叠或不在屏幕内也会走这些读取。长度未缩短时则可能把别的项目会话显示/操作在旧标题下。`Github_src/agf/src/tui/mod.rs:907-927`、`:975-980`。
- 子项动作通过相同数字下标在新过滤列表里寻找位置，也不能证明是原来的会话。`Github_src/agf/src/tui/mod.rs:826-832`、`:856-862`。建议数据版本更新时重建分组，并按稳定 group/session 身份恢复展开和选择。

### TUI-04 · P2：删除成功后，已在途的旧扫描快照可重新加入“幽灵行”

- 删除成功只从当前数组去掉对象；没有刷新代次、已删 ID 集合或未完成扫描结果隔离。随后同 agent 的一个旧快照只要还带着该会话，就会被 `extend(new_sessions)` 加回来。`Github_src/agf/src/tui/mod.rs:497-505`、`:1759-1782`。
- 前提是扫描快照在删除前已包含该项、结果在删除后才被 ingest。这是当前接收/删除次序允许的静态风险，不是已实测竞争。
- **边界**：这里能推导的是 TUI 重新出现过期行；是否进一步写进缓存由父审计处理，不能声称原始文件被恢复。

### TUI-05 · P2：max_sessions 截断后 agent_counts 不再反映实际数组

- 计数在截断之前按新 agent 的完整结果长度更新；全局截断之后没有重新统计受影响的各 agent。`Github_src/agf/src/tui/mod.rs:498-511`。
- 例如已有 B 的 1 条，A 返回更新的 3 条，cap=2 留下 A 的 2 条：计数仍可为 A=3、B=1，而实际数组只有 A 的 2 条。
- badge 使用该计数，agent 过滤循环也靠计数判断“有会话”，因此可能出现数目矛盾或循环到空 agent；删除后的单次 decrement 不能修正先前膨胀的值。`Github_src/agf/src/tui/mod.rs:419-426`、`:700-707`、`:2569-2577`。

### TUI-06 · P2：修改 summary_search_count 不会刷新当前搜索结果

- 改 search_scope 会立即 `update_filter()`；但帮助页的 `+/-` 只改 summary_search_count 并保存设置，没有重新过滤。matcher 明确接收该 count。`Github_src/agf/src/tui/mod.rs:259-265`、`:2064-2083`。
- 如果当前查询只匹配新纳入的第 N 条摘要，调大 count 后直接 Esc 返回，查询文本没有变，浏览页末尾的同步也不会触发过滤。需要额外改查询、排序、scope 或等刷新才能反映新设置。`Github_src/agf/src/tui/mod.rs:769-781`。
- 这是状态依赖没有失效处理的静态缺陷；建议用固定查询与跨 count 边界摘要作回归。

### TUI-07 · P2（条件性）：多 agent 身份只用 session_id，缺少命名空间

- 排序恢复 pivot、pin 判断/保存、摘要偏移均仅以裸 `session_id` 为键；恢复时找到第一个相同 ID 即停止。`Github_src/agf/src/tui/mod.rs:197`、`:223-237`、`:304-312`、`:1271-1279`、`:2331-2342`。
- **前提**：两个 agent 使用了相同 session_id。若发生，pin 会作用到多条、摘要偏移串用、恢复位置可能跨 agent。此文件没有建立全局唯一性保证；本次也没有证明真实本地数据已碰撞。
- 这独立于 TUI-01 的刷新前后捕获错误。父任务给定 pin 配置是 `Vec<String>`；后续若改复合身份，需处理兼容性，不能把本审计当作已批准改配置格式。

### TUI-08 · P2：删除失败缺少结果反馈，批量确认身份信息不足

- `is_ok()` 丢弃错误内容，处理结束回 Browse；批量勾选集合先 drain，失败项虽然保留行，却没有保留失败选择或显示逐项/汇总错误。`Github_src/agf/src/tui/mod.rs:1756-1788`。
- 批量确认只展示排序后的项目名，最多 5 个，再显示剩余数量；不展示 agent、ID 或路径，同项目多会话很难复核。标题数量还来自原下标集合，而名称过滤掉无效下标，二者可能不一致。`Github_src/agf/src/tui/mod.rs:1862-1884`。
- **没有误报成无确认删除**：确认存在且默认 Cancel；“失败行不移除”也是正确保护。缺口在失败可观测性、目标复核与刷新安全，不是缺少确认框。

### TUI-09 · P2 候选（需运行期验证）：鼠标命中和视口高度脱离实际布局

- 普通浏览把所有 `y>=2` 的点击解释为 `scroll_offset + y - 2`，只与整个过滤集合长度比较，没有列表区域的下边界或组件命中矩形；在长列表中，页脚区域坐标也可能通过判断并打开动作。`Github_src/agf/src/tui/mod.rs:674-685`。
- 注释按“搜索一行+分隔线一行”推导列表从 y=2 开始，但实际绘制还在搜索之前加了顶端空行。统一 `viewport_height=height-4` 也没有按各模式实际非列表行数计算。`Github_src/agf/src/tui/mod.rs:521`、`:690-715`、`:730-766`。
- **等级限制**：以上坐标计算/布局不一致可见；具体 slt 布局、终端裁剪和点击效果未做 UI/截图/事件注入验证，因此不冒充已复现视觉错位。

### TUI-10 · P2：分组“最新时间”取首项，不一定是组内最大时间

- 组内条目保持 `filtered_indices` 的插入顺序，但组排序和标题时间都取 `.first()`。这个顺序可能先按 pin、agent 或 matcher 排序，不能保证首项最新。`Github_src/agf/src/tui/mod.rs:352-381`、`:912-916`。
- **无需依赖 matcher 的例子**：项目 P 有被 pin 的旧会话和未 pin 的最新会话，项目 Q 的最新时间介于二者之间。pin 提升让 P 的旧会话排第一，组标题和排序便把旧时间当最新，可能把 Q 放在 P 前面。`Github_src/agf/src/tui/mod.rs:216-225`。
- 建议用组内最大时间独立计算排序键和标题；不要把列表顺序默认等同于项目活跃时间。

### TUI-11 · P3：刷新使摘要列表缩短时，旧偏移可让摘要持续空白

- `summary_offsets` 没有在合并时按新摘要数量校正；列表直接 `summaries.get(offset)`。若 offset=2，刷新后该会话仅剩 1 条摘要，列表得到 None；`cycle_summary` 又在 count<=1 时直接返回，不能把偏移清回 0。`Github_src/agf/src/tui/mod.rs:296-312`、`:497-511`、`:2331-2343`。
- 这是有明确前提的静态展示风险。分组和批删本身始终取第一条/recap，紧凑模式不展示摘要，因此同一会话还可能呈现跨模式差异。

## 5. 剪贴板、shell 与事件处理边界

- **通用动作**：`dispatch_action` 对非特殊项调用 `action::generate_command`，只有返回 `Some(cmd)` 才设置 result 并退出。`None` 时本地分支没有反馈或模式切换。不能仅据函数名假设被调用方无副作用，也不能把 `None` 自动判为成功复制或失败。Action::MENU 的具体内容、copy 语义、转义和跨 shell 行为属于父审计的 action/shell 证据。`Github_src/agf/src/tui/mod.rs:1075-1076`、`:1284-1291`。
- **新建/resume**：调用对应 command helper，返回字符串给 `run()` 调用方；新建预览调用 `CommandShell::from_env().cd_and(...)`。本文件没有直接的剪贴板 API 或进程 spawn 实现；是否在 helper 或主流程执行，须按外层证明。`Github_src/agf/src/tui/mod.rs:514-537`、`:1360-1365`、`:1429-1436`、`:1538-1546`、`:1645-1652`。
- **删除不同**：确认 handler 直接调用 `crate::delete::delete_session`，不是返回 shell 字符串等待外层；因此错误目标会立即传到删除层。实际磁盘删除细节未在本子任务重复审阅。`Github_src/agf/src/tui/mod.rs:1759-1782`。
- **输入消费**：Browse 预先消费和 textarea 冲突的导航/控制键，但很多菜单使用非消费的 key 查询，模式改变后也不立即 return；Preview 和分组退出路径则有 return。若 slt 一帧可同时暴露多类输入，需验证取消/数字键/Enter 是否会在旧 handler 内继续 dispatch；没有框架事件契约或事件测试支撑时，不把“同帧双触发”列为已确认缺陷。`Github_src/agf/src/tui/mod.rs:549-600`、`:1078-1145`、`:1732-1756`、`:1926-1936`。
- **搜索呈现**：匹配 positions 仅用于项目名高亮，path/summary 命中本身不在对应字段绘制高亮；查询更新发生在 Browse 的渲染/按键处理之后，多行文本先取首行同步再合并为一行，存在下一帧同步的边界。这里不额外断言 matcher 坐标单位或终端事件批处理行为。`Github_src/agf/src/tui/mod.rs:769-788`、`:2468-2475`、`:2480-2494`、`:2538-2566`。
- **断连/pending**：channel 断连不再保存 receiver，但只移除真实报告的 agent，未报告者仍留在 `scanning_agents`；Browse 仍可能显示 scanning。不能把这种保留 pending 的行为误诊为“退出时把未完成扫描当成功”，也不能据此推断缓存损坏。实际缓存策略由父代理核验。`Github_src/agf/src/tui/mod.rs:471-486`、`:741-746`。

## 6. 全部内联测试的有效性审阅

测试范围完整读取：`Github_src/agf/src/tui/mod.rs:2622-2686`，65 行，3 个 `#[test]`。以下仅为源码和算术核验，**执行次数为 0**。

| 测试 | 行号 | 静态核验 |
|---|---|---|
| `adjust_scroll_does_not_underflow_when_margin_branch_fires_early` | 2656–2664 | n=8、viewport=10、selected=7 确实进入 margin 分支；饱和计算先得 1，再按 max_offset=0 夹回 0。不是未触发目标分支的空断言。 |
| `adjust_scroll_keeps_margin_rows_below_cursor_mid_list` | 2666–2675 | n=30、viewport=10、selected=12，计算 offset=12+3+1-10=6，断言与实现一致。 |
| `adjust_scroll_clamps_to_list_end_instead_of_overscrolling` | 2677–2685 | n=15、viewport=10、selected=14，初值 8，再按 max_offset=5 截断，断言与实现一致。 |

对照实现：`Github_src/agf/src/tui/mod.rs:328-347`。这三个测试能保护对应滚动边距回归，但不能证明完整键盘流程、可视高度或分组滚动正确。

夹具通过 `App::new` 构造，也会经过其中的 `installed_agents()` 调用；它没有构造扫描通道或启用 pending，不能充当刷新隔离测试。`Github_src/agf/src/tui/mod.rs:121`、`:2626-2650`。

**本文件内没有覆盖**：扫描合并/截断/断连、cap 与缓存完整性、刷新时动作身份、重复 session ID、分组索引重建、勾选与删除结果、删除后在途快照、帮助设置触发搜索、命令/clipboard helper 契约、鼠标命中、Unicode 边界和模式切换。此处只说“本文件”，不声称仓库其他文件也没有测试。

## 7. 供父审计合并的比较结论

- 可确认的竞品能力：会话导航、按 agent 过滤、可切换摘要搜索范围、显式 pin、项目分组、摘要/recap 元数据详情、新建/resume 的模式选择及带确认的单/批量删除。不能把摘要详情宣传为完整 transcript 检索/阅读，也不能把 shell 交接当成已验证的原生执行/剪贴板体验。
- 值得借鉴：正确的 matcher 局部索引映射、显示宽度缓存、显式 pin 优先、默认取消、删除成功后才移除行、降序批删下标和针对真实 underflow 分支的窄测试。
- 不宜照搬：刷新后才捕获选择、以数组下标保持长期动作身份、模态确认继续接受后台结构变更、完整数据与显示 cap 混用、用名称集合代替可辨认删除目标、忽略失败原因。
- 最高优先合证：TUI-12 与父审计的 main/cache 链路；随后是 TUI-01/02/03 对动作正确性和可用性的影响。任何设计修复、回归测试和集成验证均需父任务另行授权，本子任务仍停留在规划研究。
- 未读取 agent-session-grep 产品源码来作最终差距裁决，也未代替其他 14 个竞品或 agf 其他文件的阅读；本文件结果应并入原定全部 15 个项目的证据矩阵，而非缩减总审计范围。
