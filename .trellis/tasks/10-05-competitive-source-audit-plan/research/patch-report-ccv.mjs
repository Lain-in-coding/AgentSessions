import fs from 'node:fs';
const p = 'C:/AgentSessions/.trellis/tasks/10-05-competitive-source-audit-plan/review-report.md';
let s = fs.readFileSync(p, 'utf8');
const anchor = '**对 ASG 的直接含义**：SG-01（cap 先于过滤）';
if (!s.includes(anchor)) throw new Error('anchor missing');
const sec = `### cc-sessions-viewer · 分片 A（T1 9,252/9,812 行；receipt: \`research/coverage-cc-sessions-viewer-core.json\`）

| ID | 级别 | 结论 | 关键锚点 |
|---|---|---|---|
| CCV-01 | P2 安全 | 前端 \`extra_args\` 未转义直拼 shell（program/args 有引号、extra_args 原样 append）；PTY/外部终端 resume/new 透传；session id 有白名单而 extra_args 无校验。反证：值来自本地前端设置 launchArgs | \`agent_command.rs:52-54,73-75\`；\`lib.rs:845-885,1225-1265,859-865\`；\`App.vue:3277,3352\` |
| CCV-02 | P2 可用性 | \`match_snippet\` 用小写串算 char 下标却切原串 → İ 类膨胀字符可 panic；被 spawn_blocking 接住转 Err，整个搜索请求失败 | \`agents/mod.rs:1089-1097\`；\`lib.rs:687-688\` |
| CCV-03 | P2 状态 | \`send()\` 先记用户消息+置 turn started 再 spawn/写 stdin；早退不清 started → 幽灵消息+重连永显 running | \`agent_chat.rs:3256-3299,3446-3461,3471-3492,4058-4069\` |
| CCV-04 | P2 合规 | README 声明 MIT 并链接 LICENSE，但本快照无 LICENSE/COPYING/NOTICE；package.json/Cargo.toml 无 license 字段（只限快照断言） | \`README.md:10,186-188\` |
| CCV-05 | P3 资源 | USER_TEXT_CACHE/USAGE_CACHE 无界无淘汰、命中整段 clone；冷路径整文件读入 | \`agents/mod.rs:39-80,637-671,1056-1085\` |
| 检索语义 | 信息 | 真·全量扫描（\`list_sessions(0,usize::MAX)\`、rayon 4 线程）+ (path,mtime) 缓存；硬约束只匹配 \`role=="user"\` 的 text 块；代际取消+200 条上限；keyword scope 先匹配标题 | \`agents/mod.rs:988-1034\` |
| 正面 | — | 命中自带 msg_index/uuid；Pi 返回 pi_leaf_id 并按 terminal lineage 搜索；写租约防并发 resume；能力缺失 fail-closed（搜索/统计 fail-open 无 partial 标记） | \`agents/mod.rs:895-902,956-986\` |

> ASG 可学：leaf/placement 坐标随命中返回、双层检索+mtime 缓存、取消代际与上限、写租约。不该学：index-only 跳转、provider 特例分支、无界缓存、extra_args 裸拼、无 freshness 的静默 partial。
> 快照口径说明：本仓库无 Git 元数据，coverage-index 记录树哈希 \`0ee8993c…\`；其引用的 src/settings.ts、README.ja.md 在快照中缺失，“无 LICENSE”类结论仅限该快照。

`;
s = s.replace(anchor, sec + anchor);
fs.writeFileSync(p, s, 'utf8');
console.log('cc-sessions-viewer section added; length', s.length);
