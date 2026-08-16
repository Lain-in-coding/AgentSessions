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
  **阻塞点**:GitHub 账号账单问题("recent account payments have failed")导致 CI job 无法启动——所有 7 个 job 未运行即 failure,非代码问题;需用户在 GitHub Billing & plans 修复后重跑 CI。已做最大限度的本地跨平台预检(见 cross-platform-precheck.md):无跨平台阻塞,修复了 gate_smoke.sh 默认用法 bug、docs 中 sh/powershell 指令、.gitattributes eol 规则。
- serve 为单线程逐连接处理(无并发)——loopback 单用户场景可接受,已加 15s 读超时防慢客户端拖死;GET-only;POST 端点未实现(记录为后续迭代,不阻塞开源)。
- 能力矩阵与 release 二进制需随新构建刷新(gate manifest 已随 8cc165e 刷新,release 二进制需 rebuild + verify-release 重跑)。

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

## Round 4(2026-08-16):开源就绪补强

- **handoff created_at 缺陷**(8cc165e):`utc_now_iso8601` 把 epoch 秒拼进分秒字段(`1970-01-01T00:00:1786844148Z`,非法时间戳)。改为手写 UTC civil 转换(无 chrono 依赖)+ 已知 epoch 单元测试(含闰日)。
- **handoff human 渲染器**(8f3644f):替换原始 JSON 投影为可读分栏(header/created_at/confidence/matched_sessions/evidence/inference/truncation),2 个单元测试。
- **serve 会话 token 安全加固**(0630971):LCG + 时间种子的 token 生成器(注释承认非密码学安全)替换为 getrandom CSPRNG(已在 lock 图,经 tempfile 传递);加 15s 读超时。
- **跨平台预检**(9cd22f7 + c03a7f7):gate_smoke.sh `set -e` 下 `[ -n "$prefix" ] && ...` 默认路径必失败 → 改显式 if;docs 中 `sh install.sh`(dash 缺 pipefail)/`powershell install.ps1`(5.1 缺 $IsWindows)改为 bash/pwsh;.gitattributes 补 `*.sh/*.py eol=lf` 防 CRLF blob 破坏 Unix CI;恢复 install.sh/smoke.sh 执行位。
- **CHANGELOG + SECURITY**(47291c0):开源必配文档。
- **CI 阻塞**:GitHub 账单问题导致三平台矩阵无法运行(见残留风险),本地预检替代,无跨平台阻塞发现。

## Round 5(2026-08-16):Linux 侧 WSL 验证(替代 CI ubuntu job)

CI 因 GitHub 账单问题不可用,在 WSL(Ubuntu,ext4,rustup stable 1.97.1 minimal)克隆仓库到 ext4 后完整验证:

| 检查 | 结果 |
|---|---|
| `cargo fmt --all --check` | ✓ 通过 |
| `cargo clippy --workspace --all-targets -- -D warnings` | ✓ 通过(57s) |
| `cargo test --workspace` | ✓ 全绿(230+98+37+48+25+8+11+43+3+5+10+14+38 passed,2 ignored 常规,0 failed) |
| `cargo build --release --locked -p agent-session-grep-cli` | ✓ 6,004,544 字节 |
| `python3 scripts/verify-release.py --asg <linux binary>` | ✓ 5/5 通过 |

结论:Linux 侧(ubuntu)验证完整替代 CI ubuntu job;macOS 无法本地模拟(无 mac 环境),仍待 CI 恢复后补跑。注意:WSL `/mnt/c` 挂载层会把 LF 文件自动转 CRLF(DrvFs 行为),`bash -n` 在挂载层报假 CRLF 错误——git-bash(msys)是 Windows 上正确的 bash 验证工具。

## Round 6(2026-08-16):P0 功能缺口补齐(semantic/resume/hook)

三个 roadmap P0 验收项从"骨架存在但无入口"补到"全链路可用":

| 功能 | 缺口 | 落地 |
|---|---|---|
| semantic/hybrid 检索 | SemanticIndex/EmbeddingModel port + RRF fusion 是死代码,search 无 dispatch | `9d62208`:AppRequest::Search 加 `mode`+`query_embedding`,App 加 `M: SemanticIndex` 槽(NoSemanticIndex 占位保持既有调用点零改动),`search --mode lexical\|semantic\|hybrid`,索引未就绪时显式 `retrieval_mode=lexical_fallback` + warning(Q54 禁止静默切换),3 个降级单测 |
| resume 执行层 | application 层能造 descriptor,但无 CLI 入口 | `80277d1`:`resume <ses-id>` 默认 dry-run(打印命令/cwd/权限模式),`--yes` 才在原 cwd spawn provider;未核验命令的 provider 报 `available:false` 不编造;cwd 缺失/二进制不在 PATH/非零退出均为结构化错误(带 stage detail);human 渲染器 + 3 单测 |
| Claude Code Hook | hooks.rs 整个模块 `#![allow(dead_code)]`,无 CLI 入口 | `904f4fa`:`hook <session-start\|user-prompt-submit>` 从 stdin 读 payload、按 hookSpecificOutput 契约输出;默认关闭(不加 `--enable` 输出空 context,Q35=B);注入文本经跨边界脱敏;未知 event 报 exit 2 不静默;4 单测 |

**端到端验证**(synthetic fixture):
- `--mode semantic` → `retrieval_mode: lexical_fallback` + warning "semantic search unavailable (mode semantic); fell back to lexical",词法命中仍返回 ✓
- `resume <sid>` dry-run → `(cd <repo> && claude --resume resume-ses-1)`;`--yes` 触发真实 claude 进程并把其非零退出转为 exit 7 + provider_error ✓
- `hook user-prompt-submit`(无 --enable)→ 空 context;加 `--enable` → 命中 + `ghp_...` 脱敏为 `[redacted:github_token]` ✓
- 命令面 16→18(`resume`/`hook`);KNOWN_COMMANDS 测试同步。

**验证矩阵**:Windows fmt/clippy/test 全绿(50 套件)、smoke 30 断言全过、verify-release 5/5、gate manifest 刷新;WSL Linux 侧同 HEAD(e66f96f)fmt/clippy/test 全绿。
