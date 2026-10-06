import fs from 'node:fs';
const p = 'C:/AgentSessions/.trellis/tasks/10-05-competitive-source-audit-plan/review-report.md';
let s = fs.readFileSync(p, 'utf8');
const anchor = '**对 ASG 的直接含义**：SG-01（cap 先于过滤）';
if (!s.includes(anchor)) throw new Error('anchor missing');
const sec = `### cass · 分片 A（pack/证据捆绑/资产状态；5 文件 / 8,421 行全文回执；receipt: \`research/coverage-cass-pack.json\`）

| ID | 级别 | 结论 | 关键锚点 |
|---|---|---|---|
| CASS-A-01 | P1 | 证据锚定弱于字面语义：\`verified\` 只查锚点存在；\`message_index\` 恒空、\`line_end=line_start\` → 可回文件+行+conversation_id+哈希，但回不到稳定消息身份 | \`src/search/pack_planner.rs:280,282,1927\` |
| CASS-A-02 | P2 | 预算只是“选材预算”：仅 evidence 段被强制；\`max_output_tokens_with_overflow\` 全库无人读取（机械验证）；omitted/metadata/outline 渲染无界 | \`pack_planner.rs:766,897-905,1527-1530\` |
| CASS-A-03 | P2 | \`context_lines\` 通过校验但零消费（全文无上下文扩展逻辑，仅回显）→ 调参无行为变化 | \`pack_planner.rs:62,72,83,1482\` |
| CASS-A-04 | P2 | 契约死条目与失真字段：\`SameSessionLowerRank\`/\`FieldMaskExcluded\` 永不产出；硬淘汰候选 \`estimated_tokens\` 恒 0 | \`pack_planner.rs:401,405,824-836,1313-1327\` |
| CASS-A-05 | P2 | 资产锁读取静默降级（不可读锁 → Default → Idle → Launch）；fail-open 仅在 lexical 可用时成立；单飞互斥依赖下游 flock（本文件无证明） | \`asset_state.rs:223,310,1785,1815,1808-1855\` |
| CASS-A-06 | P3 | 事件日志非原子截断；脱敏区间恒为整串 | \`asset_state.rs:1977-1996,1777-1792\` |
| 正面 | — | 成文 \`cass.pack.v1\` schema+limits/validate；七级确定性决胜链+测试；漏因账本；三态新鲜度+4×窗口衰减；health/warnings/recommended_action | \`pack_planner.rs:58-87,1463,1264-1293,3413-3456,384-406,1146-1168,1556-1656\` |

> 双重含义：① 竞争事实表必须承认 cass 已有 pack 级契约（“无人做到”措辞不成立）；② cass 的 pack 本身也是半成品——消息身份锚定缺失、预算/契约存在哑火位，ASG 的 Stable Message/placement/cursor generation 正是它没有的。**两边都不该吹。**

### claude-historian-mcp（41/46 文件 full，12/12 源码 8,406 行；receipt: \`research/coverage-claude-historian.json\`）

| ID | 级别 | 结论 | 关键锚点 |
|---|---|---|---|
| CH-01 | 中 | 查询词未转义进 RegExp：\`c++\`、\`foo[\` 等在 config/memories/plans 三处抛 SyntaxError 被 catch 吞掉 → 静默全空；其它 scope 用 includes 不受影响，同查询跨 scope 行为不一致 | \`search.ts:2099,2137,2240\` |
| CH-02 | 中 | 项目路径解码有损：\`replace(/-/g,'/')\` 把含连字符项目解错；\`search_config\` 对 \`.claude\` 目录探测全错；代码注释自认 lossy | \`utils.ts:199-202\`；\`parser.ts:124\`；\`search.ts:1597,2247,233-234\` |
| CH-03 | 中 | 无任何自动化行为测试：\`npm test\`=typecheck+lint；PERFORMANCE.md 的 benchmark 是人工命令 → 回归无护栏 | \`package.json:30\`；\`test.yml:18-22\`；\`PERFORMANCE.md:1220-1321\` |
| CH-04 | 中 | 迁移桩污染 tools/list：10 个旧工具桩 + search/inspect = 12 个工具，而自带 doctor 断言 \`tools.length===2\` → 自检必失败；README 仍称 "Two tools" | \`index.ts:75-88,656-664\`；\`README.md:94\` |
| CH-05 | 中 | MCP 契约漂移：\`detail_level\` 声明但被忽略；\`message_count\` 实为默认 10 的截断窗口，README 示例不可达 | \`index.ts:166-170,437-463\`；\`formatter.ts:935-959\`；\`universal-engine.ts:311,327\`；\`README.md:180\` |
| CH-06 | 低 | release.yml 手动触发 + \`if: push\` 永不执行（空转）；.dxt 实为 ZIP 而脚本产 tar.gz；icon.png 实为 JPEG | 见 \`research/claude-historian-full-audit.md\` F11-F14 |

`;
s = s.replace(anchor, sec + anchor);
const oldTail = 'resume 是否全程参数化也需自查。对应自查与整改并入 `implement.md` B 线。';
if (!s.includes(oldTail)) throw new Error('tail anchor missing');
s = s.replace(oldTail, 'resume 是否全程参数化也需自查。另加两条同族教训：fast-resume FR-01 与 claude-historian CH-01/CH-05 表明「错误被吞成空结果 / 契约字段哑火」是普遍病——ASG 的跨入口错误分类与预算/截断契约需要一次专项自查。对应自查与整改并入 `implement.md` B 线。');
fs.writeFileSync(p, s, 'utf8');
console.log('patched; length', s.length);
