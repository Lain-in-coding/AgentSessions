# Research: O7 / S3 成功后的证据发布与治理预检

- Query: 若 O7 后冻结的 S3 按既定 R4 通过，怎样精确发布报告、保留 S2 失败、绑定测量源码，并完成本地与 PR/普通 merge 治理？
- Scope: internal；产品规则、原 baseline bundle、harness/governance 源码及父基线研究只读；本轮只执行有关 CLI 的 `--help`，未执行 Git、测试、构建、benchmark、扫描或远端变更。
- Date: 2026-10-15
- Active task: `.trellis/tasks/archive/2026-10/10-15-optimize-50k-changed-sync`
- P（产品根）：`./.trellis/.runtime/attribution-closeout-20261012/canonical-repaired`。
- T（协调任务）：`./.trellis/tasks/archive/2026-10/10-15-optimize-50k-changed-sync`。
- 已读当前决定：`design.md` §9 / `implement.md` 第五组，O7 仅处理相邻同种 Edge/Placement，使用 **BATCH_IN_CHUNK=500**；Activity/Usage 保持单行。`shrink-followup-after-g4.md` 已另追加决策说明，原512候选研究未被改写。

## Findings

### 1. 发布成立的前提与身份分离

只在 **S3 新源码**的严格 oracle、实际质量门、独立检查、新 formal suite 和 R4 全部通过后，执行下面的成功发布路径。S2 的测试/测量不能替代 S3；本文件不宣布 S3 已通过，也不重新计算已经核验过的 R4 门槛。

使用三个不同身份：

| 身份 | 含义 | 绑定在哪里 |
| --- | --- | --- |
| S3 | 真正被测量的干净 O7 源码 | 三份JSON的`commit`、`baseline.source_before/after`、receipt source、manifest复现命令 |
| E | 随后的证据/说明文档提交 | PR candidate head、introduced-range、hosted checks、治理记录 |
| M | 普通merge提交 | merge结果与真实author/committer/parents核验；不是被测源码 |

**S3 → E → M 不允许回填报告的 commit。** E 相对 S3 只能含本节下列8个发布路径；若Rust、Cargo.lock、harness、生成器/协议或其他测量输入变化，旧测量仍只说明S3，不能转标为新源码结果。原 `b0fc44d75690`、旧 `88d86f4` bundle 与历史性能章节原样保留。

父基线研究 `archive/2026-10/10-13-current-source-performance-baseline/research/provenance-publication.md:102` 的最小发布写集和 S→publication 身份方法仍适用；它早期有关缺失provenance/可选`diagnostics`的描述已被正式 `baseline` 扩展取代，不能当当前实现状态。

### 2. 精确公开产物：7个bundle文件 + 1个入口修改

`<S3-prefix>` 取实际报告 S3 的无歧义12位前缀，manifest与JSON仍写完整40位SHA。

```text
docs/evidence/core-beta/<S3-prefix>/manifest.md
docs/evidence/core-beta/<S3-prefix>/run-01/core-beta-benchmark-full.json
docs/evidence/core-beta/<S3-prefix>/run-01/core-beta-benchmark-full.md
docs/evidence/core-beta/<S3-prefix>/run-02/core-beta-benchmark-full.json
docs/evidence/core-beta/<S3-prefix>/run-02/core-beta-benchmark-full.md
docs/evidence/core-beta/<S3-prefix>/run-03/core-beta-benchmark-full.json
docs/evidence/core-beta/<S3-prefix>/run-03/core-beta-benchmark-full.md
docs/performance-baseline-0.2.md
```

- 三份JSON逐字节复制正式产物；三份Markdown必须与同哈希 `render_markdown(validated_report)` 的UTF-8输出逐字节一致。不是手工改数值后的摘要。
- 不新建独立schema、CSV、额外“sanitized report JSON”、raw trace下载包或benchmark发布脚本；R4比较表和reviewed trace聚合写manifest即可。
- JSON中的 `evidence_status=locally_verified`、recovery `not_implemented`、prebuilt未独立attested等限制保留；R4通过不升级成SLO、跨平台、recall、minimum-OS或release认证，也不关闭父roadmap发布门。

