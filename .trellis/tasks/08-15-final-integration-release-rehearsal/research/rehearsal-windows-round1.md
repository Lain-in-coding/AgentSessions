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

## 一致性终检(2026-08-16,追加)

同一查询 `retry` 在 CLI/MCP/Web 三入口的 canonical JSON 比对:

| 入口 | 结果 | 与 CLI 一致性 |
|---|---|---|
| CLI `search retry` | 2 hits, mode=lexical | 基准 |
| MCP `search_sessions` | 2 hits, id/score/text 全同 | ✓ 一致 |
| Web `GET /api/search?q=retry` | 2 hits, score 0.8956/0.8057 同 | ✓ 一致 |
| Robot | 与 CLI 同构(`--output json` 即 Robot envelope) | ✓ 一致 |
| TUI | 本地渲染层(同 Application ADT) | ✓ 同源 |

结论:五入口经统一 Application ADT 共享同一搜索语义,无契约漂移。

## Round 2(2026-08-16):release 重建 + gate 刷新

- release 二进制重建(含 handoff/serve,5050368 字节),gate benchmark 刷新:
  lexical_recall_at_10=1.00 (≥0.95 ✓)、parse_loss_ratio=0.00 (≤0.05 ✓),
  discovery_coverage/resume_handoff_success 仍 not_applicable(对应功能未达阈值条件)。
- verify-release 5/5 通过。HEAD 7d73ee3 已 push。

## Round 3(2026-08-16):隐私终检

**发现**:Web/JSON 边界存在两处 secret 泄露:(a) serve.rs 从未调用 redactor(文档声称 ADR-0009 但未实现);(b) `redact_string` 只匹配独立 secret,散文内嵌 secret(`"config with key AKIAIOSFODNN7EXAMPLE"`)全部泄露。

**修复**(b1060d4):
1. serve.rs:所有 JSON API 响应(`/api/search`、`/api/context`、`/api/status` 之外的 3 个路径)经 `redacted_json()` 统一走 `redact_value`。
2. redaction.rs:拆分 `standalone_secret`(整串匹配)+ `embedded_shapes()`(10 个 prefix/min_len/marker 形状,longest-first)+ `find_embedded`(边界安全 prefix 匹配 + 消费 trailing token run + span ≥ min_len)+ 每 value 16 span 上限。

**端到端验证**(synthetic secret fixture,非真实数据):
- CLI `--output json` search:AKIA→`[redacted:aws_access_key]`、ghp_→`[redacted:github_token]`、sk-ant-→`[redacted:api_key]` ✓
- CLI human context/search:raw secret 完整展示(ADR-0004 本地不脱敏)✓
- Web `GET /api/search?q=`:hits text 全脱敏,原始响应无泄露 ✓
- 单元测试 13→16(新增 embedded prose、边界安全、embedded bearer 3 个);workspace gate 全绿(fmt/clippy/test);已 push。

**残余风险**:redaction 规则集为保守子集(AWS/GitHub/OpenAI/Anthropic/xAI/Bearer/PEM + secret-key-name),其他格式(如自定义企业 token)不在覆盖范围——后续可按需扩展 `embedded_shapes()` 并 bump RULESET_VERSION。
