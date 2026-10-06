import fs from 'node:fs';
const p = 'C:/AgentSessions/.trellis/tasks/10-05-competitive-source-audit-plan/review-report.md';
let s = fs.readFileSync(p, 'utf8');
const anchor = '**对 ASG 的直接含义**：SG-01（cap 先于过滤）';
if (!s.includes(anchor)) throw new Error('anchor missing');
const sec = `### cc-switch · 分片 B（proxy/transform；11,094/20,747 行=53.5%；receipt: \`research/coverage-cc-switch-proxy.json\`）

| ID | 级别 | 结论 | 关键锚点 |
|---|---|---|---|
| CSW-B1 | P2 | 主代理端点无入站鉴权：健康/状态端点裸奔，唯一 Bearer 校验仅 Claude Desktop 网关；/v1/messages、/v1/responses、/v1beta/* 未校验；监听地址可配 0.0.0.0（反证：默认回环，server.rs 仅 grep 级） | \`handlers.rs:57-71,129-149,261-286\`；\`server.rs:100-117\`；\`services/proxy.rs:1484-1490\` |
| CSW-B2 | P2 | Token 同步“写库失败仅 warn 继续”（注释要求 fail-fast，实际 DB 写失败仅 warn 且外层仍 Ok）→ Live 已占位、DB 陈旧 token 双端失联风险（反证：Live 备份可恢复） | \`services/proxy.rs:620-627,1068-1078,1129-1137,1181-1191,1245-1268\` |
| CSW-B3 | 正面 | failover-safe 流式提交协议：非流式全量缓冲、流式首包预热、Responses 语义起始校验、2xx 错误包络参与 failover；提交后中途失败有意不换家 | \`forwarder.rs:2285-2312,2341-2373,2433-2513,3090-3093\` |
| CSW-B4 | 正面+边界 | 日志脱敏与占位符不变量成体系（请求体只记 bytes+hash、URL 双模脱敏、错误摘要最小化、PROXY_MANAGED 拒绝出站）；边界：DB/Live 明文存 token，加密未验证 | \`forwarder.rs:2195-2199,2177-2186,3520-3521\`；\`services/proxy.rs:1068-1071,1392-1397\` |
| CSW-B5 | P3 | 静默降级+超时零值语义：models 解析失败无日志空表；Copilot debug-only；non_streaming_timeout=0 无显式超时且不缓冲 | \`handlers.rs:95-101,517-524\`；\`forwarder.rs:2600-2605,2237-2239,2354-2356\` |
| 关系 | — | 代理只会话身份与归因（session_id 提取、用量入库、工具缓存），不索引正文、无检索/恢复面；真实压力=流量咽喉的用量/成本视图+账号池+日常入口；无压力=检索质量/召回/语义/CLI-MCP 深度 | \`forwarder.rs:1255-1301,1328-1339\`；\`handlers.rs:2575-2660\` |

### agent-sessions · 分片 B（实读 8,598/17,675 行；receipt: \`research/coverage-agent-sessions-resume.json\`）

| ID | 级别 | 结论 | 关键锚点 |
|---|---|---|---|
| RS-01 | P2 | 恢复执行零反馈：七处 \`_ = await …resumeInTerminal\` 丢弃结果；canResume 预检+焦点 alert 存在，但运行时失败无出口 | \`UnifiedSessionsView.swift:3152-3154,3171,3189,3207,3225,3244,3262,3280,1162-1167,3040-3058\` |
| RS-02 | P2 | 剪贴板恢复命令是 shell 字符串（\`cd <q wd> && <q binary> --resume <q sid>\`），转义全押在各自 shellQuoteIfNeeded（本体未读）→ 条件性结论 | \`UnifiedSessionsView.swift:1527-1633\` |
| RS-03 | P2 | /status 探测 override 绕过敏 opt-in 与可见性（双窗口缺失只保留 30 分钟下限）；文件头 10min 冷却与代码 4h 不一致 | \`CodexStatusService.swift:2560-2577,30,1923\` |
| RS-04 | P2 | live 状态启发式且“探不到=空闲”（词法 marker+mtime 2.5/15/30s+尾探失败→openIdle）；presences 无 freshness 字段 | \`CodexActiveSessionsModel.swift:1874-2008,2158-2170,2341-2352,2200-2203,1548-1559\` |
| RS-05 | P3 | 两处无超时 waitForExit（osascript 主线程手势路径；登录 shell PATH 解析） | \`CodexActiveSessionsModel.swift:1809-1823\`；\`CodexStatusService.swift:3835-3868\` |
| 正面+口径 | — | 恢复=外部终端 App（iTerm2/Warp/Terminal）非嵌入式 PTY；身份=String id+cachedRowByID O(1)；命中→详情三级解析、显式拒绝 cwd-only 猜测；settled 选择+锁步跳转；尾窗先画+稳定门控 | \`UnifiedSessionsView.swift:361-366,2235-2269,2348-2379,2742-2753\`；\`SessionTerminalView.swift:997-1090\` |

> 口径修正：SessionTerminalView.swift 实为转写渲染器、不是启动/恢复链路（分片 B 如实纠正）；真实 launcher 在 AgentSessions/Resume/*（未读，列为残余）。

`;
s = s.replace(anchor, sec + anchor);
fs.writeFileSync(p, s, 'utf8');
console.log('cc-switch-B + agent-sessions-B sections added; length', s.length);
