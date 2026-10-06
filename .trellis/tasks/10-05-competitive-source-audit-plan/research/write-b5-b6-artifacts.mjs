import fs from 'node:fs';
const root = 'C:/AgentSessions/.trellis/tasks';

const b5prd = [
'# Provider 稳定性：Claude/Codex 生命周期证据与矩阵同步（B5）',
'',
'> 父任务：.trellis/tasks/10-05-competitive-source-audit-plan。用户 2026-10-06 批准。',
'> 依赖：无（与 B4 并行需避免同文件：本任务主写 provider 两个 crate + provider 矩阵文档；adapters-sqlite 只允许读）。上游：review-report P1-06、父 design.md 第 7 节。',
'',
'## Goal',
'',
'把 Claude/Codex 的支持声明变成可追溯证据：版本/变体/OS 边界明确、生命周期（append/shrink/同长改写/分叉/移动/SQLite WAL）有 golden/property 测试、矩阵与实测一致；无 span 不伪造 offset；长尾保持 experimental，不为“收敛”删能力。',
'',
'## Requirements',
'',
'1. 生命周期测试补强（provider-claude / provider-codex）：append、shrink、同长改写、分叉（parent/父子边）、文件移动、SQLite WAL 读取；使用合成/授权 fixture，不碰真实用户数据。',
'2. 证据与声明一致：docs/product/PROVIDER-MATURITY-MATRIX.md 与 PROVIDER-BETA-READINESS.md 的每条声明要么指到测试/证据锚点，要么标注 experimental/未验证；不新增无法支撑的“native/resume 成功”口径。',
'3. 无 span 的源（SQLite/整文件 JSON）继续 precision=unknown，不伪造 byte offset（对照 crates/agent-session-grep-ports/src/capability.rs 的 source_span 约定）。',
'4. 失败语义：不完整解析不得推进成功水位/覆盖 last-good（与 D3 #3 一致，回归锁已在 adapters-sqlite）。',
'',
'## Non-goals',
'',
'- 不新增 provider；不改长尾认证等级；不执行真实 native resume（环境限制，如实记录）。',
'',
'## Acceptance Criteria',
'',
'- [ ] 六类生命周期场景（append/shrink/rewrite/fork/move/WAL）各有测试或明确 N/A 理由（含 file:line 锚点）。',
'- [ ] 矩阵文档同步：涉及行有锚点/标注，无无证据的 native/完整度声明。',
'- [ ] offset 伪造专项测试（evidence precision=unknown 路径）通过。',
'- [ ] cargo fmt/clippy/test（workspace，isolated target dir）全绿。',
''].join('\n');
fs.writeFileSync(root + '/10-06-provider-stability/prd.md', b5prd, 'utf8');

const b5impl = [
'# 实施计划：provider-stability',
'',
'## Steps',
'',
'1. 读 provider-claude/provider-codex 现有测试与 fixture 布局，列出六类场景的缺口表（research/scenario-matrix.md）。',
'2. 补测试：优先复用既有 fixture 生成器；新增合成变体（shrink/rewrite/fork/move/WAL）。',
'3. 矩阵同步：docs/product/PROVIDER-MATURITY-MATRIX.md 与 PROVIDER-BETA-READINESS.md 逐条核对（不改等级，只补锚点/限定）。',
'4. offset 伪造专项测试（无 span 源 → precision unknown）。',
'5. 全量门禁 + 证据归档。',
'',
'## Validation commands',
'',
'    cargo fmt --all --check',
'    cargo clippy --workspace --all-targets --locked --target-dir C:/AgentSessions/.trellis/.runtime/target-b5 -- -D warnings',
'    cargo test --workspace --locked --target-dir C:/AgentSessions/.trellis/.runtime/target-b5',
''].join('\n');
fs.writeFileSync(root + '/10-06-provider-stability/implement.md', b5impl, 'utf8');

const b6prd = [
'# 发布闭环：三 OS 构建/安装/升级/卸载证据（B6）',
'',
'> 父任务：.trellis/tasks/10-05-competitive-source-audit-plan。用户 2026-10-06 批准（D2；发布/签名/账号动作仍属 owner 边界）。',
'> 依赖：B0/B1/B3 已落地；上游：review-report P0/P1-08、父 design.md 第 8 节。现存资产：.github/workflows/release.yml、docs/operations/INSTALL-AND-UPGRADE.md、docs/release/OWNER-RELEASE-CHECKLIST.md。',
'',
'## Goal',
'',
'让“新用户拿到具体产物可独立完成核心闭环”成为可验证事实：当前 commit 绑定三 OS 构建/测试/产物哈希；本机 Windows 完成 install/upgrade/uninstall smoke；文档列真实限制；不擅自发布/签名。',
'',
'## Requirements',
'',
'1. CI：修订 .github/workflows/release.yml（或新增验证 workflow）——三 OS build+test、产物 + SHA256、失败即红；workflow 语法本地可校验。',
'2. 本机（Windows）：release build + 干净安装 + 首次运行（version/config paths）+ 升级（覆盖安装）+ 卸载 smoke，全部留日志。',
'3. 文档：docs/operations/INSTALL-AND-UPGRADE.md 与 docs/release/OWNER-RELEASE-CHECKLIST.md 更新为当前真实流程（含双命令名、PATH、限制、owner 待办清单）。',
'4. 指标口径：preview/native resume/pack validity/任务完成率不得混用（与 B0 一致）。',
'',
'## Non-goals',
'',
'- 不 publish、不签名、不动账号/仓库权限、不清理历史。CI 定义≠执行通过：没有实际运行结果的 OS 必须标注未本地执行。',
'',
'## Acceptance Criteria',
'',
'- [ ] release workflow 改动经本地语法校验（yaml 解析或等价），三 OS matrix 完整、绑定 lockfile/feature/target。',
'- [ ] Windows smoke 脚本真实执行：安装/首跑/升级/卸载日志入 research/；失败项如实报告。',
'- [ ] 安装文档按实测更新（命令、路径、卸载、限制）。',
'- [ ] 不出现“已发布/已签名/三 OS 全绿”等未兑现表述。',
''].join('\n');
fs.writeFileSync(root + '/10-06-release-closure/prd.md', b6prd, 'utf8');

const b6impl = [
'# 实施计划：release-closure',
'',
'## Steps',
'',
'1. 盘点现状：release.yml 触发条件/矩阵/产物；scripts/release 既有资产；INSTALL-AND-UPGRADE 现状。',
'2. workflow 修订（三 OS matrix + SHA256 + artifact 上传），本地做 YAML/动作名/权限自检。',
'3. Windows smoke：构建 → 安装到临时前缀 → 首跑 → 覆盖升级 → 卸载；日志与退出码归档。',
'4. 文档更新（安装/升级/卸载/限制/owner 清单）。',
'5. 汇总证据表：哪些真跑过、哪些只有 CI 定义、哪些属 owner 未决。',
'',
'## Validation commands',
'',
'    cargo build --release --locked --target-dir C:/AgentSessions/.trellis/.runtime/target-b6',
'    cargo test --workspace --locked --target-dir C:/AgentSessions/.trellis/.runtime/target-b6',
'    node -e "JSON.parse(require(\"fs\").readFileSync(\"package.json\",\"utf8\"))"  # 无 npm 依赖时的最小心跳；YAML 用现有工具/自写解析校验',
''].join('\n');
fs.writeFileSync(root + '/10-06-release-closure/implement.md', b6impl, 'utf8');

console.log('B5/B6 artifacts written');
