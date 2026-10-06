# Research: ctx 竞品源码审计（分片 B：CLI / MCP / 语义 daemon / 安装）

- Query: 对 ctx@06bc5ed1 快照做分片 B 第一方静态审计：MCP 工具面与契约、语义 daemon 生命周期与模型加载、安装/升级安全、CLI↔agent 集成与首用契约，并与 ASG 对照。
- Scope: internal；仅 `Github_src/ctx` 源码；规划研究，不实施整改、不 build/test/install/联网、不做 git 写操作。
- Date: 2026-10-06
- Recorded snapshot: `06bc5ed17ce4d0f6c8255981e009f86802dbe32f`（经 `.git/HEAD` → `.git/refs/heads/main` 只读读取核对，未运行 git 命令）。
- Status: T1 五个必读文件按有界分段阅读完成（2 full / 3 partial）；MCP server 与部分升级/模型代码作为补充阅读；本报告只记录静态证据。
- 阅读凭证: 同目录 `coverage-ctx-cli.json`（14 个文件、8921 行阅读区间、逐文件 SHA256 初/终校验）。行号均 1-based inclusive；codegraph 的“虚拟 EOF 行”不计入。

## 1. 覆盖统计

| T1 必读文件 | 行数 | 状态 | 已读行 | 未读区间 |
|---|---:|---|---:|---|
| `crates/ctx-cli/src/integrations/mcp.rs` | 1623 | full | 1623 | — |
| `crates/ctx-cli/src/semantic/daemon.rs` | 2315 | partial | 1929 | 481-765, 1366-1466 |
| `crates/ctx-cli/src/main.rs` | 1107 | partial | 802 | 321-442, 925-1107 |
| `crates/ctx-cli/src/upgrade/install.rs` | 1717 | full | 1717 | — |
| `crates/ctx-cli/src/integrations/slash_commands.rs` | 937 | partial | 830 | 831-937 |
| 合计 | **7699** | 2 full / 3 partial | **6901** | 见 JSON |

补充阅读（用于回答 MCP/升级/模型问题，逐段有界）：`mcp.rs`(949 full)、`integrations/mod.rs`(131 full)、`semantic.rs`(35 full)、`commands/doctor.rs`(78 full)、`commands/setup.rs`(271/684 partial)、`mcp/text.rs`(90/645 partial)、`semantic/embedding_backend.rs`(251/1071 partial)、`upgrade/command.rs`(120/799 partial)、`upgrade/metadata.rs`(95/277 partial)。未读区间见 coverage JSON 的 `missing_ranges`。

T2 机械交叉（只做过滤查询，不计入阅读）：`sweep-ctx.json` 中 `integrations/mcp.rs` 有 surface_mcp 232 / provider_paths 37，`mcp.rs` surface_mcp 54，`daemon.rs` ignored_result 25 / index_storage 40 / concurrency 7，`upgrade/install.rs` shell_exec 2 / ignored_result 13，`slash_commands.rs` provider_paths 14。探针只用于选读定位，结论均以原文为准。

分片 A（`audit-ctx-core.md`、`coverage-ctx-core.json`）已交付 capture/store/packet 审读；本报告对其 packet 结论只做 MCP/CLI 侧二次确认，不修改其产物。

## 2. MCP 工具面与契约

### 2.1 tools 清单与输入形状

Server 实现在 `crates/ctx-cli/src/mcp.rs`（`ctx mcp serve`，newline-delimited stdio JSON-RPC 2.0；协议版本 2025-11-25 / 2025-06-18，`mcp.rs:38-40`）。共 6 个工具（`mcp.rs:697-779`），全部 `readOnlyHint: true`、`additionalProperties: false`（`mcp.rs:782-789`）：

