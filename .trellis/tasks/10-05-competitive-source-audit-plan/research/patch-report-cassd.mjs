import fs from 'node:fs';
const p = 'C:/AgentSessions/.trellis/tasks/10-05-competitive-source-audit-plan/review-report.md';
let s = fs.readFileSync(p, 'utf8');
const anchor = '**对 ASG 的直接含义**：SG-01（cap 先于过滤）';
if (!s.includes(anchor)) throw new Error('anchor missing');
const sec = `### cass · 分片 D（lib.rs 脊柱 + sources；lib partial 2,699/100,119 + sync/config full；receipt: \`research/coverage-cass-lib.json\`）

| ID | 级别 | 结论 | 关键锚点 |
|---|---|---|---|
| D-01 | P2 | pack readiness 注入缩水：字面常量——lexical 恒 Ready、source_sync_gaps 恒空、recommended_action 恒 None → health 块在 CLI 路径失去信息量 | \`lib.rs:23848-23867\`；对照 \`pack_planner.rs:561-573\` |
| D-02 | P2 | cursor 无 generation 绑定/无过期（base64{offset,limit}；decode 静默合并；manifest 暴露 generation 但不校验）→ 对照 ASG 的 cursor_invalid/expired/generation_mismatch 缺一档 | \`lib.rs:25308-25318,22542-22570,25349-25361,25430-25441\` |
| D-03 | P2 | sync 软失败：全部路径失败仍 \`Ok(SyncReport)\`；additive-only 镜像不反映远端删除。反证：scheduler 转 Flapping/BackingOff，风险在调用方/退出码 | \`sync.rs:1019-1104,798-813,6-11,1185\`；\`lib.rs:2882-2894\` |
| D-04 | P2 | robot \`_meta\` 多分支平行手写（JSON/JSONL/compact/toon/sessions 各自拼装），schema 有但分支等价性未证 | \`lib.rs:26117-26149,26250,26413,26504,26547,26637,82009-82059\` |
| D-05 | P3 | 100K 行单文件脊柱（Cli 1,311 行；单函数 run_doctor_impl 2,834 行；json! 宏体 9,754 行）；19% 为内联测试（33 模块/471 个 #[test]） | \`lib.rs:226-1536,73428-76261,25553-26679\` |
| 遗留答复 | — | **pack CLI 确实注入 limits**（dispatch→PackPlannerLimits+validate→candidate_fetch_limit→PackPlanRequest）；默认 12000/8/24/3/1600 与 planner 契约一致；遗留只在 readiness（D-01） | \`lib.rs:6762-6803,23629-23636,23750-23754,23811-23819\` |
| 正面 | — | 同步“可解释账本”（SyncTransportDecision+failure reason+scheduler）；config 写前校验+round-trip+原子替换+备份；44 命令/65 robot 别名结构化面；cursor manifest 机器可读 continuation 理由 | \`sync.rs:522-659,686-718,2646-2812\`；\`config.rs:473-503,1158-1189\` |

> 过程更正：分片 D 回执中 config.rs 哈希有 63 位转录笔误，主会话已按磁盘实测 64 位哈希更正并在回执中留痕（content-unchanged 结论不变）。

`;
s = s.replace(anchor, sec + anchor);
fs.writeFileSync(p, s, 'utf8');
console.log('cass-D section added; length', s.length);
