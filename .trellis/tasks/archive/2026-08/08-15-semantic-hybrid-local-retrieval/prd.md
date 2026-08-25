# Semantic Hybrid Local Retrieval

> Parent: `08-15-open-source-product-roadmap`
> 状态:planning。依赖 `08-15-unified-release-contract` 的 retrieval_mode 契约。

## Goal

为 agent-session-grep 提供首发硬门槛的 local semantic/hybrid search:
官方默认本地 embedding 模型、message/placement 粒度召回、session 聚合、
lexical 永远可用且降级必须显式标注,并以脱敏 fixture benchmark 证明质量。

## Requirements

1. **官方默认模型**(Q34/Q51):
   - 候选模型(优先 multilingual CJK 兼容,如 bge-m3 / multilingual-e5 系)
     在脱敏真实结构 fixture 上做 CJK/英文/代码混合 benchmark,达标后锁定
     一个默认模型;
   - manifest 记录 model id、文件 hash、dimension、license、benchmark 结果;
   - 首次使用显式下载并校验 hash;之后完全本地推理;支持断点/重试/取消。
2. **可插拔兼容模型 + 外部 API 仅显式配置**(Q21/Q34):兼容模型需声明
   dimension/hash/license;外部 Embedding API 不得成为默认路径。
3. **检索粒度**(Q39):message/placement 级 embedding 召回 → session
   聚合排序 → context pack 按 mainline 展开;semantic 命中必须可回溯到
   Evidence span,不允许只返回模糊 session。
4. **降级显式化**(Q54):模型下载失败/损坏/CPU 不支持/向量索引未就绪时
   自动切换 lexical,但所有入口必须返回 `retrieval_mode=lexical_fallback`
   + warning + 修复建议;禁止静默切换。
5. **Hybrid 融合**:lexical(FTS5 bm25)与 semantic 结果融合(RRF 或
   等价方法,基准对比后定),结果标注命中来源;semantic 不得破坏 lexical
   的精确命中。
6. **向量存储**:本地优先(参考 sqlite-vec / 自建表);向量索引必须是可从
   catalog 重建的投影,与现有 FTS 投影同级的 rebuild 语义;WriterLease /
   durable outbox 契约不破坏。
7. **Benchmark(Q47/Q55)**:脱敏固定 corpus/query manifest 上 lexical/semantic/hybrid
   三组 recall + p50/p95 延迟 + 模型下载大小 + 首次加载时间 + 向量索引
   磁盘占用;冻结 corpus/query 版本、阈值、重复次数、硬件/OS 环境和 SLI
   schema,由 #9 复用同一 manifest;未达质量阈值只能标 beta,不得默认替换 lexical。
8. **离线行为**:`--offline` 下 semantic 不可用必须优雅降级;语义索引
   构建可在后台进行,不阻塞 CLI/MCP 主路径。

## Acceptance Criteria

- [ ] 默认模型锁定 + manifest(id/hash/dimension/license/benchmark)入库;
- [ ] message/placement embedding + session 聚合 + Evidence 回溯链路可用;
- [ ] hybrid/semantic/lexical 三模式 + `lexical_fallback` 显式标注在
      CLI/MCP/Robot/TUI 全入口一致;
- [ ] 向量索引可从 catalog 全量重建(等价 FTS rebuild 语义),Gate D
      六不变量仍全绿;
- [ ] recall/延迟/磁盘 benchmark 报告按冻结 manifest 与 SLI 格式入库;阈值未达时
      maturity 明确保持 beta 且 lexical 仍为默认,不得仅凭模型可运行晋级;
- [ ] cargo fmt/clippy/test 全绿。

## Notes

- 实现参考(思路级,注意 license 边界):Recall 的 sqlite-vec + candle +
  e5-small 后台 worker、ctx 的 hybrid reciprocal rank fusion、cass 的
  two-tier(此仓 LICENSE 有 rider,仅 clean-room 思路)。
- 与 08-15-unified-release-contract 的 retrieval_mode 字段先定再实现。