| 工具 | 输入 | 上限/校验 |
|---|---|---|
| `status` | 无参数（`mcp.rs:699-704`） | 只读打开索引；未初始化时返回 0 计数而非报错（`mcp.rs:376-479`） |
| `sources` | 无参数（`mcp.rs:706-711`） | 仅枚举本地来源（`mcp.rs:482-491`） |
| `search` | `query`、`limit`、`provider`、`history_source`、`provider_key`、`source_id`、`source_format`、`workspace`、`since`、`primary_only`、`include_subagents`、`event_type`、`file`、`session`、`events`、`include_current_session`、`backend`、`semantic_weight`（`mcp.rs:313-335`） | `limit` 1..=200 默认 20（`mcp.rs:719`、`mcp.rs:495-498`；常量 `main.rs:67`）；`semantic_weight` 0.0..=1.0（`mcp.rs:513-516`）；`query` 与 `file` 至少一个（`mcp.rs:517-523`） |
| `sql` | `sql`（必填）、`max_rows`、`max_columns`、`max_value_bytes`、`max_sql_bytes`、`timeout_ms`（`mcp.rs:339-351`、`mcp.rs:742-754`） | 全部有 catalog 级 cap；只读 SQL（`mcp.rs:580-605`） |
| `show_session` | `ctx_session_id`（UUID 必填）、`mode` lite/full/log（`mcp.rs:761-764`） | 硬上限 200 events，超出置 `truncated.events/max_events`（`mcp.rs:41`、`mcp.rs:612-628`） |
| `show_event` | `ctx_event_id`、`before`、`after`、`window`（`mcp.rs:771-776`） | 窗口 <= 50（`main.rs:68`；`mcp.rs:638-644`） |

参数键先经白名单校验（`mcp.rs:890-909`）；类型错误由 `optional_*` 系列返回文本错误（`mcp.rs:811-888`）。

### 2.2 输出形状与错误语义

- 成功：`content[0].text`（给模型读的有界文本）+ `structuredContent`（完整结构）双份输出（`mcp.rs:668-679`）。文本渲染有硬限额与显式省略行：5 条搜索结果 / 12 个 source / 8 行 SQL / 6 列 / 8 个事件，snippet 320 字符（`mcp/text.rs:3-10`、`mcp/text.rs:134,198-201,419-420`）。
- 工具级失败：返回 JSON-RPC 成功响应 + `isError:true` + `structuredContent.error`（`mcp.rs:681-695`）；典型触发是 store 未初始化（`mcp.rs:656-666`）与参数取值非法（`mcp.rs:496-498`）。
- 协议级失败：无效 JSON/超大行 → -32700（`mcp.rs:89-102,175-181`）；非 2.0/结构错误 → -32600（`mcp.rs:186-203`）；未 initialize → -32002（`mcp.rs:220-227`）；未知方法 → -32601（`mcp.rs:236`）；未知工具/未知参数键/arguments 非对象 → -32602（`mcp.rs:285-301,361-367,890-909`）；内部错误 → -32603（`mcp.rs:241-251`）。
- 边界：`initialize_result` 的 instructions 明示"输出可能含绝对路径/正文/SQL 结果，host 可能记录或外发"，并声明不做导入/写仓库（`mcp.rs:256-270`）；单行上限 1 MiB（`mcp.rs:40`、`mcp.rs:112-147`）；`notifications/initialized` 仅用于置位（`mcp.rs:204-209`）。
- 注意：同一类"参数错误"分走两种语义——未知键是 JSON-RPC -32602，类型/取值错误是 isError 工具结果。这是源码事实，未对照官方 spec 评判合规性（见残余未决）。

### 2.3 与 packet 字段的对接（分片 A 的 cursor 无人消费，从 MCP/CLI 侧确认）

- `search` 调用 `search_packet_with_backend(..., RefreshArg::Off, ...)`，并用 `SearchRefreshReport::skipped(RefreshArg::Off, "skipped")` 包装（`mcp.rs:558-577`）；MCP 搜索**从不刷新**、只查现有索引。
- 输出侧：`SearchDto::packet` 把 `packet.pagination`、`packet.truncation` 原样写入 structuredContent（`search_render.rs:69-70`），每条结果含 `cursor`（`search_render.rs:60`）。
- 输入侧：`search` 的 inputSchema 只有 `limit`（`mcp.rs:717-735`）；`mcp.rs` 全文无 `cursor/offset/pagination/page` 命中；`commands/search.rs` 全文无 `cursor/offset` 命中；CLI `SearchArgs` 无 offset/page/cursor 参数（分片 A 读 `main.rs:317-434`；本轮 grep 复核）。
- `show_session` 的 200 事件截断只给 `truncated` 元数据，不给续取游标（`mcp.rs:612-628`）。
- 结论：packet 生成 `offset:{n}`（分片 A：`packet.rs:114-123`）而 CLI/MCP 两侧均无消费者；MCP 既输出 pagination 形状又无法接受它 —— "只写不读"从 CLI/MCP 侧成立（CTXCLI-01）。

## 3. 语义 daemon

### 3.1 生命周期与查询服务

