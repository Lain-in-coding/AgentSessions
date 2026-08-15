# agent-session-grep 开源登顶产品路线与发布总规划

> 本任务是 2026-08-15 grill-with-docs 产品拷问的最终产物,是**总规划(parent task)**。
> 所有后续开发按本文件与 `docs/product/OPEN-SOURCE-ROADMAP.md` 执行;
> 实际实现落在子任务,父任务只做集成验收与发布决策。
> 状态:planning(子任务逐个 start,父任务不直接开工)。

## Goal

把 agent-session-grep 做成"统一核心、多入口、本地优先"的 AI coding-agent 会话历史
检索/恢复/交接基础设施,一次性以完整能力开源,与外部参考项目(ctx、agentsview、AgentRecall、agent-sessions、agf、cc-switch、
claude-historian-mcp、fast-resume、hstry、memex、Recall、sessiongrep、
coding_agent_session_search)拉开可验证差距。当前精读资料集确认了 13 个外部项目；
发布 benchmark 必须在比较清单中明确列出项目来源,补齐两项独立外部基线或把宣传口径
如实调整为 13 个,不得把本项目内部报告计作外部竞品。

**产品名:`agent-session-grep`,CLI 别名 `asg`。**
("Agent Session Group" 为笔误,废弃。)

## 已锁定的不可协商决策(Q1–Q55,owner 逐轮确认)

### 定位与用户

- **第一用户**:个人 AI coding-agent 重度用户;第二入口是 agent/MCP 消费者;
  团队/企业后置(Q1)。
- **核心闭环**:搜索 → 证据 → **resume(原地恢复)+ handoff(跨 provider 交接)**
  双主路径;自动上下文注入(Hook)是可选增强层(Q2/Q6/Q17)。
- **成功指标**:留存(7 天内 ≥2 次成功检索/交接)+ 技术口碑为不可跌破底线;
  stars/安装量是结果不是目标(Q3/Q7/Q9)。

### 发布形态

- **一次性完整发布**:仓库保持 PRIVATE,核心能力全部完成后一次公开;
  不做阶段性开源,不提前泄露(Q8/Q13)。产品交付以 **CLI-first** 为主入口,
  同时保证 MCP/Robot/TUI/Web UI 的统一契约。
- **首发排除项**:团队同步、云端服务、知识图谱。其余核心能力全部首发,
  **local Web UI 与 loopback HTTP 属于首发**(Q14/Q43)。
- **统一核心 + 多入口**:Application ADT + Robot Protocol 是唯一行为权威;
  CLI/MCP/Robot/TUI 首发,Web UI(loopback)首发,原生 GUI **不属于首发交付,
  但保留为后续扩展边界**(Q5/Q12/Q18/Q40)。
- **平台**:Windows + macOS + Linux 三平台同时 GA(Q25)。
- **License**:Apache-2.0 + Provider Adapter Protocol 贡献规范(Q26)。

### Provider(16 个,名单冻结)

```text
Claude Code, Codex CLI, DeepSeek Harness, Grok Build, Antigravity,
OpenCode, Pi, Aider, Cline, Cursor, OpenClaw, Hermes, Kimi Code,
ZCode, Qoder, Tencent CodeBuddy
```

- 名单冻结(Q31);Gemini CLI / GitHub Copilot / Qwen Code 不进首发承诺,
  后续走 Adapter Protocol。
- **成熟度分级,证据晋级**(Q15/Q28/Q32/Q37/Q46):当前 maturity 必须读取
  `docs/product/PROVIDER-MATURITY-MATRIX.md`;下列是目标分级,不是当前实现状态:
  - `certified`(首发目标):Claude Code、Codex CLI——需跨平台 Gate D、golden
    fixture、incremental、source span、context graph、resume/handoff、
    tool activity 全绿,并通过正式 target CI 与 owner 晋级决策;
  - `GA`:不预先承诺,须满足矩阵中 GA/Certified 的历史 variant、混合版本、未知字段、
    崩溃恢复、正式 target、性能与回滚证据;
  - `beta`(首发目标):Grok Build、OpenCode、Pi、Antigravity、Kimi Code、
    OpenClaw、Hermes、Qoder、Tencent CodeBuddy、DeepSeek Harness;
  - `experimental`(首发目标):ZCode、Aider、Cline、Cursor(或任何未完成
    版本核验的 provider);
  - `unsupported`:未列名单者。
