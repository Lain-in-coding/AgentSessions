# CI 第二轮结果与第三轮修复（2026-10-07）

head db41494（含 F1/F2 修复）：

| workflow | 结果 |
|---|---|
| security-audit | success |
| ci | success（macOS / Windows / ubuntu 测试 + installer smoke + cargo-deny 全绿：F1/F2 确认修复） |
| core-beta-evidence | success（四个 runner/target 全绿） |
| release-verify | failure（三个 target 的 verify 作业都停在 Synthetic release smoke；required CI quality gates 全绿） |

## F3（阻塞）release-verify：verify-release.py 第 9 项期望过期

- 现象：三平台一致 9/10 checks passed，报 hook off by default: expected a JSON frame on stdout，退出码 1（本地 Windows 复现一致，与平台无关）。
- 根因：scripts/verify-release.py 的 verify_hook 期望 hook user-prompt-submit 在 stdout 输出 hookSpecificOutput JSON 帧；但 CLI 的既定契约（help 文本）是不加 --enable 时 stdout 一个字节都不写（不注入任何历史），运行事实走 stderr。脚本期望与产品契约不符，该检查在本轮之前就从未真正通过：10-05 审计日志 10-05-competitive-source-audit-plan/research/python-entrypoint-tests.log 已记录 hook off by default: enabled=False 失败。
- 说明：scripts/verify-release.py 与 origin/main 逐字节相同（非本分支引入）；B6 新增的 release-verify.yml 只是第一次在 CI 里真正运行它，把既有问题暴露出来。
- 修复：verify_hook 改为按契约断言——exit 0、stdout 长度 0、stderr 含 enabled=false，并把观测写进 step 详情。断言不是放宽：它校验 CLI 的文档化契约（默认静默 + 运行事实走 stderr），原先的 JSON 帧期望才是错的。
- 本地复验：python scripts/verify-release.py --asg <release 二进制> 得到 10/10；python -m unittest discover -s scripts/release 18 tests OK。

## F3 收尾：脚本自测同步（第三轮子修复）

- 改 verify_hook 后，scripts/test_verify_release.py 的旧测试失败：它 patch 的是 run_asg，而新实现直接调 subprocess.run，于是测试去 spawn 假二进制（CI 三平台同错，失败步骤 Release verifier and entry-point tests）。
- 处理：把原始调用抽成 run_asg_raw()（返回 CompletedProcess），run_asg() 在其上解析 JSON 帧；verify_hook 复用 run_asg_raw 以保持可注入。
- 测试改为两条（可证伪性不减）：test_hook_default_check_rejects_stdout_bytes（hook 若往 stdout 写字节则必须判失败）与 test_hook_default_check_accepts_silent_hook（exit 0 + 空 stdout + stderr enabled=false 则通过）。
- 本地复验：scripts 19 tests OK、scripts/release 18 OK、scripts/evidence 58 OK；verify-release.py 10/10。