- 命令面：`ctx daemon run|status|enable|disable`（`daemon.rs:926-937`）；`run` 在 `daemon.enabled=false` 且未 `--force` 时直接返回报告（`daemon.rs:1105-1108`）；拿不到 `DaemonLock` 时也返回报告而非报错（`daemon.rs:1109-1112`）。
- 循环：idle 默认 30s、轮询 5s（`preamble.rs:187-189`；`daemon.rs:1115-1122`）；空闲退出前要求查询服务无在途请求且 generation 未变（`daemon.rs:1136-1159`、`daemon.rs:190-256`）；退出时先写 lifecycle 状态再 drop query service（join 线程）再释放锁（`daemon.rs:1185-1197`）；状态含 pid/heartbeat/start_mode/trigger_command/last_error（`daemon.rs:1788-1810`）。
- 失败语义：`run_daemon` 捕获 `run_daemon_inner` 错误 → 写 status=failed + last_error，然后返回 Err（`daemon.rs:1072-1089`）；单次迭代中 job 失败则记 failed 并终止循环（`daemon.rs:1163-1166`、`daemon.rs:1318-1320`）。
- 查询服务：Unix socket 放在私有 daemon 目录 0600；路径超长时回退 `/tmp` 下 0700 随机目录（`daemon.rs:258-318`、`daemon.rs:332-358`）；Windows 为命名管道（`daemon.rs:448-522`）；endpoint 文件含 UUID token，请求必须匹配 token（`daemon.rs:789-799`）；请求上限 256 KiB、read/write 超时 2s（`daemon.rs:137-138`、`daemon.rs:368-396`）；支持 `ping`（含 busy 状态）与 `embed_query`（`daemon.rs:800-868`）。
- 模型句柄共享：`Arc<Mutex<Option<SemanticEmbedder>>>`；文档/查询推理失败会丢弃句柄、从缓存重载一次并重试，第二次失败才报错（`daemon.rs:49-123`）；重载不允许下载（`embedding_backend.rs:280-305`）。

### 3.2 模型获取 / 加载 / 超时 / 失败 / 降级

- 获取模式：`SemanticModelAccess{ForegroundCacheOnly, DaemonNetwork}`（`embedding_backend.rs:4-11`）；前台 `new_semantic_embedder` 仅缓存（`embedding_backend.rs:223-226`）；daemon 路径 `acquire_semantic_embedder` 允许下载（`embedding_backend.rs:228-231`）。
- daemon 获取/加载：semantic job 在 `searchable_items>0` 且模型未加载时先写 `acquiring_model`，再 acquire（`daemon.rs:1539-1554`）；CPU 路径先查内存 defer，再取 pinned revision 缓存快照；缓存缺失/可修复且允许下载时才发起 pinned revision 下载（`embedding_backend.rs:308-345`）；加载需要本地 ONNX Runtime（`embedding_backend.rs:349-385`）；macOS CoreML 优先，普通失败回落 CPU 并记录 `acquisition_fallback`，完整性失败直接中止（`embedding_backend.rs:246-267,398-440`）。
- 超时/预算：job 级最小剩余预算——模型未加载时 15s+10s grace，已加载时 2s+10s（`daemon.rs:1523-1529`；`preamble.rs:171,200-201`）；worker 秒预算=请求值 min 剩余-10（`daemon.rs:1686-1695`）；按 deadline 跳过时保留已有状态文件（`daemon.rs:1322-1332`）。
- 失败/降级状态：acquire 失败分 `model_integrity_failed` / `model_acquisition_failed` 两类，写状态并以 `skipped` + reason 返回（`daemon.rs:1568-1589`）；worker 失败写 `write_semantic_worker_failure_status` 并返回 `failed` job（`daemon.rs:1626-1647`、`daemon.rs:1829-1847`）；内存不足是独立可重试态 `model_load_deferred`，附 available/required 内存字节（`daemon.rs:1754-1772`、`daemon.rs:1877-1901`）。
- worker 侧硬约束：embedder 未加载且本地模型缓存不可用时，worker 直接报 "will not initialize or download"（`daemon.rs:1944-1950`）；查询服务遇到同样情况拒绝 `embed_query`（`daemon.rs:846-851`）。

### 3.3 离线可用性

