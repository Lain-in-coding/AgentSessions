# Research: sessiongrep 全文件审读凭证与阶段性静态审计

- Query: 对指定本地 sessiongrep 快照逐文件审读，核查解析保真、生命周期、身份/事务、检索/过滤、CLI/TUI/MCP、安装/发布与测试证据。
- Scope: internal；仅 `Github_src/sessiongrep`，规划研究，不实施整改。
- Date: 2026-10-06
- Recorded snapshot: `c5c1874f420fb20cc55e484f79bdff3d848b71a8`；包内声明版本 `0.1.0`。
- Status: **按主线程协调要求停止扫描；保留阶段性分析，不宣称主审计或整体任务完成。**

## Findings

### 1. 停止边界与实际阅读量

停止前已返回的完整工具输出覆盖全部 17 个 Rust 文件、5089 行。这里的“全文已读”仅描述阅读凭证，不表示每个推导均已复现、兼容性已验证或全部审计工作已收敛。另一 live MAIN Codex 会话继续负责总体审计。

| 口径 | 实际数量 |
|---|---:|
| 排除目录后枚举文件 | 26 |
| `full` | 24 |
| `partial` | 1：`Cargo.lock`，312 / 1660 行 |
| `not_read` | 0；不包含单独列为 excluded 的二进制 |
| `excluded` | 1：`docs/demo.gif`，仅哈希与文件头 |
| 第一方必读文本（含配置、README、LICENSE） | 23 / 23，5612 行 |
| Rust 源码（包含内联测试） | 17 / 17，5089 行 |
| 全部已阅读文本（加完整 flake.lock、部分 Cargo.lock） | 5985 / 7333 行 |
| 静态发现的 `#[test]` 声明 / 本轮执行测试 | 36 / 0 |

**机器凭证：**同目录 `sessiongrep-file-coverage.json` 为每个文件记录总行数、字节数、SHA256、实际可见阅读范围、工具输出 Chunk ID、未读范围、状态与排除理由。24 个 full 文件都有从第 1 行到真实 EOF 的范围并集，不以摘要、AST 大纲或 CodeGraph blast-radius 当全文。

- CodeGraph 首次调用返回 `src/util.rs:108-149,462-499`；文件真实总行数为 498，虚拟 EOF 行 499 不计入。其余缺口由编号源文补齐。
- 第二次 CodeGraph 调用返回 `src/timeline.rs:22-212`；只计这些可见原文，另补 `1-21,213-269`。
- 其他源码通过分段、未截断的 PowerShell 编号输出审读。停止后没有继续扫描源码。
- `.git/HEAD` 与 loose ref 的只读结果符合记录 hash；未运行 git，未以对象比较证明工作树干净。26 个文件在初始清点与停止前最后一次 SHA256 校验之间无变化；这不是对未来修改的保证。
- `.git/`、`.codegraph/`、`target/` 未递归纳入产品审计；分别是版本控制内部、生成索引、构建/依赖缓存。HEAD/ref 和 CodeGraph 是注明用途的只读例外。
- 本 worker 只创建/更新本报告和配套 JSON，**没有创建或修改** `research/coverage-index.json`、`research/coverage-sessiongrep.json`，不接管其他会话产物。

### 2. 全文件清单与一行审计结论

以下路径全部相对于 `Github_src/sessiongrep/`；范围含文件开头、导入、实现与末尾测试，而非只读表中引用段。完整哈希与阅读拆分见 JSON。

