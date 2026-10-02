# Full-scope check: CI runtime compatibility

审查日期：2026-10-02。基线与审查时 HEAD 均为
`ffaf69b93d696d07708516a326436caae48326da`，分支为
`chore/ci-runtime-compatibility`。范围是该基线上的未提交工作树：三个工作流、
八份既有任务/研究文件及其引用规范；不是未来提交或远端 CI 的验收证明。

**结论：本地全范围审查通过，未发现需要修改工作流的缺陷；完整交付验收仍待远端证据。**

## Findings (fixed)

无。审查者未修改工作流。唯一新增文件为本报告，未改任务生命周期、spec、源码或脚本。

## Findings (not fixed)

无需要扩大批准范围才能修复的缺陷。下文列出的远端验收及已批准延期项仍未验证，不能视为已通过。

## Scope and compatibility evidence

| 文件 | 获批变更行 | Git 归一化后的工作树 blob |
|---|---|---|
| `.github/workflows/ci.yml` | 21, 28, 33, 93, 98, 103, 184（7 处） | `1bc481515e280540dcefcbb4add78b42bb773716` |
| `.github/workflows/core-beta-evidence.yml` | 69, 72, 238（3 处） | `e829df15366cbf7039d0eef688b2605b65dfcfb9` |
| `.github/workflows/security-audit.yml` | 21（1 处） | `77a2a34eb1aab3a486e2aefeb701d340814ccac3` |

- **精确边界：** checkout/setup-python/cache/upload-artifact 分别为 5/3/2/1 处，
  完整 SHA 与版本注释逐项匹配 `design.md` 的批准表。仅替换指定行的引用/注释后，
  三个文件的其余字节与基线一致，CRLF 保留。全部 1,250 个 tracked blob 经 Git
  过滤/归一化后与预期一致，其余 1,247 个文件及全部 index 条目/模式均未变化。
  因此没有源码、依赖/锁文件、schema、脚本、非目标 Action、runner、权限、输入、
  job、触发器、cache、artifact、凭据设置或策略的附带编辑。
- **Release：** `.github/workflows/release.yml` 的工作树字节与 Git 按既有 EOL
  规则检出的基线完全一致（12,752 bytes；SHA-256
  `da0a6a1477165fc5fea700c27f366702f2c0c17962e217965805b8ac65fd1b92`）。
  其 Git blob 也未变。`:24` 复用 CI，因此间接受新 pin 影响；这不是完整 release DAG 已运行。
- **Checkout：** `.github/workflows/ci.yml:3-7`、`core-beta-evidence.yml:3-25`、
  `security-audit.yml:3-7` 以及唯一已知调用者 `release.yml:3-12,24` 均没有
  `pull_request_target`/`workflow_run`。固定版本安全 helper 的 event guard 与此一致；
  不需要 unsafe opt-out，也不对未知外部 reusable-workflow 调用者作兼容保证。
  `security-audit.yml:23` 的凭据不持久化设置保持不变。
- **Python/cache：** `ci.yml:28-30,98-100` 仍明确使用 Python 3.11，
  `core-beta-evidence.yml:72-74` 仍为 3.10，三个使用点均未启用 setup-python
  的可选依赖缓存或新增 pip 输入。Cargo cache 的 key/path/restore-keys 不变；
  研究中记录的只读 token 保存警告不能冒充保存成功，也不能成为扩权理由。
- **Artifact：** `core-beta-evidence.yml:238-245` 保留名称、两个路径、缺文件报错、
  7 天保留及未启用隐藏文件的设置。固定 v6 uploader 的多路径 LCA 实现对应的预期
  ZIP 布局仍含 `evidence/ci/` 和 `target/<target>/release/<binary>`；v4.6.2
  已默认排除隐藏文件，此处没有新增排除或 direct-upload 格式。
  四个 matrix ID 唯一；release 的 upload/download 链不消费该 core-beta 制品。
  以上是静态兼容性依据，不能替代四份实际上传制品的下载检查。
- **任务一致性：** PRD/design/implement/task.json 的批准范围、基线、分支与
  `in_progress` 状态一致。两份研究的 planning/未实施叙述属于有日期的历史快照，
  不是当前生命周期；旧 main 成功记录也没有被当成本次 current-head 成功。
  八份既有任务文件均通过严格 UTF-8/no-BOM、JSON/JSONL、尾随空白及常见本机路径/
  凭据模式检查；本报告写入后同样复查。

