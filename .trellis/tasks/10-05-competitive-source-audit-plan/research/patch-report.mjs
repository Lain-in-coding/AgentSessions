import fs from 'node:fs';
const p = 'C:/AgentSessions/.trellis/tasks/10-05-competitive-source-audit-plan/review-report.md';
let s = fs.readFileSync(p, 'utf8').replace(/^\uFEFF/, '');
const must = (needle) => { if (!s.includes(needle)) throw new Error('anchor not found: ' + needle.slice(0, 60)); };

// 1) header caveat
const oldHeader = '> **范围不缩水，完成度不造假：15/15 项目已进入核心源码对照；整仓逐文件、逐行审读尚未完成。**全量清点是 9820 文件、约 231 万候选源码行（含测试与可能的生成代码），不等于这些内容已经全部人工读过。本报告不是全量安全审计证书，任务不标 completed。';
must(oldHeader);
const newHeader = '> **范围不缩水，完成度不造假：15/15 项目已进入源码级对照；覆盖分三层且逐文件记账。** T1=有精确行区间回执+SHA256 的逐段全文精读；T2=14 个探针对 9,421 个非二进制文本文件的逐文件机械扫描（`research/sweep-*.json`）；T3=显式排除（二进制/超大非源文件，记录原因与哈希）。全量清点 9,820 文件、约 231 万候选源码行（含测试与可能的生成代码），不等于全部人工读过。中央账本 `coverage-index.json` 记录每个文件的 state/区间/哈希复核；截至本次更新 agf 与 sessiongrep 已完成 T1 全文回执，其余 13 项在流水线上（回执逐步并入）。本报告不是全量安全审计证书，任务不标 completed。';
s = s.replace(oldHeader, newHeader);

// 2) method bullet
const oldBullet = '- 子代理两次尝试均受模型 token 限流影响，留下的清点不是结论；主线程已接管并补上各组实际源码证据。限流只改变顺序，不删除任何项目。';
must(oldBullet);
const newBullet = oldBullet + '\n- 覆盖模型（2026-10-06 起）：T1 全文精读要求精确行区间+SHA256 前后校验，回执在 `coverage-<project>*.json`；T2 机械扫描由 `research/sweep-probes.mjs` 对 `coverage-index.json` 全部非二进制文件执行 14 类探针（shell/SQL/路径/监视/吞错/截断/并发/索引/恢复/脱敏/测试/接口等），逐文件命中计数存于 `sweep-<project>.json`；T3 排除项记录原因与哈希。T1 与 T2 均是可复核的静态覆盖，不等于运行时验证。';
s = s.replace(oldBullet, newBullet);

