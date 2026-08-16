# Final Open-Source Readiness Audit

- **Query**: 从当前代码、测试、工作流、发布材料与 GitHub 远端事实出发，审计 agent-session-grep 是否可正式公开；区分本地可闭合缺口、首发后迭代项与 owner/外部平台阻塞。
- **Scope**: 根级开源材料；08-15 roadmap/provider/benchmark/final rehearsal 事实源；产品、架构、安全、发布与运行文档；CLI/provider/存储关键实现；CI/evidence harness；仓库卫生；`qin-devs/AgentSessions` 只读 GitHub 元数据。
- **Date**: 2026-08-16
- **Audited checkout**: `aeb65f342f4d76e7bb3fa6478e7ea5ae0ff283fc`
- **Remote main observed at final update**: `c5e7292bd60cb29ba5509935e2eef272ebf6e2c4`（已包含 Codex incremental e2e、真实数据 source checksum、结构化 AdapterManifest、maturity CLI/MCP/human surfacing 与 rollback ADR；rollback governance status 仍为 `Proposed`，等待 owner/approver acceptance）
- **Local integration `main` observed at final update**: `c5e7292bd60cb29ba5509935e2eef272ebf6e2c4`
- **Latest tracked gate evidence used**: tracking commit `3ac3e3e031de25993d6cca219d27ec005a588831`；manifest 测量 commit `aeb65f342f4d76e7bb3fa6478e7ea5ae0ff283fc`

## Executive verdict

# `NOT_READY_LOCAL_BLOCKERS`

当前不能诚实判定为正式开源就绪。核心产品面已经很广，最新 gate manifest 的四个阈值指标也全部实测为绿；但仍有 **6 组 P0 本地可闭合缺口**，集中在公开仓库隐私卫生、发布/版本与宣传事实一致性、安装别名、可发布制品流水线、五入口/三平台终局演练证据，以及 RFC 明确标为 release-blocking 的 bounded ingestion。Provider 结构化 `AdapterManifest`、read-only checksum、Codex incremental、maturity surfacing 与 rollback ADR 实现均已在远端或本地 integration main 闭合；rollback ADR 仍为 `Proposed`，其 owner/approver acceptance 归 External governance。

同时存在 **5 组 External 阻塞**。最近 GitHub Actions failure 均是 job 在 0 个 step 的状态下结束；结合已确认的 account billing/spending-limit 事件，应分类为外部账户阻塞，**不是代码或 workflow step 失败**，不得通过改代码掩盖。

## 事实矩阵

