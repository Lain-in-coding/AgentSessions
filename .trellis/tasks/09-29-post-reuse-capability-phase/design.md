# 阶段设计：任务地图、顺序与集成验收

## 任务地图

| 子任务 | 交付 | 独立验收 | 依赖 |
|---|---|---|---|
| `match-centered-snippet` | `SearchHit.text` 命中窗口语义 + 契约/spec | 用例矩阵 + 字节预算 + 排名/游标不变 | 无 |
| `index-throughput` | 提交写路径优化 + 空转双读消除 | 1M A/B ≥50% 时间/RSS + 无回归 | 研究 harness（PR #13） |
| `provider-hermes-sqlite` | `hermes/sqlite-state-v1` 变体 | probe/parse/search golden + 只读/有界/身份 | 无 |
| `provider-cursor-diskkv` | `cursor/disk-kv-v1` 变体 | 同上，experimental | 无 |

## 顺序与并行约束

1. 摘要先行：小、独立、立即改善可感知体验，且不占用测量机器时间。
2. 吞吐随后：需要独占测量窗口（A/B 期间不得有并发构建/测试）；每个优化单独提交，失败单独回退。
3. Provider 后置：证据面最大；两家互不共享代码，可在吞吐测量窗口之外穿插进行。
4. 父任务不直接实现；子任务各自走 implement → check → spec sync → commit → 合并 main。

## 集成验收协议

- 每个子任务合并后在 main 上复跑本任务的验收命令，确认组合无回归。
- 摘要：`text` 语义变更需要一份跨入口一致性证据（同一命中在 CLI/MCP/Robot 输出同一 `text`）。
- 吞吐：before/after 报告必须来自同一二进制基线与同一语料哈希；失败/超时如实保留；证据 JSON 经校验器重算。
- Provider：maturity 只按 `docs/product/PROVIDER-MATURITY-MATRIX.md` 的证据规则变更；Hermes beta 晋级由 owner 在阶段评审决定，任务只提交建议与证据。
- 回滚：每个子任务独立可回滚；吞吐优化按提交粒度回退，不做全局 revert。

## 证据与许可

- 研究引用一律固定提交；Wake 代码/思想复用按 `docs/operations/REUSE-LICENSE-AUDIT.md` 四级判定，MIT 归属随实验代码保留。
- 不复制任何真实 transcript 或外部 fixture；新 fixture 全部合成并附 PROVENANCE。