### 3. 完整JSON、公开投影、私有原始文件的字段边界

**完整synthetic JSON本身就是harness的公开报告形状；不能删字段再称为同一full报告。** `public_values`与closed字段校验约束baseline/environment/receipt，child绝对路径已经在producer中转为`binary`或`scratch/...`。Markdown只是确定性的人读摘要，不是另一套隐私修补管线。

| 层 | 完整JSON必须保留 | Markdown投影/manifest的用途 |
| --- | --- | --- |
| 根身份 | schema/evidence_status/profile/generated_at_utc/commit | renderer展示status/profile/commit；manifest写完整来源关系 |
| environment | OS及版本、target/arch、CPU/核/RAM、disk/filesystem/AV、rustc/Python、SQLite来源；均为限定字段 | renderer仅展示OS/target/SQLite；完整环境差异在manifest解释 |
| dataset/binary/methodology | synthetic标记、corpus hash/counts/source bytes/query set、B hash/bytes/provenance、统计/计时/预热方法 | renderer展示摘要；JSON是复算权威 |
| metrics/RSS/sizes/recovery/limitations | 原9项legacy metrics及raw/summary，RSS原值/null原因，store/source/sidecars体积，recovery限制 | renderer输出统计表、体积、recovery/limitations；不删除raw |
| baseline provenance/plan | suite/run/times、source before/after、完整build receipt及其摘要、binary_use、child_policy、固定plan | renderer展示suite/source/harness/receipt/UTC/双时钟说明；manifest补完整B/build/环境与截止时间 |
| baseline preflight/anchor/workloads | 全部observations、role/ordinal/phase/trial、logical argv、wall/legacy duration、stdout_bytes、CLI duration、memory/correctness；corpus/census/store快照、synthetic IDs/evidence、summaries/derived、anchor final_store | renderer展示48主系列的count/P50/P95/MAD/bytes/RSS覆盖与throughput；不展示全部逐样本或身份链 |
| baseline coverage | 完整性、独立census oracle、失败计数、独立trace声明、全部limitations | renderer保留限制；实际trace成功/总时长另在manifest写reviewed结论 |

正常报告不含完整CLI stdout/stderr、真实transcript、absolute temp/Cargo paths、ambient env dump、token或私人主机信息。synthetic wire IDs、fingerprint/byte-span与logical argv是复核coverage所需字段，不能为“脱敏”删除它们。若扫描发现超界内容，先查来源并处理，不手改raw样本、删字段或扩大allowlist。

以下只保留在ignored `evidence-output/<suite>/` 或协调runtime，**不复制到public bundle**：

- `baseline-cli.exe`、`cargo-target/`、任何DB/WAL/SHM、scratch或corpus副本；
- `attempt.json`、`run-*.claim.json`、`suite.json`、`source-freeze.json`、local wrapper `build-receipt.json`；receipt公开部分已内嵌三份JSON；
- `suite-end-check.json`、私有R4计算收据、raw `index-trace-*.jsonl`、`index-trace-observation.json`、A/B/失败原型文件；只把必要的已核对聚合写manifest；
- scanner stdout/stderr、完整GitHub API/event响应、PR/merge输入、过程命令日志和机器路径。

字段依据：harness `:481`、`:1192`、`:1541`、`:1780`、`:2230`、`:2477`；产品SLI `:165`、`:200`。

### 4. S2失败必须如实保留/披露

S2是 **完整且通过格式/套件验证的测量，但未通过R4性能门**，不是损坏报告或被丢弃的“噪声样本”。原suite整个保留：

`P/evidence-output/opt50k-e8e4975817f7-20261015T121711/`

公开S3 manifest的“Earlier attempts / failed R4”至少写：

