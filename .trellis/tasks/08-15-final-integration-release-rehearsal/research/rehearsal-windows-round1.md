# Final Integration Release Rehearsal — Windows Round 1 (2026-08-16)

> 任务:08-15-final-integration-release-rehearsal
> 执行:主窗口(owner 授权 GOAL 自主推进)
> 环境:Windows 11 Home China 10.0.22631,release/debug 二进制均本机构建
> 结论:全链路演练通过,发现并修复 3 个真实集成缺陷

## 演练记录(陌生用户视角)

| 步骤 | 命令 | 结果 | 缺陷 |
|---|---|---|---|
| 1. 安装 | `install.ps1 -SkipBuild -Prefix <prefix>` | ✓ 安装 + sha256 输出 | — |
| 2. 版本 | `<prefix>/agent-session-grep.exe --version` | ✓ 0.1.0 | — |
| 3. 首次索引 | `asg --db <db> sync <gate fixture 2 个>` | ✓ generation 1 | — |
| 4. 搜索 | `search retry --max-items 5` | ✓ 2 hits, lexical | — |
| 5. Evidence | `context <ses-id>` | ✓ 3 messages + 3 evidence | — |
| 6. Resume dry-run | `get-session-resume <ses-id>` | ✓(命令面为 get-session-resume) | **独立 resume 子命令不存在**(按设计为 get-session-resume) |
| 7. Handoff pack | `handoff <query>` | ✓ pack_v1 + 2 evidence | **handoff 子命令缺失,已实现**(4f98fc2) |
| 8. Web UI | `serve --port <n>` + token | ✓ / 200(HTML)、/api/status 200 真实数据、/api/search?q= 200 2 hits、/api/context?session= 200 3 messages、无 token 401 | **serve 未接线**(stub API),已实现(355186a + 46e4c5f);**query-string 路由 404** 已修复(46e4c5f) |
| 9. 卸载 | `uninstall.ps1 -Prefix` | ✓ 删除二进制;再次运行幂等(not installed, exit 0) | — |

## 演练中修复的缺陷

1. **verify-release.py 不可运行**(06f3c43 前):sync/search/status 缺 `--db`(exit 2);`sync --discover` 扫真实数据根(30s 超时+隐私);search 用错 flag(`--limit` vs `--max-items`)。修复后 5/5 通过。
2. **smoke 脚本断言 7 个 MCP 工具**(0683f85 前):16-provider 后实际 8 个(search_sessions、get_session_resume 加入)。修复 ps1+sh 两版。
3. **`handoff` CLI 子命令缺失**(4f98fc2):application 层生成器存在但无入口。已实现 `handoff <query>`(确定性、evidence/inference 分栏、dry-run)。
4. **`serve` 未接线**(355186a):HTTP 解析/token/路由桩存在,但无 `serve` 子命令派发、无监听主循环、API 返回硬编码 stub。已接线 Application ADT(真实数据)+ token/loopback 认证。
5. **serve query-string 404**(46e4c5f):`/api/search?q=` 匹配完整 path 失败。已按 path-only 匹配。

## 残留风险

- macOS/Linux 演练未跑(本机 Windows;CI 三平台矩阵已配置,需 PR 上跑绿记录 run id)。
- `handoff` human 输出为原始 JSON 投影(无专门 human 渲染器)——功能正确,观感可后续优化。
- serve 为单线程逐连接处理(无并发);GET-only;POST 端点(body 解析已保留)未实现。
- 能力矩阵与 release 二进制基于 HEAD 46e4c5f 构建;gate manifest 需随新构建刷新。
