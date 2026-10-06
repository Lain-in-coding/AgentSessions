# Research: cc-switch 分片 B（proxy/transform 相邻能力面）竞品源码审计

- Query: cc-switch（commit f748f3ac）代理转发/格式转换/流式处理/错误重试/超时/密钥与日志边界审计，并评估与 ASG（会话检索）的竞合关系。
- Scope: internal；只读静态审计 `Github_src/cc-switch` 四个 T1 文件（forwarder.rs / handlers.rs / transform_codex_chat.rs / services/proxy.rs）；T2 `sweep-cc-switch.json` 按需过滤。无 build/test/install/网络/git 写操作，未启动应用。
- Date: 2026-10-06
- Recorded snapshot: `f748f3ac4afbbbc6554a3ca2bb7e56e4036fedfa`
- 覆盖口径: 4/4 partial（选读精读），11094 / 20747 行 = 53.5%，95 个 ≤120 行读取区间；逐文件区间与缺读见 `coverage-cc-switch-proxy.json`。本文只对已读区间下结论，未读区间不做推断。
- Status: t1_reads_complete；报告完成。

## 结论摘要

1. cc-switch 的代理能力面远超"配置切换器"定位：多 provider 故障转移 + 熔断、整流重试（thinking 签名 / thinking budget / 图片降级）、Codex Responses↔Chat/Anthropic 双向转换、流式首包预热后才记成功、2xx 语义失败也参与 failover、用量按 session 归因。
2. 错误语义是该仓库最强设计之一：客户端错误（400/405/406/413/414/415/422/501）不参与 failover，401/403/404/408/409/429/451 与 5xx 参与；官方 Codex 与 xAI 的账号级鉴权失败不 failover（避免静默换号）。
3. 日志/密钥纪律好：请求体只以 `bytes + hash` 记录（明示 content omitted）、URL 双模式脱敏（含 query 内嵌 key 场景）、错误摘要只提取 message 字段且截断。
4. 与 ASG 的关系：代理是"流量经过点"，不是"检索面"。它只提取 `session_id` 用于用量/计费归因与上游会话路由（`x-session-id` 等），不索引正文、不做检索/恢复。
5. 主要风险面：主代理端点无入站鉴权（仅 Claude Desktop 网关有 Bearer 校验，回环假设依赖默认配置）；接管前 Token 同步"写库失败仅告警继续"；Live 备份与 DB 中凭据为明文存储（加密未验证）；少数降级只落 debug/无日志。

## 1. 覆盖与阅读方法

| 文件 | 总行数 | 已读行数 | 读取区间数 | 缺读区间 | 状态 |
|---|---:|---:|---:|---|---|
| `src-tauri/src/proxy/forwarder.rs` | 5023 | 3720 | 31 | 3601-3888、4009-5023（测试区） | partial |
| `src-tauri/src/proxy/handlers.rs` | 3348 | 2674 | 23 | 2675-3348（测试区） | partial |
| `src-tauri/src/proxy/providers/transform_codex_chat.rs` | 4627 | 1975 | 17 | 1976-4627（测试区） | partial |
| `src-tauri/src/services/proxy.rs` | 7749 | 2725 | 24 | 1897-2197、2565-2626、2930-2974、3134-7749（含大量测试） | partial |

- 读取方式：`codegraph node --file <rel> --offset/--limit`（每段 120 行，行号与文件 1:1 对应，已用 grep 行号抽查核对）；grep 仅用于函数表与关键词定位。
- T2 机械扫描（未整读，按需过滤）：`forwarder.rs` secret_redaction=69 / inline_tests=83 / unwrap_or=40 / ignored_result=10 / concurrency=12；`handlers.rs` secret_redaction=8 / inline_tests=50 / unwrap_or=43 / concurrency=14；`transform_codex_chat.rs` secret_redaction=6 / inline_tests=84 / unwrap_or=53；`services/proxy.rs` secret_redaction=268 / sql_dynamic=2 / ignored_result=25 / inline_tests=72。注意 unwrap_or/ignored_result 是弱信号，正文阅读未把它们一律定性为缺陷。
- 真实性声明：四个文件的 SHA256 仅作快照指纹，不当作阅读凭证；内联测试（合计 289 个声明）未执行，仅作为"作者意图/回归线索"在个别处引用名称。

## 2. 代理的请求/响应转换与流式处理

### 2.1 请求管线（forwarder.rs 已读 1-3600 连续区间）