| 文件 | 行数 / 状态 | 职责、已审重点与边界 |
|---|---|---|
| `src/main.rs` | 9 / full | CLI 入口；错误写 stderr 并退出 1（4-8），不提供机器错误分类。 |
| `src/lib.rs` | 7 / full | 共用 config/db/indexer/models/providers/timeline/util；CLI 与 MCP 复用后端，不是两套检索实现。 |
| `src/models.rs` | 113 / full | 五 provider 枚举（4-44）；会话与扁平 transcript（48-96），没有消息 ID/父边/字节定位模型；这是定位边界。 |
| `src/providers/mod.rs` | 5 / full | 导出五个 adapter；无动态插件注册。 |
| `src/config.rs` | 266 / full | 默认本地目录与五个 enabled provider（75-119），TOML 空路径回填（155-189），Codex metadata home 固定于 home 下（261-265）；UI 配置存在未消费项。 |
| `src/indexer.rs` | 85 / full | 五源装配、mtime+size 跳过、逐会话写入；full 先 clear（28-81）；没有删除对账、sidecar/version 失效判断或源读后复核。 |
| `src/db.rs` | 640 / full | WAL/schema/FTS 初始化（21-108），单会话四表事务（144-242），候选/评分/过滤/ID 解析（281-575）；唯一测试只测 LIKE 转义 helper（630-640）。 |
| `src/providers/claude.rs` | 316 / full | 排除 memory/subagents（24-65），JSONL + last-prompt + 命令标记清理（74-257）；6 个测试主要验证过滤 helper，不是完整 parser fixture。 |
| `src/providers/codex.rs` | 283 / full | response_item 用户/助手消息、SQLite/index 标题覆盖（92-221）；metadata 失败静默降级（36-39），无本文件测试；事件镜像未重复导入。 |
| `src/providers/cursor.rs` | 372 / full | 只扫 agent-transcripts、排除 subagents（24-71）；正文 tag 提取与 mtime 时间（80-218）；Users 路径组合恢复（221-273）；5 个测试。 |
| `src/providers/antigravity.rs` | 240 / full | transcript.jsonl、上溯三层 ID、source/created_at/tool Cwd 映射（24-205）；普通完成路径无警告；1 个最小 fixture。 |
| `src/providers/pi.rs` | 334 / full | 默认/项目目录两种浅层发现（30-71,208-219）；session/message 解析、文件名 ID 回退（80-205）；3 个 fixture，主动跳过 thinking/toolResult。 |
| `src/util.rs` | 498 / full | 文本抽取、显示与 UTF-8 边界、失败记录、repo/worktree、resume/PATH（13-427）；6 个测试，含 worktree/submodule 反例。 |
| `src/cli.rs` | 508 / full | 9 个命令、自动索引、过滤与展示、受确认的 argv 恢复、三类 export、doctor/paths（34-507）；无内联测试。 |
| `src/mcp.rs` | 465 / full | stdio JSON-RPC、1500ms 索引节流、5 个工具（19-447）；1 个 tools/list 名称测试，不是协议/预算端到端验证。 |
| `src/timeline.rs` | 269 / full | UTC 日期分桶、metadata-only timeline、稳定 ID tie-break 与 1..200 限制（8-144）；8 个 helper 测试。 |
| `src/tui.rs` | 679 / full | raw/alternate screen 生命周期（27-64），逐键检索与预览（66-369），首尾回合摘要（377-497），渲染和 5 个测试（500-679）。 |
| `Cargo.toml` | 38 / full | Rust 2021、0.1.0、Apache-2.0 声明、两个 binary；bundled SQLite 依赖，但没有 rust-version/MSRV 声明。 |
| `.github/workflows/ci.yml` | 30 / full | Ubuntu + stable、check --all-targets --locked 与 test --locked（15-30）；只有流程定义，没有本轮运行结果。 |
| `.gitignore` | 4 / full | 忽略 target 与 .DS_Store，不代表 target 内容已审。 |
| `flake.nix` | 78 / full | 从 Cargo.toml 取版本、Cargo.lock 锁依赖、两个应用入口和 dev shell（20-73）；未执行 Nix。 |
| `flake.lock` | 61 / full | 生成的 Nix 输入锁；全文已看，包括三个 revision/hash，不代表验证过上游内容。 |
| `Cargo.lock` | 1660 / partial | 生成依赖数据；只看 312 行包版本/依赖关系/校验字段，其余 1348 行精确列账，不声称依赖安全审计。 |
| `README.md` | 172 / full | 首用、自动索引、MCP、配置、隐私、限制与预发布说明；声明与实现分开，见 SG-10。 |
| `LICENSE` | 201 / full | 文件声明 Apache License 2.0，189 行载明版权；本报告不复制实现、不作法律合规结论。 |
| `docs/demo.gif` | 非文本 / excluded | 1371110 字节，GIF89a 头部显示 1320×760；未逐帧/播放/视觉审查。README:13 仅声明演示为脱敏样本，不是已核验事实。 |

### 3. 端到端形态与 provider 保真边界

实际入口链为：CLI 参数 → Config → Db::open → 除 reindex/paths 外自动 incremental → 读取/展示；MCP 启动索引及 tools/call 节流刷新 → 同一 Db；TUI 从 CLI 启动，之后的 refresh 只查询 DB，不重新扫描磁盘。证据：`src/cli.rs:99-179`、`src/mcp.rs:21-90,219-280`、`src/tui.rs:295-313`。

