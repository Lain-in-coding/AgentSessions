# 维护性映射：最大文件、边界与同步点（B7 / PRD 要求 3）

方法：`Get-ChildItem crates -Recurse -Filter *.rs`（仅 `*/src/*`），按总行数排序取前 5；
“生产/测试”以表内标注的测试模块 `#[cfg(test)]` 行切分（adapters-sqlite 在 4,794 行另有
一个 `#[cfg(test)]` 门控的辅助方法，按本口径计入生产段）。行数快照 = HEAD `4bea67f`
（与 B7 测量同一工作树；crates 下无未提交改动）。

## 1. 规模与边界表

| 文件 | 行数（生产+测试） | 职责 | 可提取边界 | 风险 | 收益 | 建议 |
|---|---|---|---|---|---|---|
| `crates/agent-session-grep-adapters-sqlite/src/lib.rs` | 20,504（9,502 + 11,002；`#[cfg(test)]`@9503） | schema 定义与迁移链（`SCHEMA_VERSION=19`@8151、`RELATION_SCHEMA_VERSION=7`@8032）；catalog 读写（`impl CatalogStore`@8153）；context graph（@8318）；search index（@8922）；semantic index（@9110）；resume claims（@9320）；journal compaction（类型@1152-1277、存储@7526）；relation manifests（@522-978）；source staging（@1550） | journal compaction、semantic index、search index、schema/migration 四个子域都可独立成模块 | 这些 `impl SqliteStore` 共享私有连接/事务助手与迁移不变量；schema 版本有测试 pin（@10679-10680 断言 ==19）；并行任务在同一 crate 增删 `invariant_*.rs`，冲突面大 | 无运行时收益，仅评审/可读性 | **推迟**。没有可量化收益，属 PRD non-goal 的“看起来整洁”拆分；若做，独立任务按 semantic index → journal compaction → schema/migration 顺序，先补边界契约测试 |
| `crates/agent-session-grep-application/src/lib.rs` | 7,644（2,800 + 4,844；`#[cfg(test)]`@2801） | App 门面与请求路由（`App`@1470）；search 装配与 cursor digest（@906-940）；上下文窗口/预算投影（@1104-1470）；staging sink（@474-844） | search pipeline（digest/ranking 调用/投影）与 context 装配可独立模块化 | App 是 CLI/MCP/TUI 共享内核；4,844 行测试与私有辅助耦合，移动是纯结构大 diff | 无运行时收益 | **推迟**（同上） |
| `crates/agent-session-grep-cli/src/lib.rs` | 7,437（4,019 + 3,418；`#[cfg(test)]`@4020） | 手写参数解析（`command_name`@295、request-id@251、value-flag 前缀扫描@1641-1837）；`dispatch` 全命令分发（@1976-2683）；组合根与输出真值表（`emit_result`@706）；help 文本@1158-1524；doctor@1525 | help/subcommand-help 静态表、按命令分组的 dispatch 分支、参数解析器 | **已知同步点**：新增 flag 必须在 `command_name`/`extract_request_id`/`extract_offline_flag`/前缀扫描器多处登记；拆散扫描器会放大漏登记风险；spec 明确禁止引入 clap（需先改契约） | 无运行时收益 | **推迟**；优先做父任务 P2-07 的“flag 元数据单源 + 跨入口契约校验”，而不是物理拆文件 |
| `crates/agent-session-grep-cli/src/mcp.rs` | 3,502（1,471 + 2,031；`#[cfg(test)]`@1472） | stdio JSON-RPC 2.0 服务（`McpServer`@116-822）；tool catalog@833-1165；参数校验@1266-1460 | 传输/协议循环 vs 工具校验层（边界已相对清晰） | 协议 conformance 与静态 tool catalog 耦合，拆动易漂移 | 低 | **推迟** |
| `crates/agent-session-grep-provider-claude/src/lib.rs` / `...-provider-codex/src/lib.rs` | 2,765（1,110+1,655）/ 2,580（1,112+1,468） | provider 解析 + golden/契约测试 | 无需提取（单一职责，测试占一半以上） | — | 低 | **不做** |

附注：PRD 提及的 `application/ranking`（`ranking.rs`）仅 460 行且已独立成模块，
不构成维护性问题；本轮未发现“可量化收益”的最小提取，故**不实施任何拆分**，
仅给出上述边界与推迟理由。

## 2. 重复同步点清单（改动时需要多处同步的真实位置）

| 同步点 | 位置 | 现状 |
|---|---|---|
| CLI flag 注册 | `cli/src/lib.rs` `extract_offline_flag`@219、`extract_request_id`@251、`command_name`@295、前缀扫描@1641-1837 | spec 已列为同步风险；本轮未新增 flag |
| catalog schema 版本 | `adapters-sqlite/src/lib.rs` `SCHEMA_VERSION`@8151 + 迁移链 + 测试 pin@10679-10680；`cli/src/lib.rs:1600` doctor 读取 | v19 稳定；本轮未改 schema |
| 关系投影版本 | `adapters-sqlite/src/lib.rs` `RELATION_SCHEMA_VERSION`@8032 | 本轮未改 |
| 协议 schema 版本 | `cli/src/protocol.rs:21`（"1.1"）+ frozen JSON schema 测试@1021-1138 | 本轮未改协议 |

## 3. 结论

- 维护性映射完成；对前 5 大文件均给出边界与风险，**没有做任何物理拆分**——按 PRD
  “只在有可量化收益时做最小提取”的原则，本轮的收益评估全部为“无可量化收益”，
  真实收益方向（如 flag 元数据单源）属于父任务 P2-07 的独立范围。
- 最值得优先处理的不是“拆文件”，而是上表 4 个同步点：它们才是“每改一处要记得改
  多处”的真实来源。