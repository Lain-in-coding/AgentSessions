# cc-switch / cc-sessions-viewer：相邻产品而非可忽略的“非竞品”

全部代码只读。两个项目没有运行、安装、连接真实 agent 或执行任何删除/恢复操作。其余配置管理/PTY/富文本/系统集成模块的完整安全审计未完成。

## cc-switch — f748f3a，3.19.2，Tauri + React/TypeScript + Rust

### 源码事实

- `package.json`：Tauri 开发/打包、TypeScript/React 前端；核心产品不只是检索，配置/供应商/Profile 管理范围更广。
- `src-tauri/src/session_manager/mod.rs:7,58-93`：真实会话扫描装配 7 家，按活跃时间排序。
- `session_manager/mod.rs:96-114`：按 provider 读取消息，OpenCode/Hermes 有 SQLite 虚拟 source 处理。
- `src/hooks/useSessionSearch.ts:14-48`：**确实有搜索**，FlexSearch full tokenize 索引 sessionId/title/summary/projectDir/sourcePath；ASG 旧表的“无检索”不符合这份本地源码。
- `useSessionSearch.ts:50-69`：空查询按活动时间返回；关键词返回 FlexSearch 结果。索引的是 metadata，不是所有 transcript 正文；不能反过来吹成和 ASG 全文证据检索等价。
- `src-tauri/src/commands/session_manager.rs:5-24`：后台扫描/详情通过 spawn_blocking；`27-60` 明确把 renderer 视为可信边界，并记录任意 command 字符串的已接受风险及何时必须改成后端按 session 构造命令。
- `src/components/sessions/SessionManagerPage.tsx` 接入 useSessionSearch、分组、预览、恢复/删除，引用链和测试文件存在；本轮没有完成其所有交互测试。

### 对 ASG 的启示

这是“顺手就能找会话并继续”的相邻竞争者。不要以“它主要切配置”为由把它从用户体验比较里删掉。应借鉴 metadata / 全文检索边界解释、按项目与 provider 分组、可发现的下一步；不要把它的受信 renderer shell-bridge 模型直接搬到 ASG 的 MCP/Web/Robot 边界。

## cc-sessions-viewer — 无 `.git` 的本地快照，package 0.3.25

该目录实际为 93 文件，无法提供本地 commit。没有 `.codegraph`，通过 AST 大纲与直接源区间读取。根目录没有 LICENSE 文件；README badge 不能替代明确授权。本轮只做源码事实审阅，不建议直接复用代码，也不提供法律许可结论。

### 源码事实

- `package.json`：Vue/Vite + Tauri 2；本地不是一个 Go/Rust-only 命令工具。
- `src-tauri/src/agents/mod.rs:789-858`：跨项目收集 Session，通过 Rayon pool 并行检索，支持 cancellation，按 session.modified 排序并截断。没有将目录清点误记为持久 FTS。
- `agents/mod.rs:864-910`：搜索明确只匹配 ID/title/用户消息；assistant/thinking/tool call/result/project path 不参加正文匹配；热路径有 (path,mtime) 缓存。
- 同一段针对 Pi abandoned branch 单独遍历 terminal lineage，并回传 leaf/message index，用来正确跳转结果。
- `src-tauri/src/lib.rs:703-753`：有 rename、fork、编辑前截断 fork 的入口及 session-ID 字符约束。
- `lib.rs:756-768`：有 soft/hard delete；所以整个产品不能笼统描述成“原始会话文件永远只读”。**搜索读取是只读**和**应用提供用户触发的写/删除**是两回事。
- `lib.rs` 还登记 resume/new session/PTY/watch 等工作台行为；这些不是 ASG 当前只读证据基础设施的同义功能。

### 对 ASG 的启示

应学“点击搜索结果跳到正确消息/分支”的细节；不能学把未索引全文的扫描器当作大规模检索后台。将“只搜用户消息”做成明确范围选择，而不是把它当成全文系统的基准。首发不建议追齐它的编辑/PTY/桌宠等相邻功能。

## 校正比较方法

- cc-switch：配置与会话管理产品，具有 metadata 搜索。
- cc-sessions-viewer：会话工作台，具有用户消息扫描搜索与编辑/恢复等操作。
- 两者都应纳入任务体验比较，但不能与 ASG 按“全文索引吞吐/消息召回/证据契约”盲打同一张总分表。
- 竞争事实应记录当前快照和源码，不根据产品最初定位永久贴标签。

## 剩余边界

没有完整逐行阅读这两个项目的全部设置、第三方服务、PTY、系统进程/文件管理、发布和全部测试；不能给出整仓安全背书。无许可证快照的代码复用与发布验证应另设授权门，不能用 clean-room 一词自动消除许可问题。
