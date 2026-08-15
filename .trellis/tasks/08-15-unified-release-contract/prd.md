# 统一发布契约与 provider-scoped identity

> Parent: `08-15-open-source-product-roadmap`
> 状态:planning。本任务是发布契约的地基,先于 provider wave / semantic / web。

## Goal

把"统一核心、多入口"落成可执行契约:Application ADT 扩展、Robot Protocol
schema 1.0 → 1.1、provider-scoped session identity、retrieval_mode 标注、
error catalog 增补,使后续所有能力(provider、semantic、handoff、Web UI)
共享同一套行为权威,不出现"CLI 找得到、Web UI 找不到"的双产品。

## Requirements

1. **Provider-scoped session identity**(吸收 08-14-provider-scoped-session-identity
   的规划):canonical Session ID 引入 provider + installation namespace,
   防跨 provider ID 碰撞;legacy 迁移;多 ID fail-closed。
2. **Robot Protocol 1.1**:
   - 每个检索响应携带 `retrieval_mode`(`lexical` / `semantic` / `hybrid` /
     `lexical_fallback`)+ model id + index generation;
   - error catalog 增补 semantic/model 相关错误码(下载失败、模型损坏、
     向量索引未就绪等),每码含 layer/retryable/exit_code/redaction/
     operator_action,同步 `schemas/robot/v1/error-catalog.json` 与
     schema-drift test。
3. **Application ADT 扩展**:为 16-provider 能力矩阵(discover/parse/search/
   context/resume/handoff/tool activity)预留统一能力声明面,能力缺失
   显式返回 capability-not-supported,禁止静默降级。
4. **Schema registry 与变更 ownership**:本任务拥有 Application ADT、Robot
   envelope、error catalog、capability manifest 和跨入口 projection contract
   的版本注册与 schema-drift gate;后续 #2–#8 只能提交向后兼容的 additive
   schema 变更,必须更新 registry、fixture 和 drift test,不得私自分叉入口契约。
5. **Boundary-aware projection**:所有输出先经过显式 `OutputBoundary`
   projection(HumanCli/Tui 或 Robot/Mcp/Web/Http/Handoff),跨边界 payload、
   warning/error、metadata、tool activity、resume preview 和 config path
   不得直接序列化 Catalog 原文;redaction 状态必须随机器响应传递。
6. **Capability honesty**:入口层(CLI/MCP/Robot/Web)渲染 provider 能力
   必须读权威矩阵,禁止硬编码。
7. **Handoff pack 契约注册**:`handoff-pack/v1` JSON schema(权威结构、
   Markdown 为投影),pack_id/generation/预算/截断/脱敏字段先定契约,
   实现落在 handoff 子任务。
8. **CLI 别名 `asg`**:安装脚本与文档统一提供 `asg` 别名,正式名保持
   `agent-session-grep`。

## Acceptance Criteria

- [ ] schema v8+ 迁移含 provider-scoped identity,全量真实语料 Gate D 六不变量全绿;
- [ ] Robot 1.1 envelope schema + error-catalog 更新并通过 schema-drift test;
- [ ] 检索响应在所有入口一致携带 retrieval_mode;
- [ ] 能力矩阵单源权威(单一 manifest/schema),入口渲染不再各自硬编码;
- [ ] registry/versioning owner、OutputBoundary projection 与 schema-drift gate
      入库;Robot/MCP/Web/Handoff 不得绕过 redactor 直接序列化原文;
- [ ] 机器响应统一携带 `redaction:{mode,status,ruleset_version,
      redacted_count,audit_id?}`(默认也显式表示 applied/none/partial);
- [ ] handoff-pack/v1 schema 草案入库(可先 Draft 状态);
- [ ] `asg` 别名在三平台安装脚本落地;
- [ ] cargo fmt/clippy/test 全绿。

## Notes

- 与 08-14 既有任务的关系:08-14-resume-protocol-prerequisites(Robot 1.1、
  budget-floor、MCP doc drift)的工作并入本任务,不重复。执行前必须锁定该任务
  的最终输出基线并记录 superseded/closed 状态;未吸收的 resume metadata 工作
  由 #5 继续负责,不得与本任务并行修改同一契约。