| provider | 本地源码支持的形状、身份、时间 | 保真与验证限制 |
|---|---|---|
| Claude | JSONL；sessionId 覆盖文件 stem；message.role/type 的 user/assistant；首个及最后处理到的消息时间；last-prompt 优先标题（74-211）。 | 无 uuid/parentUuid/isSidechain 消息图；坏行跳过；命令清理可误伤普通文字。排除 subagents 是 README:160 明示取舍，不当成漏实现。 |
| Codex | session_meta + response_item/message，角色限 user/assistant；头部 ID 优先、文件名 regex 回退；外层消息 timestamp、state_5/index 标题（92-221）。 | 正确不导入 event_msg 镜像；developer/system、reasoning、turn_context 不进入正文。固定 metadata schema、无版本 fixture；不能宣称兼容所有 Codex 版本。 |
| Cursor | 顶层 role + message.content；文件 stem ID；最近用户文本作为标题；所有消息显示文件 mtime（80-165）。 | mtime 不是逐消息时间；只认 Users 编码的存在路径，不能推断 Linux/Windows CWD 完整；user_query 提取与工具过滤是有意简化。 |
| Antigravity | 固定 transcript.jsonl 名称，上溯三层取 conversation ID；USER/USER_EXPLICIT/MODEL；created_at，首个工具 Cwd（24-205）。 | discovery 不验证三层目录结构；未知 source 不入正文，但其时间可影响会话时间；没有真实格式版本矩阵。 |
| Pi | type=session 提供 id/cwd；type=message 提取 user/assistant；无 ID 则从 stem 抽 UUID；深度 1/2（80-219）。 | 版本、消息 id/parent 等不参与重建；非 user/assistant 及更深 subagent 主动排除。三个本地 fixture 的 version=3 不证明所有 Pi 版本。 |

五个 parser 都是整文件读入而非逐流解析；正常路径把原文重排为按角色/时间分段的字符串。raw_metadata_json 主要保存计数/路径，不是原始 transcript 的完整保真备份（各 provider record 构造及 `src/util.rs:161-200,322-326`）。这适合会话召回，但不能直接充当 ASG 的稳定消息/placement/证据定位契约。

### 4. 具体风险、反证与实际可达性

严重性是本次研究的排序：P1=核心正确性优先核查；P2=有条件的正确性/可用性/边界风险；P3=较低优先级或证据欠缺；I=已知产品取舍。**以下均为静态推导，不是已运行复现，也不是所有发现都要求竞品增加功能。**

#### SG-01 · P1 · FTS 全局截断发生在 provider/path/since 过滤前

- **证据：**`src/db.rs:287-295,395-423,427-477` 先全库取 limit×5 个 FTS ID，再在这些 ID 内施加过滤。只有最初 FTS ID 列表为空才走全库 fuzzy。
- **可达：**CLI search 的正常 provider/path/since 参数及 MCP provider filter。全局候选被其他 provider/路径占满时，过滤后可以返回零条，即使被筛域内还有合法匹配；这不是只涉及恶意输入的情形。
- **反证/界限：**无过滤且命中位于候选内时可正常返回；FTS 空集会 fuzzy；timeline 已先在 SQL 做路径过滤，不能归入此问题。
- **ASG 教训：**资格过滤在 cap 前完成，并用“域外高排名 > cap、域内仍有命中”的 fixture 验收。不把多扩大几倍候选当成正确性证明。

#### SG-02 · P2 · 没有文字命中也可能被正分接纳

- **证据：**`src/db.rs:318-381` 先累计文字分，再无条件添加最多 180 的近因分或 200 的当前 repo 分，最后只检查总分 > 0；第一轮零分字段也会成为 best_source。
- **可达：**一个非空查询在整个 FTS 中零命中时进入 fuzzy fallback；如果某候选也没有 fuzzy/子串命中，但时间小于 90 天或 repo 相同，它仍可能作为“相关”命中返回，并标注没有实际证据的来源。
- **反证/界限：**不是每次搜索都返回所有会话；FTS 非空时不会引入任意库内条目，旧会话且无 repo boost 也不会仅靠上述分数入选。空查询本身不作为误报例子。
- **ASG 教训：**文字/语义证据决定能否入选，recency/repo 只影响合格结果排序；零结果与推荐最近会话分开表达。

#### SG-03 · P2 · 不完整解析被缓存为 current，错误身份还会分叉或合并