| 开源必需面 | 判定 | 当前代码/证据事实 |
|---|---|---|
| License / copyright | **基本通过** | `LICENSE-MIT` 与 `LICENSE-APACHE` 均存在；workspace metadata 与 README 均为 `MIT OR Apache-2.0`（`Cargo.toml:5-9`, `README.md:92-95`）；MIT copyright 存在（`LICENSE-MIT:2`）。 |
| Third-party notices / dependency license gate | **部分通过 / P0+External** | `deny.toml` 允许集合明确且 CI 有 cargo-deny job（`deny.toml:7-41`, `.github/workflows/ci.yml:136-143`）；但复用审计仍为 Draft、无 approver/approved_at（`docs/operations/REUSE-LICENSE-AUDIT.md:4-12,79-84`），仓库没有 `NOTICE`/third-party attribution 制品，也没有 release SBOM。 |
| README 安装与 quickstart | **失败 / P0** | README 先 `cargo build --workspace`，随后直接调用 `asg`（`README.md:23-42`）；受支持 installer 只复制 `agent-session-grep[.exe]`，不安装 `asg`（`scripts/install/install.ps1:31-49,91-93`; `scripts/install/install.sh:59-72,115`），且 README 没有让用户加入 `target/debug`/安装目录到 PATH。 |
| SECURITY / contribution | **通过但治理未签署** | SECURITY 清楚说明本地人工输出不脱敏、跨边界脱敏、loopback serve 与 transcript 只读（`SECURITY.md:23-40`）；CONTRIBUTING 有 privacy、质量门与 git 规则（`CONTRIBUTING.md:20-44`）。ADR-0009 仍 Proposed，按自身治理条款未满足 accepted_at/approver（`docs/adr/ADR-0009-cross-boundary-output-redaction.md:4-15`）。 |
| Changelog / versioning / release truth | **失败 / P0** | workspace 是 `0.1.0`（`Cargo.toml:5-9`），Go/No-Go 模板却写 `v0.3.0`（`docs/release/go-no-go.template.md:1`）；CHANGELOG 声称 `0.1.0` 已发布并链接不存在的 tag/release（`CHANGELOG.md:52-68`），远端实际无 tag、无 release。CHANGELOG 的 14-provider 列表还包含当前不存在的 gemini/gpt-codex/bolt/cody/continue/windsurf（`CHANGELOG.md:12-19`）。 |
| Release workflow / artifacts / checksums / SBOM | **失败 / P0** | `.github/workflows/` 只有 `ci.yml` 与 `core-beta-evidence.yml`，没有 tag/release workflow。evidence workflow 仅上传 7 天 unsigned artifact，明确不做 publish/sign/notarize/checksum attestation/provenance/release（`.github/workflows/core-beta-evidence.yml:200-244`; `docs/operations/core-beta-evidence-matrix.md:52-68`）。安装文档也明确 no released/signed/published artifact（`docs/operations/INSTALL-AND-UPGRADE.md:1-4,166-184`）。 |
| 最低 OS / signing / notarization | **External** | ADR 候选 floor 为 Windows 10、glibc 2.31、macOS 12（`docs/adr/ADR-0002-platform-targets.md:19-32`），但自身明确当前 runner 不认证这些 floor（`:45-50`）；Authenticode/Apple/cosign/OIDC 均依赖外部身份或平台（`docs/operations/external-readiness-gate.md:11-27`）。 |
| 三平台 build/test/install smoke | **workflow 已配置，最新执行被 External 阻塞** | generic CI 为 Windows/Linux/macOS 三平台 test+installer（`.github/workflows/ci.yml:10-18,48-55`）；core evidence 为 Windows x64、Linux GNU x64、macOS Intel/ARM64 四 target（`.github/workflows/core-beta-evidence.yml:33-65`），且 provider/gate steps 已写入（`:127-155`）。最近 run 的全部 job `steps: []`，不能解释为代码失败。历史 run `30165919066` 是旧配置成功证据，不覆盖 aeb65f3 新增的 per-target provider/gate steps（`docs/operations/core-beta-evidence-matrix.md:17-25,36-43`）。 |
| 隐私：真实 transcript / redaction | **运行时大体通过；repo hygiene P0** | gate corpus 明确 synthetic、`contains_real_transcripts=false`；边界 redaction 已有实现与测试，真实数据报告只保留聚合。远端 main `989c034` 已把 source checksum 前后不变纳入真实数据 harness。可是公开树内仍有真实个人/机器绝对路径，见 P0-1。 |
| 核心功能 | **主体已实现** | CLI command/help/dispatch 覆盖 sync (`--discover`)、search、show、context、handoff、resume、hook、MCP、TUI；serve 为特殊长期运行入口（`crates/agent-session-grep-cli/src/main.rs:384-412,615-638,779-869,1180-1650,3749-3768`）；MCP 当前有 8 tools（`crates/agent-session-grep-cli/src/mcp.rs:595-812`）。缺口主要是终局一致性证据与 hook 后续过滤能力，不是入口完全不存在。 |
| Provider 16 行 / 14 implemented + 2 deferred | **通过（Experimental 口径）** | README 与矩阵明确 14 Experimental + DeepSeek/ZCode 2 deferred/Unsupported（`README.md:45-69`; `docs/product/PROVIDER-MATURITY-MATRIX.md:22-48`），没有把 deferred 宣传为 implemented。14 个 provider crate 均存在且各有单元测试；local integration main `c5e7292` 已把矩阵冲突按当前事实 reconciled：cross-target workflow 仅 configured、最新 run 因 billing 0-step，rollback 为 Proposed，而 manifest/read-only/Codex incremental 已 closed。所有 provider 继续保持 Experimental，未跨级宣传。 |
| Provider promotion / rollback / manifest | **实现闭合；rollback/最终晋级仍需 External governance** | RFC 要求 `manifest()`、variant、fixture revision、last certified target、limitations 与 rollback evidence（`docs/architecture/RFC-0002-provider-adapter-contract.md:23-42,89-100`）。结构化 AdapterManifest、maturity CLI/MCP/human surfacing 与 rollback ADR 均已在 main `c5e7292` 合入并通过质量门。rollback ADR 当前按事实保持 `Proposed`，矩阵已按当前事实 reconciled；owner/approver accepted_at 与 billing 恢复后的 exact-target promotion 另列 External。 |
| Benchmark / gate | **通过（限定 gate 定义）** | 最新 tracked manifest 四个阈值指标全部 `state=measured` 且 pass：lexical recall 1.0、parse loss 0.0、discovery coverage 1.0、resume/handoff 1.0；`gate.pass=true`, `deferred=[]`。脚本确实测量 discovery 与 resume/handoff（`scripts/evidence/open_source_gate_benchmark.py:173-244,283-382,467-546`）。本 worktree 旧 manifest 指向 83d072d，不应覆盖 remote main 3ac3e3e 的最新 tracked evidence。 |
| Semantic / hybrid 诚实度 | **gate 诚实；公开宣传失败 / P0 文档修复，真实模型 P1** | harness 明确 bigram-hash 是 fuzzy lexical、非 semantic model，semantic/hybrid 无 threshold/pass，仅 informational（`scripts/evidence/open_source_gate_benchmark.py:45-62,507-531,668-677`），CLI 建索引也发 warning（`crates/agent-session-grep-cli/src/main.rs:1871-1933`）。但竞品表仍把当前产品写成 semantic/hybrid、声称中文优于纯 FTS/RRF，还声称有 `--offline`（`docs/product/COMPETITOR-COMPARISON.md:16,35-40`），实际 CLI 没有 `--offline` parser。 |
| Supply chain | **部分通过** | Cargo.lock 已提交；cargo-deny licenses/advisories/bans/sources 配置合理（`deny.toml:10-41`），CI step 存在。Dependabot/scheduled audit、release SBOM、attestation 尚无。最新 cargo-deny job 0 step 失败属于 billing External。 |
| Repo hygiene | **混合：无二进制/DB/secrets；路径泄漏 P0** | `git ls-files` 未发现 tracked `target/`、exe/dll/so/dylib、db、pem/key、env、archive；`.gitignore` 覆盖这些（`.gitignore:1-35`）；合成 secret-shaped fixture 只出现在 redaction tests。另一方面，多份 tracked task/research/source docs 含真实绝对路径与个人 username。 |
| GitHub remote | **External** | repo `PRIVATE`，default branch `main`；open issue/PR 均 0；release/tag 均 0；branch-protection API 返回 403（private repo 当前 plan 不支持）；最近 exact-config runs 全为 0-step failure。 |

