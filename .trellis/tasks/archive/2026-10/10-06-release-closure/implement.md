# 实施计划：release-closure

## Steps

1. 盘点现状：release.yml 触发条件/矩阵/产物；scripts/release 既有资产；INSTALL-AND-UPGRADE 现状。
2. workflow 修订（三 OS matrix + SHA256 + artifact 上传），本地做 YAML/动作名/权限自检。
3. Windows smoke：构建 → 安装到临时前缀 → 首跑 → 覆盖升级 → 卸载；日志与退出码归档。
4. 文档更新（安装/升级/卸载/限制/owner 清单）。
5. 汇总证据表：哪些真跑过、哪些只有 CI 定义、哪些属 owner 未决。

## Validation commands

    cargo build --release --locked --target-dir C:/AgentSessions/.trellis/.runtime/target-b6
    cargo test --workspace --locked --target-dir C:/AgentSessions/.trellis/.runtime/target-b6
    node -e "JSON.parse(require("fs").readFileSync("package.json","utf8"))"  # 无 npm 依赖时的最小心跳；YAML 用现有工具/自写解析校验