- **证据：**五个 adapter 的逐行 JSON 错误均 continue，成功 record 的 parse_warning 为 None；如 `claude.rs:89-96,194-211`、`codex.rs:105-112,198-215`。外层文件错误转 `minimal_record`，后者按文件 stem 造 ID 并产生空 transcript（`src/util.rs:329-367`）。Indexer 不区分成功/失败，一律 upsert（70-78）；事务总会更新 files_seen（`src/db.rs:224-241`）。
- **可达：**新文件读取失败后，仅修复访问权限而不改变 mtime/size，后续 refresh 会跳过；已变化文件解析失败也可能覆盖同 ID 的旧缓存正文。全部坏 JSON 行但可读的文件能成为零消息、无警告的“成功”记录。
- **身份特例：**正常 Antigravity ID 来自 conversation 目录，失败却统一成为 `antigravity:transcript`；多个失败文件会争用该 ID。Codex/Pi 正常 ID 与带日期 stem 的失败 ID 也可能不同，留下另一个错误记录。只在正常/失败 ID 恰好相同时才声称旧条目被覆盖，不能泛化给所有 provider。
- **反证/界限：**文件级失败 record 有 parse_warning，CLI 能展示并 warnings-only 过滤；坏尾行被跳过可帮助读取正在追加的日志；原始 session 文件没有因此被删除。问题是没有区分可恢复尾部、真实坏行与失败快照的持久化政策。
- **ASG 教训：**保留 last-good；解析完成度与错误计数显式传播；失败不能推进成功水位，失败和成功身份须一致。

#### SG-04 · P2 / I · 单会话事务可靠，但批量刷新与 freshness 不是同一契约

- **正面证据：**`src/db.rs:144-242` 同一事务写 sessions、transcripts、FTS、files_seen 后提交。不能把这个项目说成“完全没有事务”。provider 前缀隔离跨 provider 的原生 ID。
- **full 风险：**`src/indexer.rs:28-30` 先 clear，再发现/逐个提交；没有覆盖清空与全部重建的原子切换。中断或后续失败可留下空/部分缓存。README:21,154,164 明示其为可丢弃 cache，所以不是原始会话数据丢失，也不等价 ASG 权威 catalog 的失败。
- **增量边界：**`src/db.rs:124-141` 只比较 transcript 的 mtime/size；content_hash 字段始终写 null（224-238），parse_version 仅存储。没有删除文件/禁用 provider 后的清理、解析器升级自动失效、源读前后 snapshot 比较（`src/indexer.rs:32-81`）。这些是保留旧缓存的行为边界，不凭空推断用户要求它自动删除。
- **可达的 sidecar 漏刷新：**Codex 每次构造读取 state_5.sqlite/session_index.jsonl（`codex.rs:36-39`），但未变化的 rollout 在 parse 前已被跳过；仅改标题等 sidecar 不会更新现有条目。所有 adapter 在 enabled 判断前构造，Codex 禁用时也会读取 metadata。
- **同 ID 多来源：**不同路径同 provider/id 由后一次 upsert 覆盖，files_seen 却按路径分别保持 current；无冲突比较、来源合并或显式优选规则（`db.rs:38-67,163-208`）。同内容副本去重合理，不同内容副本的取舍没有契约。
- **ASG 教训：**把缓存可重建、事务原子性、源一致性、sidecar freshness、重复来源规则分别验收；不为追求轻量体验删去 ASG 已有一致性要求。

#### SG-05 · P2 · 普通过滤与 ID 解析的 LIKE 字面量语义不一致

- **证据：**`src/db.rs:453-457,550-555` 直接给 path_prefix 加 `%`；输入中的 `_`/`%` 会成为模式字符。`resolve_session` 同时查 exact 与未转义 prefix（480-504），没有 exact 优先阶段。
- **可达：**真实项目名包含下划线或百分号时，普通 list/search 的范围可能扩大；ID 含模式字符时也可能扩张匹配。同一完整 ID 又是另一 ID 的前缀时可能被判 ambiguous，后者对常规等长 UUID 不常见。
- **反证/界限：**参数是绑定值，不是 SQL 注入；多匹配会拒绝，不是随便恢复错误会话。timeline 已使用专门的 LIKE escape 并有 helper 测试（252-278,621-639）。纯“字符串前缀”包含同名前缀兄弟目录是设计语义，不单独报目录越界。
- **ASG 教训：**统一字面前缀函数、exact/prefix 分阶段，测试 `_`、`%`、反斜杠及 exact-prefix 交集；不要重新引入已有 timeline 修复过的问题。

#### SG-06 · P2 / I · Cursor 路径恢复先穷举，且仅有特定平台启发式