- S2完整SHA `e8e4975817f71647c158f6d802a7b98cee2f1b15`，B2 SHA `03c168a0d146ec9d0e7cfc3d49602468457e9e3643bd17f443e0407ee70bd6cd`；安全suite ID和三个原报告hash（从 `shrink-followup-after-g4.md`抄已核验值，不能只列最快一份）。
- initial三跑P50为7819.6887/8661.5382/6953.7125 ms，中位7819.6887；shrink为7146.9575/10717.6615/8717.9847 ms，中位8717.9847。shrink失败，其余47主项与8 legacy通过；不改原始数值或门槛。
- S2含trace/收尾1717156 ms，trace available；这是S2自己的suite耗时。S3单独报告其完整耗时；若披露总投入，再明确加上S2等尝试，不能把两套合并冒充一个3600秒suite。
- 后续O7是在真实代码上做有界改动后才重测；不是原样重跑S2挑最快样本。raw S2报告/trace私有保留，按本任务约束不新增失败bundle的公开写集。
- 披露同机Windows、OS cache未受控、全部跨run spread、单次trace局限，以及发现的计时标签问题；不要从关联观测推断Defender/温控等未测因果。
- 若需要概述O3/O5或陈旧binary无效前置尝试，用已核对的决定/无效原因和私有保留说明；它们不是成功S3样本，不改变S2正式失败的表述。

可采用的英文核心表述（实际manifest还需填全hash/比较表）：

> An earlier complete suite measured S2 and passed report validation.
> Its 50k shrink median did not meet R4. All samples were retained.
> O7 changed the relation-delete path before the new S3 suite.
> The successful comparison uses all three S3 runs against the original
> published baseline; no S2 sample is substituted into that suite.

### 5. S3 manifest与性能入口内容清单

- S3/H/B3、完整SHA/哈希/二进制bytes、receipt对象摘要、schema版本、Cargo.lock/manifest关联、真实工具链/SQLite证明。
- 三run JSON hash/bytes、Markdown hash、固定scale order、原始样本全部保留的声明。
- suite起点、各run区间、**含trace与收尾**的suite终点/耗时、300秒child约束、资源边界；不只抄trace前的run3 completed_elapsed_ms。
- 正式validators命令/结果与R4独立判定；48主系列和另列8 legacy的口径、old/new全表比较、全部spread。数值从已审阅R4产物来，本研究不重算。
- reviewed trace诊断与限制，不发布原始trace；`relation_upserts`包含delete的范围不能改称纯upsert成本。
- S2失败披露与原基线失败披露的关联；本suite与此前尝试耗时分开。
- 真实环境差异，特别fresh `explicit-build-target`/uncached receipt与原基线cached `source-root/target`区别；不声称可重复构建字节或独立attestation。
- 复现命令固定 **实际S3完整SHA**，先对比workspace HEAD；不要把 `git rev-parse HEAD`直接当期望source（在E/M上会测错源码）。复用既有formal协议，不加`--binary`，不改harness/hash。
- `docs/performance-baseline-0.2.md`追加实际日期小节，链接新manifest，保留2026-10-14基线与旧章节，简述O7作用/结果/限制。不写尚未出现的E/M SHA，不用“latest main”替代source绑定。

### 6. 复制、投影与报告校验命令（未来执行，不是本次执行结果）

变量由主会话填入**实际完成的S3 suite**；不能沿用S2路径。以下不会运行benchmark，但会复制审阅后的公开文件。先完成R4/trace/源冻结核对。

