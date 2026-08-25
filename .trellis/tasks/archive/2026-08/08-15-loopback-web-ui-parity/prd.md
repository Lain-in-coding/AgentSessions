# Loopback Web UI Parity

> Parent: `08-15-open-source-product-roadmap`
> 状态:planning。依赖 unified-release-contract(#1)+ semantic(#3)/
> handoff(#4)的能力面定型。

## Goal

提供 local Web UI 核心功能 parity:`asg serve` loopback HTTP API 作为唯一
后端,Web UI 只是协议客户端,复用 Application ADT / Robot 契约,让
搜索、evidence、context、resume 预览、handoff pack、tool activity、
semantic/hybrid 结果全部可视化。

## Requirements

1. **`asg serve`**(Q40):
   - loopback HTTP API,复用 Application ADT;同一搜索/预算/错误/cursor/
     generation 语义;不出现 CLI 与 Web 行为分叉;
   - 默认仅绑定 127.0.0.1 + 随机本地 token + Host/Origin 校验(Q27);
   - 显式 LAN 模式:强制 token、Origin/Host 校验、审计日志;
   - CSP、gzip、SSE(索引进度/后台任务)可选。
2. **Web UI 功能面**(Q22=A):
   - 搜索(lexical/semantic/hybrid 切换、retrieval_mode 标注)、过滤
     (provider/time/project/facet)、分页(cursor);
   - evidence 定位(span 高亮、原文查看);
   - session context(mainline/full 切换、tool activity、subagent 树);
   - resume 预览(默认只预览,显式确认)与 handoff pack 预览/导出;
   - 索引/模型/后台任务状态页。
3. **危险动作策略**(Q36):所有 resume/handoff/启动 provider 默认预览;
   首次安装强制预览一次;自动执行需显式 opt-in;handoff 永远先展示
   pack 内容。
4. **脱敏**(ADR-0009):Web 只集成 #8 提供的 redaction policy/service、
   secret fixtures 与 shared tests;Web 负责展示、显式 reveal 交互和 audit hook,
   不复制 detector 或规则。
5. **技术形态**:浏览器只是协议客户端;前端静态资源可嵌入二进制
   (rust-embed 类方案)或随包分发;不做 Electron/Tauri。
6. **零遥测**:Web UI 无任何外部请求(字体/CDN 全本地)。

## Acceptance Criteria

- [ ] `asg serve` 启动即用,API 与 CLI 行为一致性测试(同查询同结果),以
      canonical JSON 比较并忽略 transport envelope/非语义排序字段;
- [ ] loopback `asg serve` 是唯一后端,危险动作(resume/handoff/启动 provider)
      首次预览与显式确认测试通过,无 CLI/Web 分叉;- [ ] 搜索/evidence/context/resume 预览/handoff 预览全部可用;
- [ ] 默认 loopback + token + Host/Origin 校验;LAN 模式显式开启且留审计;
- [ ] 脱敏默认,reveal 显式;页面无外部网络请求(可断网验证);
- [ ] 三平台(Windows/macOS/Linux)浏览器冒烟;
- [ ] cargo fmt/clippy/test 全绿。

## Notes

- 与 offline-privacy-hooks 任务共同落实 ADR-0009 与 Web 安全边界。
- 参考(思路级):agentsview 的 auth/host/csp 中间件链与 loopback 判定。
