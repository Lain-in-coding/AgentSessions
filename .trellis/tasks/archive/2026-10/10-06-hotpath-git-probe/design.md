# 设计：hotpath-git-probe（最小改动）

## 数据流（目标）

CLI parse → 读取时间（独立函数）→ 仅当用例需要时解析一次 repo 上下文 → 构造一次 App/SearchContext → Application 调用。

## 关键决策

1. 读时钟不建 App：把“取当前时间”提取为不触碰文件系统的路径（保留 App::with_clock 测试语义）。
2. 按用例分级：Search/Handoff = 需要 repo；get/show/status/preview 纯读 = 不需要。
3. 每请求一轮解析：解析结果以值传递（或轻量上下文对象），不跨请求缓存；保留 ASG_CURRENT_REPO 注入语义。
4. 不改变默认排序：repo-aware 排序输入相同 → 输出相同；用固定输入做 before/after JSON diff。

## 兼容性

- 不新增/删除 CLI 参数；协议枚举/退出码不变；MCP/Web 走同一装配函数但调用次数减少。
- Windows/macOS/Linux：仅减少子进程调用，不新增平台分支。