- 首次语义索引需要一次成功的模型获取：daemon 会尝试下载（`daemon.rs:1541-1543`）；离线时 acquire 失败 → job skipped `model_acquisition_failed`，不崩溃、不静默降级到 lexical（`daemon.rs:1568-1589`）。
- 获取成功后完全离线可用：前台/worker/query 都只读本地缓存与 ONNX Runtime（`daemon.rs:1943-1950`、`daemon.rs:853-855`）；重试路径明确禁止下载（`embedding_backend.rs:289-299`）。
- `setup` 在语义开启但 daemon 关闭时直接失败并提示三选一（开 daemon / 去掉 --no-daemon / 关闭语义）（`setup.rs:38-44`）——语义首用与常驻 daemon 强耦合；lexical 路径不受影响（config gate 在 `daemon.rs:1061`）。
- cloud sync 为硬禁用占位（`network_allowed:false`，`daemon.rs:1774-1786`）。

## 4. 安装/升级安全

### 4.1 脚本执行面

- 升级应用：下载字节先验 SHA-256（二进制与 ONNX Runtime 两次）后才进入 `apply_artifact`（`upgrade/command.rs:526-541`）；发布元数据本身用 RSA PKCS1-SHA256 验签（`upgrade/metadata.rs:183-198`）；`verify_artifact_sha` 用 SHA-256 全量比较（`metadata.rs:269-274`）。
- 暂存阶段：写入 `.ctx-upgrade-{pid}.{unix}.new`，`sync_all`，Unix 上按目标权限补可执行位，然后**执行暂存二进制**做版本探测（`upgrade/install.rs:69-77`、`install.rs:519-528`）。
- Windows：生成 PowerShell helper 并以 `powershell -NoProfile -ExecutionPolicy Bypass -File` 分离 spawn（`install.rs:1441-1451`）；脚本等待父进程退出、校验自己持有升级锁（锁文件首字段等于 PID），用 `System.IO.File.Replace` 替换二进制、脚本内 catch 回滚、写完终态后自删（`install.rs:1280-1419`）；参数引用用单引号加倍转义（`install.rs:1470-1477`）。
- Windows 运行时解包也走生成的 PowerShell 脚本 + `-ExecutionPolicy Bypass`（`install.rs:374-504`）。
- Debug 注入钩子（`CTX_UPGRADE_*_FOR_TESTS`、`CTX_RELEASE_SKIP_SIGNATURE_VERIFY_FOR_TESTS`）均有 `cfg!(debug_assertions)` 守护（`metadata.rs:184`；`install.rs:508-516,974-975,1226-1239`）。

### 4.2 路径处理

- 安装路径必须是 install_path 的父目录，staged/backup 名由进程 id+时间戳拼出（`install.rs:58-68`）；journal 校验在恢复时强制 install_path == 当前受管安装路径、staged/backup 形状、标签白名单、runtime 必须位于所选 runtime 根下且平台键匹配（`install.rs:662-774`）。
- 发布名/URL 校验：artifact 名拒绝 `/`、`\`、`..`、CR/LF（`metadata.rs:228-239`）；base URL 必须 https 且在 `RELEASE_BASE_PREFIX` 下（或显式环境放开 / file://）（`metadata.rs:213-226`）；ONNX Runtime 版本必须 MAJOR.MINOR.PATCH（`metadata.rs:241-257`）。
- 运行时压缩包：Unix tar 仅接受精确文件集合（LICENSE/ThirdPartyNotices/VERSION_NUMBER/GIT_COMMIT_ID/lib/*.so|dylib），拒绝绝对路径、反斜杠、`..`、`//`、`.//`、重复项与非普通文件，拒绝 setuid/setgid/sticky 位，限制 1 GiB 展开体积并校验 VERSION_NUMBER 精确匹配（`install.rs:228-358`）；Windows zip 有对应白名单、条目名/属性检查与体积上限（`install.rs:369-504`）。

### 4.3 失败回滚

- Unix：journal 两个阶段（publishing/committed）先写盘再发布；发布顺序 runtime→binary→marker，marker 最后（`install.rs:918-940`）；任一发布失败即按 marker→binary→runtime 反向回滚并删 journal（`install.rs:942-973,987-1006`）；committed journal 写失败也回滚（`install.rs:958-973`）；成功后 binary 备份保留为 `<name>.previous`（`install.rs:981,1186-1207`）；升级启动时 `recover_interrupted_install`：publishing→回滚，committed→收尾，随后删 journal（`install.rs:638-654`；调用点 `upgrade/command.rs:472-477`）。
- Windows：无 journal 恢复（`recover_interrupted_install` 在非 Unix 恒返回 false，`install.rs:656-658`）；依赖 helper 脚本内的 catch 回滚与 backup 文件（`install.rs:1372-1404`）；失败终态写入 state 文件供后续观察（`install.rs:1313-1339`）；helper pid 通过锁文件移交（`upgrade/command.rs:551-556`）。
- 标记文件：`<install>.install.json` 记录 manager=ctx-hosted-installer、platform、version、sha256、metadata/artifact URL 等；读取时校验 manager、platform 与当前二进制 SHA-256（`install.rs:1546-1638,1641-1670`）；缺失/不匹配时提示用官方安装脚本重装（`install.rs:1546-1550,1632-1637`）。

