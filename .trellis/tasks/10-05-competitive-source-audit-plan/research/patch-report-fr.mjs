import fs from 'node:fs';
const p = 'C:/AgentSessions/.trellis/tasks/10-05-competitive-source-audit-plan/review-report.md';
let s = fs.readFileSync(p, 'utf8');
const anchor = '**对 ASG 的直接含义**：SG-01（cap 先于过滤）';
if (!s.includes(anchor)) throw new Error('anchor missing');
const sec = `### fast-resume（T1 10+10 文件 / 5,595 行回执；receipt: \`research/coverage-fast-resume.json\`）

| ID | 级别 | 结论 | 关键锚点 |
|---|---|---|---|
| FR-01 | P1 | search 六个方法用 \`unwrap_or_default()/unwrap_or(0)\` 把查询错误吞成空结果/0；CLI 与 TUI 初始搜索受影响（TUI 另一路径会显示 "search failed"） | \`src/search.rs:27-63\`；\`src/main.rs:113-119\`；\`src/tui/state.rs:112,116-141\`；反证 \`src/tui.rs:204-217\` |
| FR-02 | P2 | 全量 rebuild 先 delete_all 再补写、无 last-good；provider 暂时不可读即返回空集 → 窗口期静默缩水（可自愈） | \`src/index.rs:83-97\`；\`src/main.rs:80-96,153-157\`；\`src/adapters/opencode.rs:109-130\` |
| FR-03 | P2 | Codex 正文双源双写（response_item + event_msg）导致 prompt 重复；\`turns/user_prompts\` 只认 event_msg，为空即整体丢弃 | \`src/adapters/codex.rs:85-106,122-124\` |
| FR-04 | P2 | TUI 新鲜度只有启动时单次增量：无文件监听（sweep fs_watch=0）、无手动刷新键 | \`src/tui.rs:54-81\`；\`src/tui/input.rs:16-58\` |
| FR-05 | P2 | 增量只信 mtime（容差 0.001s）、schema 无 content hash；复制保时间戳场景会漏更新 | \`src/adapters/shared.rs:27-36\`；\`src/index/schema.rs:44-63\` |
| 正面 | — | resume/launch 用 argv 数组直执行、无 shell 拼接；last-good 屏障（JSONL 健康分级+不完整扫描零删除）；容错查询解析 | \`src/main.rs:214-226\`；\`src/adapters/shared.rs:38-57,158-185\`；\`src/index/queries.rs:56-89\` |

> 口径：fast-resume 自我定位就是可重建缓存（README:72-78），FR-02/FR-05 在其定位内是边界而非缺陷断言；对 ASG 的意义恰是反例——权威 catalog 不能套用“可删缓存”语义。

`;
s = s.replace(anchor, sec + anchor);
fs.writeFileSync(p, s, 'utf8');
console.log('report patched with fast-resume section');
