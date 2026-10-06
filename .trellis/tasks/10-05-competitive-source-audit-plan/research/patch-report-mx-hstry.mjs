import fs from 'node:fs';
const p = 'C:/AgentSessions/.trellis/tasks/10-05-competitive-source-audit-plan/review-report.md';
let s = fs.readFileSync(p, 'utf8');
const anchor = '**对 ASG 的直接含义**：SG-01（cap 先于过滤）';
if (!s.includes(anchor)) throw new Error('anchor missing');
const sec = `### memex（9/9 T1 文件 full、7,133 行；receipt: \`research/coverage-memex.json\`）

| ID | 级别 | 结论 | 关键锚点 |
|---|---|---|---|
| MX-01 | P2 | 注入丢锚：只有 8 字符会话前缀+层标签，无 message id/时间戳/生成标注；L0 原文与 L1-L3 摘要同栏混排；消息静默截 500 字符。反证：底层有 \`at\`→\`around\` 回原文链路，缺的是接进注入格式 | \`inject/mod.rs:670-676,802-806,845-852\`；\`mcp/mod.rs:2050-2134\` |
| MX-02 | P2 | 时间排序静默换引擎：\`order_by≠Score\` 强制 FTS-only 丢弃向量结果，仅 info 日志，响应无 effective_mode | \`search/mod.rs:287-296,366-394\` |
| MX-03 | P2 | 检索失败与零命中不可区分：FTS/向量出错仅 warn 后继续，皆空即 \`Ok([])\` | \`search/mod.rs:335-364\`；\`vector/mod.rs:215-218\`；对照 \`mcp/mod.rs:801-813\` |
| MX-04 | P2 | 回退后 level 不更新：L3→L2→L0 逐级回退但响应 \`level\` 仍写请求层 | \`mcp/mod.rs:571-575,712-730,772-777\` |
| MX-05..11 | P3 | 预算硬 break+chars/4 估算；time_decay 无效/Dot 复用 cosine；注释工具数失真；Lite mtime 过滤；"只读"封装含写方法；DTO 硬编码 source=claude；\`source=local\` 本地不足时仍补查 sync server（隐私张力） | 见 \`research/audit-memex.md\` §4 |
| 能力分离 | — | Lite=无索引现场 regex grep 本地 CLI；完整服务=FTS+LanceDB+RRF+LLM compact+inject+HTTP/MCP+remote。"Rust CLI hybrid"单标签作废 | — |
| 正面 | — | 多层记忆落地；RRF 保留 sources/fts_rank/vector_distance 归因；搜索命中→原文 at 锚闭环（有测试） | \`mcp/mod.rs:2050-2134\` |

### hstry（8 必读文件 7 full + main.rs 86.8%；合计 15,316 行阅读；receipt: \`research/coverage-hstry.json\`）

| ID | 级别 | 结论 | 关键锚点 |
|---|---|---|---|
| H-01 | P1 | 长消息正文投影截断（每 part 500 字符/累计 4000/>2000 字触发）且 **FTS 索引的正是截断后 content**，show/导出同读；parts_json 保留全文 | \`crates/hstry-core/src/db.rs:3322-3354,1468-1470\`；\`migrations/001:98-104\`；\`main.rs:2589-2593,4410-4416\` |
| H-02 | P2 | \`resume --json\` 谎报 \`launched:true\`（json 分支直接返回，spawn 在其外） | \`main.rs:5409-5431,5095-5097,5203-5205\` |
| H-03 | P2 | adapter 抛错信息丢失（TS 写 stdout 后 exit(1)；Rust 只读 stderr） | \`adapters/types/index.ts:375-378\`；\`runner.rs:329-332\` |
| H-04 | P2 | \`external_id\` NULL 时每次同步重复建会话（UNIQUE 对 NULL 不去重） | \`migrations/001:27\`；\`db.rs:577-598\`；\`ingest.rs:63-93\` |
| H-05 | P2 | 后过滤在 limit×4 截断之后；但单角色/source/时间已在 SQL 内先于 LIMIT（优于 SG-01） | \`main.rs:2003,2064-2097,2139-2141\`；\`db.rs:2135-2167\` |
| H-06/07 | P2 | 磁盘 migrations 目录遮蔽内嵌集合；web sync 仅 ChatGPT 实现 | \`db.rs:128-150\`；\`web-runner.ts:73-91\` |
| 正面 | — | 16 adapters 含 ASG 没有的来源（ChatGPT/Claude.ai/Gemini 导出、Jan/LM Studio/Open WebUI/Goose）；SQL 先过滤再 cap；batch 单事务+单写者；UUID v5 幂等；verify/reseed；peek 预算 | 见回执 |

> 广度压力点：hstry 用“一协议 × 16 目录”换来源覆盖——ASG 的 14 家矩阵在“导入历史（ChatGPT/Claude.ai/Gemini 导出文件）”这类来源上确实缺席，这是真实竞争压力，但不应用追 adapter 数量的方式回应。

`;
s = s.replace(anchor, sec + anchor);
const tail = 'ASG 的跨入口错误分类与预算/截断契约需要一次专项自查。';
if (!s.includes(tail)) throw new Error('tail missing');
s = s.replace(tail, 'ASG 的跨入口错误分类与预算/截断契约需要一次专项自查。hstry H-01 再补一刀：索引与展示共用截断投影会让「搜不到」与「看不到」同时发生——ASG 的投影/索引/导出是否共用同一截断版本需要自查。');
fs.writeFileSync(p, s, 'utf8');
console.log('memex+hstry sections added; length', s.length);
