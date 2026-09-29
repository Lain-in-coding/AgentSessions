# 设计：Application 层命中窗口摘要

## 边界

- 只改 `crates/agent-session-grep-application`：新增 `snippet` 模块 + `assemble_search_hit` 改用窗口构建器；端口类型与 wire 字段不变。
- 输入：`full_text`（payload 的规范 `text` 字段）、`query_terms`（调用方已用 `guidance::literal_terms(&query)` 派生，与 `why_matched` 同一实例）、`max_snippet_chars`。
- 输出：`Option<String>`——无文本 → None；有词元证据 → 窗口；无证据 → `chars().take(max_snippet_chars)` 前缀。

## 算法

1. 对每个词元在原文做大小写不敏感查找：逐字符 lower-case 展开建立 `原字符下标 → 展开串区间` 映射，匹配后回映到原字符边界，禁止用展开串偏移直接切原文。
2. 命中锚点 = 最早起始位置，平局按词元顺序；无命中 → 前缀回退。
3. 以锚点区间为中心扩展：交替优先向右 2 个字符、向左 1 个字符，直到达到 `max_snippet_chars` 或原文边界；锚点本身超过上限时取锚点起始的连续 `max_snippet_chars`。
4. 输出严格为原文连续切片；不添加标记。

## 集成点与预算

- `assemble_search_hit`（`crates/agent-session-grep-application/src/lib.rs`）替换现有 `chars().take(...)`；`why_matched` 继续使用完整 `full_text`，不受窗口影响。
- `search_hit_charge` 已按 `hit.text` 估计序列化字节；确认其口径覆盖窗口文本（含 JSON 转义），否则在预算枚举中同步。窗口不改变排序与 cursor digest。

## 测试

- 单元：锚点选择、左/右扩展、锚点超限、emoji/组合字符、`İ` 展开回映、控制字符转义、空/无匹配/语义-only、极小 `max_snippet_chars`。
- 回归：同一查询的命中顺序、`why_matched`、建议命令、cursor 续页与改动前一致（用既有 Application 测试夹具扩展）。
- 字节：`max_response_bytes` 收紧时窗口被正确计费并触发既有 truncation 语义。