## 5. CLI ↔ agent 集成、首用与 setup

- 集成命令树：`ctx integrations install mcp|skills|slash-commands`、`ctx integrations status mcp|skills`（`integrations/mod.rs:17-56`）。
- MCP 安装矩阵：15 个 agent（含项目级 12 个）各有 target 与配置形态；Codex TOML `mcp_servers.ctx`，Claude/Cursor/Gemini/Qwen/Kiro/Warp/Zed/Roo JSON（root 有 mcpServers/mcp/context_servers 之分，server 形状有 Plain/StdioType/OpenCodeLocal/CopilotLocal/ClineLocal），Continue/Goose YAML（`integrations/mcp.rs:271-301,390-595,1154-1182`）；默认安装按 detected 选择（`mcp.rs:880-938`）；冲突默认拒绝、`--force` 覆盖（`mcp.rs:940-1002,1122-1130`）；status 五态 current/missing/conflict/invalid_config/unsupported（`mcp.rs:719-738`），不可支持项给出 reason（`mcp.rs:489-495,588-593`）。
- Slash-command 安装：13 个 agent 中仅 5 个直接写命令文件（OpenCode/MiMoCode/GeminiCli/QwenCode/Windsurf），Codex/Claude/Cursor/Antigravity/Copilot/Pi 走 skill-only 提示，Goose/Continue 为 manual-only（明确说明不代为修改 YAML）（`slash_commands.rs:82-105,165-245`）；文件写入带 `.ctx-slash-commands.json` 哈希元数据，区分 current/stale/modified/missing 并要求 `--force` 覆盖本地修改（`slash_commands.rs:407-428,604-728`）；拒绝 symlink/目录并校验路径不逃逸（`slash_commands.rs:686-693,803-816`）。
- 命令正文契约：让 agent `ctx search` → `ctx show event/session` 复核 → 用 ctx citation 回答；默认文本输出，`--json` 只用于管道/脚本（`slash_commands.rs:19-40,769-787`）。
- 首用/setup：`ctx setup` 创建 data root、store 与默认 config（`setup.rs:33-37`）；语义开启时要求 daemon（`setup.rs:40-44`）；默认后台模式（catalog/inventory + daemon 继续索引），`--wait` 或 `--no-daemon` 时前台导入（`setup.rs:45-101`）；JSON 输出声明 `network_required:false` / `repo_writes:false`（`setup.rs:211-212`）；全部来源导入失败则 bail（`setup.rs:291-293`）；成功后就地再触发 daemon autostart（`main.rs:632-642,751-755`）。
- daemon autostart：用当前 exe（或 `CTX_DAEMON_AUTOSTART_EXE`）spawn `daemon run --start-mode auto --trigger-command ...`，环境带 `CTX_DAEMON_BACKGROUND_CHILD=1`、`CTX_ANALYTICS_OFF=1`，stdio 全空；daemon 关闭/无 db/CI/已有活锁/不合适的 JSON 输出时跳过（`daemon.rs:2149-2234`）。
- doctor：只读打开 store 做 integrity+FK、聚合 semantic health 与 daemon report；JSON 给 `ok/findings/daemon`；人读模式打印 findings 但函数仍返回 Ok（退出码 0）（`doctor.rs:15-77`）。

## 6. 发现清单（file:line + 严重度 + 证据 + 反证）

### CTXCLI-01 · P2 · packet 的 pagination/cursor 在 CLI/MCP 只写不读（与分片 A 交叉确认）

