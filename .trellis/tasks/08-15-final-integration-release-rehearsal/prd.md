# Final Integration Release Rehearsal

> Parent: `08-15-open-source-product-roadmap`
> 状态:planning。P0 终局任务:全部子任务完成后,以"陌生用户视角"完整演练发布。

## Goal

在提请 owner 公开前,以全新环境模拟真实用户从安装到核心闭环的完整旅程,
暴露集成缝隙,而不是等开源后第一批 issue 来发现。

## Requirements

1. **全新环境演练(三平台各一轮)**:
   - 使用固定 platform/environment manifest(Windows、macOS、Linux 的 OS/build、
     clean VM/container image、安装包 hash、provider fixture 许可与脱敏状态),
     执行安装 → 首次索引(授权脱敏 fixture 或明确本机授权真实数据)→ 搜索(lexical/
     semantic/hybrid)→ evidence 查看 → context → resume dry-run →
     handoff pack 生成 → Web UI 全流程 → 卸载;
   - 每步记录命令、run id、耗时、报错、困惑点和 pass/fail 标准;任何"文档没写/命令不对/结果异常"
     都是缺陷回流。
2. **契约一致性终检**:
   - CLI/MCP/Robot/TUI/Web 五入口行为一致性按 canonical JSON 比较(忽略
     transport envelope 与非语义排序字段),固定 query/generation/budget/cursor;
   - schema(handoff-pack/v1、Robot 1.1、error-catalog)与实现零漂移;
   - 能力矩阵与实际行为零漂移。
3. **隐私终检**:
   - 网络抓包验证零遥测(仅显式模型下载);`--offline` 全流程;
   - 跨边界输出脱敏抽查(含 secret fixture);
   - 日志/诊断无 transcript 泄漏。
4. **性能终检**:全量真实语料 Gate D 六不变量 + benchmark 报告刷新。
5. **发布材料终检**:README/quickstart/对比表/demo 数据集/benchmark
   数字互相一致;LICENSE/third-party/fixture 来源合规复查。
6. **Go/No-Go 报告**:残留风险清单 + 建议,提交 owner 做最终公开决策。

## Acceptance Criteria

- [ ] 三平台全新环境演练记录(含 environment manifest、命令、run id、全部缺陷与修复确认);
- [ ] 五入口一致性测试套件通过;
- [ ] 隐私/性能/材料三终检通过;
- [ ] Go/No-Go 报告入库,owner 决策记录在案;
- [ ] platform/environment manifest、真实数据授权或 fixture 脱敏证明、
      canonical JSON 比较规则与 pass/fail 记录齐备;
- [ ] cargo fmt/clippy/test 全绿。

## Notes

- 本任务通过即视为父任务(发布门)可提请 owner 最终裁量。