- **证据：**`src/providers/cursor.rs:235-273` 只接受 `Users-...`，对剩余 n 个片段生成全部连续分组后才查找存在路径，组合数量为 2^(n-1)。`parse_inner:87` 每次解析都会进入这条路径。
- **可达：**含很多连字符的项目编码目录及新/变更的 Cursor transcript；即使第一种路径已经存在，也先完成整张组合表。同步 CLI 自动刷新和 MCP 启动/刷新都能遇到这个工作量。
- **反证/界限：**非 Users 前缀直接返回 None；短路径组合很小；本轮没有测量时间、内存或运行 OOM。缺少其他平台 CWD 解码是兼容性边界，不因没有桌面全平台承诺就定性成产品 bug。
- **ASG 教训：**不从不可逆目录编码无限穷举；优先权威 metadata，启发式设工作上限并报告未知来源。验收区分“会话可索引”和“项目关联完整”。

#### SG-07 · P2 / I · 清理标记与扁平正文会失去实际内容/消息身份

- **证据：**Claude 的 `should_skip_message` 与 `strip_command_markup` 只要正文任意位置包含 command-name 就按 command-args 处理，没有先验证特定封装形状（`claude.rs:230-256`）；Cursor 对拼接正文任意 user_query tag 抽取内文（`cursor.rs:169-211`）。
- **可达：**用户/助手讨论这些标签、展示相关代码/样例，而非真正调用命令时，正文可能整条跳过或只保留标签内的一小段。普通保真内容不能只靠“contains 某个标记”识别为系统噪声。
- **反证/界限：**删除明确的 UI bookkeeping、工具 payload、thinking 或 subagent 是本项目有测试/文档支持的降噪策略，不把这些限制本身列为缺陷；Codex 正确只导入 response_item/message，避免 event_msg 镜像重复。
- **另列 I：**SessionRecord/ParsedSession 没有结构化消息 ID、父边、branch、源字节范围（`models.rs:48-96`），同文本/同 ID 消息的去重和重建没有相应模型；Cursor 时间来自文件 mtime。这说明不能把其“完整 transcript”当原始日志或事件级证据，不证明已发生了某个特定 provider 版本的数据错乱。
- **资源边界：**五源 fs::read_to_string；Claude/Antigravity 还将每个解析 Value 克隆进未参与最终输出的 raw_meta（`claude.rs:75,86,97,189-192`；`antigravity.rs:69,88,100,177-180`）。`Db::list_recent` 先加载匹配会话全部 transcript 再排序截断（`db.rs:245-249,532-575`）。这是可见的全量内存路径，不是实测性能排名。
- **ASG 教训：**只对已验证封装做归一化；保留稳定消息身份和来源定位；列表优先 metadata-only，不能拿少读/丢数据换取“更快”的不等价比较。

#### SG-08 · P2 / I · MCP 可用但 freshness、预算和恢复字符串有明确边界

1. **刷新失败不可由工具结果辨识。**`mcp.rs:21-31,79-90` 将失败写 stderr 后继续服务，并更新节流时间；返回工具内容没有 freshness/partial 字段。优点是旧索引仍可用，不能误称所有错误都被吞：DB/search 的错误会进入 isError（219-251,278-280）。ASG 可保留可用性，但应区分 last-good、刷新失败与最新完成。
2. **预算不是硬边界。**search/list 的 limit 从 u64 直接转换，缺上限（254-275,360-382）；DB 还执行 limit×5（`db.rs:288`）。极大输入存在溢出/异常工作量条件，具体 debug/release 表现未运行。get_session 先读完整 transcript，再按逻辑行 take，默认全量，也没有截断提示/后续位置（316-357）；单个超长行仍无字节上限。仅 timeline 有 1..200 clamp 与 metadata-only SQL 限制（410-425；`timeline.rs:8-21`；`db.rs:252-278`）。
3. **协议实现窄而同步。**无效 JSON 行直接跳过、输入行无显式长度限；initialize 回固定 2024-11-05 而不读取客户端 params；取消通知直接忽略；stdout 写错误也忽略（36-108）。这些是源码事实，不在未核查官方协议的条件下判定全套 MCP 合规性。没有网络 listener、HTTP server 或后台线程的源码入口证据。
4. **恢复字符串不等于安全执行。**MCP 对 cwd 使用 shlex，但把 command 各参数直接用空格连接（428-446）；header ID 可来自源文本，未做 UUID 约束。若特殊字符 ID 被消费者交给 shell 执行，会改变命令语义。风险前提是源数据含这种 ID 且下游实际执行字符串，**不是 MCP 已自动执行命令或已经远程 RCE**。CLI/TUI 使用独立 argv（`cli.rs:157-164`、`tui.rs:52-59`），反证不能遗漏。
5. **取舍不是缺陷。**只提供 5 个 task-oriented 工具，没有 generation cursor、认证网络服务或向量检索，不以功能数量判错。返回会话正文是明确授权给本地 MCP 客户端的能力；源码无网络请求不保证客户端/模型也不向外发送这些内容。

