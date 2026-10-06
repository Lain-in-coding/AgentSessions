# cass 分片 D 审计：lib.rs 脊柱 + sources 连接器（T1 回执）

- 审计对象：`C:\AgentSessions\Github_src\coding_agent_session_search`（cass），commit `aa92a45311e10a3ac9c40c3730664a7366af2fe8`（沿用任务给定值，并与分片 A/B 回执交叉核对；本分片未执行任何 git 操作）。
- 日期：2026-10-06。范围：T1 精读 `src/lib.rs`（partial，指定子系统全覆盖）、`src/sources/sync.rs`（全文）、`src/sources/config.rs`（全文）；T2 机械扫描只按 path 过滤 `sweep-coding_agent_session_search.json`，不整读。
- 口径：静态源码推导，行号为 1-based；未执行 build/test/install/网络/git 写操作；未修改竞品源码与其它 TASKDIR 文件。
- 覆盖：`src/lib.rs` 2,699/100,119 行（22 个有界窗口、13 个 missing 段）；`src/sources/sync.rs` 4,612/4,612；`src/sources/config.rs` 2,743/2,743。三文件读前/读后 SHA-256 一致（3/3）。
- 方法披露：`codegraph node --file src/lib.rs` 返回 “No indexed file matches”（4.05 MB 未入索引），改用 PowerShell 行号有界窗口（目标 ≤150 行；其中 7 个窗口因循环上界 off-by-one 实际打印 151 行，如实记录，不宣称 ≤150）。机器凭证见同目录 `coverage-cass-lib.json`。

## 一、lib.rs 为什么 100K 行（测试/生成占比）

| 口径 | 数值 | 锚点/方法 |
|---|---:|---|
| 总行数 / 字节 | 100,119 / 4,047,370 | 文件统计 |
| 模块声明 | 70（`pub mod` 63 + `pub(crate) mod` 5 + 条件/其它 2） | `src/lib.rs:3-72` |
| 内联测试模块 | 33 个 `#[cfg(test)] mod *_tests`；471 个 `#[test]` | 字符串/注释感知括号匹配计数脚本 |
| 测试行数 | 18,991 行 = 全文件 19.0% | 同上 |
| 最大测试模块 | `doctor_asset_taxonomy_tests` 9,665 行（57576-67240）；`cli_read_db_tests` 2,041（71275-73315）；`indexed_conversation_fallback_tests` 1,494（91418-92911） | 同上 |
| 非测试行 | ≈81,128 | 100,119 − 18,991 |
| 最大顶层非测试项 | `run_doctor_impl` 2,834 行（73428-76261）；`Cli`+`Commands` 1,311（226-1536）；`output_robot_results` 1,127（25553-26679）；`run_cli_search` 1,098（22464-23561）；`run_index_with_data` 802（86474-87275）；`run_export_html` 772（89309-90080） | 括号跨度排名 |
| 内联 JSON | 1,064 处 `serde_json::json!`，覆盖 9,754 行（非测试 8,927） | 宏括号匹配 |
| 空白 / `//` 注释行 | 5,430 / 4,219 | 纯文本统计 |

- **不是生成代码**：lib.rs 内无 `include!`/`OUT_DIR`/生成头；`build.rs:1-60` 只做依赖 pin 与契约校验（辅助有界读）。所以 100K 行 = ~19% 内联测试 + ~81% 手写内容：CLI 表面、逐命令编排（doctor/export/html/status 等）、robot 输出拼装、以及内联 JSON schema/帮助文本——仅 `json!` 宏体就占近 1 万行（schema 区（约 81600-84600）是密集来源之一）。
- 体量高度集中：单个 `run_doctor_impl` 就是 2,834 行。这既是"脊柱即全产品面"的体现，也是维护性风险（D-05）。

## 二、CLI 表面与命令清单

