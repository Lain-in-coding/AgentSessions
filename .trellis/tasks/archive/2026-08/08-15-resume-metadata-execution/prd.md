# Resume Metadata Execution

> Parent: `08-15-open-source-product-roadmap`
> 状态:planning。是 08-14 Resume Metadata 流(historical-session-discovery-resume
> 树)的执行层收口,不重复其 PRD,只补"执行与安全"层。

## Goal

把 resume 从"展示恢复命令"补全为"原地恢复":原 provider、原 session ID、
原 cwd、原 approval/permission mode,默认 dry-run 预览,确认后执行,
可配置显式 opt-in 自动执行。

## Requirements

1. **Resume 命令矩阵**:16 provider 的 resume 命令、cwd、权限 flag
   (approval/permission mode,含 yolo 类)进入 capability matrix;
   无权威值的 provider 显示 null/—,不编造。
2. **Dry-run 默认**(Q24):`asg resume <session>` 默认输出完整命令
   (provider/cwd/session id/权限 flag)供用户确认;`--yes`/配置项显式
   opt-in 后才实际执行;首次安装无论如何强制预览一次。该任务拥有
   resume descriptor、preview、execute 和 provider process spawn;未核验
   provider 的命令值必须为 null/—,不得从报告摘要猜测。
3. **执行器**:按平台启动(替换进程或 spawn);执行前校验 cwd 存在、
   provider 二进制可用;失败返回结构化错误,不静默。
4. **权限模式传递**:provider 支持 yolo/full-auto/acceptEdits 类模式时,
   resume 可携带用户显式选择的模式 flag;默认不带。
5. **MCP/Web 面**:get_session_resume 工具与 Web UI 的 resume 按钮遵循
   同一预览优先契约;Web UI 端默认只预览(见 offline-privacy-hooks 任务)。
6. **身份正确性**:resume 目标 session id 必须来自 provider-scoped
   identity(见 unified-release-contract),跨 provider ID 碰撞在此层
   必须已消解。

## Acceptance Criteria

- [ ] capability matrix 含 16 provider 的 resume 支持列(或 null);
- [ ] dry-run 默认 + opt-in 执行 + 首次强制预览,CLI/MCP/Web 一致;
- [ ] Claude Code / Codex 真实 resume 冒烟(本机授权环境)通过;
- [ ] 执行失败路径(二进制缺失、cwd 不存在、session 不存在)有结构化
      错误码与 operator_action;
- [ ] cargo fmt/clippy/test 全绿。

## Notes

- 依赖:08-14-resume-metadata-core-surfaces / provider-resume-metadata-extraction
  的产出(横表、MCP 工具)是本任务输入。
- 参考(思路级):agf 的 deliver_command 三级回退、fast-resume 的 exec
  进程替换、sessiongrep 的 resume_plan dry-run。