## P0 本地可闭合（6 组）

### P0-1 — 公开树与 Git 历史含个人/机器绝对路径

**证据**：

- `.trellis/tasks/08-15-open-source-product-roadmap/research/2026-08-15-review-sqlite-write.md:33` 含真实本机用户目录；
- `.trellis/tasks/08-15-open-source-product-roadmap/research/2026-08-15-review-provider-codex.md:8` 含真实本机 provider root；
- `.trellis/tasks/08-15-semantic-hybrid-local-retrieval/research/agentrecall-semantic-reference.md:35-37` 含真实本机 cache/research roots；
- `docs/product/OPEN-SOURCE-ROADMAP.md:95-97`、`docs/product/COMPETITOR-COMPARISON.md:5-7` 与多个 provider crate module docs（例如 `crates/agent-session-grep-provider-opencode/src/lib.rs:9`, `...-pi/src/lib.rs:10`, `...-codebuddy/src/lib.rs:14`）硬编码本机绝对工程路径；
- 多份 `.trellis/tasks/**/{implement,research,check}.*` 记录 worktree/CARGO_TARGET_DIR。仅改当前文件仍会在公开 Git history 中保留旧值。

**最小修复**：对全部 tracked text 做 privacy scan，将证据坐标改成仓库相对路径或 `<repo>/<user-home>`；对包含真实用户名/个人 cache root 的既有历史，在公开前建立经过 scrub 的公开历史（或经 owner 明确批准做 history rewrite），再复跑 secrets/path/large-file scan。不得把真实 transcript 内容加入替代证据。

### P0-2 — 发布版本与公开产品主张不一致

**证据**：`Cargo.toml:7` 是 0.1.0；`docs/release/go-no-go.template.md:1` 是 v0.3.0；`CHANGELOG.md:52-68` 声称不存在的 v0.1.0 release/tag；`CHANGELOG.md:12-19` 把非当前 14-provider 名称写成已实现；`docs/product/COMPETITOR-COMPARISON.md:16,35-40` 把 bigram-hash 宣传为 semantic，并声称当前有不存在的 `--offline` 与未被 benchmark 支撑的“中文优于”结论。