// 3) new section before 四
const anchor = '## 四、必须正视的发现';
must(anchor);
const sec = `## 三·B 竞品侧源码级新证据（T1 回执支撑，2026-10-06）

> 口径：以下是**竞品自身的缺陷/教训**，用途只有三个：修正竞争事实表、找出 ASG 可能共病的坑、校准“对手也不是成品”的预期。**不得**用它宣布 ASG 优势——ASG 是否犯同类错误需 ASG 侧独立验证（本轮未做）。

### agf（33/34 文件 T1 全文回执 + TUI 2686/2686 行；receipt: \`research/agf-file-coverage.json\`、\`research/coverage-agf-tui.json\`）

| ID | 级别 | 结论 | 关键锚点 |
|---|---|---|---|
| AGF-01 | 高 | 普通扫描（list/stats/resume/默认 TUI 刷新）会以“孤儿清理”名义**删除 Codex 源库 rows**；逐文件读失败被跳过、遍历有错但非空即返回部分集合、缺目录视为可信空集，均可把存在的 row 误判为孤儿删除；删除跨多库无 rollback/无确认 | \`src/main.rs:111,170,183\`；\`src/scanner/codex.rs:25-38,75-95,100-127,156-195,228-234\`；\`src/cache.rs:299-327\` |
| AGF-02 | 高（有前提） | provider 会话 ID 原样插入单引号 shell 模板、无转义；项目路径有转义函数但不覆盖 ID → 命令解析错误/注入风险（需特殊字符 ID 落到缓存或导入） | \`src/scanner/pi.rs:104-128\`；\`src/model.rs:76-87\`；\`src/shell.rs:64-94\` |
| AGF-03 | 中高 | “新鲜”只看监听源 mtime（深度 4 最大秒级），不覆盖实际扫描输入（projects JSONL/.git HEAD/DB WAL）；刷新后写缓存才重采 mtime；\`max_sessions\` 裁剪发生在写缓存前 → 截断视图被持久化为“新鲜全量” | \`src/cache.rs:119-140,201-208,258-269,319-326\`；\`src/main.rs:223-227,261-265\`；\`src/plugin.rs:102-123\` |
| AGF-04 | 高（竞态） | 异步刷新后，批量删除/勾选仍按旧数组下标解析目标；刷新排序变化后可指向另一场会话；选中身份在刷新之后才捕获 | \`src/tui/mod.rs:194-197,471-475,488-511,1681-1684,1759-1769\` |
| AGF-05~07 | 中 | provider 删除非原子；Hermes 无 wrapper 的恢复缺口；新鲜度/错误/统计口径需向用户说清 | 见 \`research/agf-full-audit.md\` |
| 反证 | — | watch 有 AtomicBool 防重叠扫描、刷新后游标夹取；SQL 参数化删除、每库事务 | \`src/watch.rs:35-58\`；\`src/scanner/codex.rs:100-127\` |

### sessiongrep（25/26 文件 T1 全文回执 + 1 显式排除；receipt: \`research/coverage-sessiongrep.json\`）

| ID | 级别 | 结论 | 关键锚点 |
|---|---|---|---|
| SG-01 | P1 | FTS 先全库取 limit×5 候选，再做 provider/path/since 过滤 → 全局截断可把域内合法匹配挤没，过滤后甚至返回 0 条 | \`src/db.rs:287-295,395-423,427-477\` |
| SG-02 | P2 | 文字分为 0 的候选仍可因近因分/当前 repo 分被接纳为正分“相关”结果（零证据升格） | \`src/db.rs:318-381\` |
| SG-03 | P2 | 不完整解析/读取失败会被固化为 current；失败身份可能与正常身份分叉或争用 | 见 \`research/sessiongrep-full-audit.md\` |
| SG-04 | P2 | 单会话写事务可靠，但 full 重建“先 clear 再逐条提交”无原子切换；mtime/size-only 增量、content_hash 恒 null、无源快照比较 | \`src/db.rs:144-242\`；\`src/indexer.rs:28-30\` |
| SG-05~09 | P2 | LIKE 字面量语义不一致；Cursor 恢复穷举启发式；清理标记/扁平正文丢消息身份；MCP freshness/预算边界；TUI 状态/分隔符问题 | 见回执报告 |
| SG-10 | P3 | 首用直观，但配置/安装/发布声明缺闭环证据 | \`README.md\` + CI 配置 |
| 反证 | — | UTF-8 截断修复字符边界、repo 检测覆盖 worktree/submodule、FTS 错误以 Result 返回（非全盘吞错） | \`src/util.rs:26-79,225-282,464-497\` |

**对 ASG 的直接含义**：SG-01（cap 先于过滤）、SG-02（零证据升格）、SG-03（失败固化为 current）、AGF-03（裁剪视图进缓存）、AGF-02（shell 参数拼接）这五类是 ASG 必须拿自己的过滤/缓存/水位/resume 契约逐条自查的坑；ASG 侧已有 filter-before-topk 与 snapshot/generation 机制（见 §五），但本轮未按这些场景做专项验证，resume 是否全程参数化也需自查。对应自查与整改并入 \`implement.md\` B 线。

`;
s = s.replace(anchor, sec + anchor);
fs.writeFileSync(p, s, 'utf8');
console.log('patched, new length', s.length);