## Verification

| 检查／实际命令 | 结果 |
|---|---|
| `python -B -m unittest discover -s scripts/release -p test_release_workflow.py` | PASS，3/3 |
| `python -B ./.trellis/scripts/task.py validate .trellis/tasks/10-02-ci-runtime-compatibility` | PASS，implement/check 各 6 项 |
| `git diff --check`；`git diff --check ffaf69b93d696d07708516a326436caae48326da` | PASS，无空白错误 |
| `git diff --cached --check`；`git diff --cached --quiet` | PASS，暂存区为空 |
| `python -B -`，执行下附只读边界证明 | PASS，11 处精确替换、1,250 个归一化 blob、release 原字节及 index/mode 检查 |
| `python -B -`，本地已有 PyYAML 6.0.3 的重复 key/结构断言 | PASS，4 份当前 + 4 份基线 YAML；只逆转获批 uses 后结构相等；30 个第三方引用均为完整 SHA |
| `python -B -`，任务文件 UTF-8/JSON/JSONL/空白/路径及凭据模式检查 | PASS，最终含本报告共 9 份；模式扫描不是穷尽性秘密审计 |
| Lint | PASS（上述 diff 空白、YAML/结构及工作流契约检查）；actionlint/yamllint 未安装，未安装或声称运行它们 |
| TypeCheck | 不适用：没有改变 typed source，未运行 Cargo 类型检查；YAML 结构检查不能冒称 Rust type-check |

上游依据：复用了 `research/action-runtime-upgrades.md` 的正式 tag-to-commit 账本，
并通过只读公开 `gh api --method GET` 重新读取四个批准 SHA 的
`repos/actions/<action>/contents/action.yml?ref=<sha>`，全部明确声明 node24。
另读了固定 checkout 的 `src/unsafe-pr-checkout-helper.ts`、固定 uploader 的
`src/shared/search.ts` 及基线 uploader 的 `action.yml`，核对上述实际适用路径。
最低 runner 2.327.1 由下列官方首个原生版本发布说明重新确认：

```text
gh api --method GET repos/actions/checkout/releases/tags/v5.0.0
gh api --method GET repos/actions/setup-python/releases/tags/v6.0.0
gh api --method GET repos/actions/cache/releases/tags/v5.0.0
gh api --method GET repos/actions/upload-artifact/releases/tags/v6.0.0
```

没有假定每个回补版本 README 都重复此下限：额外的 checkout README 关键字断言
首次未通过，改为核对研究指定的首个原生 release 后通过；这不是工作流测试失败。
网页工具的原始 URL 打开未返回正文，未作为证据；成功的 contents/release API
读取才用于确认。没有重跑广泛 latest-version 研究、安装依赖或运行 Cargo。

## Spec sync and tests

建议保留 `.trellis/spec/` 不变：此批次没有新产品接口、平台保证或发布门禁契约；
现有 CLI evidence-honesty 与外部就绪规范已覆盖必要边界。兼容性细节留在任务研究。
既有 workflow suite 加精确边界断言足以覆盖此次文本编辑，不新增 helper/依赖或改源码测试。
协调者可据报告更新检查进度，但不能据此勾选远端验收或完成任务。

## Still unverified / acceptance pending

- 当前 PR head 对应的 12 个 job（7 CI + 1 audit + 4 evidence），以及实际合并 SHA
  上的 main CI/evidence 11 个 job（7 + 4）；测试树与合并树一致性、正常 guarded merge、
  PRIVATE 状态回读及无 billing/visibility/gate 绕过，均待协调者执行和记录。
- 四份实际 core-beta artifacts 的名称、ZIP 成员布局、unsigned binary、对应运行的
  source/target 身份、hash、保留期，以及实际 hosted runner 版本满足 2.327.1，均待核验。
- release-only pins 和完整演练明确延期。研究记录的既有 tag 前提不足不能通过新建 tag、
  放宽输入/发布条件或一次普通 CI 成功消除；此次未重新查询私有 tag，也未 dispatch。