```powershell
$ErrorActionPreference = 'Stop'
$publicationProduct = './.trellis/.runtime/attribution-closeout-20261012/canonical-repaired'
# Set this to the completed S3 suite, not the known failed S2 suite.
$publicationSuite = Join-Path $publicationProduct 'evidence-output/<actual-S3-suite-id>'
$publicationFreeze = Get-Content -LiteralPath (Join-Path $publicationSuite 'source-freeze.json') -Raw | ConvertFrom-Json
$publicationS3 = $publicationFreeze.source_commit
if ($publicationS3 -notmatch '^[0-9a-f]{40}$') { throw 'Missing frozen S3.' }
if ($publicationS3 -eq 'e8e4975817f71647c158f6d802a7b98cee2f1b15') { throw 'S2 failed R4.' }
$publicationBundle = Join-Path $publicationProduct ('docs/evidence/core-beta/' + $publicationS3.Substring(0,12))
if (Test-Path -LiteralPath $publicationBundle) { throw 'Do not overwrite an existing bundle.' }
$publicationHarness = Join-Path $publicationProduct 'scripts/evidence/core_beta_benchmark.py'
if ((Get-FileHash -LiteralPath $publicationHarness -Algorithm SHA256).Hash.ToLowerInvariant() -ne $publicationFreeze.harness_sha256) { throw 'Use the measured renderer/checker bytes.' }
$publicationReports = @()
foreach ($publicationRun in 1..3) {
    $publicationName = 'run-{0:D2}' -f $publicationRun
    $publicationInput = Join-Path $publicationSuite $publicationName
    $publicationOutput = Join-Path $publicationBundle $publicationName
    $publicationData = Get-Content -LiteralPath (Join-Path $publicationInput 'core-beta-benchmark-full.json') -Raw | ConvertFrom-Json
    if ($publicationData.commit -ne $publicationS3) { throw 'Mixed measured source.' }
    New-Item -ItemType Directory -Path $publicationOutput | Out-Null
    foreach ($publicationFile in @('core-beta-benchmark-full.json','core-beta-benchmark-full.md')) {
        $publicationFrom = Join-Path $publicationInput $publicationFile
        $publicationTo = Join-Path $publicationOutput $publicationFile
        Copy-Item -LiteralPath $publicationFrom -Destination $publicationTo
        if ((Get-FileHash -LiteralPath $publicationFrom -Algorithm SHA256).Hash -ne (Get-FileHash -LiteralPath $publicationTo -Algorithm SHA256).Hash) { throw 'Evidence copy differs.' }
    }
    $publicationReport = Join-Path $publicationOutput 'core-beta-benchmark-full.json'
    python -B $publicationHarness validate-report $publicationReport
    if ($LASTEXITCODE -ne 0) { throw 'Published report validation failed.' }
    $publicationReports += $publicationReport
}
python -B $publicationHarness validate-baseline @publicationReports
if ($LASTEXITCODE -ne 0) { throw 'Published suite validation failed.' }
@'
import importlib.util, json, pathlib, sys
product, bundle = map(pathlib.Path, sys.argv[1:])
module_path = product / 'scripts/evidence/core_beta_benchmark.py'
spec = importlib.util.spec_from_file_location('publication_harness', module_path)
harness = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = harness
spec.loader.exec_module(harness)
for ordinal in (1, 2, 3):
    report_path = bundle / f'run-{ordinal:02d}/core-beta-benchmark-full.json'
    report = json.loads(report_path.read_text(encoding='utf-8'))
    expected = harness.render_markdown(report).encode('utf-8')
    assert report_path.with_suffix('.md').read_bytes() == expected, 'Markdown is not the exact measured renderer projection'
print('All three Markdown projections match byte-for-byte.')
'@ | python -B - $publicationProduct $publicationBundle
if ($LASTEXITCODE -ne 0) { throw 'Projection verification failed.' }
```

然后按§5写新manifest/入口，检查全部相对链接存在，用 `Get-FileHash -Algorithm SHA256` 与 `Get-Item ... .Length`填写hash表；report文件保持复制原样。`render_markdown`是Python函数，CLI没有`render`/`publish`子命令（harness`:2554`）。

### 7. 本地质量与治理命令

质量收据须覆盖最终代码。S3冻结前应已完成O7测试、严格oracle、独立检查及下列质量门；纯文档E不应重新跑完整formal suite。若后续代码发生变化，更新质量证据，不能沿用S3结果冒充新源码。

```text
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
python -B -m unittest discover -s scripts/evidence -p "test_*.py" -v
python -B -m unittest discover -s scripts -p "test_*.py" -v
```

以上命令从P执行，逐条检查退出码；只把实际执行与通过的收据写入PR。harness相关历史兼容可用原bundle执行 `validate-report`，新bundle已按§6逐一验证。没有为本任务找到独立Markdown/YAML lint配置；文档用链接/表格/投影审核与 `git diff --check`，不要虚构已运行的lint工具或为此安装工具。未修改release/workflow，不擅自新增release变更；CI现有release/feature检查仍由其真实结果覆盖。

