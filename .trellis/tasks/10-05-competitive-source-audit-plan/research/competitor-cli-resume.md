# 轻量检索 / 恢复及 Wake 对照

## 范围与证据等级

本报告分析实际入口、检索/刷新实现、恢复边界和可借鉴机制，不把 README 或文件数当作能力证明。没有编译运行这些竞品，没有跨项目同机 benchmark；以下代码结论为静态确认。全部文件已纳入项目清点，本文明确列出的链路是深读范围，不等于每个文件逐行读完。工作树产品源码无已跟踪修改；`.codegraph` 属本地工具资产。

## agf — 44be9cf，0.12.0，Rust TUI/CLI

### 证据
- `Cargo.toml:1-18`：Rust edition 2024，superlighttui/nucleo/clap；ASG 旧表的 Go 分类错误。
- `src/main.rs:93-179`：resume/query/list 是直接用户入口，默认扫描即可使用，不要求用户先手工挑 DB 路径。
- `src/scanner/mod.rs:24-44,103-145`：首行/头尾有界读取，明确可能跳过中间内容；这是会话定位器的性能取舍，不是完整全文索引。
- `src/scanner/mod.rs:177-201`：8 个 scanner 并发，失败转空；panic 仅在 AGF_DEBUG 下打印。
- `src/model.rs:161-170,227-240`：索引/模糊匹配的用户视图是项目路径、标题摘要、分支，并非 ASG 的完整 Message/Placement 图。
- `src/scanner/mod.rs:216-231,274-285` 有 bounded read 边界测试；本轮只审读未运行。

### 判断
它在“找到那场会话然后继续干活”上定位清楚。ASG 应学缩短首用路径、会话卡片和按任务组织命令，而不是照抄只读头尾、隐藏扫描错误，更不能把它的轻量延迟当成全文证据系统的同口径性能。

## fast-resume — 66e42cf，2.5.0，Rust + Tantivy

### 证据
- `src/main.rs:25-74,76-130`：直接 query、默认 TUI、--list、--stats、重建、resume；--yolo 是显式选择，不是默认安全行为。
- `src/main.rs:113-130`：list 路径先 refresh 再查，TUI 退出携带 command/directory 后执行 resume。
- `src/index.rs:47-80`：有默认索引路径；schema 不匹配会删掉旧索引后重建。它把索引当可再生缓存，不等价 ASG 的权威 catalog 迁移策略。
- `src/index.rs:83-115`：Tantivy writer 事务重建、手动 reader reload、独立增量刷新模块。
- `src/index.rs:342-356,375-405,410-445`：provider+session 复合身份更新隔离、FTS filter、拼写错误检索测试。
- `src/search.rs:27-62`：多个便捷 API 将查询错误转为空列表/0；`search_result`（65-74）才保留 Result。ASG 不应为了“友好”抄这种故障与零命中混淆。

### 判断
对 ASG 的直接压力是默认路径、自动刷新、搜索—预览—恢复闭环和合理 typo 体验。没有证据证明它在本任务的中文、证据精度、崩溃恢复、跨格式一致性上全面胜出；不要比较不存在的实测数字。

## sessiongrep — c5c1874，Rust SQLite CLI/TUI/MCP

### 证据
- `src/indexer.rs:22-85`：明确装配 5 家 provider；mtime_ns + size 判 no-op。full 先 clear_all，再逐源写入，不能照抄到 ASG 的失败保留数据契约。
- `src/mcp.rs:19-31,57-90`：启动刷新，工具调用最多每 1.5s 刷新一次；失败只 stderr，旧索引继续服务。这是可用性/新鲜度取舍，不能让调用方误认为一定最新。
- `src/mcp.rs:115-213`：不是只有 search；提供 timeline_for_repo、list_sessions、get_session、get_resume_command，任务导向工具名值得借鉴。
- `src/db.rs:287-295`：先拿 limit*5 的全局 FTS candidate，再应用过滤；这有筛选后召回不足的静态风险，ASG 当前 prefilter SQL 不应倒退。
- `src/db.rs:318-375`：字段加权 fuzzy + recency + repo boost。候选无文字命中也可能因为近因/repo 分数为正进入结果；尚未动态复现，不能写成已运行缺陷。
- `src/mcp.rs:254-313`：结果直接给 title/provider/CWD/time/snippet，简单可读，但没有 ASG 等价的 generation-bound cursor / byte budget 契约证据。

### 判断
该学它围绕用户任务的工具接口，而不是学它给搜索失败和旧索引套上无声降级。功能少但直观不是原罪；ASG 的正确性机制有保留价值。

## Wake — 71aeca6，0.8.5，Rust + GPUI 原生桌面，附 CLI/MCP

Wake 不是轻量 CLI，不能错误归类。它同时是桌面与 agent 入口的重要对标。没有 `.codegraph`，本轮通过文件清点、AST 大纲和直接源区间审读。

### 证据
- `Cargo.toml:1-26`：wake-core/wake workspace、GPUI 产品形态，含 reqwest、watcher、SQLite；与 ASG 的“默认无 HTTP client”边界不同，不等于默认上传。
- `README.md:9-28` 的功能声明已沿核心链路核查；应用还支持用户选择的更新检查、远程 SSH 镜像等，不能把“local-first”曲解为完全没有任何联网功能。
- `crates/wake-core/src/adapters/mod.rs:37-95`：cheap enumeration、metadata 快路径、sidecar 独立刷新、按 host/agent/native-id 分域；统一详情与 FTS parser 以保证跳转 ordinal 一致。
- `crates/wake-core/src/db.rs:2542-2578,2618-2687`：FTS5 trigram，短于三码点的词走 LIKE，结果标记 degraded；有 recency boost、同一组 agent/project/time filters。
- 同一 schema 中 messages/memories/title FTS 的 trigram 定义位于 `db.rs:68,131,171`。这足以反驳“竞品基本不处理 CJK”的无边界结论，但不证明 Wake 的两字中文质量/延迟更优。
- `crates/wake-core/src/mcp/mod.rs` 提供 read-only index 打开，tools.rs:97-104 明确定义 search/sessions/session/projects/memories 五工具；CLI 和 MCP 共用 core 的方向与 ASG 多入口共享后端相容。

### 判断
应学：查询直接跳到命中消息、source/sidecar freshness、平台化的恢复入口、明确展示降级状态。不要照抄桌面/远程/删除/图片/统计的全部功能，ASG 首发用户不是缺另一个桌面工作台。

## 本组给 ASG 的可验收要求

1. 初次用户不需要先理解 catalog/generation/wire-id 才能找到会话；保留高级 --db 与机器契约。
2. 结果里可以识别会话、项目、来源、命中内容与下一步；搜索—上下文—恢复/交接一条链走通。
3. 任何自动刷新必须报告 freshness/失败，不用空结果隐藏故障。
4. 过滤在候选截断前生效；排序与输入坏数据测试不能为延迟指标让路。
5. 轻量路径和完整证据路径按工作量比较，不用不同级别任务的性能数字做宣传。

## 未完成边界

四仓库的完整逐文件阅读、安装实测、跨 OS 交互、全量恢复命令版本矩阵和独立性能对跑仍未完成。此报告不声称所有解析器、UI 与测试正文都已逐行审阅。
