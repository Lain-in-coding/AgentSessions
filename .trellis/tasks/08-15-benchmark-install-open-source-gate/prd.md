# Benchmark, Install and Open-Source Gate

> Parent: `08-15-open-source-product-roadmap`
> 状态:planning。P0 收口任务:在 #2–#8 主体完成后执行。

## Goal

把"登顶"变成可复现证据:公开 benchmark(含与已核验外部参考项目的对比基线)、
三平台安装器、完整开源交付物、发布前 final gate。对比清单必须明确列出当前
13 个外部项目;只有补齐两项独立外部基线后才可使用“15 个外部项目”的宣传口径。

## Requirements

1. **公开 benchmark(Q55)**:
   - 脱敏固定 corpus + 固定 query set(中/英/代码/路径/错误分类);
   - 指标:discovery coverage、parse loss、lexical/semantic/hybrid recall、
     p50/p95 延迟、index size、resume/handoff success;
   - 发布脚本与原始结果(JSON,按 SLI-AND-BENCHMARK-FORMAT 的必填字段);
   - 与当前已核验外部参考项目的公开对比表(当前 13 个):只列可复现事实(provider 数、能力
     矩阵、license、形态),不伪造竞品数字;不能合法/稳定运行的竞品
     标"不可比"。
2. **三平台安装器**(Q25):
   - Windows/macOS/Linux 安装、卸载、升级路径;`asg` 别名;
   - smoke test(安装→索引 demo corpus→搜索→resume dry-run);
   - 供应链:release 构建 `--locked`、二进制 SHA-256、SBOM/依赖审计
     (deny.toml 已有,补 release 流程)。
3. **开源交付物**(Q48=A):
   - README(5 分钟 quickstart、能力矩阵、provider 成熟度分级表);
   - 脱敏 demo dataset + 可复现 benchmark 指引;
   - CLI/MCP/Robot/Web API 文档与示例;
   - 安全模型(零遥测、脱敏边界、Hook 默认关闭、Web 安全)说明;
   - CONTRIBUTING + Provider Adapter Protocol 贡献规范 + issue/PR 模板
     + roadmap;
   - LICENSE:Apache-2.0 + third-party notice(REUSE-LICENSE-AUDIT 更新,
     明确 cass 等 rider 项目的 clean-room 边界)。required artifacts、
     evidence manifest、README/matrix/benchmark 一致性由固定校验脚本验证。
4. **发布 final gate evidence**:本任务只生成 gate evidence manifest/status、
   benchmark run id、artifact consistency report 与残留风险,不代替父任务
   勾选发布门,不作 owner 的公开决定;#10 负责独立复核并提交 Go/No-Go。

## Acceptance Criteria

- [ ] benchmark 报告(原始 JSON + Markdown 投影)入库且可一键复现;
- [ ] 三平台安装器 + smoke 全绿(CI 记录 run id);
- [ ] 上述交付物全部存在且互相一致(README 数字 = 矩阵 = benchmark);
- [ ] 与当前 13 个外部项目的对比表入库(或先补齐两项独立外部基线再恢复 15 项口径),
      无可复现依据的宣称为零;
- [ ] gate evidence manifest(status、证据路径、执行者、run id、残留风险)入库;
- [ ] required artifact 清单与一致性校验脚本/CI job 入库(README、matrix、
      benchmark、provider evidence、fixture manifest、license notice 数字一致);
- [ ] 父任务验收清单由 #10 复核后提交,本任务不代替 owner 最终勾选;
- [ ] cargo fmt/clippy/test 全绿。

## Notes

- 本任务不产出新核心功能,只收口证据与交付;功能缺口回流对应子任务。