- 全局 `Cli`（226-269）：`--db`、`--robot-help`、`--trace-file`（env `CASS_TRACE_FILE`）、`-q/-v`、`--color`、`--progress`、`--wrap/--nowrap`、全局 `--robot-format`、可选 subcommand（267-268）。
- `Commands`（273-1536，44 个变体）：Tui、Index、Completions、Man、RobotDocs、Search、Pack、Stats、Diag、Storage、Dedup、Status、Capabilities、Triage、SupportBundle、State、ApiVersion、Introspect、View、Health、Onboarding、Guide、Doctor、Context、Sessions、Resume、Upgrade、Export、ExportHtml、Expand、Timeline、Pages、Quarantine、Forget、Mirror、Sources、Models、Fleet、Lessons、Swarm、Import、Analytics、ReleaseVerify、Daemon。
- 嵌套子命令族（11 个 `#[derive(Subcommand)]`）：QuarantineCommand(1540)、MirrorCommand(1602)、SwarmCommand(1641)、ImportCommand(1922)、SourcesCommand(1941)、ModelsCommand(2160)、FleetCommand(2253)、LessonsCommand(2289)、MappingsAction(2354)、AgentsAction(2398)、AnalyticsCommand(2451)；其中 MappingsAction/AgentsAction 是 SourcesCommand 的动作族。
- robot 表面：`RobotFormat{json,jsonl,compact,sessions,toon}`（2651-2663）；65 处 `visible_alias="robot"`；`is_robot_mode`（19799-19840+）在结构化模式抑制 INFO 日志；human 模式的 readiness 提示走 stderr、stdout 只留结果（23541-23558）。
- 规模：`#[arg` 471、`#[command` 21。`Pack` 参数表 565-652：`--limit 0` 表示"用 planner 默认抓取公式"；`--max-tokens 12000 / --max-sessions 8 / --max-evidence 24 / --context-lines 3 / --max-excerpt-chars 1600`；`--freshness-policy prefer-recent|strict|allow-stale` + `--freshness-window-seconds`；`--require-evidence / --explain-selection / --refresh / --timeout`。

## 三、pack CLI 接线：limits / readiness 的实际注入（分片 A 遗留）

**结论：limits 确实从 CLI 注入 pack_planner，且默认与校验完全对齐；readiness 也有注入，但语义缩水为常量/空字段（字段在、生产者缺）。**

注入链（全部在 `src/lib.rs`）：

1. dispatch `Commands::Pack`（6762-6803）先 `resolve_search_defaults(timeout, limit, mode)`（6767-6768；函数定义 22395，函数体在未读区间）→ `run_cli_pack(..., eff_limit, ..., max_tokens, max_sessions, max_evidence, context_lines, max_excerpt_chars, ...)`（6769-6803）。
2. `run_cli_pack`（23564-23911）构造 `PackPlannerLimits{max_tokens,max_sessions,max_evidence,context_lines,max_excerpt_chars}`（23629-23635）→ `limits.validate()`（23636；错误映射 `pack_invalid_limit_error` 23954-23968，错误 kind `pack-invalid-limit`）。
3. 候选上限：`--limit==0` → `pack_candidate_fetch_limit(&limits)`（planner 公式，分片 A 已读 :734-743）；否则直接用 CLI `--limit`（23750-23754）。
4. `client.search_with_fallback(query, filters, candidate_limit, 0, ...)`（23756-23764）→ `PackCandidate::from_search_hit` 批量转换并补 `hybrid_rank`/`source_explicitly_requested`（23794-23808）→ `PackPlanRequest{now_ms, limits: limits.clone(), freshness_policy, freshness_window_seconds, candidates, explain_selection}`（23811-23818）→ `plan_answer_pack`（23819）→ `PackRenderRequest{... limits ...}`（23831-23868）。
5. 因此分片 A 的遗留问题（“CLI 实际传入的 limits”）答案明确：CLI 默认 12000/8/24/3/1600（584-597）与 planner 默认/validate（分片 A :66-87）一致，`--limit` 只影响候选抓取量，不覆盖输出预算。

readiness 实际注入（23848-23867，字面构造，无外部输入）：

- `index_generation`：仅当 pack 前 lexical self-heal（23672-23688）写入了 docs 时才是 `indexed_docs:N`，否则 null（23849-23851）；
- `lexical_readiness`：`rebuild_active ? Rebuilding : Ready`（23852-23856）——不读 stale/partial/corrupt 状态，**默认乐观 Ready**；
- `semantic_readiness`：`mode_meta.fallback_tier.is_some() ? FallbackLexical : Disabled`（23857-23861）；
- `source_sync_gaps: Vec::new()`（恒空，23865）、`recommended_action: None`（恒空，23866）、`missing_database: !db_path.exists()`（23864）。
- 对照分片 A 读到的 `health{warnings,recommended_action,source_sync_gaps,source_readiness}`（`search/pack_planner.rs:561-573,1556-1656`）：在 CLI 路径上 health 只能由上述两态推导，source_sync_gaps/recommended_action **永不产出**（P2，D-01）。
- 补充：`--mode semantic` 在 pack 直接 fail-closed（code 15，23720-23727），hybrid 标记 lexical fallback（23729-23732）；`--fields` 只对 JSON 系列做投影（23883），JSONL 时忽略并 warning（23893-23895），Markdown 分支静默无投影（P3；残余项，不单列发现 ID）。