- **证据硬规则**(Q46/Q52):每个 provider 必须有真实格式证据 + canonical
  source fixture + probe/parse/search golden test;DeepSeek Harness 与 ZCode
  当前**无可靠 transcript 证据**,发布前必须补齐,做不到就推迟公开,
  不得猜测格式,不得拿模型配置/平台名冒充会话存储。
- **Discovery 策略**(Q41):默认只扫描已登记 canonical root;用户
  `asg sync --discover` 显式发现;每 provider 报告
  complete-root / incomplete-root / unsupported-layout,不静默跳过。
- **社区扩展**(Q33/Q42):版本化外部进程 Provider Adapter Protocol
  (stdin/stdout JSON,manifest 声明 provider_id/variant/roots/capabilities/
  maturity/license/网络权限);官方 16 个内置 Rust 实现;社区 adapter 默认
  只读、无网络、安装与首次运行需确认;非法 canonical event 必须隔离。
  任一 provider 的发现、读取或解析失败必须隔离到该 provider 的诊断结果,
  保留其他 provider 的成功增量,并遵循失败不删除原则,不得让单一 provider
  故障中止整次同步或产生误导性 tombstone。
- **宣传口径**(Q53):"16 provider adapters,按成熟度分级" + 公开完整
  capability matrix;禁止跨级宣传。

### 检索

- **Semantic search 是首发硬门槛**(Q16/Q21/Q34/Q39/Q47/Q51/Q54):
  - 官方默认本地 embedding 模型(候选模型先做 CJK/英文/代码混合 benchmark
    达标后锁定;manifest 记录 model id/hash/dimension/license);
  - 首次使用显式下载,离线校验;可插拔兼容模型;外部 Embedding API 仅显式配置;
  - 检索粒度:message/placement 召回 → session 聚合排序 → context pack 按
    mainline 展开;
  - lexical 永远可用;模型不可用自动降级且**必须**显式标注
    `retrieval_mode=lexical_fallback` + warning + 修复建议,禁止静默切换;
  - 脱敏固定 corpus 的 lexical/semantic/hybrid 三组 recall benchmark +
    p50/p95 延迟 + 模型体积 + 首载时间 + 索引磁盘占用;
  - 未达质量阈值只能标 beta,不得默认替换 lexical。
- 搜索保持 plain-text-only(ADR-0003)+ CJK bigram(ADR-0007)。

### Resume / Handoff

- **Resume**(Q17/Q24):原地恢复——原 provider、原 session ID、原 cwd、
  原 approval/permission mode;默认 dry-run 预览,用户确认后执行;
  可配置自动执行(显式 opt-in)。
- **Handoff**(Q17/Q24/Q38/Q44/Q50):跨 provider 交接;输出
  **`handoff-pack/v1` 版本化契约**——JSON 为权威结构,Markdown 是
  deterministic projection;包含 pack_id、catalog generation、query、
  matched sessions、mainline、evidence spans、source 定位、tool activity、
  budget、truncation、redaction、confidence;**原文证据与 inference 摘要
  分栏**;默认 deterministic 生成,local LLM 摘要仅用户显式启用且永远标记为
  inference;pack 可离线保存/校验/重渲染/复现。
- Handoff 永远先展示 context pack 与 evidence,不静默注入其他 agent。

- **Hook 后续边界**:首发只提供 pull 型、默认关闭的 Claude Code Hook;
  `push API` 不属于首发契约,后续若实现必须另立版本化契约、权限模型、
  审计和离线/脱敏验收,不得由当前 Hook 设计隐式承诺。

### 隐私与安全

- **零遥测、零上传、默认离线**(Q23):唯一联网行为是用户显式触发的模型
  下载与显式配置的外部 API;提供 `--offline`;日志不含 transcript。
- **脱敏边界**(Q45/Q49,修订 ADR-0004 → 见 ADR-0009):Catalog 保留原文;
  **机器/跨边界输出(Web UI、Handoff Pack、MCP、Robot、HTTP API)默认脱敏,
  显式 reveal 才显示原文**;每次 reveal 都要记录不含 secret 原文的本地审计事件;
  CLI/TUI 本地人工查看保持不脱敏(ADR-0004 原状)。
- **Hook 全部默认关闭**(Q30/Q35,owner 改选 B):SessionStart 与
  UserPromptSubmit 均不默认注入任何历史;用户显式启用后仍受 max_tokens、
  provider/time filter、时间衰减、`--offline`、一键禁用约束。
