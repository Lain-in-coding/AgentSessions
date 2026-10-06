# 首用闭环：默认路径/显式 sync/结果可读性（B3）

> 父任务：.trellis/tasks/10-05-competitive-source-audit-plan。用户 2026-10-06 批准（D2 CLI-first）。
> 依赖：B1（10-06-hotpath-git-probe）先落地，本任务在其装配形态上做最小增量；上游：review-report P1-05、父 design.md 第 5 节。

## Goal

把“首次成功路径”从开发者试卷变成五步闭环：拿到二进制/检查版本 → 确定读取范围 → 显式 sync → search → context/resume/handoff。

## Requirements

1. DB 路径解析：--db 显式值优先；未给时解析已有平台默认数据位置（单一解析函数，不引入多套隐式 fallback 链）；解析失败给明确指引。
2. 读命令遇到未初始化库：不写磁盘、不伪装空成功；返回 catalog_error 并附一条准确的显式 sync 指令。
3. 人类输出优先展示标题/项目/provider/时间/命中片段/可采取动作（若现状已满足，补回归测试锁定）。
4. discovery 不可用或未配置 provider 时，明确说明需要指定源，而不是让用户猜“为什么零结果”。
5. Quickstart 文档与真实 CLI 行为一致（README 等），五步顺序可直接照做。

## Non-goals

- 不造复杂向导程序；不自动扫描全部 HOME；不执行 provider；不修改其配置。
- 不改 Robot schema/退出码/显式 DB 优先级；跨边界路径脱敏保留。
- 默认行为若变化，必须有 opt-out/显式路径与版本说明，不破坏旧脚本。

## Acceptance Criteria

- [ ] 隔离环境实验（临时 HOME/DB）：按 Quickstart 五步可复现首条结果；原始日志存 research/。
- [ ] 读命令在缺库时不创建文件（测试断言）+ 错误信息含正确 sync 指令。
- [ ] 默认路径解析有单元/集成测试（显式值优先、默认值、解析失败）。
- [ ] README/Quickstart 更新且与 --help 行为一致。
- [ ] cargo fmt/clippy/test（workspace，isolated target dir）全绿。
