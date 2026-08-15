# Evidence Handoff Pack

> Parent: `08-15-open-source-product-roadmap`
> 状态:planning。依赖 `08-15-unified-release-contract` 的 handoff-pack/v1 契约。

## Goal

把"搜索 → 证据 → 交接"做成核心差异化:从历史 session 生成
`handoff-pack/v1` 版本化交接包——带 evidence spans、预算约束、来源引用、
原文/推断分栏的 context pack,可交给任何当前 agent 继续工作。

## Requirements

1. **handoff-pack/v1 契约**(Q50):
   - JSON 是权威结构;Markdown 是 JSON 的 deterministic projection;
   - 必含字段:pack_id、catalog generation、query、matched sessions、
     mainline 摘要、evidence spans、source path/cursor、tool activity、
     token/byte budget、truncation 原因、redaction 状态、confidence;
   - **原文证据(evidence)与推断摘要(inference)分栏**,任何 LLM 生成
     内容必须标记 inference,不得混入证据栏;
   - pack 可离线保存、校验(hash)、重新渲染(同 generation/query/budget
     下结果可复现)。
2. **Deterministic 默认**(Q44):默认不调用任何模型,纯确定性生成;
   local LLM 摘要仅用户显式启用,失败回退 deterministic pack。
3. **预算控制**:pack 生成受 ResponseBudget 约束,截断必须显式记录原因
   与被截断内容定位;用户可指定 max_tokens/max_evidence/context_lines。
4. **跨 provider 交接**:pack 的 target 字段支持声明目标 provider/agent,
     但 `asg` 本身只输出 pack 与建议命令,不静默注入其他 agent(Q17/Q24)。
     本任务只消费 #5 提供的 resume descriptor/preview 信息并序列化到 pack,
     不实现 provider 命令矩阵、预览执行器或 provider process spawn。
5. **入口面**:CLI `asg handoff`、MCP 工具、Robot envelope、Web UI 操作;
   全部读同一 Application ADT 用例。
6. **脱敏**:pack 默认跨边界脱敏(按 ADR-0009),显式 reveal 才含原文;
   revealed pack 默认不得写入 cache/catalog/deterministic source,只有显式
   export 才可落盘且必须带 `revealed` 状态与 `audit_id`;从 revealed 输入重建
   的 pack 必须重新默认脱敏。

## Acceptance Criteria

- [ ] handoff-pack/v1 JSON schema 正式化(非 Draft)并通过 schema-drift test;
- [ ] CLI/MCP/Robot 三入口均可生成 pack,内容一致(同输入同输出);
- [ ] evidence/inference 分栏 + budget/truncation/redaction 字段齐备;
- [ ] deterministic 生成可复现性测试(同 generation/query/budget);
- [ ] local LLM 摘要为显式 opt-in 且输出标记 inference;
- [ ] 脱敏 fixture 的端到端 handoff 测试(含 secret 的 transcript 生成
      pack,默认输出不含明文 secret);默认 pack/Markdown/hash/cache/重渲染均
      不含 secret, revealed export 单独显式确认且可追溯 audit_id;
- [ ] cargo fmt/clippy/test 全绿。

## Notes

- 差异化定位:超过 memex 的"注入"与 fast-resume 的"只恢复"——pack 是
  可审计、可校验、可复现的证据交接单元,当前已核验外部参考项目中未见同等契约;
  发布前必须以 benchmark/源码证据更新该比较表,不得作无依据的绝对断言。
- 与 08-15-resume-metadata-execution 共享 session identity/resume 命令面。
