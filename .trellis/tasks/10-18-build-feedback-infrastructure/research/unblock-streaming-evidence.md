> Historical v1 research from 2026-10-19. The approved task PRD/design and versioned implementation supersede its admission/N statements. Read-only observations below are historical, not current admission or new performance evidence.

> Code line numbers and defect findings below describe the research-date snapshot. Current implementation, fix status and validation are recorded in implement.md and task.json; do not redispatch these historical findings without checking the current code.

# Research: 流式日志与证据校验的解阻点

- Query: 根据 unblock_streaming_evidence 派发名称，核对 build-feedback 工具的流式读取机会、现有证据边界与可复现的校验不一致。
- Scope: mixed（当前仓库实现 + Python 3.10 官方文档；未修改实现）。
- Date: 2026-10-19

## Findings

### 结论

当前采集端已直接把 stdout/stderr 写入文件；主要内存问题在完成后的读取、哈希复核及 Cargo JSON 重解析。可以用固定块哈希、重叠窗口搜索和统一的 Cargo 行解析减少额外内存，并修复一处有效日志被统计模块拒绝的问题。这些改进独立于磁盘准入；它们不能让尚未通过准入的真实 Pilot/Formal 获得运行资格。

本次研究没有写代码、运行 Cargo、清理缓存、执行 Git 操作或修改官方验证器。实际完成的验证是纯内存 SHA-256/跨块标记实验、当前 runner 与统计重解析表达式的对照复现、CPython 3.10.11 的 json.load 实现检查，以及 C 盘只读空间查询。磁盘证据另见 current-admission-evidence.md。

### Files found

| 文件 | 说明 |
| --- | --- |
| scripts/evidence/build_feedback_runner.py | Windows Job Object 采集、Cargo JSON/产物身份校验、并发阶段及准入。 |
| scripts/evidence/build_feedback_statistics.py | 原始证据与哈希验证、正式资格检查、固定 N 配对统计及官方 release 报告导入。 |
| scripts/evidence/build_feedback_benchmark.py | 标准库 CLI 协调与已有块读取 SHA-256 实现。 |
| scripts/evidence/build_feedback_inventory.py | NTFS 身份去重、实际分配量与固定 B 的预算判断；不是 stdout 流式问题的主要位置。 |
| scripts/evidence/test_build_feedback_runner.py | CargoArtifactTests 已声明并验证 build-finished 后允许测试输出；Windows 原生采集与磁盘快速拒绝测试。 |
| scripts/evidence/test_build_feedback_statistics.py | 证据身份、篡改、release 配对及回归测试；检索未找到测试输出尾部解析一致性的专项回归。 |
| scripts/evidence/core_beta_benchmark.py | 当前/冻结源码授权的官方 full 验证器，必须保持其验证标准及入口不变。 |
| docs/operations/BUILD-FEEDBACK.md | 实际运行契约、39 GiB 预测、80 GiB 最低空闲、0/5 与 0/100 状态。 |
| .trellis/tasks/10-18-build-feedback-infrastructure/prd.md | 用户授权与固定预算、源冻结、正式资格契约。 |
| .trellis/tasks/10-18-build-feedback-infrastructure/design.md | 模块职责、进程树、源/协议绑定及原始 release 证据要求。 |
| .trellis/tasks/10-18-build-feedback-infrastructure/implement.md | 已有 127 测试检查点、冻结验证器 API smoke 与未完成实测记录。 |

### Code patterns：已存在的流式能力

- runner.sha256_file 在 scripts/evidence/build_feedback_runner.py:69 以 1 MiB 块读取并增量更新 SHA-256；CLI 的 scripts/evidence/build_feedback_benchmark.py:59 使用同类实现。优先复用/统一这些模式，不另引依赖。
- WindowsJob.start 在 scripts/evidence/build_feedback_runner.py:655 为 stdout/stderr 以 xb 打开独立文件；:673 将其设为原生标准句柄；:678 创建暂停进程，:680 先关联 Job Object，:684 再恢复运行。采集端不经父进程内存缓存全部输出。改后处理不应改变进程树归属或计时边界。
- 产物必须由本次 Cargo 日志指名，并在指定 target 之内；scripts/evidence/build_feedback_runner.py:909 逐一绑定路径/身份/哈希。流式实现仍须保持这条来源链。

### Code patterns：应优先调整的整文件读取

