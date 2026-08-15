# Research: CLI/MCP/TUI 面审查(现状 + 路线图对照)

- **Query**: 审查 agent-session-grep 的 CLI/MCP/TUI 表面——启动性能、main.rs 结构、robot envelope、MCP、TUI、--offline/--retrieval-mode 注册、冗余工作
- **Scope**: internal(全部主分支代码,行号经 Read/Grep 实测)
- **Date**: 2026-08-15
- **证据基线**: main 分支 HEAD f4175a0;本文件所有行号来自当前工作区实测

---

## 0. 任务书假设 vs 现状(先读这个)

任务书要求"verify"的 4 个声称,经全仓 Grep 核验**全部与当前代码不符**——它们都是 PRD 规划中的目标态(子任务 #3/#8),尚未实现:

| 任务书声称 | 现状 | 证据 |
|---|---|---|
| schema 1.1 "just bumped" | **schema 恒为 1.0**;git log 无 bump | `protocol.rs:21` `SCHEMA_VERSION = "1.0"`;`schemas/robot/v1/envelope.schema.json` const "1.0";schema 目录最近 commit 是 60ef4a0(rename) |
| `redact_success_envelope`/`redact_error_envelope` | **不存在**;crates 全仓 grep `redact` 仅命中注释与 fixture 政策文档 | `protocol.rs:44` 仅注释提到 redaction 映射;现存的唯一"脱敏"是 `protocol.rs:242-247` 对 PortError::Backend/SourceIo 的固定消息掩码 |
| `--offline` 注册于所有 prefix scanner | **不存在**;cli crate grep `offline` 零匹配 | PRD `prd.md:119,126` 是规划(子任务 #8 `08-15-offline-privacy-hooks`);"fail-closed 语义"无从验证 |
| `--retrieval-mode` 注册于 6 个 scanner | **不存在**;crates 全仓零匹配 | PRD `prd.md:92` `retrieval_mode=lexical_fallback` 是规划(子任务 #1/#3) |

结论:CLI/MCP/TUI 的现状面是 **v0.3 契约(robot v1 schema 1.0)**,offline/redaction/retrieval-mode 属于路线图未落地项。审查按现状面展开,规划项标注出处。

---

## 1. 现状总览

| 文件 | 行数 | 职责 |
|---|---|---|
| `crates/agent-session-grep-cli/src/main.rs` | 2727 | 组合根 + 全部参数解析 + dispatch + ingest/sync 编排 + render 投影(含 400+ 行测试) |
| `crates/agent-session-grep-cli/src/protocol.rs` | 985 | robot envelope 4 帧 + CanonicalCode 13 码 + 错误映射 + stdout 唯一出口(含 500+ 行测试) |
| `crates/agent-session-grep-cli/src/mcp.rs` | 1275 | 手写 JSON-RPC 2.0 stdio server,6 工具(含 620 行测试) |
| `crates/agent-session-grep-cli/src/human.rs` | 1001 | 人类渲染器,每命令专属版式 + 兜底 kv(含 600 行测试) |
| `crates/agent-session-grep-cli/src/tui/mod.rs` | 471 | TUI glue:crossterm 事件 → KeyInput,Effect 同步执行,ratatui 装配 |
| `crates/agent-session-grep-cli/src/tui/core.rs` | 868 | 纯 Elm 核心:Model/Msg/Effect/update + view-model 纯函数(含 430 行测试) |

---

## 2. 焦点 1:启动性能

**每次调用发生什么**(从 main 到 dispatch 的完整路径):

```
main()                     main.rs:78   env::args().skip(1).collect()(1 次分配)
  ├─ command_name()        main.rs:79   全参数扫描 1(第 1 遍)
  ├─ parse_output_mode()   protocol.rs:261  全参数扫描 2(第 2 遍)
  ├─ extract_request_id()  main.rs:176  全参数扫描 3(第 3 遍)
  └─ run()
     ├─ intercept_help_or_version()  main.rs:253  全参数扫描 4(第 4 遍)
     ├─ command_name() ×2  main.rs:326,329  (doctor/config 判定,第 5、6 遍)
     ├─ bare_positionals() main.rs:330  (config 路径,第 7 遍)
     ├─ parse_db_flag()    main.rs:872  = extract_db_flag(第 7/8 遍)+ 再扫一遍构建 rest(第 8/9 遍)
     └─ dispatch()         main.rs:935
```

- 一个简单 `search` 调用,args 向量被**完整遍历约 7-8 次**。绝对成本微小(args < 10 个 token),但这是扫描器增殖(焦点 2)的直接后果——每加一个 flag,这些扫描全部要同步。
- **db open 是唯一的必然开销**,且已做对:help/version/config paths 在 `parse_db_flag` 之前返回(`intercept_help_or_version` ADR-0006,main.rs:311-325),`doctor` 无 `--db` 不 open(main.rs:750),零文件副作用。写入命令抢 writer lease,读路径多读者并发(main.rs:346-355)。
- **probe 只在 ingest/sync 路径发生**,search/list/get/show/context/status/mcp/tui 无 probe。
- 与 fast-resume/sessiongrep 对照:三者都是"每次调用冷启动 CLI + 打开存储",无 daemon/常驻索引;本项目 MCP serve 是唯一长驻形态(持有 store 到 stdin EOF,mcp.rs:36-57)。架构一致,无缺口。
- **可懒化的候选**:无大头。真正可做的是一次性参数解析产出全部事实(见焦点 2),把 7-8 遍扫描合并为 1 遍。

**严重度:P2**(结构性冗余,绝对耗时小)。

---

## 3. 焦点 2:main.rs 结构与 prefix scanner 增殖

**2727 行 = 参数解析族(~500 行)+ dispatch(~200 行)+ ingest/sync 编排(~700 行)+ render 投影(~170 行)+ 测试(~800 行)**。cass 式巨型单文件未到(见 inventory Don't-Borrow #1),但参数解析部分是真实的维护雷区。

**7 个扫描函数 + 1 个 flag 名单**,每个自带 flag 列表,存在 3 个不一致变体:

| # | 扫描器 | 位置 | 带值 flag 跳过列表 |
|---|---|---|---|
| 1 | `parse_output_mode` | protocol.rs:261 | 8 个(`--db --request-id --cursor --max-items --max-bytes --max-messages --policy`) |
| 2 | `extract_request_id` | main.rs:176 | 7 个(**缺 `--request-id`**,自己处理) |
| 3 | `command_name` | main.rs:218 | 8 个(含 `--request-id`) |
| 4 | `intercept_help_or_version` | main.rs:253 | 8 个(含 `--request-id`) |
| 5 | `extract_db_flag_impl` | main.rs:829 | 7 个(含 `--request-id`;**缺 `--db`** 自己处理) |
| 6 | `parse_db_flag` | main.rs:872 | 3 个(`--db --output --request-id` 之外的不消费——依赖前置 extract_db_flag 已跳过) |
| 7 | `bare_positionals` | main.rs:917 | 8 个 |
| — | `is_known_flag_name` | main.rs:794 | 13 个 flag 全集(取值守卫) |

另有裸 flag 列表 `--robot --no-color --help -h --version -V` 在 3 处重复(command_name:226、parse_db_flag:886、bare_positionals:926)。

**维护风险实测**:加一个全局 flag(路线图上的 `--offline` 正是如此)需要同步 8-10 处——`is_known_flag_name`、`command_name`、`intercept_help_or_version`、`extract_request_id`、`extract_db_flag_impl`、`parse_db_flag`、`bare_positionals`、`parse_output_mode`(带值时)、`help_text`、`subcommand_help_text`、相关测试。漏一处即产生不一致语义(当前 `extract_request_id` 与其它 6 个扫描器的列表差异就是实例)。dispatch 内 search/list/context 还各自 `rest.to_vec()` 克隆参数再 `extract_flag` 逐个 remove(O(n) 移位,main.rs:1020-1023, 1079-1082, 1106-1109)。

**具体重构边界(建议)**:
1. 单一 `fn parse_args(args: &[String]) -> Result<ParsedArgs, CliError>` 一次扫描产出 `{db, mode, request_id, command, rest}`,替代 7 个扫描器;flag 表收敛为一份 `const VALUE_FLAGS: &[&str]` / `const BARE_FLAGS: &[&str]` + 一份已知 flag 全集(由前两者组合,消除 is_known_flag_name 第三份名单)。
2. `command_name` 调用收敛为 1 次(当前 3 次:main.rs:79 + 326 + 329),ParsedArgs 携带 command 字段。
3. dispatch 的 `rest.to_vec()` 克隆改为 `&[String]` 视图 + 只读索引提取(flag 提取可改为迭代器一次遍历)。

**严重度:P1**——路线图(16 provider、--offline、--retrieval-mode、新子命令)落地时,每个新 flag 都要过这 8 处同步,是发布节奏中最现实的结构性债务。与 inventory Don't-Borrow #1(cass 巨型单文件)同族但未到危险线;拆分粒度以"参数解析族 → `cli_args.rs`"为第一步即可,不要求整体模块化。

---

## 4. 焦点 3:Robot envelope

**现状 schema 1.0**(见 §0),四帧 oneOf:success / error / progress / diagnostic(protocol.rs:335, 371, 400, 406;schema 文件 oneOf 4 项,测试断言 protocol.rs:861-887)。

- **envelope 形状**:`{schema_version, frame_type, command, request_id, ok, outcome, data|error, warnings, page, meta}`。request_id 缺省生成 `cli-<pid>-<millis>`(protocol.rs:306-316),调用方提供则逐字回显。
- **错误码映射**:13 个 CanonicalCode → 稳定 wire 串(protocol.rs:80-96)/ exit code(99-113)/ retryable(116-121)/ operator_action(127-151),四张映射各自独立 match,并有测试与 `schemas/robot/v1/error-catalog.json` 双向比对(protocol.rs:787-821)。**retryable = WriterBusy | SourceChanged 两码**。
- **消息脱敏现状**:仅两处掩码——`PortError::Backend` → 固定 "数据库内部错误"、`SourceIo` → "源文件无法读取"(protocol.rs:242-247,测试 544-561 断言 NUL 查询 / 绝对路径不外泄)。**无 envelope 级 redact 层**(`redact_success_envelope` 等不存在);PRD 的"机器输出默认脱敏"(prd.md:120-123)是 ADR-0009 规划,落点 08-15-offline-privacy-hooks。
- **未覆盖路径审计**:
  - `error_envelope` 的 `duration_ms` 恒 0(protocol.rs:391)——错误路径不测耗时,调用方拿不到失败耗时。P3。
  - `error.details` 的"≤32 属性"是有文档约定,无运行时强制(protocol.rs:159)。构造方只有 cursor mismatch 两类 details(protocol.rs:200-213)。P3。
  - `diagnostic_frame` 定义完备但 v1 无发射点(protocol.rs:405 注释明说)。
  - **stderr sink 形状**(全部路径审计):
    - human 模式:警告 → stderr 逐行 `warning: ...`(main.rs:405-407);错误 → stderr 两行 `error [<code>]: <msg>` + `下一步:...`(main.rs:131-134)。
    - robot/json/jsonl 模式:stdout 只输出 envelope 对象;stderr 仅剩 `write_stdout_line` 非 EPIPE 写失败的一行诊断 + exit 5(protocol.rs:443)。
    - EPIPE → 静默 exit 0(protocol.rs:441)。
    - 结论:stderr 无协议泄漏,形状与 CONTRACT §6 一致;`--no-color` 是死 flag 但无害(见 §8)。

**严重度:整体 P2**(一致性强、测试密度高;缺口集中在错误路径细节与 redact 层未落地)。

---

## 5. 焦点 4:MCP

**手写 JSON-RPC 2.0,零 SDK 依赖**(mcp.rs 全文),与 sessiongrep 手写 MCP(inventory W3,Apache-2.0)同构但更完整:本项目有 initialize 门闩(握手前只放行 initialize/ping,mcp.rs:171-173)、版本协商诚实回落(mcp.rs:368-375)、错误分层(-32602 协议错误 vs `isError:true` 业务帧,mcp.rs:8-12)、`additionalProperties:false` 代码侧强制(mcp.rs:592-599)。ctx 是 SDK 方案,本项目选型 lean,与 W3 结论吻合。

- **JSON-RPC 处理**:逐行 stdin(serve 循环,mcp.rs:42-55),batching 拒绝(-32600,mcp.rs:91-99,2025-06-18 协议已移除),notification 永不回应(mcp.rs:106-120)。`notifications/cancelled` 是 documented no-op。
- **工具**:6 个,契约 §8 固定顺序(search_sessions / get_session_context / list_sessions / list_providers / get_status / doctor),schema 下限与运行时预算校验双向断言(测试 mcp.rs:1049-1082)。
- **content/structuredContent 双载体**:成功路径 `content.text = payload.to_string()` + `structuredContent = payload`(mcp.rs:220-227),测试断言两者严格同形(mcp.rs:1192-1196);业务错误路径 text 只带 message、structuredContent 带完整 error 结构(mcp.rs:552-565)——不对称但有意的(text 供人读)。
- **批量/流式**:无 batch、无流式,单请求单响应,无累积状态。
- **OOM 风险评估:无**。响应大小受 App 层 ResponseBudget 硬门(默认 `max_response_bytes = 4MiB`,`budget.rs:10`;App 层 clamp_items 执行),MCP 不放大不受控输入。唯一放大是序列化(见 §8)。
- **与 sessiongrep 节流对比**:sessiongrep 有 1500ms reindex 节流;本项目索引同步是 CLI 命令(ingest/sync),MCP 不触发索引,无对应物,不需要。
- **单点注意**:`tools/list` 每次请求重建整个 6 工具目录(mcp.rs:174, tool_catalog 全量 json!)——可提升为常量,成本小。P3。

**严重度:P2**(整体 lean 且测试充分;放大与重建是小项)。

---

## 6. 焦点 5:TUI

**架构良好**:core.rs 是纯 Elm 核心(不 import ratatui/crossterm/store/App,core.rs:4-7),mod.rs 是薄 glue;状态转移与 view-model 全部可无终端单测(430 行测试)。ratatui 用新版 API `try_init()/restore()/DefaultTerminal`(mod.rs:46-53)。

- **render loop**(mod.rs:59-87):`loop { draw → poll(250ms) → 有键则 update → 内联执行 Effect 并回灌 }`。
- **效率问题 1:空闲时每 250ms 全量重绘**。`draw` 在 `poll` 之前无条件执行,poll 超时 `continue` 回到 draw——空闲 TUI 每 250ms 做一次完整 draw,且 draw 内全量重建行字符串(`hit_lines`/`context_lines`/`List::new(items)`/`join`,mod.rs:295-310)。应改为"poll 超时 → continue 不 draw"或 dirty 标记。P2。
- **效率问题 2:Effect 在 UI 线程同步执行 SQLite 查询**(mod.rs:74-82 `execute`)。搜索/context 大查询期间 UI 完全冻结——任务书点名的已知 tech debt,确认为现状(查询经 App 走 SqliteStore,同步阻塞)。P2(路线图上值得做后台线程 + channel 回灌,core 的 Msg 模型已为异步回填备好——`Msg::SearchLoaded` 等正是回灌通道,改造面小)。
- **滚动实现**:Context 屏用 `Paragraph::scroll((scroll as u16, 0))`,scroll > u16::MAX 截断(tui/mod.rs:308-309)——行数超 65535 的极端场景,无碍。P3。
- **业务规则零复制**确认:分页令牌/分支策略/命中→会话解析全部经 AppRequest 交回 Application;投影复用 CLI `render()`(tui/mod.rs:216),截断/警告与三面同源。
- **已处理的坑**:Windows Release 键双发过滤(`KeyEventKind::Press`,mod.rs:13,95-97);策略切换即重取(core.rs:268-283);空追加页保持选中行(Minor-10,core.rs:620-639);新查询清除旧 ContextView(Minor-11,core.rs:775-797)。

**严重度:P2**(架构分留得对,两个效率项都是明确可改的局部问题)。

---

## 7. 焦点 6/7:--offline 与 --retrieval-mode

**验证结论:两个 flag 在 CLI 中均不存在**(cli crate 与全 crates grep 零匹配,见 §0)。"注册于所有 prefix scanner"的声称无法成立,因为**任何 scanner 都没注册过**。

- `--offline`:PRD 规划于 "零遥测、零上传、默认离线" 节(prd.md:118-119,"提供 `--offline`")与 Hook 约束(prd.md:126),落点子任务 #8 `08-15-offline-privacy-hooks`(依赖 ADR-0009)。**fail-closed 语义无可验证对象**;若按规划落地,建议语义为"显式拒绝任何联网路径(模型下载/外部 API)+ 检查点拦截",与现状零联网基线一致(当前构建无任何联网代码,offline 是声明性保障而非行为开关)。
- `--retrieval-mode`:PRD 的 `retrieval_mode=lexical_fallback` 是**降级标注字段**(prd.md:92,"禁止静默切换"),属子任务 #1(unified-release-contract 的 retrieval_mode 字段)+ #3(semantic-hybrid-local-retrieval)。当前检索纯 lexical(ADR-0007 CJK bigram),无模式切换,无字段。

落地这两个 flag 时,焦点 2 的 8 处同步点就是直接改造成本。

---

## 8. 焦点 8:冗余工作清单(按成本排序)

| # | 冗余 | 证据 | 成本量级 | 严重度 |
|---|---|---|---|---|
| 1 | **双重 probe**:`stage_with_registry` 在 `select_and_stage`(内部已 probe 并复用,application `lib.rs:375` 注释"不再对同一字节第二次 probe",测试 `lib.rs:2432` 断言恰好一次)之后,又对全体 adapter 再 probe 一遍取最高置信度 variant | main.rs:1258-1274 | **probe 是全量扫描**(provider-claude `lib.rs:271-278` 整文件 from_utf8 + 逐行判定)。现 2 adapter → 每源 4 次全量扫描;**16 provider 规划下是每源 32 次**,sync 多文件大语料时是主导成本 | **P1** |
| 2 | 参数向量遍历 7-8 次 + dispatch 每次 `rest.to_vec()` 克隆 + `extract_flag` O(n) remove | §2 表格 | 绝对小,维护成本大 | P1(与 #2 合并,结构性) |
| 3 | MCP content.text 双重序列化:payload 先 `to_string()` 再包进 `json!` 整体 `to_string()` | mcp.rs:220-227 | 帧大小放大 ~2-3 倍(引号/换行双重转义);默认预算 4MiB 下最坏 ~12MiB 帧,无 OOM 但内存放大 | P2 |
| 4 | TUI 空闲每 250ms 全量 draw + 每帧重建全部行字符串 | mod.rs:64-70, 295-310 | 空闲 CPU 唤醒 + 渲染,量小 | P2 |
| 5 | `command_name` 调用 3 次全扫(main.rs:79, 326, 329) | — | 微小 | P3 |
| 6 | `--no-color` 死 flag:3 处扫描器 + is_known_flag_name 均接受,无任何消费者(human.rs 无颜色是写死的) | main.rs:226, 801, 886, 926 | 语义空洞;新 flag 照抄会放大名单 | P3 |
| 7 | human.rs `snippet` 字段回退是死代码:main.rs `render()` 永不产 snippet(测试 `machine_render_never_emits_snippet_field` 断言),渲染器仍 `or_else` 读旧字段 | human.rs:95-98 vs main.rs:1726-1742 | 两行,文档价值 | P3 |
| 8 | `provider_registry()` 每次 stage 重建 2 个 adapter;`App::new` 每命令重建 | main.rs:1222-1227, 1033 等 | 轻量无状态,可忽略 | P4 |
| 9 | 无重复 db open——确认过:单 store 一次打开(main.rs:350-355),doctor/mcp/tui 各一次,无 double open | — | — | 无问题 |

---

## 9. 借用候选(inventory 引用,供路线图实施)

- **W3 sessiongrep 手写 MCP**(`sessiongrep/src/mcp.rs`,Apache-2.0):本项目 mcp.rs 已是其超集(门闩/分层错误/isError 封套),**无需再借**;其 1500ms reindex 节流在本项目无对应物。若未来 MCP 加"搜索前自动 sync"能力,节流设计可回借。
- **R4 sessiongrep `get_resume_command` + shlex 引号**(Apache-2.0):路线图 #5 resume 的 MCP 工具落地时 verbatim 可用。
- **H3 sessiongrep 书挡式 transcript 摘要**(Apache-2.0):TUI Context 屏的上下文预算展示可参考。
- **fast-resume/sessiongrep 启动设计**:无特殊模式,与现状架构同构,无借点。
- **扫描器重构**:13 个对手仓库无直接可搬项(clap 是外部依赖,本项目刻意手写);重构边界见 §3。

---

## Caveats / Not Found

- 任务书 4 项声称(schema 1.1、redact envelope、--offline、--retrieval-mode)与 main 分支现状不符——若这些功能存在于其它分支/工作区,本审查未覆盖;当前 main 无其任何痕迹。
- probe 行数成本未实测(未跑 benchmark);"4 次全量扫描"按 2 adapter × 2 轮推断,adapter probe 实现确为整文件处理(provider-claude lib.rs:271-278)。
- TUI 空闲重绘的 250ms 周期是代码阅读结论,未实测 CPU 占用。
- `--no-color` 可能服务于未来颜色支持(help_text 未提它,current 行为无颜色);标 P3 而非建议删除。