治理输入必须先具体化，并全部放ignored私有目录（例如 `P/evidence-output/<S3-suite>/publication-governance/`）：

| 变量 | 精确含义/示例文件 |
| --- | --- |
| `$govBase` | 独立读取的当前remote main完整SHA；不是固定开发起点de2c95d，也不是B2/B3二进制hash |
| `$govHead` | 当前PR候选E或后续审阅修复的完整SHA；应与本地候选相等 |
| `$govTrustedRoot` | 已审阅的**当前base完整checkout**；helper与可信privacy配置一起来自base，不从candidate选配置 |
| `$govEvent` | `publication-governance/pr-event.json`，当前完整GitHub event envelope，见下文 |
| `$govRepositoryId` | API独立确认的正整数repository ID |
| `$govMergeMessage` | `publication-governance/merge-message.txt`，完整scoped header、空行、英文why |
| `$govMergeMetadata` | `publication-governance/merge-metadata.json`，真实`author:{name,email}`/`committer:{name,email}` |
| `$govMergeSubject` / `$govMergeBody` | 前者为上述完整message的第一行；后者为`publication-governance/merge-body.txt`，仅含空行后的body，不能重复header |
| `$publicationCommitMessage` | `publication-governance/publication-commit-message.txt`，实际E提交的完整已校验message |
| `$publicationPrTitle` / `$publicationPrBody` | 实际PR标题 / `publication-governance/pr-body.md`；保留真换行、英文why/风险/验证，不留模板提示 |
| `$govArchive` | 既有核验的 `gitleaks_8.30.1_windows_x64.zip`原archive，不是exe；wrapper重新核hash |

当前Windows pinned archive SHA-256为 `d29144deff3a68aa93ced33dddf84b7fdc26070add4aa0f4513094c8332afc4e`（trusted `provision_gitleaks.py:16`）。没有archive不能假称扫描通过；此研究不自动安装或下载。

**必须使用complete history**：range是reachable(head)减reachable(base)，包括merges、后来删掉的中间blobs/path/messages；base不一定是head祖先，但图必须完整且有共同祖先。不要用HEAD-only或`--whole-history`替代当前introduced-range。已有base治理目录完整时，不能改用旧bootstrap SHA。

下面路径参数是当前实际核验值；命令输出通过执行器保存到private governance目录，尤其scanner原始输出不粘贴PR：

执行顺序分两阶段：E提交后、首次feature push前先跑range/tree/commit与真实merge输入的扫描，此时尚无PR则从scanner命令省略`--event-file`和`--repository-id`，不运行或伪造`check_pr`输入。PR创建后再用真实完整event执行下面全组命令，补齐PR metadata覆盖；前一阶段不能宣称已检查尚不存在的PR。两阶段都不绕过当前授权或真实门禁。

```powershell
$govTools = Join-Path $govTrustedRoot 'scripts/governance'
python -B (Join-Path $govTools 'check_commit.py') --message-file $govMergeMessage
if ($LASTEXITCODE -ne 0) { throw 'Merge message rejected.' }
python -B (Join-Path $govTools 'check_commit.py') --repo $publicationProduct --base $govBase --head $govHead
if ($LASTEXITCODE -ne 0) { throw 'Introduced commit metadata rejected.' }
python -B (Join-Path $govTools 'check_pr.py') --event-file $govEvent `
  --repo $publicationProduct --expected-base $govBase --expected-head $govHead `
  --repository-id $govRepositoryId --merge-message-file $govMergeMessage `
  --merge-metadata-file $govMergeMetadata
if ($LASTEXITCODE -ne 0) { throw 'PR/current-range metadata rejected.' }
python -B (Join-Path $govTools 'scan_paths.py') --repo $publicationProduct `
  --base $govBase --head $govHead --event-file $govEvent `
  --repository-id $govRepositoryId --merge-message-file $govMergeMessage `
  --merge-metadata-file $govMergeMetadata
if ($LASTEXITCODE -ne 0) { throw 'Path/privacy gate rejected.' }
python -B (Join-Path $govTools 'scan_credentials.py') --repo $publicationProduct `
  --base $govBase --head $govHead --gitleaks-archive $govArchive --platform windows_x64 `
  --event-file $govEvent --repository-id $govRepositoryId `
  --merge-message-file $govMergeMessage --merge-metadata-file $govMergeMetadata
