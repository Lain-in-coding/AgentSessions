# 合并后热路径复测与"是否回退"判定（2026-10-07）

合并提交 `161b83e`（origin/main `2edf2dc` 合入 fix/session-relocation-identity）。

## 1. 合并后 20 次中位（同口径 check3-measure.ps1）

| 变体/命令 | 中位 ms | 探测 |
|---|---:|---:|
| git_detect/get | 12.02 | 0 |
| git_detect/show | 12.47 | 0 |
| git_detect/search | 48.45 | 2,2,2 |
| seam/get | 11.86 | 0 |
| seam/show | 13.22 | 0 |
| seam/search | 11.66 | 0 |

单轮解析成本 = 48.45 - 11.66 = **36.79 ms**。

## 2. 同会话单进程成本（判定是否丢重叠的关键）

同一时刻实测 `git -C C:\AgentSessions rev-parse --show-toplevel` x20：
min 27.79 / **median 29.31** / max 36.17 ms。

- 若两个探测**串行**，单轮成本应约为 2 x 29.31 = **58.6 ms**；
- 实测 36.79 ms 约为 **1.26 x 单进程**（与 B8 收口时的 19.5 -> 23.23，比值 1.19 同形）。

结论：**重叠派发在合并结果上仍然生效**；36.79 ms 是"一次进程启动 + 轻微争用"，不是两次串行。
B8 收口时：单进程 19.5 ms / 单轮 23.23 ms；本次机器整体约慢 1.5 倍（get 8.19->12.02、show 8.08->12.47 同比例），
故 search 绝对值 31.97->48.45 属机器差异，非代码回退。

## 3. 语义与门禁（同一合并提交）

- 42 上下文语义矩阵：**ALL CONTEXTS EQUIVALENT**（脚本仅在写已归档路径时报错，结论行正常输出）。
- hotpath_repo_probe（workspace 测试内）：get/show=0 探测、search=2 探测，断言未放宽。
- cargo test --workspace --locked：**1912 passed / 0 failed / 22 ignored**。
- cargo fmt --all --check / cargo clippy --workspace --all-targets --locked -D warnings：exit 0。
- Python 三套（scripts / scripts/release / scripts/evidence）：exit 0；web_ui：11 pass / 0 fail。
- 冲突标记 0 命中；SCHEMA_VERSION=19、RELATION_SCHEMA_VERSION=7 保持不变；208 个"仅 main 改过"的文件无一出现在 origin/main..HEAD diff 中。