**最小修复**：冻结首次公开版本（0.1.0 或另一个单一版本），统一 Cargo/SECURITY/CHANGELOG/release templates；在真正 tag/release 前把 0.1.0 内容放回 Unreleased 或明确为 planned；把 provider 名单改成 README/matrix 的真实 14+2；把 bigram-hash 写成 experimental fuzzy-lexical vector mode，移除 semantic/优越性/`--offline` 的当前完成态宣称，直到对应功能与 benchmark 落地。

### P0-3 — 受支持安装路径不提供 README 承诺的 `asg` 命令

**证据**：CLI crate 构建两个 bin（`crates/agent-session-grep-cli/Cargo.toml:8-14`），但两套 installer 仅复制 canonical binary（`scripts/install/install.ps1:31-49,91-93`; `scripts/install/install.sh:59-72,115`）；README quickstart 直接调用 `asg`（`README.md:4,23-42`）。

**最小修复**：二选一并端到端统一：A) installer/upgrade/uninstall/smoke 同时安装并验证 `asg[.exe]`；或 B) 首发公开文档全部使用 `agent-session-grep`，把 `asg` 限定为 cargo build 产物而非 installed alias。README 还需给一条真实可执行的安装/PATH 步骤。

### P0-4 — 没有可发布制品流水线、release checksums/SBOM/third-party attribution

**证据**：没有 release workflow；current evidence workflow 明确只上传 7 天 unsigned artifact 且不 publish/sign/notarize/attest/create release（`.github/workflows/core-beta-evidence.yml:225-244`; `docs/operations/core-beta-evidence-matrix.md:52-68`）；安装文档明确 source-only、not published（`docs/operations/INSTALL-AND-UPGRADE.md:166-184`）；REUSE audit 仍 Draft 且把 SBOM/attribution 作为 exit condition（`docs/operations/REUSE-LICENSE-AUDIT.md:25-31,79-84`）。

**最小修复**：增加 tag-gated release workflow，至少构建冻结 target、产出 versioned archives、`SHA256SUMS`、SBOM/third-party licenses、artifact provenance inputs，并在 upload 前验证 version/tag/lockfile/manifest 一致。签名凭据本身保持 External；workflow 无凭据 dry-run 与 unsigned checksum 制品可本地闭合。完成 REUSE audit/NOTICE（如无第三方 notice 义务，也要生成可审计结论）。

### P0-5 — 最终 release rehearsal 仍是 framework，不是五入口/三平台完成证据

**证据**：`scripts/rehearsal/compare_entrypoints.py:49-64` 声称五入口但只把 CLI/MCP/Robot 设为 implemented；`:275-285,415-476` 把 Web/TUI skip，且 skipped 不导致 failure（`:29-31`）。因此 `overall_verdict=consistent` 不能证明五入口一致。`docs/release/rehearsal-runbook.md:115-179,222-262` 仍把 semantic/resume/handoff/Web/privacy/performance 标成 pending；仓库只有 environment/go-no-go templates，没有完成的三平台 manifest 与 owner Go/No-Go（`docs/release/environment-manifest.template.json:1-42`; `docs/release/go-no-go.template.md:1-111`）。

**最小修复**：让 consistency harness 实际调用 Web，并为 TUI 建立可重复的 headless/application projection comparison；如果某入口不能自动比，就不得让 skip 产出 overall consistent。更新 runbook 到真实命令，完成 Windows/Linux/macOS environment manifests 与缺陷回流记录，最后生成非 template Go/No-Go。macOS host/CI billing 与 owner sign-off 另列 External，不用本地假证据代替。

### P0-6 — Provider ingest 仍整体读入 transcript，违反 RFC release-blocking bounded-buffer 约束

**证据**：`ReadOnlySourceSnapshot` 生产读取会按整个文件长度分配 Vec 并 `read_to_end`，capture 与 verify 各整读一次（`crates/agent-session-grep-adapters-sqlite/src/source_fs.rs:32-48,56-86`）；ProviderAdapter 仍接收完整 `&[u8]`（`crates/agent-session-grep-ports/src/lib.rs:812-820`）。RFC-0002 明确把“禁止整体加载大型 transcript、使用 bounded buffer”列为 Release 阻断级（`docs/architecture/RFC-0002-provider-adapter-contract.md:95-103`）。