**ASG 教训：**学习 search/list/get/timeline/resume 的任务分工；给资源上限、截断、freshness 与机器错误明确契约；优先返回结构化 argv/cwd，提示字符串单独按目标 shell 转义。不要把额外工具扩张视为已批准需求。

#### SG-09 · P2 / P3 / I · TUI 摘要有价值，但有状态与分隔符问题

- **P2，滚动状态：**query refresh 将 selected 重置为 0，却不重置 preview_scroll；load_preview 更新正文/行数也不复位（`tui.rs:295-313,343-367`）。可达操作是滚动较长预览后输入新查询，短的新预览仍沿用旧偏移。上下移动选项会复位（316-338），所以不是所有切换都坏。本轮未做实际终端复现。
- **P2，正文被当成回合边界：**`TurnRole::parse` 接受任何方括号前缀后紧跟 user/assistant 的行，`parse_turns` 把它当消息头（377-425）。原始 message 正文若含这种示例行，就可能在 TUI 被分裂/重归属；`util.rs:322-326` 的扁平格式没有区分正文与头部。数据库正文未被这个显示步骤修改。
- **P3，标记碰撞：**`marked_spans_with_style` 把正文中的双中括号也当高亮标记（562-596）；字面 wikilink 等内容会失去括号，不一定来自查询高亮。
- **P3，终端清理：**run_app 返回错误后仍先执行清理，这是正确反证（27-40）；但 raw mode 开启后、进入屏幕/创建 Terminal 的早期失败以及 panic 没有 guard 覆盖，不能因此声称一般错误都会损坏终端。
- **I，摘要范围：**首个/末个 user/assistant 四段、8/4/8/14 行上限，有 hidden/elision 提示和测试（442-497,631-678）；这不是伪装全文的无提示截断。不过 search 丢掉 hit snippet 后仅展示首尾摘要（295-309,352-365），中间命中可能看不到；无加载后续结果的分页，键盘 PageUp/Down 仅移动现有列表。
- **ASG 教训：**保留短摘要，同时支持命中中心的证据视图；不要用可与正文碰撞的字符串协议重建结构；查询刷新重置/约束视图状态。

#### SG-10 · P3 / I · 首用直观，但配置、安装与发布声明缺少闭环证据

- `Config.ui.preview_lines` 有默认/反序列化（`config.rs:49-53,71-72,113`），TUI 实际用固定四段上限（`tui.rs:453-459`）；停止前对已读源码的字段引用核对未发现消费该配置的代码。README:142-143 给了可配置示例，属于可见配置不生效的静态疑点。
- CLI 查询默认 limit=25，只有显式 0 才采用 config.search.default_limit（`cli.rs:53-64,221-225`）；MCP 默认为 10/20，TUI 至少 100。这些入口差异应写清楚，不直接把默认值不同说成算法缺陷。
- README:31 说 Rust 1.70+；Cargo.toml 没 rust-version，CI 仅 stable；`cli.rs:194` 使用 is_multiple_of，lock 格式为 4。**尚未核查准确稳定版本与依赖 MSRV，因此这里只报告声明缺证，不伪造“已在 1.70 编译失败”。**
- README:34-52 的源安装命令不是已完成安装证据，未使用 --locked；CI 使用 --locked；Nix 配置复用 Cargo.lock（`flake.nix:20-32`）。README:164 的“尚无 tag”仅是本快照的作者声明，不是本轮核实的最新发布状态。
- `which` 只检查 PATH 下无扩展字面路径是否 exists，不校验可执行性或 PATHEXT（`util.rs:421-426`）；resume_plan 对不存在的 cwd 直接去掉 cwd（381-418）。不能由 doctor 的存在检查推断实际恢复成功或 Windows 支持。Cursor/Antigravity 不支持 resume 是 README:159 的明确限制，不是发现的新 bug。
- DB open 会创建目录/初始化 cache；CLI 在 paths/doctor 分发前也已开 DB（`cli.rs:99-108`）。本工具的“只读”主要指不主动改原始 transcript；export 是显式文件写出，resume 是用户确认后委托原生命令。Codex metadata 查询仅 SELECT，但使用普通 Connection::open（`codex.rs:233-260`），没有源码级只读连接约束；未观察/断言实际修改源数据库。
- 未发现 target 以外独立 tests/fixtures、发布脚本、发布流水线、安装回执或历史 CI 运行记录；这是本次 26 文件范围的静态存在性描述，不代表它们在上游其他地方不存在。