| 位置 | 当前行为 | 实施建议与必须保留的契约 |
| --- | --- | --- |
| runner.py:1076、:1208、:1280 | 为非零退出的内存失败分类整份读取 stderr，再 lower() 复制。前两处也在成功路径读取。 | 固定块扫描三个 ASCII 标记，保留最大标记长度减一的尾部，跨块大小写不敏感匹配；仍保存原始完整文件和 SHA-256。Job Object 通知与 memory_limited 分类必须保留。 |
| runner.py:869、:878、:1310 | 先 read_bytes，再整体严格 UTF-8 decode/splitlines；积累 Cargo 对象。 | 将文件/二进制行迭代器作为解析输入，保留 bytes 入口兼容测试；统一严格解码、build-finished 状态机、诊断/产物规则。内存约为最大行 + 必须保留的元数据，不能宣称与行长/元数据数量无关的严格常量内存。 |
| statistics.py:242、:250 | _proof_file 一律读入全部字节后哈希，所有 caller 都付出整文件内存。 | 拆分仅校验路径/完整 SHA-256/字节数的接口与需要内容的 JSON/小脚本接口。对日志、产物、源文件使用仅校验接口；不能只改部分 caller 后宣称全面流式。 |
| statistics.py:294、:328、:347、:643 | 同一 stdout 在 capture 校验和 arm 重解析时再次整份读取；产物通常只需要身份/哈希。 | 使用块验证及行解析，保留完整文件哈希；即使很早找到 build-finished 或某个诊断，也必须消费/验证完整文件，不能漏掉尾部篡改。 |
| statistics.py:329 | 用列表推导无条件解析每个以 { 开头的行，并保留全部 messages。 | 使用与 runner 一致的 Cargo 状态机，只保留资格所需的 finished、产物路径及必要诊断。当前实现存在下述可复现差异。 |
| statistics.py:541、:542 | 为验证 release .exe 的哈希与长度整份读入 binary。 | 完整 SHA-256 + 实际计数字节即可；仍保留 arm target、suite copy、originating build 和 binary_size_bytes 对照。 |
| statistics.py:675、:676、:677、:679 | 诊断资格将 stdout+stderr 拼接、整体 replace 解码，并整读 source。 | 针对经验证的完整流检查精确 file:line；使用增量 UTF-8 解码及跨块尾部，保持既有 replace 错误处理和路径斜杠归一规则。不要凭一行摘要通过资格。 |
| runner.py:1509 | 工具版本采集后拼接 stdout/stderr。 | 通常较小，可置于次优先级；若设置长度上限，必须明确拒绝/保留完整原始输出，不能静默裁剪后做身份判断。 |

### 已复现的 Cargo 尾部解析不一致

runner 的 scripts/evidence/build_feedback_runner.py:870 注释明确允许 build-finished 后的测试文本；:882 捕获 JSONDecodeError 后仅在 finished 尚未出现时记错。对应现有测试 scripts/evidence/test_build_feedback_runner.py:444 使用如下尾部，确认 runner 接受：

```text
{"reason":"build-finished","success":true}
{test text after Cargo}
```

本次纯内存运行当前 runner.parse_cargo_json 得到 build_success=true、errors=[]。对同一字节串执行 statistics.py:329 的原始表达式，得到：

```text
JSONDecodeError: Expecting property name enclosed in double quotes: line 1 column 2 (char 1)
```

statistics.py:910 调用 _arm_proof_valid，:911 捕获 ValueError 等后将 arm 证明视为无效；因此这里造成有效 runner 证明被拒绝，不构成放宽验证的理由。更一般地，两处对 malformed-before-finished、重复 finished、artifact-after-finished 的处理也应统一。

复现仅验证真实 runner 和统计重解析表达式；没有执行完整正式 campaign fixture，也没有声称某次真实 Cargo 运行已产生该尾部。应由 implement agent 增加跨模块回归，在保留非法 Cargo 帧拒绝规则的前提下修复。

### 安全的实施边界与验证重点

1. 先拆分大文件仅哈希与需要解析的证据读取，更新 artifacts/binaries/logs 的全部 caller；官方 validator 仍调用授权的 validate_report(path, expected_profile="full")。
2. 保留路径、哈希、字节数、源清单与每次调用身份绑定，以及验证前后复核；不能通过缓存一次校验而忽略同一路径后续变化。至少对同一 handle 的读取前后 metadata/身份及完整内容哈希作一致性判断；检测变化则拒绝资格。
3. Cargo reader 应复用同一状态机、保留当前 bytes 测试入口，避免 runner/compare 再次形成不同语义。严格 UTF-8 输入不能改成 ignore/replace；日志中 Unicode 跨块/多种换行需要回归。明确兼容 decode().splitlines() 的现有边界，普通 file readline 的换行集合未必完全等价。
4. stderr 标记检查覆盖大小写、跨块、标记位于末尾、无标记及非零退出；不能读有限前缀后忽略末尾内存失败。
5. 证据回归覆盖大日志、末尾追加/替换、错误哈希/长度、空文件、非法 UTF-8、合法测试尾部、malformed-before-finished、重复 finished、artifact-after-finished，以及同名陈旧 exe。保留故障/污染/超时证据。
6. 以禁止 read_bytes/read(-1) 的文件替身或计量读取大小，证明仅校验接口与 Cargo 文件入口不整份读取；端到端内存实验只能在单独授权、预算满足的范围内进行。不要将小 fixture 的正常通过当成峰值内存收益测量。

本次纯内存块读取 smoke：1,048,601 字节数据，最大块 1,048,576 字节；两个块；增量 SHA-256 与一次性 SHA-256 一致、计数字节一致，且故意跨 1 MiB 边界的 `OuT Of MeMoRy` 被正确识别。该结果验证基础机制，不是工作负载内存 benchmark。

### External references（docs / versions）

- Python 3.10 官方 hashlib 文档：https://docs.python.org/3.10/library/hashlib.html#hashlib.hash.update 。Context7 先解析 Python，选 /websites/python_3_10，再查询；文档明确多次 update 等价于连接后一次 update。
- Python 3.10 iter(callable, sentinel)：https://docs.python.org/3.10/library/functions.html#iter 。Context7 返回固定块读取 binary file 的示例。
- Python 3.10 I/O：https://docs.python.org/3.10/library/io.html 。Context7 返回显式 UTF-8 编码与缓冲 I/O 信息。
- Python 3.10 JSON：https://docs.python.org/3.10/library/json.html#json.load 与 https://github.com/python/cpython/blob/3.10/Lib/json/__init__.py 。本机 Python 3.10.11 的 inspect.getsource(json.load) 进一步显示 load 调用 loads(fp.read(), ...)；单纯把 json.loads(path.read_text()) 改成 json.load(file) 仍整份读取 JSON。不能把这种替换称为流式 JSON。
- 上述均来自同一官方文档体系，未作为多个独立性能来源计算。没有外部性能数据或速度收益主张。
- 当前任务固定 Cargo/rustc 1.97.1、Python 3.10+，MBX 1.24.1、sccache 0.18.0；这些版本来自已批准任务记录，未在本研究重新运行候选工具。

### Related specs

- .trellis/workflow.md:9 要求持久化研究；研究角色上下文限制只写当前 task/research，未加载 implement.jsonl/check.jsonl。
- .trellis/spec/guides/code-reuse-thinking-guide.md:21 要求先找已有实现；本研究找到两处块哈希与一处 Cargo 状态机，可作为复用基础。
- .trellis/spec/guides/cross-layer-thinking-guide.md:24 要求梳理数据流/边界；本次确认 capture -> arm receipt -> statistics 的尾部解析语义分歧。
- .trellis/spec/agentsessions-cli/backend/index.md:80 起的 CLI/MCP stdout 边界属于产品契约；这次目标是 scripts/evidence 开发工具，不应由产品输出限制推断其资格行为。
- .trellis/spec/agentsessions-cli/backend/quality-guidelines.md 仍为占位内容，未找到 build-feedback 专属流式/内存规范；以当前任务 PRD/design、代码与已有测试为事实依据。
- .trellis/tasks/10-18-build-feedback-infrastructure/design.md:20 要求授权冻结 validator 与完整原始 release 证据；流式重构必须保持此契约。

## Caveats / Not Found

- 派发消息正文为空，明确可见的主题仅为 unblock_streaming_evidence；范围根据该名称与当前活动任务推断。如果父代理原意是另一项流式改动，需另补具体范围。
- 未修改任何实现、规格、任务元数据；修复必须由 implement agent 执行。未写脚本/临时 fixture，Python import 显式关闭 bytecode 写入。
- 未运行完整测试集；127 测试通过、冻结官方验证器 API smoke 是既有检查点，不能视为本次新验证，更不能说明新发现已经修复。
- CodeGraph 已先调用，但定位返回与请求无关的产品符号，并提示若干文件待同步；上述结论依赖直接读取的当前 scripts 源码与最小复现，不依赖缓存摘要。
- 普通逐行处理仍可能为单条极长行及保留的诊断/产物元数据分配大量内存；JSON 标准库/官方 full validator 也不是增量 JSON 解析器。未找到现有全流程严格内存上限协议，不应擅自新增截断规则。
- 本研究没有实测端到端内存、磁盘、速度收益，没有进行 Pilot/正式配对；流式读取不能替代固定磁盘预算与 24 GiB 进程树契约。