- 入口与计数：`forward_with_retry`（forwarder.rs:347-375）在客户端请求维度记 `total_requests`/活跃连接（RAII guard 102-114），per-attempt 计数在 inner。
- provider 循环与上限：`max_attempts = max_retries + 1`（forwarder.rs:143-148, 215-219），到达上限或有熔断拒绝即停止（423-448）；单 provider 场景跳过熔断检查（412-413）。
- 目标语义路由：`codex_responses_to_chat` / `codex_responses_to_anthropic` / 官方 Codex 透传判定（forwarder.rs:1145-1157）；官方透传要求客户端携带原生 authorization，拒绝占位符（43-57, 1155-1157）。
- 模型与请求体改写：ClaudeDesktop 未知 route 直接报错、不落默认模型兜底（forwarder.rs:1160-1168）；`[1M]` 后缀按路径差异剥离（1186-1201, 1493-1499）；私有 `_` 前缀字段出站前过滤，本地 override 合并（1579-1592, 3337-3434, 3485-3487）；受保护头（authorization/x-api-key/host/accept-encoding 等）禁止被 override（3436-3483）。
- 请求体转换矩阵：Responses→Chat（1445-1456，走 `transform_codex_chat`）、Responses→Anthropic（1457-1512，注入缓存断点、可选 Claude Code 拟态 1363-1368, 1500-1502）、Claude→OpenAI/Gemini（1513-1528）、原生 Responses 的 xAI 兼容清洗（1533-1573）。
- 认证注入：Copilot/CodexOAuth/xAI OAuth 均在发送前解析真实 token（forwarder.rs:1620-1775）；token 同时登记进 `log_secrets` 仅用于日志脱敏（1766-1770）。认证头按"原始位置替换"策略重建（1967-1990），缺失时追加（2086-2091）。
- 发送层：`accept-encoding` 在转换/SSE 路径强制 identity（2023-2037, 2093-2099）；保留原始 header 大小写只对原生 Claude 路径启用（3268-3283）；SOCKS5/常规路径走 reqwest 连接池（2225-2262）。
- 出站日志：目标 URL 双模式脱敏后记录，请求体只记 `bytes + hash`（forwarder.rs:2177-2199）；Gemini `?key=` 场景在 cache trace 中剥 query（3520-3521）。

### 2.2 响应转换（handlers.rs 已读 1-2674 连续区间）

- Claude 侧 transform：按 `api_format` 选择 Responses/Gemini/Chat 流式转换器（handlers.rs:411-428）；非流式在"未标记 SSE"时按 Chat/Responses 聚合兜底并携带诊断（528-570）。
- Codex Chat→Responses：错误体统一规整为 `{"error":{message,type,code,param}}`（handlers.rs:1192-1196, 1663-1732）；正常响应转换后落 `codex_chat_history`（1328-1339）。
- Codex Anthropic→Responses：SSE 直通或把非流式 JSON/未标记 SSE 聚合后转产生完整事件序列（handlers.rs:1438-1497）。
- 2xx 语义失败检测：Anthropic 错误包络与 Responses `failed/cancelled` 均在 retry 循环内被识别为失败，从而仍可 failover（forwarder.rs:2379-2430；handler 层聚合时同样显式失败 handlers.rs:2074-2080）。

### 2.3 流式语义（failover-safe 是本仓库的亮点）

- 成功不能只看响应头：非流式在 failover 开启时先把整个 body 读入内存（forwarder.rs:2341-2373），读超时/中断回 retry loop；流式至少等首个 chunk（2515-2548）。
- Responses 流起始校验：只把 `response.created/in_progress/queued` 视为生命周期事件继续等待，产出/终态事件才提交；2xx `response.failed`/error 事件在提交前触发 failover；256KB 缓冲上限后被迫提交（forwarder.rs:2433-2513, 3025-3095）。
- 提交后中途失败不再切换 provider（注释明示：暴露给转换器但有意不换家，避免双计费；forwarder.rs:3090-3093）。
- 请求侧流式检测覆盖 `stream:true`、Gemini `streamGenerateContent`/`alt=sse`、`Accept: text/event-stream`（forwarder.rs:3285-3303）。
- SSE 聚合边界处理：`event: error` 无论是否已聚合出 choice 都判失败（handlers.rs:2289-2299）；占位 `error:{}`/空消息不误杀（2300-2309）；缺 `finish_reason` 且无 `[DONE]` 判截断（2430-2442）；尾块半截 JSON 仅在"已拿到完成证据"时忽略（2027-2033, 2419-2428）；tool_call index 用 BTreeMap 防 `index=4e9` OOM（2256-2260）。