### 5. 测试与正面反证清单

| 文件 | 声明数 | 已读测试实际覆盖 |
|---|---:|---|
| `src/db.rs:630-640` | 1 | LIKE 转义字符串 helper；不是 SQLite 查询集成测试。 |
| `src/mcp.rs:449-465` | 1 | tools/list 包含 timeline 名称；不证明 framing、取消、参数上限或刷新状态。 |
| `src/providers/claude.rs:260-316` | 6 | meta/local-command、参数保留、普通文本 helper；没有完整 JSONL parse/reindex fixture。 |
| `src/providers/cursor.rs:275-372` | 5 | 父 transcript 发现/解析、subagent 排除、user_query、工具忽略、empty-window。 |
| `src/providers/antigravity.rs:209-240` | 1 | 两条消息、目录 ID、工具 Cwd 的最小 fixture。 |
| `src/providers/pi.rs:221-334` | 3 | 标准解析/子目录排除、文件名 ID 回退、项目目录作 root。 |
| `src/timeline.rs:146-269` | 8 | 日期分桶/顺序、prefix、大小写、limit、空结果、unknown、总 cap。 |
| `src/tui.rs:599-679` | 5 | 正常 turn 格式、首尾省略、短会话、长正文省略、空正文。 |
| `src/util.rs:429-498` | 6 | 嵌套文本、预览、snippet、高亮、worktree、submodule 反例。 |
| 合计 | **36** | 全部为代码声明；**本轮执行 0，不提供通过率。** |

没有发现 Codex parser、本地索引器、CLI/config 的内联测试；上述测试正文也没有端到端覆盖 SG-01 的过滤召回、错误缓存、读中变更、事务崩溃、多进程 writer、MCP 字节预算、TUI 状态复位或真实 provider resume。不可把 helper 测试数量当这些契约已获证明。

应保留的明确反证：

- UTF-8 截断/snippet 已修正字符边界（`util.rs:66-79,225-282`），不复述“任意中文都会 panic”。
- repo 检测考虑 worktree 与 submodule，且有测试（`util.rs:26-64,464-497`），不能声称完全不支持 worktree。
- FTS 错误本身以 Result 返回（`db.rs:395-423`）；只有特定 MCP 刷新/Codex metadata 路径吞错误，不能把阶段报告的概括扩大为所有搜索错误都变空结果。
- timeline 是真正的 metadata-only、字面量 escape、SQL 先过滤 cap 的正面样例，不应沿用普通 search 的反例攻击它。
- 明确文本呈现与用户确认、独立 argv 执行，是 CLI/TUI 的安全边界；MCP 只返回 resume 建议，不执行。
- 没有中文专用 tokenizer 配置或 CJK 测试证据，不等于证明中文一概不可用；本轮没有 tokenizer/依赖内部审计或质量评测。

### 6. 给 ASG 的可执行研究结论（建议，不是已批准实现）

1. **保留**简短 task-oriented 入口、默认本地路径、metadata 卡片、摘要、明确不支持恢复的 provider；同时保留 ASG 稳定身份和证据契约，不按功能数量评价产品。
2. **先核对正确性：**过滤在 cap 前、零证据不被 boost 升格；验收包含域外高排名压过域内结果、全无文字命中但会话很新等合成场景。
3. **再核对生命周期：**last-good、完成度、水位、sidecar 失效、多来源冲突与 full 原子切换；先定义缓存/权威数据边界，再决定复杂度，不直接照抄 clear-all。
4. **然后收敛人机接口：**metadata-only list、命中附近证据、freshness 和截断标志、有界 output、结构化 resume plan；TUI 摘要/分页应复用已定义的数据契约。
5. **最后补运行证据：**provider 版本样本、MSRV/OS 安装、真实 native resume、取消/断管/终端清理及大数据评测。未经批准不执行本轮禁止的操作，也不以这些后续建议宣布 ASG 产品变更。

### 7. 外部参考与版本记录（仅本地记录，未联网查证）