**最小修复**：把 snapshot/provider parse 边界改为 bounded reader/chunked stream，并保留 captured_len/fingerprint/post-read verification；至少添加大文件内存上限回归与 source-changed 原子回滚测试。若短期选择显式 max-source-size，必须作为受测限制/诊断写入 manifest，不能继续声称满足 streaming contract。

## P1 开源后迭代（6 组，不阻断首次公开前提是先修正宣传）

1. **真实本地 embedding 模型与 semantic threshold**：当前 bigram-hash informational 做法诚实；真实模型、下载/hash/license/dimension/CJK corpus benchmark 另有调研，可在首次公开后落地，但在此之前不能宣传为 semantic model。
2. **Hook 高级过滤与显式 offline UX**：当前 hook 默认关闭、`--enable` 才注入且有 token budget；`hooks.rs:7-14` 提到 provider/time decay/`--offline`，CLI help 实际只有 enable/max-tokens（`main.rs:817-822,1515-1538`）。由于当前二进制没有网络出口，这不是首次公开安全 blocker；应删掉未实现承诺或后续补齐。
3. **Dependabot / scheduled RustSec**：cargo-deny on push/PR 已有，增加 Dependabot 与 scheduled audit 有价值但不是公开仓库必要前置。
4. **Issue/PR templates 与 Provider Adapter contributor guide**：CONTRIBUTING 已有基础规则；模板和完整社区 adapter protocol 可在首次公开后补。
5. **serve hardening beyond current read-only loopback**：Origin 显式校验、并发连接、LAN mode、审计事件与 POST 危险动作是后续；当前 GET-only、loopback、Host check、CSPRNG bearer token 的首发边界可接受，但文档不得宣称尚未实现能力。
6. **渠道与运维完善**：crates.io/Homebrew/Scoop/winget、自动升级/rollback、Linux musl 补充 artifact、production snapshot/bundle API 均可在权威 GitHub Release 后迭代。

## External（5 组）

1. **GitHub visibility / release owner action**：仓库当前 `PRIVATE`，default branch `main`；无 tag/release。visibility 切换、最终 tag 与 GitHub Release 发布只能由 owner 执行。
2. **GitHub Actions billing/spending limit**：最新 `ci` run `31931867185` 与 `core-beta-evidence` run `31931867182` 的每个 job 均 `steps: []` 并在约 3–5 秒内 failure；这是账户计费/额度外部阻塞，不是测试、编译或 workflow step 失败。修复 billing 后 rerun exact SHA，不要改代码规避。
3. **Branch protection / rulesets**：main protection REST 请求返回 HTTP 403“Upgrade to GitHub Pro or make this repository public”。公开后由 owner 配置 required checks/review/ruleset。
4. **Artifact signing / notarization / transparency / attestation credentials**：Windows Authenticode、Apple signing+notarization、cosign/OIDC、GitHub Artifact Attestation 与 crates.io trusted publishing 均需 owner 身份、证书或外部平台配置（`docs/operations/external-readiness-gate.md:11-27`）。
5. **正式平台环境与治理签署**：macOS 12 runtime、glibc 2.31/minimum-OS clean machine、如承诺则 musl，以及 ADR-0002/ADR-0009/REUSE audit/最终 Go-No-Go approver sign-off 都不能由本地代码伪造。历史 hosted success 也不等于 minimum-OS certification。

## 建议发布顺序

1. **先冻结宣传与版本**：决定首次公开版本；修 P0-2/P0-3；把 bigram-hash/Experimental/14+2 口径写实。
2. **完成隐私发布分支**：修 P0-1，扫描当前树与历史；确认无真实 transcript、token、个人绝对路径、大文件或 stale binaries。
3. **保持 provider 诚实分级**：main `c5e7292` 已闭合 AdapterManifest/maturity surfacing/rollback 实现；在 billing 恢复、exact-target run 与 owner promotion 决策前继续保持 Experimental。完成 bounded ingestion 或明确受测限制。
4. **落地可复现 release pipeline**：unsigned dry-run 先绿，产出 archives/checksums/SBOM/third-party license bundle；不等待签名凭据才开始本地 workflow 工作。
5. **修终局 rehearsal**：Web/TUI 不能 skip-as-pass；更新 runbook；在可用环境完成三平台 manifests 与 Go/No-Go 草案。
6. **owner 修复 GitHub billing 并 rerun**：对最终 release SHA 跑 `ci` 与 `core-beta-evidence`，记录 exact run/job/artifact hashes；0-step failure 不进入产品 defect 统计。
7. **owner 完成外部治理**：branch protection、签名/公证/attestation（若首发要求）、ADR/license approvals、Go/No-Go。
8. **最后才切 public + tag/release**：先 visibility/ruleset，再推 tag/触发 release workflow；下载公开资产重新校验 SHA-256 与安装 smoke，随后发布渠道 promotion。