### 2.4 超时语义

- 非流式默认 600s（`non_streaming_timeout=0` 时），主请求超时只在显式非零时设置；流式总期限 24h，另加首包超时（forwarder.rs:2201-2260）。
- 首包超时同时用于 send 阶段与 prime 阶段（forwarder.rs:2244-2260, 2528-2535）；Responses 流起始校验也受首包超时约束（2447-2458）。
- handler 侧读 body 的超时依赖 `auto_failover_enabled && non_streaming_timeout>0`，否则传 `Duration::ZERO`（无显式超时）（handlers.rs:517-524, 1093-1100, 1291-1298, 1451-1458, 1674-1681）。
- 注意：`RequestForwarder::new` 的 `_streaming_idle_timeout` 参数未在 forwarder 内使用（forwarder.rs:210-211），静默期超时若存在只可能在 `response_processor`（未读，见"残余未决"）。

## 3. 错误/重试/超时语义与"是否静默降级"

- 重试对象：客户端错误 vs provider 错误分级见 `categorize_proxy_error`（forwarder.rs:2655-2711）——400/405/406/413/414/415/422/501 不重试；其余 4xx（401/403/404/408/409/429/451）与 5xx 重试；Timeout/ForwardFailed/ProviderUnhealthy/ConfigError/TransformError/AuthError/StreamIdleTimeout 可重试；NoAvailableProvider 与内部错误终止。
- 账号级特例：官方 Codex 的 AuthError 与 401/403、xAI OAuth 的 AuthError 一律 NonRetryable，防止"静默换号"（forwarder.rs:2655-2679）。
- 整流重试：每个 provider 独立、各一次——media 降级（forwarder.rs:554-669，图片被替换为标记后重试同家）、thinking 签名整流（672-820）、budget 整流（822-979）；整流后再失败的收尾语义见 283-338（provider 错误继续 failover，客户端错误立即返回）。
- 熔断耦合：Retryable 才记失败/污染健康度，NonRetryable 仅释放 HalfOpen permit（forwarder.rs:1003-1065）；整流重试不计熔断。
- 静默降级盘点（已读区间内）：

| 行为 | 可见性 | 锚点 | 评价 |
|---|---|---|---|
| 接管前 Token 同步到 DB 失败 | `log::warn` 后继续接管 | services/proxy.rs:1068-1078, 1129-1137, 1181-1191；外层 1245-1268 仍返回 Ok | 与 621-627 的 fail-fast 意图不一致（见 FP-02） |
| models 目录解析失败 → 返回 `{"models":[]}` | 无日志 | handlers.rs:95-101（读取失败才有 warn，98） | 静默降级（见 FP-04） |
| Copilot live model 列表不可用 → 跳过模型解析 | `log::debug` | forwarder.rs:2600-2605 | 低可见度降级 |
| 非 JSON 上游错误体 → 客户端 1KB 文本透传 | `log::warn`（日志 content omitted） | handlers.rs:1683-1704 | 面向客户端、非日志泄漏 |
| 接管/备份读取失败 → 重建接管 | `log::warn` | services/proxy.rs:763-773, 785-788 | 显式降级、有注释 |
| `non_streaming_timeout=0` → 无显式请求/读超时、不缓冲 body | 无提示 | forwarder.rs:2201-2244, 2354-2356；handlers.rs:517-524 | 取决于默认配置（未验证） |

## 4. 密钥/账号/日志边界

### 4.1 是否记录请求正文：否（已读区间）

- 出站请求日志明示 `content omitted`，只记字节数与 canonical hash（forwarder.rs:2195-2199）。
- cache trace 只记字段级 hash 与结构摘要（forwarder.rs:3489-3535），Gemini endpoint 先剥 query 再落盘（3520-3521）。
- 错误摘要只提取 message/detail 并 180 字符截断（forwarder.rs:2777-2835）；上游 body 诊断只输出 content-type/encoding/长度/分类（handlers.rs:2152-2203）。
- 工具调用被丢弃时只记 index/是否空 id/参数字节数，注释明示"不记 arguments 内容（可能包含用户代码）"（transform_codex_chat.rs:1570-1591, 1657-1669）。
- 例外边界：非 JSON 上游错误体会截断 1024 字节后放进**客户端响应**（handlers.rs:1683-1704）——不回写日志；Codex 代理错误体把 endpoint 原样回显（1740-1742, 1872-1873，注释声明仅用于本地 Codex 路由、不可复用到 query 携带凭证的端点）。