- 仓库定位来自 `Cargo.toml:7`：`https://github.com/braincompany/sessiongrep`；announcement 链接仅见 README:15，本轮未读取，不用其支持结论。
- 本地 manifest `0.1.0` / Rust 2021（Cargo.toml:1-4），MCP 自报协议 `2024-11-05`、server `0.1.0`（mcp.rs:93-108）。不声称最新版本或全部协议兼容。
- 已读 lock 中的版本：chrono 0.4.44（145-146）、clap 4.6.0（159-160）、fuzzy-matcher 0.3.7（440-441）、ignore 0.4.25（554-555）、libsqlite3-sys 0.35.0（656-657）、ratatui 0.29.0（823-824）、regex 1.12.3（864-865）、rusqlite 0.37.0（893-894）、serde 1.0.228（976-977）、serde_json 1.0.149（1006-1007）、shlex 1.3.0（1050-1051）、tempfile 3.27.0（1138-1139）、toml 0.9.12+spec-1.1.0（1180-1181）。这里 libsqlite3-sys 版本不是 SQLite engine 版本。
- crossterm 同时锁定 0.28.1 和 0.29.0（Cargo.lock:258-290）；ratatui 依赖前者（831），产品直接依赖后者（1034）。版本并存不是自动成立的缺陷；未审第三方内部兼容。
- `flake.lock:7-13,22-28,44-50` 记录 flake-utils、nixpkgs、systems 的固定 revision/narHash；只确认锁文件内容，不确认下载/构建成功。
- LICENSE:1-3,189-201 的声明与 Cargo.toml:6、README:170-172 一致。报告仅总结行为和锚点，没有复制竞品实现；未做依赖许可证合规分析。

### 8. Related specs / 已读上下文

这些是 ASG 对照与本任务方法的背景，不是要求 sessiongrep 必须实现 ASG 的所有契约：

- `.trellis/workflow.md:183-199,353-380`：保持 Phase 1、研究落盘。
- 活动任务 `prd.md:18-23,30-37`：完整覆盖、严重性/反证、保留未完成范围，不直接实施。
- 活动任务 `research/competitor-cli-resume.md:33-44,69-71`：先前 sessiongrep 链路报告及未完成边界；本报告补阅读凭证，但不修改原报告。
- `.trellis/spec/agentsessions-provider-claude/backend/index.md:19-75`：原生消息身份、父边、严格形状归一化、read-only；对照 SG-07。
- `.trellis/spec/agentsessions-provider-codex/backend/index.md:18-76`：只取权威 response_item、镜像排除、稳定身份与 occurrence 时间、streaming；该 spec 仍标 Experimental/Draft，不能夸大 ASG 成熟度。
- `.trellis/spec/agentsessions-adapters-sqlite/backend/error-handling.md:17-53,101-110`：snapshot/Schema/WriterBusy 及 fail-closed；对照 SG-03/04。
- `.trellis/spec/agentsessions-cli/backend/error-handling.md:9-35,41-56`：stdout/stderr、错误分类、cursor 语义；对照 SG-08。仅作契约参考，本轮没有重新审计 ASG 产品实现。

## Caveats / Not Found

### 精确剩余账本

- **第一方必读文本：**没有未读文件或缺失行；17 个 Rust 文件的全文阅读在协调停止前已经返回。此事实不等价于主会话的完整审计已结束，也不证明本报告已囊括所有缺陷。
- **Cargo.lock：**仅已读 `1-4,144-178,258-294,439-447,553-570,655-667,822-850,863-906,975-1066,1137-1150,1179-1195`，合计 312 行。未读 `5-143,179-257,295-438,448-552,571-654,668-821,851-862,907-974,1067-1136,1151-1178,1196-1660`，合计 1348 行，维持 partial。生成的依赖表不冒充第一方人工源码，依赖安全/MSRV/许可证审计仍未完成。
- **docs/demo.gif：**只有 SHA256/字节数/头部信息；画面、每帧、生成过程和脱敏声明未验证，维持 excluded。README 明说生成脚本在仓库外，本轮未找/读那些脚本。
- **目录排除：**`.git/`、`.codegraph/`、`target/` 不逐文件审读，不提供其内容覆盖率；不把缓存里的依赖、构建结果或工具资产当第一方产品证据。
- **行为验证剩余：**所有 SG 编号的运行复现、根因优先级复核、实际 provider 格式/版本矩阵、安装/发布/MSRV/跨平台、真实恢复、负载/性能/多 writer/故障恢复、TUI/MCP 交互与外部协议核查均未执行。本轮没有 git/build/install/test/model/network 命令、没有 Cargo、没有恢复真实会话。
- **证据时点：**只代表指定本地记录快照及停止前 SHA256 所标识的内容；不能声称上游最新、release 可用、依赖无漏洞或测试通过。
- **协调所有权：**用户已指定另一 live MAIN Codex 会话继续总体审查和其覆盖账本；本 worker 停止扫描并仅交付这两份独立文件。协调停止不删除任何总体范围、不替其他会话标记完成，也不修改 runtime/task lifecycle 或其它产物。
