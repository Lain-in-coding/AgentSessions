import fs from 'node:fs';
const p = 'C:/AgentSessions/.trellis/tasks/10-05-competitive-source-audit-plan/review-report.md';
let s = fs.readFileSync(p, 'utf8');
const anchor = '**对 ASG 的直接含义**：SG-01（cap 先于过滤）';
if (!s.includes(anchor)) throw new Error('anchor missing');
const sec = `### ctx · 分片 B（CLI/MCP/daemon/install；14 文件 8,921 行；receipt: \`research/coverage-ctx-cli.json\`）

| ID | 级别 | 结论 | 关键锚点 |
|---|---|---|---|
| CTXCLI-01 | P2 | packet cursor 只写不读（CLI/MCP 侧确认）：search 输入无 cursor/offset（全文零命中）；但仍输出 per-result cursor 与 packet.pagination/truncation；show_session 截断无续取游标 | \`mcp.rs:717-735,612-628\`；\`search_render.rs:60,69-70\` |
| CTXCLI-02 | P2 | agent 配置写入非原子 + JSONC 丢注释 + 多目标半成功：fs::write 无 temp/rename/备份；jsonc 解析后按纯 JSON 重写；failed>0 不回滚 | \`integrations/mcp.rs:1050-1073,1136-1152,804-841\`；\`slash_commands.rs:684-707\` |
| CTXCLI-03 | P2 | Windows 升级无 journal 恢复（非 Unix 恢复恒 false；分离 PowerShell helper+ExecutionPolicy Bypass；版本探测用 contains）；反证：下载先验 SHA-256+元数据验签 | \`install.rs:638-658,1441-1451,1303-1311,519-528\`；\`upgrade/command.rs:526-541\`；\`metadata.rs:183-198\` |
| CTXCLI-04 | P2 | 模型“谁能下载”不齐 + 语义首用强依赖 daemon/网络（worker 无缓存拒绝下载；语义开启+daemon 关闭直接 bail）。反证：缓存后可完全离线 | \`embedding_backend.rs:4-11,228-231\`；\`daemon.rs:1944-1950,846-851\`；\`setup.rs:40-44\` |
| CTXCLI-05 | P3 | 错误契约碎片化：CLI anyhow→exit 1；doctor findings 非空仍 exit 0；MCP 未知参数键 -32602 vs 类型错误 isError | \`main.rs:663-757\`；\`doctor.rs:53-77\`；\`mcp.rs:890-909,370-373\` |
| 正面 | — | 逐目标集成状态机（15 MCP 目标五态+冲突默认拒绝+slash 哈希 Modified）；升级信任链（签名→SHA→暂存探测→journal 发布/回滚/重启恢复）；MCP 最小正确性面（initialize 前置、1MiB 行限、read-only open、截断元数据）；daemon 资源状态机（retryable defer、token+0600+2s 超时） | \`integrations/mcp.rs:271-301,719-738,967-980\`；\`metadata.rs:183-198\`；\`mcp.rs:220-227,40,370-373,656-666\`；\`daemon.rs:1754-1772,137-138,320-358\` |

`;
s = s.replace(anchor, sec + anchor);
fs.writeFileSync(p, s, 'utf8');
console.log('ctx-B section added; length', s.length);