if ($LASTEXITCODE -ne 0) { throw 'Credential gate rejected.' }
```

`check_pr`仅`--event-file`时不覆盖commit history；`--repo`与独立expected值不能漏。event必须有真实允许的action（opened/synchronize/reopened/edited/ready_for_review）、number、repository.id、完整pull_request（含number、base.repo.id/base.sha/head.sha/title/body等）。**`gh pr view --json`的精简对象或裸REST pull对象不是合法完整event**。沿用trusted workflow的实际event/API冻结方式，不编造action或删去volatile字段来稳定摘要。

`check_pr.py`的`metadata_sha256`摘要域为校验后的fields +完整event摘要及merge字段；两个scanner的同名字段摘要域是完整event/merge输入容器。**不要强制三者摘要相同**；按helper各自标记保存，check_pr摘要与PR/base/head/审阅记录绑定。相同输入下两scanner共享同一metadata边界。扫描前后再次API读取完整event并比较其canonical JSON，全字段任何变化都需要刷新检查；不能用未扫描的新API快照覆盖旧冻结文件。

治理self-tests使用candidate代码、已核验archive，独立于trusted扫描。不允许skip/expected failure/空suite。下列是现有workflow实际严格runner的本地等价调用；环境只作用于当前进程并恢复：

```powershell
$govHadArchive = Test-Path Env:GOVERNANCE_TEST_ARCHIVE
$govPriorArchive = $env:GOVERNANCE_TEST_ARCHIVE
try {
    $env:GOVERNANCE_TEST_ARCHIVE = $govArchive
    Set-Location -LiteralPath $publicationProduct
    python -B -c "import unittest, sys; suite = unittest.defaultTestLoader.discover('scripts/governance', pattern='test_*.py'); result = unittest.TextTestRunner(verbosity=2).run(suite) if suite.countTestCases() else None; sys.exit(0 if result is not None and result.testsRun > 0 and result.wasSuccessful() and not result.skipped and not result.expectedFailures else 1)"
    if ($LASTEXITCODE -ne 0) { throw 'Governance tests failed, skipped, expected-failed or empty.' }
} finally {
    if ($govHadArchive) { $env:GOVERNANCE_TEST_ARCHIVE = $govPriorArchive }
    else { Remove-Item Env:GOVERNANCE_TEST_ARCHIVE }
}
```

依据：CONTRIBUTING`:176`、`:190`、`:212`、`:278`；governance workflow`:345`、`:528`；helper本地`--help`已读取。不要扩allowlist、加inline allow、baseline suppression或探测live凭据；unsupported binary/archive/encoding coverage和工具错误都是失败。

### 8. E提交、PR与普通merge约束

本研究只给后续命令，不执行。所有Git/发布动作由主会话沿用本会话已有授权处理；不因本清单重复索取已经授权的操作。

- scoped Conventional Commit，允许type以CONTRIBUTING为准；完整header≤50 Unicode codepoints，英文祈使、无末尾句号；空行后实质英文why，body每行≤72。PR标题/正文、proposed merge message同样适用。例标题：`perf(sqlite): speed up changed sync`；文档提交可用 `docs(evidence): publish sync results`，正文必须填写真实原因和验证，不使用占位符。
- 明确stage上述8路径；不要`git add .`。E只发布docs，O7代码已有独立原子提交。未来可执行：

```powershell
Set-Location -LiteralPath $publicationProduct
git diff --check
if ($LASTEXITCODE -ne 0) { throw 'Whitespace check failed.' }
git add -- $publicationBundle 'docs/performance-baseline-0.2.md'
if ($LASTEXITCODE -ne 0) { throw 'Staging failed.' }
git diff --cached --name-only
# Confirm exactly the eight allowed paths, then inspect the staged diff.
git diff --cached --check
if ($LASTEXITCODE -ne 0) { throw 'Staged whitespace check failed.' }
# Commit only with the separately reviewed message file and genuine identity.
git commit --file $publicationCommitMessage
if ($LASTEXITCODE -ne 0) { throw 'Publication commit failed.' }
```

- PR只开到 `Lain-in-coding/agent-session-grep`的main，feature为当前任务 `perf/50k-changed-sync`；不直接push main。推送前先完成本地tree/introduced-range/消息扫描；PR创建后补全真实event/merge metadata扫描与hosted checks。
- PR按`.github/pull_request_template.md`写清why/scope、S3/原基线、S2失败、验证、风险与回滚、真实attribution和限制。多行body放私有UTF-8文件，经`--body-file`传递，不能把JSON.stringify当shell转义。
- 候选当前SHA必须实际完成 mandatory checks。仓库要求读回current main protection、GitHub Actions App的 `gate`与`all required verification checks`，以及PR要求、管理员enforcement、ordinary-merge-only。实际结果非success（failed/cancelled/skipped/missing/pending）均不能当绿。旧checks、S2或S3的旧head、YAML存在均不证明E通过。
- `ci.yml`的`all required quality checks`是另一层聚合；`release-verify.yml:893`才是`all required verification checks`。`gh pr checks --required`为空不等于无门槛，需要读回真实保护/规则。
- CONTRIBUTING`:115`的文本要求owner在当前PR/head审阅后明确接受。**本会话已有明确Git授权，主会话已要求持续沿用，因此本清单不据该条增加新的批准停点。** 实际技术/治理门禁仍必须通过，新的head/metadata须重新检查；记录现有授权即可，不声称发生了未实际进行的逐SHA人工审阅，不伪造第二审阅者或self-APPROVED review，也不把helper pass当人工审阅。
- 普通merge保留原子commits/SHA/真实authors。已推送commit不amend/rebase/squash，除非明确要求；不用`--admin`、默认GitHub merge消息或skip-CI指令绕过规则。

相关实际CLI形状（本地`--help`已确认；远端命令本轮均未执行）：

```powershell
# Only after the authorized feature push and a reviewed body exist.
gh pr create -R Lain-in-coding/agent-session-grep --base main `
  --head perf/50k-changed-sync --title $publicationPrTitle --body-file $publicationPrBody
# Refresh actual metadata/controls; retain full responses privately as needed.
gh pr view $publicationPrNumber -R Lain-in-coding/agent-session-grep `
  --json number,url,baseRefOid,headRefOid,title,body,mergeStateStatus,statusCheckRollup