## Sources

### Repository files

- `README.md:1-104`
- `CHANGELOG.md:1-68`
- `SECURITY.md:1-40`
- `CONTRIBUTING.md:1-51`
- `LICENSE-MIT:1-20`; `LICENSE-APACHE:1-201`
- `Cargo.toml:1-36`; `crates/agent-session-grep-cli/Cargo.toml:1-47`
- `.github/workflows/ci.yml:1-143`
- `.github/workflows/core-beta-evidence.yml:1-244`
- `scripts/evidence/open_source_gate_benchmark.py:41-70,173-244,283-382,421-707`
- `scripts/rehearsal/compare_entrypoints.py:1-64,275-285,415-476`
- `scripts/install/install.ps1:31-49,91-139`; `scripts/install/install.sh:59-72,103-145`
- `docs/product/PROVIDER-MATURITY-MATRIX.md:22-65,98-142`
- `docs/product/COMPETITOR-COMPARISON.md:12-55`
- `docs/product/OPEN-SOURCE-ROADMAP.md:11-72,88-99`
- `docs/architecture/RFC-0002-provider-adapter-contract.md:23-42,65-103`
- `docs/adr/ADR-0002-platform-targets.md:19-50`
- `docs/adr/ADR-0009-cross-boundary-output-redaction.md:4-28`
- `docs/operations/REUSE-LICENSE-AUDIT.md:1-31,79-84`
- `docs/operations/core-beta-evidence-matrix.md:17-25,36-68`
- `docs/operations/external-readiness-gate.md:1-32`
- `docs/operations/INSTALL-AND-UPGRADE.md:1-24,109-184`
- `docs/release/rehearsal-runbook.md:15-50,104-262,297-332`
- `docs/release/environment-manifest.template.json:1-42`
- `docs/release/go-no-go.template.md:1-111`
- `crates/agent-session-grep-ports/src/lib.rs:799-827`
- `crates/agent-session-grep-adapters-sqlite/src/source_fs.rs:32-120`
- `crates/agent-session-grep-cli/src/main.rs:384-412,615-638,779-869,1180-1650,1871-1933,3749-3768`
- `crates/agent-session-grep-cli/src/mcp.rs:595-812`

### GitHub remote facts

- Repository: https://github.com/qin-devs/AgentSessions — `PRIVATE`, default branch `main` during audit.
- Final main observed: https://github.com/qin-devs/AgentSessions/commit/c5e7292bd60cb29ba5509935e2eef272ebf6e2c4
- Provider governance closure included AdapterManifest, maturity surfacing and rollback ADR; rollback remained `Proposed` pending owner/approver acceptance.
- Gate tracking commit: https://github.com/qin-devs/AgentSessions/commit/3ac3e3e031de25993d6cca219d27ec005a588831
- Latest tracked gate manifest: https://github.com/qin-devs/AgentSessions/blob/main/scripts/evidence/out/gate-manifest-gate.json
- Latest `ci` zero-step failure: https://github.com/qin-devs/AgentSessions/actions/runs/31931867185
- Latest `core-beta-evidence` zero-step failure: https://github.com/qin-devs/AgentSessions/actions/runs/31931867182
- Previous exact aeb65f3 runs, also zero-step: https://github.com/qin-devs/AgentSessions/actions/runs/31931588255 and https://github.com/qin-devs/AgentSessions/actions/runs/31931588263
- Historical successful evidence run: https://github.com/qin-devs/AgentSessions/actions/runs/30165919066（旧 workflow 配置；不证明当前 per-target provider/gate steps）。
- Releases: none；tags: none；open issues/PRs: none/none。
- Branch protection query: REST `/repos/qin-devs/AgentSessions/branches/main/protection` returned HTTP 403 requiring GitHub Pro or a public repository.
