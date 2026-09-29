# 实施与验证：命中窗口摘要

## Ordered steps

1. 读取 application/ports spec 与实验 A 原型（归档路径），确认 `literal_terms` 与窗口算法边界。
2. 新增 `snippet` 模块与单元测试（锚点、扩展、Unicode 回映、回退语义）。
3. 接入 `assemble_search_hit`，更新 `SearchHit.text` 契约注释与 `docs/contracts/` 相关段落。
4. 扩展 Application 回归测试：排名/游标/`why_matched` 不变 + 字节预算。
5. 运行 fmt/clippy/test（application 与 workspace 受影响范围）。
6. 更新 `.trellis/spec/agentsessions-application/backend/index.md`（契约与测试要求）。
7. 提交（`feat(application): show match-centered search snippets`），合并 main 后跑跨入口一致性抽查（CLI/MCP/Robot 同源 text）。

## Validation commands

```text
cargo fmt --all --check
cargo clippy -p agent-session-grep-application --all-targets --offline -- -D warnings
cargo test -p agent-session-grep-application --offline
cargo test -p agent-session-grep-cli --offline
git diff --check
```

## Rollback

单一 Application 改动；如需回退直接 revert 该提交，不涉及 schema/迁移。