### 4.2 凭据如何存储与脱敏（已读区间）

- 接管时 Live 配置只写占位符 `PROXY_MANAGED`，真实 token 由代理出站时注入（services/proxy.rs:21-22, 174-224；Gemini 1584-1598；Grok 1536-1543）。Claude 占位符策略区分 managed account 与普通 provider（90-125, 181-224）。
- 真实 token 的流向：接管前从 Live 同步进 DB provider `settings_config`（services/proxy.rs:966-981, 983-1243），并显式跳过占位符与官方 Codex 行（1011-1013, 1093-1098）。此路径是明文写 SQLite 的调用（1068-1071; 1129-1133; 1181-1185），库内是否加密未在 T1 范围验证。
- 出站注入在 forwarder（1623-1772），并登记 `log_secrets` 供 URL 脱敏；`PROXY_MANAGED` 若被错误解析到认证头，发往 GitHub Copilot/ChatGPT backend/xAI 的出站前被硬拒绝（forwarder.rs:3230-3266 + 内联测试 4007-4056）。
- Live 备份含原始 token 的明文 JSON（services/proxy.rs:1392-1397, 1466-1471），关闭接管/停止时删除（882-886, 1329-1333, 2208-2212）；备份里的占位符配置被识别为异常历史并拒绝写回（2314-2328, 1841-1866）。
- Codex 官方登录保护：恢复/热切换时若 live `auth.json` 持有真实登录材料，优先保留 live、不写回备份快照（services/proxy.rs:2688-2722, 2739-2781, 2979-3019）。
- `services/proxy.rs` 的 probe 命中 secret_redaction=268 大部分来自测试与占位符分支；正文确认了"占位符不出站 + 真实值不入日志"这一对不变量，而不是靠字符串模式推断。

## 5. 与"会话检索"的关系（结论）

- 在这四个文件里，代理对"会话"的唯一兴趣是**身份与归因**：`metadata.user_id/session_id/x-session-id` 提取（forwarder.rs:1255-1301）、Codex OAuth 会话路由头（3208-3228）、用量行写入（handlers.rs:304-354, 2575-2660）以及 Codex Chat 历史的工具调用恢复（handlers.rs:1202, 1328-1339 走 `codex_chat_history` 缓存，缓存的是工具调用结构而非用户正文检索）。
- 这四个文件不包含：正文索引、FTS/BM25/语义检索、跨 agent 语料、历史会话列表/恢复/回放。会话管理与检索若存在，在分片 A 的 `session_manager`（另一条线），代理本身不做检索。
- 因此 cc-switch 的产品形态是"配置/凭据/流量三件事"：供应商切换 + 本地代理 + 用量统计；会话检索是它的**相邻功能**（分片 A）而不是代理能力。

### 对 ASG 的真实竞争压力

- 在：① 流量咽喉——能顺手拿到每次请求的 session 归因与 token/成本视图，形成"看用量"的会话侧入口；② 账号池/故障转移把"多 provider 之一"变成用户日常入口，桌面端托盘/接管是强触达；③ 与分片 A 的会话管理页组合后，构成"能看会话、看用量、切供应商"的一体化体验，对只做检索的产品形成入口竞争。
- 不在：检索质量/召回/语义评分/跨工具历史语料/未走代理的存量会话，这些代理一律不可见；CLI/MCP 集成深度、恢复/回放链路也不在此文件面内。

### 与 ASG 对照：该学 / 不该学

**该学：**

