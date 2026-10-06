import fs from 'node:fs';
const research = 'C:/AgentSessions/.trellis/tasks/10-05-competitive-source-audit-plan/research';
const csv = fs.readFileSync('C:/AgentSessions/Github_src/cc-sessions-viewer/src-tauri/Cargo.toml', 'utf8');
const rayon = /rayon/.test(csv);
const md = `# 竞品事实表修正证据（P0-01 / B0 准备件）

> 日期：2026-10-06。证据来源：本地固定 clone 的机械检测（\`research/stack-matrix.json\`）与源码锚点。本文件只准备修正证据，**不修改** \`docs/product/COMPETITOR-COMPARISON.md\`（未经 B0 批准）。
> 口径：只列可复核的形态/技术栈事实；不据此作质量结论。

## 逐行错误（引 \`docs/product/COMPETITOR-COMPARISON.md\`）

| # | 行 | 现表说法 | 实测事实 | 证据 |
|---|---|---|---|---|
| 1 | L22 cass | "Python CLI" | **Rust** CLI/TUI（约 100K 行 \`src/lib.rs\`、Tantivy 生态、cargo 工程） | \`stack-matrix.json\`；\`Github_src/coding_agent_session_search/Cargo.toml\` |
| 2 | L23 agentsview | "TS CLI" | **Go 后端 + Svelte 前端**（go.mod、internal/*.go、frontend/package.json 依赖 svelte） | \`stack-matrix.json\`；\`agentsview/frontend/package.json\` |
| 3 | L24 AgentRecall | "Python CLI" | **TypeScript/Electron/React 桌面**（package.json 依赖 electron+react、src/renderer/src/*.tsx） | \`stack-matrix.json\`；\`AgentRecall/package.json\` |
| 4 | L28 agent-sessions | "Go CLI" | **Swift/SwiftUI macOS 桌面**（455 个 .swift；Views/Services/Indexing） | \`stack-matrix.json\`；\`agent-sessions/AgentSessions/\` |
| 5 | L29 agf | "Go CLI" | **Rust** CLI+TUI（Cargo.toml；src/*.rs；TUI 2686 行已全文回执） | \`stack-matrix.json\`；\`agf/Cargo.toml\`；\`research/coverage-agf-tui.json\` |
| 6 | L34 claude-historian-mcp | "Python MCP" | **TypeScript/Node** MCP（package.json；无 pyproject/setup.py） | \`stack-matrix.json\` |
| 7 | L30 + L49 cc-switch | "无检索(配置切换器)"、"不可比" | **Tauri+React+Rust**，含 **FlexSearch 会话 metadata 检索** 与 7 家 scanner/会话管理；至少应改为"metadata 检索、非全文"并重新评估可比性 | \`stack-matrix.json\`（FlexSearch 依赖）；\`cc-switch/src/hooks/useSessionSearch.ts:14-69\`；\`src-tauri/src/services/session_usage_codex.rs\` |
| 8 | L25 hstry | "TS adapter 生态" | **Rust core（crates/hstry-core、hstry-cli、hstry-tui）+ TS adapters** 混合形态；"FTS+adapter 架构"需补 Rust 主体 | \`stack-matrix.json\`；\`hstry/crates/\` 与 \`hstry/adapters/\` |
| 9 | 名单 | 14 项外部项目 | 本地对照为 **15 项**：**Wake**（Rust+GPUI，FTS5 trigram，commit 71aeca6）未出现在表中 | \`stack-matrix.json\`；\`review-report.md\` 第三节目录 |
| 10 | L33 memex | "Rust CLI" | 需拆分：**memex-lite**（Regex 现场扫）与 **memex-rs**（FTS/vector/RRF+compact 服务）为两套；另有 web(Vue) | \`memex/memex-lite/Cargo.toml\`、\`memex/memex-rs/Cargo.toml\`、\`memex/web/src/views/SearchView.vue\` |

## 待核项（未定，不得抢写）

- cc-sessions-viewer L31 声称"rayon 并行全量扫描"：本机检测 \`src-tauri/Cargo.toml\` ${rayon ? '**命中 rayon 依赖**' : '**未命中 rayon**（需人工再看代码，可能用 std thread / 其他并行库）'}。
- 各行 provider 数（40+/16/…) 为历史 deep-read 快照口径，本轮未逐仓重数；B0 修正时要么重数、要么标注快照日期与口径。
- license 行不在本轮核验范围（cass rider、cc-sessions-viewer 无 LICENSE 与前述报告一致）。

## B0 修改建议（未实施）

1. 上表 1-8 逐行改形态列，保留原始快照日期与 commit；不删除历史记录，改为"修正说明"。
2. cc-switch 从"无检索/不可比"改为"metadata 检索（FlexSearch），非全文证据检索"；保留"与 ASG 不同问题域"的谨慎表述。
3. 名单补 Wake 一行（Rust+GPUI、FTS5 trigram、CLI/MCP）。
4. 任意"无人/唯一"措辞逐条对证据复核（见 review-report §四 P0-01）。
`;
fs.writeFileSync(research + '/fact-table-corrections.md', md, 'utf8');
console.log('written; rayon=', rayon);