- 证据：`mcp.rs:717-735` 的 search inputSchema 无 cursor/offset；`mcp.rs:493-577` 只用 limit；`mcp.rs`、`commands/search.rs` 全文 grep 无 cursor/offset/pagination/page；`search_render.rs:60,69-70` 仍把每结果 cursor 与 packet.pagination/truncation 序列化给消费者；`mcp.rs:612-628` 的 show_session 截断无续取游标。分片 A：`packet.rs:114-123` 生成 `offset:{n}`。
- 反证/界限：`limit` 可到 200（`mcp.rs:719,495-498`）；truncation reason 是机器可读的（分片 A：`search.rs:55-59`）；同一快照排序稳定（分片 A）；死字段不丢数据，只代表"契约承诺了取回能力但不可消费"；packet 层 pagination 语义本身未定义（分片 A：`docs/contracts/json.md:355`）。
- ASG 教训：handoff-pack 要发 cursor 就必须定义输入闭环与稳定排序键，并以"第 1 页→第 2 页无重无漏"验收；否则删掉 cursor 只留 truncation。

### CTXCLI-02 · P2 · agent 配置写入非原子；多目标安装部分成功不回滚；JSONC 注释被静默丢弃

- 证据：`integrations/mcp.rs:1050-1073` `write_target` 直接 `fs::write`（无 temp+rename、无备份）；`mcp.rs:1101-1152` update_json 重新序列化，jsonc 解析（1136-1146）后统一 `serde_json` pretty 输出（1148-1152），而 mimocode 默认目标可能是 `.jsonc`（`mcp.rs:182-203`）→ 注释/排版丢失；`mcp.rs:804-841` 对多目标收集结果，failed>0 时在已写入若干目标后返回 Err（不回滚）；`slash_commands.rs:684-707` 同样是 `fs::write` + metadata 写。
- 反证/界限：冲突/本地修改默认拒绝覆盖，需 `--force`（`mcp.rs:967-980`；`slash_commands.rs:623-636`）；slash 拒绝 symlink/目录并做路径守卫（`slash_commands.rs:686-693,803-816`）；JSONC 重写仅影响声明为 jsonc 的目标；以上为静态可达风险，未做崩溃注入。
- ASG 教训：修改第三方配置文件应 temp+rename、必要时备份，并用保留注释的专用编辑方式；多目标安装要么原子，要么明确逐目标回执。

### CTXCLI-03 · P2 · Windows 升级无 journal 恢复；helper 依赖 PID 锁 + ExecutionPolicy Bypass；版本探测用 contains

- 证据：`upgrade/install.rs:638-658` `recover_interrupted_install` 在非 Unix 恒 false；Windows 走生成脚本 + 分离 spawn（`install.rs:1260-1452`），持锁判定是"锁文件首字段 == helper PID"（`install.rs:1303-1311,1342-1350`），回滚只在脚本 catch 内（`install.rs:1372-1404`）；终态才写 state 文件（`install.rs:1313-1339`）；`verify_staged_version` 用 `version.contains(expected)`（`install.rs:519-528`）。
- 反证/界限：下载字节在进入 apply 前已验 SHA-256（`upgrade/command.rs:526-541`），元数据已验签（`upgrade/metadata.rs:183-198`），因此 contains 的偏差不构成任意二进制执行，只是版本自述校验不严；脚本内已有备份与回滚；Unix 有完整 journal 与下次启动恢复（`install.rs:830-983`；`upgrade/command.rs:472-477`）；未在 Windows 实际运行升级。
- ASG 教训：跨平台统一可恢复契约（journal 或等价物）；调度型 helper 需要超时与可观察回执；版本探测用精确比较。

### CTXCLI-04 · P2 · "谁能下载模型"路径不齐；语义首用强依赖 daemon 且首次需要网络

- 证据：`embedding_backend.rs:228-231` daemon 获取允许网络，前台仅缓存（223-226）；`daemon.rs:1539-1567` 模型未加载即 acquire；但 worker 在无缓存时拒绝初始化/下载（`daemon.rs:1944-1950`），查询服务同样拒绝（`daemon.rs:846-851`）；`setup.rs:40-44` 语义开启 + daemon 关闭直接 bail；acquire 失败以 skipped `model_acquisition_failed` 收场（`daemon.rs:1568-1589`）。
- 反证/界限：下载仅发生在缓存缺失/可修复且允许下载时（`embedding_backend.rs:322-335`），且是 pinned revision；失败不崩溃且有 last_error；内存不足是可重试 defer（`daemon.rs:1754-1772`）；一旦缓存成功，worker/query 可完全离线；语义本身有配置开关（`daemon.rs:1061`）；未实测"离线首用"的完整 UX。
- ASG 教训：把 "foreground cache-only" 与 "daemon may fetch" 写成显式契约；首用应能在不联网、无常驻进程时给出可用的 lexical 结果与明确状态，而不是把 daemon/网络作为 setup 成败条件。