- 未来 Ubuntu 26.04 image 执行仍无证据；研究记录的 2026-10-19 至 2026-11-19
  迁移计划不是已实跑结果。保留 floating/fixed runner 选择，不据 hosted green 推导
  minimum-OS、glibc、clean-machine、签名/公证、SLO 或 provider tier 认证。

## Reproducible bounded proof

从仓库根目录将以下代码交给 `python -B -`；不写 Git 对象、不 stage、不改文件。
完整 SHA/版本对与精确行集合均固定，不从当前文件自推预期值。

```python
import hashlib, json, subprocess
from pathlib import Path
B = 'ffaf69b93d696d07708516a326436caae48326da'
P = {
 'checkout': ('11bd71901bbe5b1630ceea73d27597364c9af683 # v4.2.2', 'fbc6f3992d24b796d5a048ff273f7fcc4a7b6c09 # v5.1.0'),
 'setup-python': ('a26af69be951a213d495a4c3e4e4022e16d87065 # v5.6.0', 'ece7cb06caefa5fff74198d8649806c4678c61a1 # v6.3.0'),
 'cache': ('0057852bfaa89a56745cba8c7296529d2fc39830 # v4.3.0', 'caa296126883cff596d87d8935842f9db880ef25 # v5.1.0'),
 'upload-artifact': ('ea165f8d65b6e75b540449e92b4886f43607fa02 # v4.6.2', 'b7c566a772e6b6bfb58ed0dc250532a479d7789f # v6.0.0'),
}
S = {
 'ci.yml': {21:'checkout',28:'setup-python',33:'cache',93:'checkout',98:'setup-python',103:'cache',184:'checkout'},
 'core-beta-evidence.yml': {69:'checkout',72:'setup-python',238:'upload-artifact'},
 'security-audit.yml': {21:'checkout'},
}
def g(*args, data=None):
    return subprocess.run(['git', *args], input=data, stdout=subprocess.PIPE,
                          stderr=subprocess.PIPE, check=True).stdout
def substitute(data, sites):
    lines = data.splitlines(keepends=True)
    for n, action in sites.items():
        old, new = (f'actions/{action}@{v}'.encode() for v in P[action])
        assert lines[n-1].count(old) == 1
        lines[n-1] = lines[n-1].replace(old, new)
    return b''.join(lines)
assert g('rev-parse', 'HEAD').decode().strip() == B
tree = {}
for entry in g('ls-tree', '-r', '-z', B).split(b'\0'):
    if entry:
        meta, path = entry.split(b'\t', 1)
        mode, kind, oid = meta.decode().split()
        assert kind == 'blob' and mode in {'100644','100755'}
        tree[path.decode('utf-8')] = (mode, oid)
index = {}
for entry in g('ls-files', '--stage', '-z').split(b'\0'):
    if entry:
        meta, path = entry.split(b'\t', 1)
        mode, oid, stage = meta.decode().split()
        assert stage == '0'
        index[path.decode('utf-8')] = (mode, oid)
assert index == tree
expected = {p: oid for p, (_, oid) in tree.items()}
for name, sites in S.items():
    p = '.github/workflows/' + name
    raw = Path(p).read_bytes()
    assert raw == substitute(g('cat-file', '--filters', f'{B}:{p}'), sites)
    want = substitute(g('show', f'{B}:{p}'), sites)
    assert raw.replace(b'\r\n', b'\n') == want
    assert b'\n' not in raw.replace(b'\r\n', b'')
    expected[p] = g('hash-object', '--no-filters', '--stdin', data=want).decode().strip()
paths = sorted(tree)
listed = ''.join(json.dumps(p, ensure_ascii=False)+'\n' for p in paths).encode('utf-8')
actual = g('hash-object', '--stdin-paths', data=listed).decode().splitlines()
assert len(paths) == len(actual) == 1250
assert dict(zip(paths, actual)) == expected
assert not g('diff', '--summary', B)
r = '.github/workflows/release.yml'
assert Path(r).read_bytes() == g('cat-file', '--filters', f'{B}:{r}')
print('PASS: 11 bounded substitutions; 1250 normalized blobs; 1247 unchanged; index/modes unchanged')
print('release.yml raw SHA256:', hashlib.sha256(Path(r).read_bytes()).hexdigest())
```
