# Offline Privacy and Hooks

> Parent: `08-15-open-source-product-roadmap`
> 状态:planning。P0:ADR-0009 脱敏边界 + 零遥测可验证 + Hook(默认关闭)。

## Goal

落实三条信任底线:零遥测零上传默认离线、跨边界输出默认脱敏(ADR-0009)、
Hook 全部默认关闭且显式启用——并让它们**可验证**而不是口头承诺。

## Requirements

1. **ADR-0009 跨边界脱敏**(Q45/Q49,修订 ADR-0004 的适用范围):
   - Catalog 保留原文;CLI/TUI 本地人工查看保持不脱敏(ADR-0004 原状);
   - **Web UI、Handoff Pack、MCP、Robot JSON/JSONL、HTTP API 默认脱敏**,
     显式 reveal(带审计提示)才显示原文;
   - secret 检测:高置信模式(API key/token/password/private key/
     环境变量值)优先;检测规则与 fixture 入库;JSON escaped/Unicode/base64、
     多行 key、tool_result、结构化 activity、截断边界和坏 JSON 均需覆盖;
   - reveal 仅对已认证本地请求生效,默认单响应,不得写入 Catalog/cache/handoff
     持久化物;显式 export 才可落盘。审计事件含 audit_id/request_id、channel、
     actor/auth context、target occurrence/message IDs、ruleset/version、timestamp、
     reason/result,且事件与普通日志不得含 secret 原文;输出带 `[revealed]` 与 audit_id;
   - 所有机器输出统一携带 `redaction:{mode,status,ruleset_version,
     redacted_count,audit_id?}`,覆盖 payload、warning/error、metadata、tool
     activity、resume preview、config path;不允许入口直接序列化原文;

2. **零遥测可验证**(Q23):
   - 网络出口审计:代码级禁止(除显式模型下载与用户配置的外部 API 外
     无任何 HTTP client 调用);CI 加静态检查(依赖图/代码扫描),并用
     deny-all proxy/network namespace + packet capture 验证 `--offline` 下
     search/serve/MCP/Hook/adapter/resume 全流程零 socket;
   - `--offline` 全局 flag:禁用一切联网(含模型下载、外部 adapter egress),
     显式下载/API 必须用户确认并受 allowlist 约束;
   - 日志/诊断不含 transcript 内容与 secret;provider diagnostics、error
     frames、未知参数回显和 config paths 同样经过 redactor;
   - README 安全模型章节逐条对应实现。
3. **Hook 默认关闭**(Q30/Q35,owner 改选 B):
   - 提供 Claude Code Hook 集成(SessionStart/UserPromptSubmit)但
     **安装后不启用**,需用户显式开启;
   - 启用后仍受:max_tokens 预算、provider/time filter、时间衰减、
     `--offline`、一键禁用约束;
   - 注入内容默认脱敏;Hook 输出格式符合 Claude Code
     hookSpecificOutput.additional_context 契约;
   - 不静默注入任何历史到用户当前 prompt。
4. **威胁模型更新**:THREAT-MODEL.md 增补 Web serve(LAN 模式)、
   Hook、semantic 模型下载、外部 Embedding API 四个新攻击面。

## Acceptance Criteria

- [ ] ADR-0009 入库(Proposed→Accepted 流程按仓库规范);
- [ ] 跨边界默认脱敏 + 显式 reveal + 审计标记,含 secret fixture 端到端测试;
      fixture 必须覆盖 CLI `--robot`/JSONL、MCP `content`+`structuredContent`,
      Web/HTTP、handoff、warning/error、resume preview、tool activity、config
      paths;递归检查所有序列化字段不含 secret;Human CLI/TUI 按 ADR-0004 保持可见;
      reveal 重复请求、copy/export、重启/重渲染、loopback/LAN token 均验证
      audit 存在且无 secret,默认后续请求仍脱敏;
- [ ] ADR-0009 在 owner/approver 正式签署并记录 `accepted_at`、approver、
      implementation evidence 前不得视为发布门通过;governance CI 检查引用方
      status 与 ADR 文件一致;
- [ ] 零遥测静态检查入 CI;`--offline` 下全功能(除模型下载)可用;
- [ ] Hook 集成默认关闭,显式启用测试覆盖;
- [ ] THREAT-MODEL.md 四个新攻击面评审记录;
- [ ] cargo fmt/clippy/test 全绿。

- **人机输出矩阵**:Human CLI/TUI 的 search/list/get/show/context/resume preview
  可按 ADR-0004 显示原文;Robot/MCP/Web/HTTP/Handoff 全部默认走同一
  boundary-aware redactor。`config paths` 在 machine output 中只返回
  redacted placeholder/basename,不得回显用户 home/绝对路径。

- ADR-0004 保持有效但范围收窄为"CLI/TUI 本地人工输出";ADR-0009 不推翻
  它,而是划出跨边界输出的独立规则。