gh api 'repos/Lain-in-coding/agent-session-grep/branches/main/protection'
gh api 'repos/Lain-in-coding/agent-session-grep/rules/branches/main'
gh api 'repos/Lain-in-coding/agent-session-grep' `
  --jq '{id,allow_merge_commit,allow_squash_merge,allow_rebase_merge}'
gh pr checks $publicationPrNumber -R Lain-in-coding/agent-session-grep `
  --required --json name,state,bucket,workflow,link
# Only at the current checked SHA under the standing explicit Git authorization.
# Subject + body must reconstruct the validated full merge-message.txt.
gh pr merge $publicationPrNumber -R Lain-in-coding/agent-session-grep `
  --merge --match-head-commit $govHead --subject $govMergeSubject --body-file $govMergeBody
# Read the resulting merge and its actual parents/message/author/committer.
gh pr view $publicationPrNumber -R Lain-in-coding/agent-session-grep `
  --json number,url,mergedAt,mergeCommit,headRefOid,baseRefOid
gh api ('repos/Lain-in-coding/agent-session-grep/commits/' + $publicationMergeSha)
```

远端命令每条检查退出码；分页endpoint必须完整读取，API错误/403/缺覆盖不推断成规则不存在。merge前立即复核current base/head、checks与metadata binding，沿用已存在的明确Git授权；`--match-head-commit`防head漂移但不替代base/metadata复查。merge后核对真实message、parents、author/committer及实际结果，记录M，**不编辑报告S3字段**。不顺带删除feature branch、改保护/CI/父发布门或清除失败证据。

## Files / Related Specs / References

| 文件 | 本次使用 |
| --- | --- |
| T `design.md` §9、`implement.md` 第五组/发布段 | O7=500参数、S3重新验证、仅R4通过后成功发布 |
| T `research/formal-protocol-verification.md` | 已定的48主+8legacy协议、精确门槛、trace/deadline收尾 |
| T `research/shrink-followup-after-g4.md` | S2失败全部摘要、原报告hash、O7决定追加 |
| 父baseline `archive/2026-10/10-13-current-source-performance-baseline/research/provenance-publication.md:102` | 原始最小publication写集与S/文档提交分离 |
| P `CONTRIBUTING.md:27`、`:66`、`:103`、`:176` | 50/72、隐私、introduced range、真实owner/merge/当前checks约束 |
| P `.github/pull_request_template.md` | PR完整内容与现有验收要求 |
| P `docs/evidence/core-beta/b0fc44d75690/manifest.md`及三个run目录 | 7文件bundle布局、hash表、失败/trace/复现披露范式 |
| P `docs/product/SLI-AND-BENCHMARK-FORMAT.md:200` | 来源、有限执行、raw/private/public界限 |
| P `scripts/evidence/core_beta_benchmark.py:481`、`:1192`、`:2230`、`:2477`、`:2554` | public字段、logical argv、验证、Markdown、实际CLI |
| P `scripts/governance/check_commit.py`、`check_pr.py:7`/`:72`、`scan_paths.py`、`scan_credentials.py` | 实际参数、event envelope、摘要域、完整range/metadata扫描 |
| P `scripts/governance/provision_gitleaks.py:12`、`.github/workflows/governance.yml:528` | pinned scanner archive、拒绝skip/空suite的runner |
| P `.github/workflows/ci.yml`、`release-verify.yml:893`、`governance.yml:557` | 当前checked-in聚合名称，不能替代live托管证明 |
| 本地 gh skill 与 `gh pr create/merge/checks --help` | 当前已安装CLI参数面；不调用远端或采用skill中的过时search子命令示例 |

External references：本研究代理没有读取远端资料；主体依据指定本地合同、历史publication研究和当前CLI help/source。另附主会话本轮live API回报作为时间点观察；不等于本研究独立查询，也不保证最终推送/合并时状态未变。

## Timestamped observation: 主会话实时治理预检回报

- 研究入档时间：**2026-10-15 05:11:49 UTC**（北京时间13:11:49）；这是本代理记录主会话消息的时间，主会话未另附API采样到秒的时间。
- 来源：主会话在本轮消息中说明已完成只读live GitHub preflight；本研究不重复查询远端。

| 已回报字段 | 时间点值 |
| --- | --- |
| main | `de2c95d2c93f7453ce5b5ccc169614d3aa7892ee` |
| repository ID | `1338069966` |
| required contexts | `gate`、`all required verification checks`；两项App ID均为`15368` |
| strict / enforce_admins | `true` / `true` |
| approving_review_count | `0` |
| merge policy | ordinary merge only |
| force-push / branch delete | disabled / disabled |

最终push/merge前仍需主会话再次读取实际base/head、保护和checks；此快照不代替候选E的成功结果。`approving_review_count=0`只是托管设置，不证明发生了任何人工逐SHA审阅。现有明确用户Git授权持续有效，不新增批准停点，不豁免实际门禁。

## Caveats / Not Found

- S3尚未由本研究测量/验证；实际S3、suite、E/M、PR ID、trusted base checkout与本地archive路径未在此猜测。repository ID/current main等已收到上文主会话快照，最终仍重新核对。示例suite占位符必须来自真实S3输出。
- 本次没有运行tests、build、benchmark、scanner、Git或远端读写；`--help`不是质量门通过证据。
- `implement.md`早期发布段仍写`<S2-prefix>`；第五组明确下一次成功测量为S3。执行时按**实际被测source**生成目录，不能机械沿用S2失败前缀；本研究不修改任务计划文件。
- 现有明确用户Git授权由主会话持续适用；本清单不新增权限或批准停点，也不声称进行了未发生的逐SHA人工审阅。缺失输入/未通过检查须如实记录，不伪造成功或审阅者。
