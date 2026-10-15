# O7 未达标收尾独立复核

日期：2026-10-15。结论：**否决 O7、恢复精确 S2、保留未合并实验的处理可接受；
AC3/R4 仍未通过。** 本次只读核验既有证据，唯一写入为本文；未构建、运行测试、
重跑 benchmark 或修改 Git。检查顺序为上下文清单、PRD、design、implement。

## Findings (fixed)

- `implement.md` 原先“既有产品五个原子提交保留不改写”无法清楚表达公开身份
  修正。已向主会话指出，主会话改为原始被测提交保留于私有 ref，公开五个提交
  只替换邮箱并保持逐提交 tree、姓名、时间、消息。本次已复读修正文案。
- 未发现需修改产品源码的收尾问题。SQLite spec 已明确 O1/O2/O4/O6 属于
  **未合并 S2 实验**，shrink 验收失败，O3/O5/O7 均否决，未采用 schema 20；
  没有把该实验写成 main 的现有行为。

## 独立数值核验

直接以 Decimal 读取 `o7-prototype-01/ab-01/samples.jsonl`，未使用派生摘要
代替原始计时。核实 8 条未插桩 measured、8 条独立 trace、2 条 preflight；
18 个完成相位 exit 0、schema 19，二进制身份与清单一致，两个实存二进制的
SHA-256 重算吻合。协议清单哈希与首条声明吻合。

| 未插桩 wall time | S2 对照中位数/ms | O7 中位数/ms | 对照减 O7/ms |
| --- | ---: | ---: | ---: |
| initial | 8570.845200 | 9743.801750 | -1172.956550 |
| shrink | 8176.849750 | 8122.592700 | 54.257050 |

shrink 两对节省分别为 **-202.354500 / +310.868600 ms**；既不满足两对均为正，
也未满足中位节省严格大于 **266.255945 ms**。initial 中位回归 **13.685425%**，
通过其 15% 探索护栏。三项检查为 false/false/true，总探索门失败。
独立复算与 `result.json`、主会话 `independent-result-review.json` 一致。
原始样本 SHA-256：
`a52ddf1561832b3ab371cddfe4dbc25ae1c3e845f125cbc4ad08bf7553d69bd5`。
trace 未混入 wall 中位数，局部阶段下降不能改变该结论。

S2 正式失败沿用已完成的逐原始观测独立复核
`shrink-followup-after-g4.md`：initial 7819.6887 ms，通过；shrink 8717.9847 ms，
高于严格上界 8451.728755 ms，共 47/48 主系列通过、8/8 legacy guard 通过。
本次未重跑正式 suite，不将 O7 探索样本混入 S2 正式结果。

## 源码保全与公开身份

- 原始被测 S2：`e8e4975817f71647c158f6d802a7b98cee2f1b15`。
- 公开等价树 head：`c9af8bbc89e14e676471cd7be8eb55a4312273dd`。
- 两者 tree：`f1a99f12e093fd669958102a23e470c37a2ad784`。

直接读取 Git 对象，五对提交的 tree、完整消息、author/committer 姓名和时间
全部相等，父关系逐对映射；变化限于公开邮箱及其导致的新提交/祖先身份。
私有 `refs/heads/private/opt50k-measured-s2` 仍指向原始 S2。当前产品 worktree
无 diff，adapter 字节与 S2 blob 完全相等，SHA-256 为
`2ec45e89d3c10e58690d60a1816b62779bd4263624893b4388530fb39f9c97a1`。
原 harness 哈希也吻合。

回退目录中 O7 源码、完整 patch、失败结果及三个原 S2 质量/冻结收据的实存
文件哈希均与 `rollback-rejected-01/receipt.json` 匹配。没有删除失败原型来
掩盖结果。`public-identity-receipt.json` 的五对映射已独立验证；公开身份只是
相同源码树的发布载体，**没有对新 commit hash 进行另一次性能测量**。
拟定 PR 正文已区分原始 measured head 与 public head。

## Findings (not fixed)

- **性能验收失败是实验结果，不是已修复问题。** PRD 明确允许“如实报告 +
  保留现状”结束；应以 `performance_gate_failed` 归档，保持 AC3/R4 未满足，
  不合并实验、不更新成功性能 bundle、不关闭父发布门。
- 本复核未读取最终 hosted checks 或完成 PR 发布；其状态由主会话在实际
  public head 上记录。本文不声称 hosted 通过或任务的发布步骤已完成。

## Verification

- Lint：既有 S2 `fmt`/`clippy` 通过；恢复后的 fmt 收据 exit 0。
- TypeCheck：既有 S2 workspace clippy 包含类型检查并通过。
- Tests：既有 S2 workspace 日志独立汇总为 **2012 passed / 0 failed /
  23 existing ignored**；evidence 143、governance 93 通过。质量收据及日志
  与 `final-quality-review.md` 一致，精确源码/tree 核验通过后复用，未重复运行。
- 本次离线原始数据、二进制哈希、回退文件与 Git 对象断言全部通过；未将 S2
  收据用于认证已否决 O7 的源码。
