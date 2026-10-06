# 发布闭环：三 OS 构建/安装/升级/卸载证据（B6）

> 父任务：.trellis/tasks/10-05-competitive-source-audit-plan。用户 2026-10-06 批准（D2；发布/签名/账号动作仍属 owner 边界）。
> 依赖：B0/B1/B3 已落地；上游：review-report P0/P1-08、父 design.md 第 8 节。现存资产：.github/workflows/release.yml、docs/operations/INSTALL-AND-UPGRADE.md、docs/release/OWNER-RELEASE-CHECKLIST.md。

## Goal

让“新用户拿到具体产物可独立完成核心闭环”成为可验证事实：当前 commit 绑定三 OS 构建/测试/产物哈希；本机 Windows 完成 install/upgrade/uninstall smoke；文档列真实限制；不擅自发布/签名。

## Requirements

1. CI：修订 .github/workflows/release.yml（或新增验证 workflow）——三 OS build+test、产物 + SHA256、失败即红；workflow 语法本地可校验。
2. 本机（Windows）：release build + 干净安装 + 首次运行（version/config paths）+ 升级（覆盖安装）+ 卸载 smoke，全部留日志。
3. 文档：docs/operations/INSTALL-AND-UPGRADE.md 与 docs/release/OWNER-RELEASE-CHECKLIST.md 更新为当前真实流程（含双命令名、PATH、限制、owner 待办清单）。
4. 指标口径：preview/native resume/pack validity/任务完成率不得混用（与 B0 一致）。

## Non-goals

- 不 publish、不签名、不动账号/仓库权限、不清理历史。CI 定义≠执行通过：没有实际运行结果的 OS 必须标注未本地执行。

## Acceptance Criteria

- [ ] release workflow 改动经本地语法校验（yaml 解析或等价），三 OS matrix 完整、绑定 lockfile/feature/target。
- [ ] Windows smoke 脚本真实执行：安装/首跑/升级/卸载日志入 research/；失败项如实报告。
- [ ] 安装文档按实测更新（命令、路径、卸载、限制）。
- [ ] 不出现“已发布/已签名/三 OS 全绿”等未兑现表述。
