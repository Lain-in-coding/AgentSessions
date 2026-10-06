# 实施计划：six-invariant-selfchecks

## Steps

1. 定位六项各自的 owner 模块（application search/ranking、adapters-sqlite sync/投影、cli resume/装配），记录当前实现事实。
2. 为每项写最小可执行证据：
   - 第 1、2 项：application 层测试（合成 catalog/索引；域名过滤+cap 场景、零证据场景）。
   - 第 3、4 项：adapters-sqlite 层测试（失败注入/水位断言；裁剪后重开不得变“新鲜全量”）。
   - 第 5 项：cli 层测试（resume 计划结构为 argv 数组、无 shell 字符串；检查现状是否已满足）。
   - 第 6 项：跨层断言（索引可搜正文 == 全文；导出与索引一致）。
3. 运行并记录原始输出；写 research/invariant-matrix.md。
4. 失败项不要顺手改：写清锚点/建议归属，留给主会话建后续任务。

## Validation commands

    cargo test --workspace --locked --target-dir .trellis/.runtime/target-invariants

## Review gate

- “通过”必须有测试断言支撑，不接受“看代码觉得没问题”。
- 新增测试不得依赖时间/环境/网络（时钟注入、合成数据）。