- **Web UI 安全**(Q27/Q36/Q40):默认仅 loopback + 随机本地 token +
  Host/Origin 校验;显式 LAN 模式强制 token + 审计日志;所有危险动作
  (resume/handoff/启动 provider)默认预览,首次安装强制预览一次;
  `asg serve` 是唯一后端,Web UI 只是协议客户端。

## 子任务地图(执行顺序)

| # | 子任务 | 优先级 | 依赖 |
|---|---|---|---|
| 1 | `08-15-unified-release-contract` | P0 | 无;但须先锁定 08-14-resume-protocol-prerequisites 的输出基线/关闭状态 |

| 2 | `08-15-sixteen-provider-evidence-wave` | P0 | #1 的 identity/contract 决策 |
| 3 | `08-15-semantic-hybrid-local-retrieval` | P0 | #1(retrieval_mode 字段) |
| 4 | `08-15-evidence-handoff-pack` | P0 | #1(handoff-pack/v1);与 #2/#3 并行 |
| 5 | `08-15-resume-metadata-execution` | P0 | 08-14 resume 树收尾 |
| 6 | `08-15-structured-activity-context-facets` | P1 | #1 |
| 7 | `08-15-loopback-web-ui-parity` | P1 | #1 + #3/#4 的能力面 |
| 8 | `08-15-offline-privacy-hooks` | P0 | #1(ADR-0009) |
| 9 | `08-15-benchmark-install-open-source-gate` | P0 | #2–#8 主体完成 |
| 10 | `08-15-final-integration-release-rehearsal` | P0 | 全部 |

与既有任务的关系:08-13/08-14 任务树(competitor-borrowings、
historical-session-discovery-resume 等)是本规划的 **Phase 0**,照常收尾;
其产出(resume metadata、provider-scoped identity、source discovery、
sidechain facets、structured tool activity 的 PRD)直接被 #1/#2/#5/#6 吸收,
不重复建设。

## 父任务验收标准(= 发布门,全部满足才提请 owner 公开)

- [ ] 16 provider 各自证据、fixture、能力矩阵公开,分级如实;
      DeepSeek Harness / ZCode 证据补齐或明确推迟决策;
- [ ] Claude Code / Codex 达 certified(跨平台 Gate D 等);
- [ ] ≥5 个“主流” provider 达到完整 beta 主路径:discover/parse/search/context/
      source span/incremental,名单、阈值和测试证据冻结在 provider evidence manifest;
      GA 还必须满足矩阵定义的历史 variant、混合版本、未知字段、崩溃恢复、
      正式 target、性能与回滚证据;不得用窄版 discover/parse/search 代替完整 beta;
- [ ] semantic/hybrid 上线且 benchmark、降级标注、模型 manifest 齐备;
- [ ] handoff-pack/v1 + resume dry-run/opt-in 执行落地;
- [ ] Web UI(loopback)核心 parity + 安全模式;
- [ ] 零遥测可验证(网络出口审计)、跨边界默认脱敏(ADR-0009)落地;
      ADR-0009 必须在 owner/approver 正式签署并记录 accepted_at、approver、
      implementation evidence 后才算发布门通过;
- [ ] Hook 默认关闭、显式启用;
- [ ] 三平台安装器 + demo dataset + benchmark 脚本 + 文档 + 对比表齐备;
- [ ] 公开 benchmark(b)可复现:discovery coverage、parse loss、lexical/
      semantic recall、p50/p95、index size、resume/handoff success;
- [ ] #9 只生成 gate evidence manifest/status,#10 负责三平台复核与 Go/No-Go;
      父任务与 owner 保留最终勾选、残留风险裁量和是否公开的决定。
- [ ] owner 自测满意 → owner 最终决定公开(发布是 owner 的最终裁量)。

## Notes

- 详细路线图(阶段、里程碑、竞品对比基线)见
  `docs/product/OPEN-SOURCE-ROADMAP.md`。
- 输出脱敏边界 ADR:`docs/adr/ADR-0009-cross-boundary-output-redaction.md`。
- 16 provider 证据清单与 source root/format/identity/resume 线索见
  子任务 #2 的 PRD 附表。
- 术语以 `CONTEXT.md` 为准;本轮新增决策已记入其 Decision log (2026-08-15)。
