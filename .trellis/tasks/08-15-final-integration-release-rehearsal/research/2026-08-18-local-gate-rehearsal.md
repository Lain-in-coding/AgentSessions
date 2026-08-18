# 2026-08-18 全量本地发布门演练(origin/main f7e2a49)

> 任务:08-15-final-integration-release-rehearsal
> 范围:runbook 第 11 节之后可本地执行的全部 gate(干净环境三平台演练的 macOS 一腿仍受 GitHub CI billing 外部阻塞,不属本次范围)
> 结论:**8/8 主 gate 全绿,0 个 repo-local 缺陷,无需修复**

## 环境(聚合事实,无个人路径)

- OS: Windows 11(build 10.0.22631,AMD64)
- 工具链: rustc 1.97.1,cargo 1.97.1,Python 3.10.11,pwsh 7.6.1
- 源码 commit: `f7e2a49b007c25035d060edac5826dffd5c1f29a`(origin/main)
- 演练产物为临时目录/已忽略输出,不留痕于仓库

## Gate 结果表

| # | Gate | 命令 | 结果 | run id |
|---|---|---|---|---|
| 1 | release 构建 | `cargo build --release --locked -p agent-session-grep-cli` | ✓ 1m34s,5,917,184 字节,exit 0 | rehearsal-windows-20260818-build |
| 2 | 发布自检 | `python scripts/verify-release.py --asg target/release/agent-session-grep.exe` | ✓ **10/10** | rehearsal-windows-20260818-verify |
| 3 | 五入口一致性 | `python scripts/rehearsal/compare_entrypoints.py --binary target/release/agent-session-grep.exe` | ✓ overall_verdict=consistent,cli/mcp/robot/web/tui 全直接比对,skipped/aliases/unimplemented 均空 | rehearsal-windows-20260818-consistency |
| 4 | 安装 gate | `pwsh -NoProfile -File scripts/install/gate_smoke.ps1 -Prefix <tmp> -OutputDir <tmp>` | ✓ install→smoke→upgrade→uninstall→幂等 uninstall→reinstall→smoke→final uninstall 共 11 步全过,manifest pass=true | rehearsal-windows-20260818-install-gate |
| 5 | 表面 smoke | `pwsh -NoProfile -File scripts/install/smoke.ps1 -Binary target/release/agent-session-grep.exe -AliasBinary target/release/asg.exe` | ✓ 全断言通过(robot envelope/退出码/MCP 9 tools 握手) | rehearsal-windows-20260818-smoke |
| 6 | 发布包 dry-run | `python scripts/release/build-manifest.py` validate→inventory→package(x86_64-pc-windows-msvc, zip)→checksums | ✓ 四步全过;二次运行 SHA256SUMS 与 zip **字节一致**(确定性) | rehearsal-windows-20260818-manifest |
| 7 | Gate D benchmark | `python scripts/evidence/open_source_gate_benchmark.py run --profile rehearsal --output-dir scripts/evidence/out` + `validate-report` | ✓ gate.pass=true,failures=[],deferred=[];manifest schema 校验通过 | rehearsal-windows-20260818-gate-d |
| 8 | 许可/隐私/脚本单测 | `cargo deny check licenses bans sources`;`python scripts/evidence/privacy_scan.py --repo .`;`python -m unittest discover -s scripts/release -p "test_*.py"` | ✓ licenses/bans/sources 全 ok;隐私扫描无发现;6 tests OK | rehearsal-windows-20260818-license-privacy |

## Gate D 关键数字(manifest 已记录,不提交生成物)

- lexical_recall_at_10 = 1.00(≥0.95 ✓)、parse_loss_ratio = 0.00(≤0.05 ✓)
- discovery_coverage = 1.00(≥0.95 ✓)、resume_handoff_success = 1.00(≥1.0 ✓,9/9)
- semantic_recall/hybrid_recall = 1.00(实测,无阈值,bigram-hash-v1 标注为模糊词法向量,非语义模型)
- 检索 latency p50/p95:search 13.9/15.5 ms;无 telemetry、fixture 全合成(contains_real_transcripts=false)

## 补充质量门(提交前)

- `cargo fmt --all --check` ✓;`cargo clippy --workspace --all-targets -- -D warnings` ✓
- `cargo test --workspace` ✓ 全绿(0 failed;少数常规 #[ignore])
- `cargo test -p agent-session-grep-cli --test e2e_consistency` ✓ 2/2
- `scripts/` 顶层四个脚本单测(verify_release/compare_entrypoints/install_entrypoints/benchmark)✓ 14 tests OK

## 发现与说明

- **repo-local 缺陷:0 个**。全部 gate 首次运行即通过,未改动任何代码/脚本。
- `target/release/asg.exe` 与 `agent-session-grep.exe` 哈希不同属预期:两者为同一 `main.rs` 的两个 `[[bin]]`(crate name 嵌入二进制元数据,非陈旧构建;删除后重建哈希复现且 smoke 行为一致)。安装脚本对前缀内两个命令做字节级同源拷贝并校验版本一致,不依赖构建产物哈希相等。
- 外部未决项(不变,非本仓库代码问题):GitHub Actions 因账号账单("recent account payments have failed")无法启动,三平台干净环境矩阵中的 macOS 一腿仍缺;其余平台已由本地(Windows)+ WSL(Linux,见 rehearsal-windows-round1.md)替代验证。

## 结论

本地可执行的发布门全部通过;Go/No-Go 外部未决项仅剩 CI billing(macOS 干净环境演练)一项,待用户在 GitHub Billing 修复后补跑。