### CTXCLI-05 · P3 · 错误契约碎片化：全 CLI 折叠 exit 1；doctor findings 仍 exit 0；MCP 参数错误分走两种语义

- 证据：`main.rs:663-757` `fn main() -> Result<()>` 全命令一个出口，无 canonical code/exit 映射；`doctor.rs:53-77` findings 非空仍返回 Ok；MCP 未知参数键/未知工具 → JSON-RPC -32602（`mcp.rs:890-909,361-367`），参数类型错误 → isError 工具结果（`mcp.rs:370-373,811-856`）。
- 反证/界限：MCP 的 isError vs 协议错误分界是常见且自洽的用法；各命令 JSON 输出有 schema_version 字段；ASG 自身 spec（`agentsessions-cli/backend/error-handling.md:9-56`）声明了更强契约，不能用 ASG 未完全落地的标准要求竞品；影响面主要是脚本/CI 的可判定性。
- ASG 教训：保持 ASG 的 CanonicalCode → exit code → JSON envelope 单一映射（含 cursor 错误码）；不要学 anyhow 字符串 + exit 1；doctor 类命令的 findings 必须与退出语义挂钩或在文档中显式声明。

### CTXCLI-06 · P3 · setup 文案硬编码 "Codex"

- 证据：`commands/setup.rs:227-231` 在 catalog 模式打印 `Prepared {N} Codex sessions.`，N 来自 `catalog.cataloged_sessions`（全局 catalog 计数，见 `setup.rs:109-116`）。
- 反证/界限：纯文案，无功能影响；只影响非 Codex provider 的首用输出观感；未运行多 provider 场景复现。
- ASG 教训：用户可见状态文案与 provider 泛化一致，避免把早期 provider 名固化进通用路径。

### CTXCLI-07 · I · 生产路径中的环境变量重定向钩子

- 证据：`CTX_UPGRADE_TARGET` 覆盖 current_install_path（`upgrade/install.rs:1705-1710`）；`CTX_DAEMON_AUTOSTART_EXE` 覆盖自启动二进制（`daemon.rs:2274-2280`）；`CTX_ALLOW_CUSTOM_RELEASE_BASE_URL` 放开非官方域（`upgrade/metadata.rs:217-224`）；`CTX_RELEASE_METADATA_PUBLIC_KEY_PEM` 覆盖内置公钥（`metadata.rs:200-203`）。
- 反证/界限：均为本地环境前提；override 可能服务镜像/企业部署；测试专用钩子有 debug 守护；未发现远程可利用链路。
- ASG 教训：区分"契约 env"与"仅测试 env"；升级目标、发布源、信任根这类不变量不宜由普通 env 静默覆盖。

### CTXCLI-08 · I（正面样例）· MCP 有损文本带显式省略标注，结构化字段保真

- 证据：`mcp.rs:668-679` 同时输出有界 text 与完整 structuredContent；`mcp/text.rs:3-10` 限额常量；省略行 `push_omitted_line`（`mcp/text.rs:134,198-201,419-420`）；SQL `truncated.rows/values` 透传（`mcp/text.rs:376-381`）；`show_session` 的 truncated 元数据（`mcp.rs:618-628`）。
- 反证/界限：文本确实有损（text 只显示 5 条搜索结果等），只读 text 的客户端会丢信息；但 structuredContent 与省略行让损失可判定，属于 token 预算取舍而非隐瞒。
- ASG 教训：学"有损输出必须显式标注省略/截断"；机器消费者优先读结构化字段，并把该优先级写入契约。

## 7. 与 ASG 对照（该学 / 不该学）

**ctx 强、值得学（含验收建议）**