1. failover-safe 的流式提交协议：先首包预热/语义校验再记成功、2xx 失败包络参与重试、提交后中途失败不换家（forwarder.rs:2285-2312, 2341-2373, 2433-2513, 3090-3093）。ASG 的索引/写入一旦"看起来成功"，同样应延迟提交/水位推进。
2. 客户端错误与 provider 错误分离，避免把请求级错误放大成重试风暴、污染健康度（forwarder.rs:2655-2711）。对应 baseline SG-03/SG-04 的教训：失败不能推进成功水位。
3. 结构化+脱敏日志范式：bytes+hash 代替正文、URL 双模式脱敏、错误摘要最小化（forwarder.rs:2177-2199, 3489-3535；handlers.rs:2123-2203）。
4. 配置分级与崩溃安全：SSOT/Live/Backup 三层 + `PROXY_MANAGED` 占位符 + "先置恢复标记再写接管" + 崩溃后 `recover_from_crash`（services/proxy.rs:641-693, 1835-1895, 2198-2216）；严格模式与尽力而为模式分路径（1447-1474 vs 1681-1756）。
5. 幂等重入设计：`set_takeover_for_app` 在"有备份且 live 指向当前代理"时直接返回，半接管时重建（services/proxy.rs:751-788）。

**不该学（或需改造后再学）：**

1. 本地 HTTP 服务无入站鉴权 + 可配置 `0.0.0.0` 监听（见 FP-01）：ASG 若暴露本地 API，应默认回环 + token 校验。
2. 多层兜底堆叠的复杂度：SSE 嗅探聚合、双占位符策略、三种 Key 命名兼容同时存在（handlers.rs:528-570；services/proxy.rs:181-224）——对检索类产品是维护负担而非价值。
3. 凭据多处落盘：Live↔DB↔Backup 都有明文 token 面（services/proxy.rs:1392-1397 等），ASG 不需要凭据的场景不应复制这种存储模型。
4. "同步失败仅 warn"的降级（FP-02）：ASG 的索引/水位写入失败必须显式化，不能以继续执行掩盖陈旧状态。
5. debug 级静默降级（FP-04）：重要降级应进入可观测事件流。

## 6. Findings

严重度标尺：P1=高（数据丢失/安全边界可被现实利用）；P2=中（正确性/可运维性风险，有使用条件）；P3=低（体验/一致性）。

### FP-01 · P2 · 主代理端点无入站鉴权（仅 Claude Desktop 网关校验 Bearer）

- 证据：`handlers.rs:57-71` 健康/状态端点无鉴权；`handlers.rs:129-149` 与 `261-286` 显示唯一带 Authorization 校验的入口是 Claude Desktop 网关（`get_or_create_gateway_token` + Bearer 比对）；其余 `/v1/messages`、`/v1/chat/completions`、`/v1/responses`、`/v1beta/*` 处理器（122-127, 703-709, 769-788, 1929-1933）未见鉴权检查。监听地址来自 `config.listen_address`（server.rs:100-117，grep 核对），`services/proxy.rs:1484-1490` 显式处理 `0.0.0.0` 监听形态。
- 可达性：当用户把 listen_address 设为 `0.0.0.0`/非回环地址时，同一网络的任何进程可消耗该代理背后的账号额度；健康/状态端点也会泄露代理状态。
- 反证/界限：默认回环场景威胁有限；本次未全文精读 `server.rs`（仅 grep 核对绑定与无 auth 字样），也未验证默认 `listen_address` 值（默认值定义不在 T1 四文件内）。
- ASG 教训：任何本地守护进程默认只绑回环；即使回环也加显式 token（cc-switch 在 Claude Desktop 网关做到了，应推广到全部端点）。

### FP-02 · P2 · 接管前 Token 同步"写库失败仅告警继续"，与 fail-fast 意图不一致

- 证据：`start_with_takeover` 注释声明 Token 同步失败要清理备份并中止（services/proxy.rs:620-627），但 `sync_live_config_to_provider` 中 DB 写失败只 `log::warn` 后继续（1068-1078, 1129-1137, 1181-1191），外层 `sync_live_to_providers` 仍返回 Ok（1245-1268）。
- 可达性：写库失败 + 接管继续 → Live 被写占位符，而 DB 仍是旧 token（或缺失），代理出站认证使用陈旧凭据 → 401 循环；数据库与 Live 双端都无法确认最新凭据。
- 反证/界限：接管前的 Live 备份仍保留原始 token（1382-1397），恢复路径可取回；读取失败会经 `read_claude_live()?` 中止。
- ASG 教训：索引/SSOT 写入必须区分"读失败"与"写失败"，写失败必须阻断状态推进（或降级为只读旧水位），不能只留日志。

### FP-03 · P2（正面） · 流式 failover-safe 提交协议完整，但提交后错误窗口依赖上游契约

