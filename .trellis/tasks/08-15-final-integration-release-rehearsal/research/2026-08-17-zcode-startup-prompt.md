# Zcode 启动 Prompt（直接粘贴）

你是本项目新接手的开发 agent。仓库根目录有 `AGENTS.md`，请先读它，再按下面顺序执行。全程不联网下载依赖（用 `cargo --offline`）；提交遵循 Conventional Commits；不要把当前向量器 `bigram-hash-v1` 宣传成"语义模型"。

## 第一步：读三份文件建立全局（5 分钟）

1. `AGENTS.md`（仓库根目录——官方入口，含任务读取顺序与信任边界）
2. `.trellis/tasks/08-15-final-integration-release-rehearsal/research/2026-08-17-ultimate-handoff.md`（终极交接文档：先读 §0 TL;DR，再读 §2 环境、§9 未完成清单、§11 第一步）
3. `docs/product/PROVIDER-MATURITY-MATRIX.md` + `crates/agent-session-grep-ports/src/capability.rs`（16 行 Provider 能力矩阵单源）

## 第二步：验证环境与全量质量门（10 分钟，全绿再动手）

```text
python ./.trellis/scripts/task.py --help
cargo fmt --all --check
cargo clippy --workspace --all-targets --offline -- -D warnings
cargo test --workspace --offline
python -m unittest discover -s scripts -p "test_*.py" -v
python -m unittest discover -s scripts/evidence -p "test_*.py" -v
python scripts/evidence/privacy_scan.py --repo .
```

## 第三步：第一个任务（按优先级选一个开工）

**优先做 #1（语义模型收尾，最大差异化缺口）**：
- 当前状态：optional `semantic-candle` feature 已落地（`crates/agent-session-grep-application/src/candle_embedding.rs`，本地 Candle + pinned multilingual-e5-small，`asg model import --dir <bundle>` 离线导入，`index embeddings` 与 `search --mode semantic` 已接线）。
- 你的第一个任务：**下载 multilingual-e5-small（revision `614241f6`，config.json / tokenizer.json / model.safetensors）→ 用 `asg model import` 导入 → 跑 `index embeddings` 与 `search --mode semantic` 端到端验证真实推理工作**；然后按 `docs/operations/SEMANTIC-MODEL-BUNDLE.md` 补一个 frozen recall benchmark（CJK/英文/代码）并记录。若模型下载受限，改做 #2。
- **#2**：`docs/product/PROVIDER-BETA-READINESS.md` 里列出每个 provider 的 Beta 本地缺口（context/handoff/incremental/tool_activity 覆盖），逐个补齐测试与实现。

## 硬规则

- 不提交 secret/真实路径/真实 transcript；provider 源只读。
- 不代 owner 签署 ADR/威胁模型；不伪造跨平台 CI 认证（当前 CI 被 GitHub billing 阻塞，属外部）。
- 每个任务在自己的 git worktree 里做，质量门全绿后 commit/push 到 main（用户已授权自主提交推送）。