## 四、cursor 字符串编解码与跨页一致性

- 定义：`search --cursor`（443-445）——“base64-encoded offset/limit payload from previous result”。
- encode（25308-25318）：`BASE64_STANDARD(JSON {"offset":N,"limit":M})`；`limit==0` → 不产出。
- 发出（25972-25985）：`has_more = has_more_results || clamped_unemitted_hits`；`next_cursor = encode(offset + returned_count, cursor_page_limit)`，仅当 `cursor_page_limit>0 && returned_count>0`。
- decode（22542-22570）：base64 → JSON；`offset`/`limit` 覆盖 CLI 值（cursor 优先于同时给出的 `--offset/--limit`）；字段缺失时静默沿用 CLI 值；错误 kind 为 `cursor-decode`（base64 失败）/`cursor-parse`（JSON 失败）（`src/model/cli_error_kind.rs:68-69,174-175`）。
- 页大小：`cursor_page_limit`（22925-22931：聚合→limit；limit=0→token budget 页大小；否则 limit）+ 非聚合 overfetch by one（22922-22940）。
- 跨页一致性评估：
  - 单页自洽：offset 在去重/过滤之后按“实际发出的命中数”推进（25981-25985），与分片 B 的 fetch-from-0→后过滤语义一致；
  - 绑定缺失：cursor 不携带 query/filters/mode/fields/**generation**，无签名/TTL；复用条件只出现在 `continuation_reason` 文案（"reuse the same query, filters, mode, and fields"，25361）。两页之间索引变化或调用方改参 ⇒ 可能重复/漏行，且没有 `generation_mismatch`/`cursor_expired` 类错误（对照 ASG `CanonicalCode` 的 cursor_invalid/cursor_expired/generation_mismatch，`.trellis/spec/agentsessions-cli/backend/error-handling.md:28-35`）。
  - manifest 缓解：`cursor_manifest`（25343-25450；schema 81839-81918）给出 `continuation_safe/continuation_reason/count_precision/next_offset/requested_limit/realized_limit/returned_count/search_page_count/field_mask/token_budget/cache_generation/index_generation{lexical_shard_generation,freshness,stale,partial,partial_reason,rebuilding,pending_sessions}/semantic_fallback`；但 `continuation_safe = next_cursor_present && !timed_out && !index_rebuilding`（25349）只反映**本页生成时**的信号，不比对上一页 generation，也不阻止重放旧 cursor。
  - `total_matches` 可能是下界（`count_precision=lower_bound`，25363-25372），分页终止以 `has_more` 为准。
- 判定：P2 契约缺口（D-02）；亮点是把“为什么不能续页”做成了机器可读理由。

## 五、robot / JSON 元数据字段

- search `_meta`（仅 `--robot-meta`；26116-26149）：`elapsed_ms、search_mode、requested_search_mode、mode_defaulted、fallback_tier、fallback_reason、semantic_refinement、refinement_level、semantic_fallback_reason、wildcard_fallback、cache_stats{hits,misses,shortfall,prewarm_scheduled,prewarm_skipped_pressure}、timing{search_ms,rerank_ms,other_ms}、tokens_estimated、max_tokens、request_id、next_cursor、hits_clamped、query_plan、cursor_manifest、explanation_cards`。
- 条件块：`state`（26150）、`index_freshness`（26155）、`search_completeness`（26162）、`storage_integrity`（26169）、`timeout_ms/timed_out/partial_results`（26174-26183）、`ann_stats`（26185-26191）；顶层 `_warning`（26195）、`_timeout`（26202）。
- JSONL 的 `_meta` 是另一份独立拼装（26250-26280 起）；compact/toon/sessions 分支也在各自位置手写 cursor/_meta（grep 锚点：26413、26455-26458、26504、26547、26589-26592、26637；这些行在未读区间，仅机械锚点）。
- dry-run 同样有结构化输出：`{dry_run, valid, query, explanation, estimated_cost, warnings, request_id, _meta}`（22600-22623）。
- 契约 schema：`response_schema_search_meta`（82009+）、`cursor_manifest`（81839-81918）、`query_plan`（81766-81837）、`explanation_cards`（81920-81958）、`search_completeness`（81960-81977）、`root_cause_attribution`（81979-82007）。
- pack robot：`--json/--robot` → `PackRenderFormat`（23935-23951）；`--robot-format sessions` 明确拒绝并给替代命令（23940-23949）；pack 顶层字段白名单与预设（24115-24198：minimal/summary/all 与单字段/前缀白名单）。
- 风险：同一 `_meta` 语义在多个输出分支平行手写（26117、26250、26413±、26547±），存在漂移可能（P2，D-04）。

## 六、核心类型与错误

- `CliError{code:i32, kind:&'static str, message, hint, retryable}`（2807-2814）；`CliResult`（2818）；`Display` 输出 `"<message> (code N)"`（2820-2824）；`already_reported` 哨兵（2816、2829-2851）。
- kind 词表：`src/model/cli_error_kind.rs:59-156`（约 95 个变体，含 `CursorDecode/CursorParse/PackEmptyQuery/PackInvalidLimit/PackInvalidField/PackNoEvidence/PackUnsupportedFormat/SemanticUnavailable/Timeout` 等），`kind_str`（164+）注释要求与 lib.rs 字面量 golden 一致（160-163；文件底部 golden 测试未读）。
- code 分布（lib.rs 内 `code: N` 字面量，机械计数 427 处）：9×102、2×59、4×43、5×39、10×31、13×15、14×15、3×14、1×13、11×9、15×8、7×7、20×6、22×5、21×4、6×3、23×2、0/24 各 1；缺 8、16-19。没有单一 code→语义映射表，各命令内联构造（对照 ASG 的 CanonicalCode + schema 漂移测试）。
- 其它核心类型：`ProgressResolved/WrapConfig`（2874-2895）、`SearchModeMeta`（25192-25246，含 fail-open 到 lexical、refinement level、typed fallback reason）、`Aggregations`（2798-2805）、`RobotFormat/DisplayFormat`（2651-2675）。

## 七、sources/config.rs 与 sources/sync.rs

### 7.1 config.rs（全文 2,743 行；测试 1423-2743）

- 模型：`SourcesConfig{sources, disabled_agents}`（118-128）；`SourceDefinition{name,type,host,paths,sync_schedule,path_mappings,platform}`（131-164）；`SyncSchedule{manual,hourly,daily}`（413-438）。
- 配置路径：`XDG_CONFIG_HOME` → 已存在的 platform dir → 已存在的 `~/.config/cass/sources.toml` → 否则 platform 路径供创建（643-672）。
- 读写生命周期：`load/load_from`（444-470；缺失→空配置）；`save/save_to`（472-503）= validate → TOML → **round-trip 解析** → temp(create_new) → 原子替换；`write_with_backup`（1158-1189）先 copy 唯一备份再写。load 只做结构校验（523-525），坏 path 保留给操作级报告（测试 1741-1764），再保存会被严格校验拦截（1760-1763）。
- 校验：名字非空/无前后空白/保留名 `local`/无路径分隔/非 `.`/`..`（298-331）；SSH host 仅 ASCII+`.-_@`、无空白控制符、不以 `-` 开头、至多一个 `@`（333-369）；paths 非空/无空白/无控制符（371-387）；path_mappings 非空且 agents 无空元素（214-240）；重名大小写不敏感（51-57、527-543）；`disabled_agents` 归一化（`claude-code`→`claude`、`open-claw`→`openclaw`；63-70、601-640）。
- 原子写细节：temp `create_new`+`sync_all`（1346-1374）；替换不跟随符号链接（测试 1508-1537）、识别 dangling symlink（1320-1327、1541-1555）；Windows 备份-替换-回滚链（1247-1318）；备份名含 pid+时间戳+nonce（1376-1395）。
- SSH config 发现：`discover_ssh_hosts`/`parse_ssh_config`（749-837）只读 `~/.ssh/config`，跳过通配/否定模式，供 SFTP 回退取 user/hostname/port/identity_file。
- 生成器：`SourceConfigGenerator`（966-1145）从 probe 生成 remote source、paths（仅检测到者并去重）、path_mappings（remote_home/projects→local、/data/projects）、platform；probe 失败/重名/无效定义分别记 `SkipReason`（860-871、1103-1141）。

### 7.2 sync.rs（全文 4,612 行；测试 3213-4612）

- **连接器生命周期**：`SyncEngine{local_store, connection_timeout=10s, transfer_timeout=300s}`（855-873）；mirror 布局 `{data_dir}/remotes/{source}/mirror`（875-881）；`sync_source`（1019-1104）= 校验（remote/host/paths/structure）→ mirror 准备（反符号链接）→ 按需远端 HOME 探测（1043-1056；`ssh host printf CASS_HOME_MARKER:$HOME`，904）→ openrsync 探测（1058-1074）→ 逐 path 同步 → 聚合报告；`sync_all` 单源失败不拖垮其它源（1106-1120）。
- **传输阶梯**：`detect_sync_method`（972-1013）：native rsync → Windows WSL rsync → system scp → ssh2 SFTP；`RsyncArgProtection{None,ProtectArgs,SecludedArgs}`（47-105）+ 远端 openrsync 探测与每 host 缓存（177-255、185-222）+ 远端拒绝保护旗标时"去旗标+手工 shell 引用"重试（1248-1287）。
- 各传输实现：rsync（1132-1343；`-avz --links --safe-links --stats --partial`，**无 `--delete`**，1185；`--timeout=transfer_timeout`）；WSL（1349-1510；Windows 路径→`/mnt/<drive>`，2436-2447）；scp（1521-1852；先 `find -P … -type f -print0` 只列常规文件（263-268），再逐文件 `scp -B` 下到 temp→`sync_all`→原子发布（1714-1830），`StrictHostKeyChecking=yes`（1753））；SFTP（1858-2126；ssh2，认证顺序 agent→identity_file→默认 `~/.ssh/id_{ed25519,rsa,ecdsa}`（2136-2183），lstat 跳过远端符号链接（2196-2236、2303-2319），32KB 分块→temp→replace（2334-2369））。
- **同步失败/部分失败**：单 path 失败不中止（1075-1100）；`SyncResult{Success,PartialFailure,Failed,Skipped}`（2586-2619）；`all_succeeded/successful_paths/failed_paths`（769-795）；稳定原因映射 `sync_failure_reason`（686-718：host_key_verification/missing_host/missing_paths/invalid_source/authentication/invalid_path/connection_timeout/connection_refused/command_unavailable/remote_path_not_found/local_io/transfer_failed）；`SyncTransportDecision` 账本（522-659：chosen/auth_source/fallback_rationale/attempted_transports[unavailable/failed/chosen/not_attempted]）。
- 状态与调度：`SyncStatus`（2848-2930；落盘 `sync_status.json`，原子写 2855-2879）；`update` 维护 `consecutive_failures`（2882-2894）；`SourceSyncDecision::evaluate`（2696-2812）+ `SourceHealthKind{never_synced,healthy,stale,high_latency,flapping,auth_failed,backing_off}`（2643-2668）；退避 5min×2^n 上限 1h（3032-3043）；高延迟阈值 60s（2933）；auth 失败识别（3045-3055）；`SyncAction{sync,skip,defer}` 与手工覆盖（2621-2641、2772-2784）。
- **隐私与网络边界**：出站仅 SSH/rsync/scp/sftp 到配置 host（SFTP 走 DNS 解析，1951-1973）；无遥测、无第三方 API；凭证不落地——rsync/scp 委托系统 OpenSSH（`-e`/`-S ssh`，140-153、1744-1753），SFTP 用 ssh2 agent 或既有私钥路径（2148-2183），不复制、不缓存私钥；本地所有写路径做符号链接拒绝 + 原子发布；远端符号链接默认跳过（scp `find -P`、sftp lstat、rsync `--safe-links`）。新增网络成本：首 sync 额外 1 次远端 HOME 探测（仅当路径含 `~`）+1 次 openrsync 版本探测（rsync/wsl 路径）。
- 已知弱点：① additive-only + `--partial`，远端删除/轮换不会反映到镜像，本地镜像长期后向累积（模块文档 6-11 自述，无 GA 语义）；② 全部 path 失败仍 `Ok(SyncReport)`（见 D-03）；③ `SyncStatus` Windows 替换窗口非事务（备份-替换-回滚，3092-3168）；④ WSL 分支无条件 `--protect-args`（1421），对 <3.0 rsync 不适用（未验证可达）；⑤ `parse_ssh_host` 对多 `@` 直接 split（2454-2462），但 config 校验已限一个 `@`（401-411）。

### 7.3 与 lib.rs 脊柱的接线

- `sources` 命令族（1449、1941+）与 sync 状态/决策在 lib.rs 的消费面落在未读区间；本片只确认 config/sync 自身契约。
- 落差：sync 侧有完整 health/backoff/decision（2696-2812），但 pack 的 `source_sync_gaps`/`recommended_action` 恒空（23865-23866）——就绪度信息未贯通到 pack 输出（D-01 的一部分）。

## 八、与 ASG 对照：强在哪、弱在哪、该学/不该学

**cass 强项（ASG 可学）**

1. 同步/传输的“可解释账本”：`SyncTransportDecision`（522-659）+ 稳定失败分类（686-718）+ 调度健康/退避（2646-2812、3032-3043）。ASG 若做远端源，可把 attempted_transports/auth_source/fallback_rationale/failure_reason 映射进 ProtocolError details，把 health/backoff 映射进 meta；sessiongrep 等竞品没有这层。
2. pack limits 真接线 + 校验 + 默认对齐（23629-23636、23750-23754、23811-23819）：可作为 ASG handoff pack limits 的落地模板（补上分片 A §5.3 所缺的 CLI 端证据；遗留只在 readiness）。
3. cursor manifest 的“结构化理由”（81839-81918、25343-25450）：continuation_safe/continuation_reason/count_precision/field_mask/token_budget/cache/index generation。ASG 已有更严的 cursor_invalid/expired/generation_mismatch 错误层，可吸收“把失败理由结构化进 meta”的粒度。
4. config 写入工程：写前校验 + round-trip + temp create_new + 原子替换 + 备份（473-503、1158-1189、1346-1374）；名字/host 注入白名单（298-369）。ASG 的配置与持久化面可照此验收。
5. CLI 表面广度：44 顶层命令、65 处 robot 别名、全局 `--robot-format`；human 提示走 stderr 保持 stdout 干净（23541-23558）。ASG 的单一 envelope 更严格，但可借鉴“每个子命令都有结构化路径”的清单纪律。

**cass 弱项（不要学）**

1. pack readiness 乐观常量与空字段（23848-23867）：字段在、生产者缺——正是 ASG 契约明确反对的形态。
2. cursor 纯位置分页、无 generation 绑定、无过期/签名（25308-25318、22542-22570）：ASG 不应退化掉 cursor_expired/generation_mismatch 语义。
3. 错误码内联散落、无单一映射（lib.rs 427 处 `code:` 字面量 + kind 字符串；`cli_error_kind.rs:59-163` 只保证 kind 词表）：ASG 的 CanonicalCode+漂移测试（`agentsessions-cli/backend/error-handling.md:26-56`）更强。
4. 单文件 100K 行脊柱 + 多分支平行手写 robot `_meta`（D-04/D-05）：不要把 CLI/编排/schema/测试堆进一个文件，也不要多份平行拼装同一元数据。
5. sync 软失败 `Ok(report)` 与 additive-only 镜像（1019-1104、8-11）：若 ASG 做同步，必须显式定义“全失败”的退出语义与“远端删除”的处理，不能默认照抄。

## 九、本分片 Top-5 发现

| ID | 级别 | 一句话结论 | 关键锚点 |
|---|---|---|---|
| D-01 | P2 | pack readiness 注入是常量/空字段：lexical 恒 Ready（除非有重建锁）、semantic 仅看 fallback、source_sync_gaps/recommended_action 恒空；pack 的 health 块在 CLI 路径失去信息量 | `src/lib.rs:23848-23867`（23852-23856、23864-23866）、23672-23691 |
| D-02 | P2 | cursor = base64(offset,limit)，无 query/generation 绑定、无过期；跨页一致性靠调用方复用参数，manifest 只暴露 generation 信号不校验 | `src/lib.rs:25308-25318`、22542-22570、25349-25361、25430-25441 |
| D-03 | P2 | sync_source 全 path 失败仍返回 Ok(report)；退出/调用契约依赖调用方检查 all_succeeded；additive-only 镜像不反映远端删除 | `src/sources/sync.rs:1019-1104`、798-813、6-11、1185 |
| D-04 | P2 | search `_meta` 在 JSON/JSONL/compact/toon 多分支手写拼装 + 条件块分裂；schema 存在但分支等价性未证 | `src/lib.rs:26117-26249`、26250-26280、82009-82059；grep 锚点 26413/26504/26547/26637 |
| D-05 | P3 | lib.rs 100K 行/4MB：19% 内联测试（33 模块/471 测试/18,991 行），最大函数 2,834 行，`json!` 宏体 ~9.8K 行；无生成痕迹 | `src/lib.rs:226-1536`、73428-76261、25553-26679、22464-23561 |

**各条证据/反证要点**

- D-01 证据：readiness 快照是字面构造，无 stale/partial 输入；反证：pack 前 lexical self-heal 与重建锁检测覆盖了“正在重建”的主场景（23672-23691），且 `index_generation` 在 self-heal 有写入时给出 `indexed_docs:N`。定性：元数据可信度不足而非功能性错误。
- D-02 证据：cursor 载荷仅 offset/limit，decode 静默合并；反证：单页内 offset 在去重/过滤之后推进（25972-25985），continuation_reason 已把复用条件写清（25349-25361），manifest 提供 generation 字段可供外部比对。定性：跨页正确性契约缺口。
- D-03 证据：`sync_source` 只对结构错误返 Err，全部路径失败也是 `Ok(SyncReport{all_succeeded:false})`；反证：`SyncStatus.update` + 调度器会把失败转为 Flapping/BackingOff/Defer 并保留 per-path error（2882-2894、2715-2762），说明软失败是有意设计，风险集中在调用方/退出码。
- D-04 证据：JSON 分支 26117-26149 与 JSONL 分支 26250 起各自拼装；反证：`response_schema_tests`（83769-84561）与 `subcommand_robot_output_tests`（99747-99955）覆盖 schema/输出，但本片未读完四分支，不能证明逐字段等价。
- D-05 证据：计数脚本 + 跨度排名；反证：单文件集中使错误/格式解析全局一致、测试贴近实现，属工程权衡而非功能缺陷。

## 十、残余未决（Open Items）

1. **lib.rs 未读 97,420 行 / 13 段**（missing_ranges：371-564、655-2644、2701-2789、2906-6599、6831-19789、19841-22519、22641-22909、22971-23539、23991-24109、24261-25191、25591-25959、26261-81759、82061-100119）；26261-81759（doctor/export/html/schema 主体）与 82061-100119（schema 测试主体）尤其大。任何依赖这些区间的结论均未验证。
2. robot `_meta` 在 compact/toon/sessions 分支的字段集未逐字对齐（只读 JSON/JSONL 开头）；“多分支等价”未证。
3. `resolve_search_defaults`（22395）函数体在未读区间，CLI/env/config 优先级的实现细节未验证（只有 6684-6691 注释与调用点）。
4. `sources/mod.rs` 的 SSH 辅助（`strict_ssh_cli_tokens`、`configure_child_process_group`、`wait_for_child_output_with_timeout`、`host_key_verification_error`）未读；StrictHostKeyChecking/BatchMode/超时的实际选项内容未核。
5. 未运行任何 build/test/网络/git 写；传输回退、Windows 文件锁、rsync/scp/sftp 真机行为未实测。
6. pack_planner 的 health 渲染如何消费空 source_sync_gaps/recommended_action 依赖分片 A 的阅读；本片只确认注入侧常量（未复读 pack_planner.rs）。
7. T2 sweep 命中（lib.rs secret_redaction 948、unwrap_or 1079、inline_tests 630 等）仅作机械计数引用，未逐条核验语义。
8. `search --robot-meta` 与 `cass doctor/status` 的 storage_integrity/state 是否单一来源未跨文件验证（query.rs/readiness.rs 属其它分片）。
9. 本分片读取方式披露：lib.rs 有 7 个窗口为 151 行（off-by-one），其余 ≤150；未把哈希/探针命中当阅读。

## 覆盖凭证

- 机器凭证：同目录 `coverage-cass-lib.json`（read_ranges / missing_ranges / SHA-256 / T2 sweep 命中 / 统计脚本口径）。
- 读法：PowerShell 行号有界窗口；`src/lib.rs` 因未入 codegraph 索引（4.05 MB）改用行号窗口；sync/config 每窗 ≤150 行且 1..真实 EOF 闭合。
- 本文件与 `coverage-cass-lib.json` 为本 worker 唯一创建物；未修改任何竞品源码与其它 TASKDIR 文件（分片 A/B/C 回执仅读取参考）。