- 证据：非流式全量缓冲、流式首包预热、Responses 语义起始校验（forwarder.rs:2341-2373, 2433-2513, 2515-2548）；2xx 错误包络参与 failover（2285-2312）；测试名与实现一致（3889-3982 的 4 个 tokio 测试）。
- 反证/界限：提交后中途失败明确不再换 provider（3090-3093 注释），此时用户侧看到的是上游错误或转换错误——这是"避免双计费"的有意取舍；`inspect_responses_start_event` 对未知事件一律视为可提交（3090-3093 之外的默认分支 3093），语义盲区依赖上游规范。
- ASG 教训：把"已成功"定义成缓冲区级证据（首字节/终态事件）而不是 HTTP 状态码；提交点与回滚点显式化。

### FP-04 · P3 · 两处静默降级：models 目录解析失败无日志、Copilot 模型解析仅 debug

- 证据：`handlers.rs:95-101` JSON 解析失败直接回 `{"models":[]}` 且无日志（仅文件读取失败有 warn）；`forwarder.rs:2600-2605` live model 列表不可用时 `log::debug` 后跳过解析，可能导致模型名不被归一化。
- 反证/界限：两者都是"返回空/跳过"而不是伪造成功；目录为空时 Codex 会视为可达性检查失败而非静默错误；Copilot 场景有活跃模型缓存兜底这一概率降低。
- ASG 教训：解析失败≠空数据。空结果与解析失败要在数据与日志上有不同表达。

### FP-05 · P2（正面） · 日志脱敏与占位符不变量成体系

- 证据：请求体 bytes+hash（forwarder.rs:2195-2199）、URL 双模式脱敏（2177-2186, 3520-3521）、错误摘要最小化（2777-2835）、诊断分类不落正文（handlers.rs:2123-2203）、工具参数只记字节数（transform_codex_chat.rs:1570-1591）、`PROXY_MANAGED` 拒绝出站（forwarder.rs:3230-3266）。
- 反证/界限：DB provider 配置与 Live 备份含明文 token（services/proxy.rs:1068-1071, 1392-1397, 1466-1471），加密/凭据库存储未验证；`log_secrets` 仅脱敏 URL，其他日志若直接拼接 token 不在此机制覆盖内（已读区间未见此类日志）。
- ASG 教训：把"内容不进日志"做成结构性机制（只暴露 hash/长度/分类），并把密钥脱敏绑定到统一收口函数。

### FP-06 · P3 · 超时零值语义不统一：failover 关闭时可能无显式超时、无缓冲

- 证据：`non_streaming_timeout=0` 时不再设置 reqwest timeout（forwarder.rs:2237-2239）、不缓冲非流式 body（2354-2356）；handler 侧读取 body 超时仅在 `auto_failover_enabled && non_streaming_timeout>0` 时启用（handlers.rs:517-524）。默认 600s 只在 `non_streaming_timeout.is_zero()` 且走 hyper 路径/流式首包回退时使用（forwarder.rs:2202-2206, 2245-2249）。
- 反证/界限：`auto_failover_enabled=false` 是显式配置选择，语义是"单家直连，不做 failover 缓冲"；默认配置值不在已读区间（未验证）。
- ASG 教训：超时用 Option/显式枚举表达（禁用 vs 默认值），避免 0 同时承担"禁用"与"默认"两种语义。

## 7. 残余未决（对主会话透明）

1. 静默期超时实现未确认：`_streaming_idle_timeout` 在 forwarder 中未使用（forwarder.rs:210-211）；实际是否由 `response_processor::create_logged_passthrough_stream` 的 timeout_config 执行（handlers.rs:492-500 传入）未读该文件。
2. 凭据静态加密：DB（SQLite）与备份是否加密、是否走 OS 凭据库，未在 T1 范围内验证（本报告只确认"明文写入调用"事实）。
3. `server.rs` 的入站鉴权结论为 grep 级（无 auth/Bearer/401 字样命中，绑定 listen_address），未全文精读。
4. 默认配置值（listen_address / non_streaming_timeout / max_retries / auto_failover_enabled）定义不在四个 T1 文件内，未验证。
5. 未执行任何测试/构建；四个文件内联合计 289 个 `#[test]/#[tokio::test]` 声明仅按名称采信，测试区间（forwarder 3601-3888、handlers 2675-3348、transform 1976-4627、services/proxy 3134-7749 等）未逐行读取。
6. `services/proxy.rs` 1897-2197（SSOT 重建与占位符清理实现）与 2565-2626（热切换回滚收尾）未读，相关结论仅基于调用点与函数表。