1. 逐目标集成状态机：15 MCP 目标/12 项目目标 + 五态 status + 冲突默认拒绝（`integrations/mcp.rs:271-301,719-738,967-980`），slash 侧用哈希元数据区分 stale/modified 并要求 force（`slash_commands.rs:407-428,604-728`）。ASG installer smoke 已要求 surface→smoke 映射（`agentsessions-cli/backend/index.md:182-186`），可补"本地修改保护 + 逐目标状态"验收。
2. 升级信任链：签名元数据（`upgrade/metadata.rs:183-198`）→ SHA-256（`upgrade/command.rs:526-541`）→ 暂存版本探测（`upgrade/install.rs:519-528`）→ Unix journal 发布/回滚/恢复（`install.rs:589-654,918-983`）→ 安装标记校验（`install.rs:1546-1638`）。可对照 ASG `index.md:192-200` 的安装脚本契约补齐"验证→暂存→事务发布→重启恢复"。
3. MCP 最小正确性面：initialize 前置、协议协商、1 MiB 行上限、工具错误与协议错误分离、read-only store open、readOnlyHint、show_session 截断元数据（`mcp.rs:220-227,256-282,40,370-373,656-666,697-779,612-628`）。ASG `index.md:70-87` 已定义 MCP stdout/legacy payload 要求，可逐条对照是否也有行级上限与截断契约。
4. daemon 资源状态机：内存不足可重试 defer（含数值）、获取失败带 reason、查询服务 token+0600+2s 超时、空闲退出与在途请求绑定（`daemon.rs:1754-1772,1568-1589,320-358,137-138,1136-1159`）。ASG semantic 入口（`index.md:267-279`）可借鉴显式降级态。
5. 有损输出标注（CTXCLI-08）：`mcp/text.rs` 的省略行与 truncated 透传可作为 ASG handoff/工具输出的验收项。

**ctx 弱、不要学**

1. 无统一错误目录/退出码，doctor findings 仍 0（CTXCLI-05）——ASG 已定义 `error-handling.md:9-56`，必须保持。
2. cursor 只写不读、截断无续取（CTXCLI-01）——ASG 已承诺 `page.next_cursor/has_more` 与 cursor 错误码（`index.md:40-41`；`error-handling.md:28-35`），必须闭环。
3. 配置重写非原子、JSONC 丢注释、多目标部分成功（CTXCLI-02）。
4. Windows 升级无 journal 恢复、helper 以 PID 锁 + Bypass 执行（CTXCLI-03）。
5. 语义首用绑定 daemon 且首索引需网络（CTXCLI-04）；不要把"语义开启"做成 setup 失败条件。
6. 环境变量静默重定向升级目标/信任根（CTXCLI-07）。

## 8. 残余未决（未读 / 未证清单）

1. `daemon.rs:481-765`（Windows 命名管道细节、流读取/超时实现）与 `daemon.rs:1366-1466`（history refresh job 主体）未读；`daemon.rs` 内 Windows 特有路径未审。
2. `main.rs:321-442`（SearchArgs 细节；分片 A 已读 317-434，本分片未记入）与 `main.rs:925-1107`（analytics/parse helpers/tests）未读。
3. `slash_commands.rs:831-937` 内联测试未读；`skill/`（Agent Skill 安装实现，`skill/install.rs` 等）完全未审。
4. `mcp/text.rs:91-645` 渲染细节未读（限额与省略标记已由 grep 定位）；MCP 协议合规性未对照官方 spec（未联网）。
5. `upgrade/command.rs` 其余 675 行（状态记录、后台升级、plan 构建、日志）与 `metadata.rs` 解析段未读；`net.rs` 下载实现（重定向/代理/TLS、下载大小限）未审——"https 限制"只在 metadata 校验层确认。
6. `embedding_backend.rs` 其余段（批处理/quiet policy/线程数）、`cpu_model_cache.rs`、`ort_runtime.rs`、`resource_policy.rs`、`health_search.rs` 未读：pinned revision 下载的完整性校验（依赖 fastembed/hf_hub 内部）未证。
7. 未运行：MCP e2e、daemon 实际调度/超时/并发查询、Windows/macOS 升级回滚、并发升级互斥、离线首用与模型缓存修复；所有结论均为静态阅读。
8. T2 `sweep-ctx.json` 仅做按文件/探针的过滤查询，未消费其余 600+ 文件的机械命中。

## Caveats / Not Found

- 快照身份：`.git/HEAD` 指向 refs/heads/main = `06bc5ed17ce4d0f6c8255981e009f86802dbe32f`；未运行 git 命令，未以对象比较证明工作树干净（分片 A 记录工作树仅 `.codegraph/` 未跟踪）。
- 阅读边界：两个 T1 文件（`integrations/mcp.rs`、`upgrade/install.rs`）全文显示；`daemon.rs`、`main.rs`、`slash_commands.rs` 按指示只读关键区间并保留 missing_ranges，见 coverage JSON。
- 严重度口径：P2=中（契约/安全面静态可达），P3=低（体验/一致性），I=情报/正面样例；不含 P1 等级发现——未发现已证实的远程可利用或数据破坏链路。
- 本报告的"该学/不该学"是研究建议，不是已批准的 ASG 实现需求；未修改任何源码或任务目录外文件。