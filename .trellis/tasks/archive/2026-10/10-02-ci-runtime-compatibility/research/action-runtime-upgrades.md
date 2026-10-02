# Research: 官方 Actions 的 Node24 原生版本与最小升级边界

- Query: 截至 2026-10-02，核验五个直接使用的官方 Action 的首个 Node24 原生 major、实际 latest stable、完整 commit SHA、指定 SHA 的 `action.yml`、最低 runner 和兼容性；提出最小升级或延期建议。
- Scope: mixed；外部官方源码/发布信息为主，仅加载规划目标和适用的本仓库规范，不重复协调者的私有 workflow 使用点或 runner-label 审计。
- Date: 2026-10-02
- Access date: 2026-10-02；下列发布时间为 GitHub API 返回的 UTC 时间。
- Status: **研究与待审提案，不是 owner 实施批准。**

## Findings

### 1. 规划边界与结论

依据协调者在本次研究期间提供的范围收窄，第一批只能提议修改 `.github/workflows/ci.yml`、`.github/workflows/core-beta-evidence.yml`、`.github/workflows/security-audit.yml` 中的 **checkout / setup-python / cache / upload-artifact 四种 Action**。`release.yml` 必须保持逐字节不变；download-artifact 及全部 release-only 使用点延期。

协调者报告：当前没有满足 release 手动演练前提的现有 `v*` tag，而推送此类 tag 会触发实际发布。本研究没有重新查询私有仓库，也没有创建 tag、启动/切换任务、dispatch、改 workflow 或运行演练。解除这一验证阻塞需要另行授权安全方案，不能用制造 tag 代替批准。`ci.yml` 被 release quality 复用，因此三个 workflow 的将来成功运行也不等于完整 release 链路已经重新验证。

**首个原生 major 是 checkout v5、setup-python v6、cache v5、upload-artifact v6、download-artifact v7。** 特别注意：upload-artifact **v5.0.0** 和 download-artifact **v6.0.0** 虽在 release 中写了支持 Node24，但其实际 `action.yml` 仍为 `node20`；不可把“可在 Node24 下运行”当成“原生声明 node24”。见下方固定 SHA 和官方 [upload v6 发布说明](https://github.com/actions/upload-artifact/releases/tag/v6.0.0)、[download v7 发布说明](https://github.com/actions/download-artifact/releases/tag/v7.0.0)。

### 2. 发布与 SHA 核验账本

判定规则：使用公开官方仓库的 `releases/latest`，并用分页完整 release 列表核对最高稳定 SemVer；排除 draft、prerelease 和非 `vX.Y.Z` 名称。latest 是 GitHub 指定的当前稳定版本，不是“最后发布时间最大的旧 major 回补”。本次 checkout v5.1.0/v6.1.0/v4.4.0、cache v5.1.0 均有晚于各自 latest major 的回补发布时间。

#### 2.1 给定基线：五个 tag/SHA 全部匹配，声明均为 node20

| Action | 当前版本 | 官方 tag 解析得到的完整 commit SHA | 固定源码中的 runtime |
|---|---|---|---|
| checkout | v4.2.2 | `11bd71901bbe5b1630ceea73d27597364c9af683` | [action.yml:107](https://github.com/actions/checkout/blob/11bd71901bbe5b1630ceea73d27597364c9af683/action.yml#L107) |
| setup-python | v5.6.0 | `a26af69be951a213d495a4c3e4e4022e16d87065` | [action.yml:40](https://github.com/actions/setup-python/blob/a26af69be951a213d495a4c3e4e4022e16d87065/action.yml#L40) |
| cache | v4.3.0 | `0057852bfaa89a56745cba8c7296529d2fc39830` | [action.yml:41](https://github.com/actions/cache/blob/0057852bfaa89a56745cba8c7296529d2fc39830/action.yml#L41) |
| upload-artifact | v4.6.2 | `ea165f8d65b6e75b540449e92b4886f43607fa02` | [action.yml:68](https://github.com/actions/upload-artifact/blob/ea165f8d65b6e75b540449e92b4886f43607fa02/action.yml#L68) |
| download-artifact | v4.3.0 | `d3f86a106a0bac45b974a628896c90dbdf5c8093` | [action.yml:42](https://github.com/actions/download-artifact/blob/d3f86a106a0bac45b974a628896c90dbdf5c8093/action.yml#L42) |

#### 2.2 首个稳定 Node24 原生 major

| Action | 首个版本 / UTC 发布日期 | 完整 commit SHA | 固定源码 / 官方最低 runner |
|---|---|---|---|
| checkout | [v5.0.0](https://github.com/actions/checkout/releases/tag/v5.0.0) / 2025-08-11 | `08c6903cd8c0fde910a37f88322edcfb5dd907a8` | [action.yml:107 = node24](https://github.com/actions/checkout/blob/08c6903cd8c0fde910a37f88322edcfb5dd907a8/action.yml#L107) / **2.327.1** |
| setup-python | [v6.0.0](https://github.com/actions/setup-python/releases/tag/v6.0.0) / 2025-09-04 | `e797f83bcb11b83ae66e0230d6156d7c80228e7c` | [action.yml:42 = node24](https://github.com/actions/setup-python/blob/e797f83bcb11b83ae66e0230d6156d7c80228e7c/action.yml#L42) / **2.327.1** |
| cache | [v5.0.0](https://github.com/actions/cache/releases/tag/v5.0.0) / 2025-12-11 | `a7833574556fa59680c1b7cb190c1735db73ebf0` | [action.yml:41 = node24](https://github.com/actions/cache/blob/a7833574556fa59680c1b7cb190c1735db73ebf0/action.yml#L41) / **2.327.1** |
| upload-artifact | [v6.0.0](https://github.com/actions/upload-artifact/releases/tag/v6.0.0) / 2025-12-12 | `b7c566a772e6b6bfb58ed0dc250532a479d7789f` | [action.yml:68 = node24](https://github.com/actions/upload-artifact/blob/b7c566a772e6b6bfb58ed0dc250532a479d7789f/action.yml#L68) / **2.327.1** |
| download-artifact | [v7.0.0](https://github.com/actions/download-artifact/releases/tag/v7.0.0) / 2025-12-12 | `37930b1c2abaa49bbe596cd826c3c89aef350131` | [action.yml:42 = node24](https://github.com/actions/download-artifact/blob/37930b1c2abaa49bbe596cd826c3c89aef350131/action.yml#L42) / **2.327.1** |

反例也已实际读取：upload v5.0.0 → `330a01c490aca151604b8cf639adc76d48f6c5d4`，[action.yml:68](https://github.com/actions/upload-artifact/blob/330a01c490aca151604b8cf639adc76d48f6c5d4/action.yml#L68) 为 node20；download v6.0.0 → `018cc2cf5baa6db3ef3c5f8a56943fffe632ef53`，[action.yml:42](https://github.com/actions/download-artifact/blob/018cc2cf5baa6db3ef3c5f8a56943fffe632ef53/action.yml#L42) 仍为 node20。download v5.0.0 → `634f93cb2916e3fdff6788551b99b062d0335ce0` 的 [action.yml:42](https://github.com/actions/download-artifact/blob/634f93cb2916e3fdff6788551b99b062d0335ce0/action.yml#L42) 也为 node20。

#### 2.3 本次真正抓到的 latest stable（不是全部升级目标）

| Action | `releases/latest` 返回版本 | `published_at`（UTC） | 完整 commit SHA / runtime |
|---|---|---|---|
| checkout | [v7.0.1](https://github.com/actions/checkout/releases/tag/v7.0.1) | `2026-07-20T15:10:05Z` | `3d3c42e5aac5ba805825da76410c181273ba90b1` / [node24, action.yml:116](https://github.com/actions/checkout/blob/3d3c42e5aac5ba805825da76410c181273ba90b1/action.yml#L116) |
| setup-python | [v7.0.0](https://github.com/actions/setup-python/releases/tag/v7.0.0) | `2026-07-20T03:15:01Z` | `5fda3b95a4ea91299a34e894583c3862153e4b97` / [node24, action.yml:42](https://github.com/actions/setup-python/blob/5fda3b95a4ea91299a34e894583c3862153e4b97/action.yml#L42) |
| cache | [v6.1.0](https://github.com/actions/cache/releases/tag/v6.1.0) | `2026-06-26T19:17:06Z` | `55cc8345863c7cc4c66a329aec7e433d2d1c52a9` / [node24, action.yml:41](https://github.com/actions/cache/blob/55cc8345863c7cc4c66a329aec7e433d2d1c52a9/action.yml#L41) |
| upload-artifact | [v7.0.1](https://github.com/actions/upload-artifact/releases/tag/v7.0.1) | `2026-04-10T17:31:14Z` | `043fb46d1a93c77aae656e7c1c64a875d1fc6a0a` / [node24, action.yml:74](https://github.com/actions/upload-artifact/blob/043fb46d1a93c77aae656e7c1c64a875d1fc6a0a/action.yml#L74) |
| download-artifact | [v8.0.1](https://github.com/actions/download-artifact/releases/tag/v8.0.1) | `2026-03-11T15:44:25Z` | `3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c` / [node24, action.yml:52](https://github.com/actions/download-artifact/blob/3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c/action.yml#L52) |

五个 latest release 的 `draft=false`、`prerelease=false` 均直接核对。完整列表返回的 release 数 / 稳定 SemVer 数分别为 checkout **58/56**、setup-python **54/54**、cache **74/67**、upload-artifact **47/43**、download-artifact **37/36**；全部分页已读取，不把默认首页当成全量。

#### 2.4 最小候选 SHA（四项）与 release 延期项

| 状态 | Action / 候选版本 | 可用于 owner 审查的完整 SHA | `runs.using` / runner 下限 | 选择理由（待审） |
|---|---|---|---|---|
| 第一批候选 | checkout [v5.1.0](https://github.com/actions/checkout/releases/tag/v5.1.0) | `fbc6f3992d24b796d5a048ff273f7fcc4a7b6c09` | [node24:116](https://github.com/actions/checkout/blob/fbc6f3992d24b796d5a048ff273f7fcc4a7b6c09/action.yml#L116) / 2.327.1 | 首个原生 major 的最新稳定回补；包含 2026-07 fork-PR 安全变更，保留 v5 凭据存储方式。 |
| 第一批候选 | setup-python [v6.3.0](https://github.com/actions/setup-python/releases/tag/v6.3.0) | `ece7cb06caefa5fff74198d8649806c4678c61a1` | [node24:44](https://github.com/actions/setup-python/blob/ece7cb06caefa5fff74198d8649806c4678c61a1/action.yml#L44) / 2.327.1 | 留在首个原生 major，纳入其已有修复；不为 runtime 目标额外迁移 ESM major。 |
| 第一批候选 | cache [v5.1.0](https://github.com/actions/cache/releases/tag/v5.1.0) | `caa296126883cff596d87d8935842f9db880ef25` | [node24:41](https://github.com/actions/cache/blob/caa296126883cff596d87d8935842f9db880ef25/action.yml#L41) / 2.327.1 | 含只读 cache 权限处理回补，避免附带 v6 的 ESM 迁移。 |
| 第一批候选 | upload-artifact [v6.0.0](https://github.com/actions/upload-artifact/releases/tag/v6.0.0) | `b7c566a772e6b6bfb58ed0dc250532a479d7789f` | [node24:68](https://github.com/actions/upload-artifact/blob/b7c566a772e6b6bfb58ed0dc250532a479d7789f/action.yml#L68) / 2.327.1 | 第一个实际声明 node24 的 major；保持原有 ZIP、name、path、隐藏文件契约，不引入 v7 direct-upload 输入。 |
| **不在第一批；仅后续候选** | download-artifact [v7.0.0](https://github.com/actions/download-artifact/releases/tag/v7.0.0) | `37930b1c2abaa49bbe596cd826c3c89aef350131` | [node24:42](https://github.com/actions/download-artifact/blob/37930b1c2abaa49bbe596cd826c3c89aef350131/action.yml#L42) / 2.327.1 | 现阶段不修改；待安全 release 演练获批后再决定 v7 还是 v8 的额外行为变化。 |

这里的“最小”指避免无必要的新 major 行为，并非锁定最早的 x.0.0 或拒绝已有修复。不是对旧 major 永久支持或安全完备性的承诺。候选中的最低 runner 来自上述首个原生版本发布说明以及固定 README；checkout v6/v7 在 Docker container action 内执行认证 Git 命令另需 **2.329.0**，不是所有 checkout 用法一律要求该版本（[latest README:16–25](https://github.com/actions/checkout/blob/3d3c42e5aac5ba805825da76410c181273ba90b1/README.md#L16-L25)）。

本次对基线、首个原生版本、候选、latest 及易混淆 artifact 版本共 **21 个互异 SemVer refs** 做了 ref-object 核验：均直接返回 `object.type=commit`，所以没有需要剥离的 annotated tag object。核验流程仍包含 `type=tag` 时递归读取 `git/tags/<object-sha>` 直至 commit 的分支；**不能把 annotated tag object SHA、release 的 target_commitish 或页面短 SHA 当成 workflow pin。**

### 3. 逐项兼容性与保留契约

#### 3.1 checkout：v5.1.0 是有安全回补的最小 major，不是零行为变更

- 保留 `repository`、`ref`、`path`、`fetch-depth`（默认 1）、`fetch-tags`、submodules、token 等现有输入；不借 runtime 维护更改权限、触发器或凭据开关。`persist-credentials` 默认仍为 true，v5 仍把 token 配入本地 Git config 并在 post-job 清理；[action.yml:52–79](https://github.com/actions/checkout/blob/fbc6f3992d24b796d5a048ff273f7fcc4a7b6c09/action.yml#L52-L79)、[README:9–15](https://github.com/actions/checkout/blob/fbc6f3992d24b796d5a048ff273f7fcc4a7b6c09/README.md#L9-L15)。实际 token 路径由 [src/git-auth-helper.ts:275–318](https://github.com/actions/checkout/blob/fbc6f3992d24b796d5a048ff273f7fcc4a7b6c09/src/git-auth-helper.ts#L275-L318) 计算为 `.git/config`。
- **v5.1.0 回补了 breaking security change**：`allow-unsafe-pr-checkout` 默认 false，拒绝特定 `pull_request_target` / PR 来源 `workflow_run` 中的 fork PR head/merge checkout。同仓 PR 和普通 `pull_request` 不在此阻断范围。见 [v5.1.0 release](https://github.com/actions/checkout/releases/tag/v5.1.0)、[action.yml:101–109](https://github.com/actions/checkout/blob/fbc6f3992d24b796d5a048ff273f7fcc4a7b6c09/action.yml#L101-L109)、[src/unsafe-pr-checkout-helper.ts:13–80](https://github.com/actions/checkout/blob/fbc6f3992d24b796d5a048ff273f7fcc4a7b6c09/src/unsafe-pr-checkout-helper.ts#L13-L80)。**适用与否由协调者用实际触发器/ref 核对；本研究不宣称所有使用点天然无影响，也不建议添加 unsafe opt-out。**
- 升到 latest v7.0.1 会包含 v6 的凭据文件迁移（改为 `$RUNNER_TEMP` 下独立文件）及 v7 的 ESM 迁移；认证 Git 的 Docker container action 还有 runner 2.329.0 条件。正常 Git 调用官方说明可继续工作，但直接解析 `.git/config`、跨容器读取凭据等不能假定等价。[latest README:3–25](https://github.com/actions/checkout/blob/3d3c42e5aac5ba805825da76410c181273ba90b1/README.md#L3-L25)
- **提案**：在三个获准进入规划的 workflow 内优先评审 v5.1.0；不要为“最小”退回缺少该回补的 v5.0.0/v5.0.1。latest 可作为后续独立凭据/ESM 审查选项，而非本次默认授权。

#### 3.2 setup-python：Action 的 Node 与被安装的 Python 是两件事

- 当前 v5.6.0 的输入均仍存在于 v6.3.0，`python-version`、`architecture`、`check-latest=false`、`update-environment=true`、可选 cache、输出 `python-version/cache-hit/python-path` 未被移除。v6.3.0 额外暴露 `pip-version` 和 `pip-install`，**本提案不启用它们、不升级既有 Python 版本要求**。[基线 action.yml:5–43](https://github.com/actions/setup-python/blob/a26af69be951a213d495a4c3e4e4022e16d87065/action.yml#L5-L43)、[候选 action.yml:5–47](https://github.com/actions/setup-python/blob/ece7cb06caefa5fff74198d8649806c4678c61a1/action.yml#L5-L47)
- v6.0.0 同时改变了 `.python-version`/Pipfile 解析并修复 Windows/PyPy 相关行为；不能把所有内部变化缩写成只换 Node。v6.3.0 又将 Linux distro 纳入自动生成的 Python 依赖缓存 key；使用内建 cache 时可能产生一次冷缓存，不能承诺旧 key 必命中。[v6.0.0 release](https://github.com/actions/setup-python/releases/tag/v6.0.0)、[v6.3.0 release](https://github.com/actions/setup-python/releases/tag/v6.3.0)、[src/cache-distributions/pip-cache.ts:65–75](https://github.com/actions/setup-python/blob/ece7cb06caefa5fff74198d8649806c4678c61a1/src/cache-distributions/pip-cache.ts#L65-L75)。缓存默认关闭见 [README:79–83](https://github.com/actions/setup-python/blob/ece7cb06caefa5fff74198d8649806c4678c61a1/README.md#L79-L83)。
- latest v7.0.0 迁移 ESM，并**移除 v6 的 `pip-install` 输入**。尽管 [v7 README:14–16](https://github.com/actions/setup-python/blob/5fda3b95a4ea91299a34e894583c3862153e4b97/README.md#L14-L16) 泛称无输入变化，[release](https://github.com/actions/setup-python/releases/tag/v7.0.0) 与固定 [action.yml:29–34](https://github.com/actions/setup-python/blob/5fda3b95a4ea91299a34e894583c3862153e4b97/action.yml#L29-L34) 证明存在这一例外；以具体发布说明和源码为准。给定 v5.6.0 原本没有该输入，因此这不是“v5 无法升 v7”的证据。
- **提案**：v6.3.0 是边界较窄的 Node24 原生选项；是否改选 v7.0.0 要由 owner 明确接受额外 major 范围，不能从 README 的概括句推导已经批准。

#### 3.3 cache：保持 key/path 与命中契约，不为 runtime 清缓存

- 对给定 v4.3.0 与候选 v5.1.0 的 `action.yml` 做了归一化换行后的逐字比较：**除 node20 → node24 外完全相同**。`path/key/restore-keys`、`enableCrossOsArchive=false`、`lookup-only=false`、`fail-on-cache-miss=false`、success-only post 行为都保留；`save-always` 的既有弃用警告也仍在。[基线 action.yml](https://github.com/actions/cache/blob/0057852bfaa89a56745cba8c7296529d2fc39830/action.yml)、[候选 action.yml:4–44](https://github.com/actions/cache/blob/caa296126883cff596d87d8935842f9db880ef25/action.yml#L4-L44)
- 这里并非首次迁移 cache backend v2：基线 v4 已使用该服务。没有仅因 Node major 改变而必须改 cache key、path、compression 或清理现有条目的官方要求。实际命中还受 key、cache version、branch 等条件影响；`cache-hit` 是字符串，exact hit 为 `true`，非 exact 命中为 `false`，miss 为空。[README:41–49](https://github.com/actions/cache/blob/caa296126883cff596d87d8935842f9db880ef25/README.md#L41-L49)、[README:98–108](https://github.com/actions/cache/blob/caa296126883cff596d87d8935842f9db880ef25/README.md#L98-L108)
- v5.1.0 回补只读 cache token 处理：restore 仍可用，写入被拒时发出一次 warning、跳过 save，不把“未保存”写成“缓存保存成功”。这不是放宽测试门禁或赋予写权限。[release](https://github.com/actions/cache/releases/tag/v5.1.0)、[README:112–118](https://github.com/actions/cache/blob/caa296126883cff596d87d8935842f9db880ef25/README.md#L112-L118)、[src/saveImpl.ts:65–91](https://github.com/actions/cache/blob/caa296126883cff596d87d8935842f9db880ef25/src/saveImpl.ts#L65-L91)
- latest v6.1.0 的 major 变化是 ESM/toolkit 升级，亦有同类只读 cache 修复；元数据的现有输入并未删除。不声称 v6 必然不兼容，但 runtime-only 目标不要求它。[v6.0.0 release](https://github.com/actions/cache/releases/tag/v6.0.0)、[v6.1.0 release](https://github.com/actions/cache/releases/tag/v6.1.0)
- **提案**：v5.1.0；现有 key/path、命中判断和权限不动，验证时如实区分冷缓存、只读 save warning 和真正失败。

#### 3.4 upload-artifact：v6 原生 Node24；隐藏文件与 ZIP 不是新默认

- v4.6.2 → v6.0.0 的 `action.yml` 逐字比较同样**只有 runs.using 改变**。保留 `name`（默认 artifact）、必填 `path`、`if-no-files-found`、retention、compression、overwrite、隐藏文件输入和三项 artifact 输出。[候选 action.yml:4–69](https://github.com/actions/upload-artifact/blob/b7c566a772e6b6bfb58ed0dc250532a479d7789f/action.yml#L4-L69)
- **隐藏文件默认排除早已存在于给定 v4.6.2**，v6 未新改默认。`include-hidden-files=false`；点文件/点目录受此规则控制，Windows hidden attribute 本身不等价于点前缀。不要为了“兼容升级”自动打开它，尤其不能上传凭据文件。[基线 action.yml:43–47](https://github.com/actions/upload-artifact/blob/ea165f8d65b6e75b540449e92b4886f43607fa02/action.yml#L43-L47)、[v6 README:449–469](https://github.com/actions/upload-artifact/blob/b7c566a772e6b6bfb58ed0dc250532a479d7789f/README.md#L449-L469)
- 路径层次规则不应被更换：wildcard 后的目录层次保留，多 search paths 用 least common ancestor，排除项不改变 root；上传单个具体文件以其父目录为 root。[README:199–217](https://github.com/actions/upload-artifact/blob/b7c566a772e6b6bfb58ed0dc250532a479d7789f/README.md#L199-L217)、[src/shared/search.ts:82–90,125–156](https://github.com/actions/upload-artifact/blob/b7c566a772e6b6bfb58ed0dc250532a479d7789f/src/shared/search.ts#L82-L156)
- 仍是不可变 ZIP artifact；多个 job/step 不能默认写同名 artifact，`overwrite=true` 是删除重建、会得到新 ID，不是原地追加。每 job 500 artifacts 上限和下载后的普通文件权限丢失（目录 755、文件 644）不是本次新引入行为；已有 tar 包装/解包流程必须保留。[README:64–79](https://github.com/actions/upload-artifact/blob/b7c566a772e6b6bfb58ed0dc250532a479d7789f/README.md#L64-L79)、[README:473–487](https://github.com/actions/upload-artifact/blob/b7c566a772e6b6bfb58ed0dc250532a479d7789f/README.md#L473-L487)
- latest v7.0.1 增加 `archive`，**默认仍为 true**；只有显式 `archive=false` 才 direct-upload 单个文件、忽略 `name` 改用实际文件名，多文件会失败。因此“v7 默认不再打 ZIP/必改 artifact 名称”是错误结论。v7 另有 ESM/toolkit 变化。[v7.0.0 release](https://github.com/actions/upload-artifact/releases/tag/v7.0.0)、[v7.0.1 action.yml:43–53](https://github.com/actions/upload-artifact/blob/043fb46d1a93c77aae656e7c1c64a875d1fc6a0a/action.yml#L43-L53)
- **提案**：第一批用 v6.0.0，保持 name/path/retention/hidden/overwrite 的现值；不要引入 direct-upload，更不能把 v7.0.1 当成完成 Node24 迁移的唯一选择。

#### 3.5 download-artifact：完整研究，但整个 Action 延期

- 第一个原生版本是 v7.0.0，不是 v5 或 v6。v4.3.0 → v7.0.0 的元数据也只改 runtime，**但实现确实有路径变化**，不能从 action.yml 相同推导兼容。[v7 action.yml](https://github.com/actions/download-artifact/blob/37930b1c2abaa49bbe596cd826c3c89aef350131/action.yml)
- 自 v5 起，单个 ID 的下载由 `path/<artifact-name>/` 变为直接 `path/`；按 `name` 直接下载本来就是后者。v7 实现的判断还包含 `artifacts.length === 1`，所以**pattern 只命中一个或“下载全部”最终仅有一个 artifact 同样可能少一层目录**；不是只检查 `artifact-ids` 即可。多个 artifact 且 `merge-multiple=false` 仍分子目录，true 则合并到同一目的目录。[README:35–47](https://github.com/actions/download-artifact/blob/37930b1c2abaa49bbe596cd826c3c89aef350131/README.md#L35-L47)、[基线 src/download-artifact.ts:172–180](https://github.com/actions/download-artifact/blob/d3f86a106a0bac45b974a628896c90dbdf5c8093/src/download-artifact.ts#L172-L180)、[v7 源码:172–183](https://github.com/actions/download-artifact/blob/37930b1c2abaa49bbe596cd826c3c89aef350131/src/download-artifact.ts#L172-L183)
- `name` 与 `artifact-ids` 仍互斥，跨 run/repository 下载仍需 `github-token` 及相应访问权限，`repository/run-id` 不应被本次升级改写。[action.yml:5–37](https://github.com/actions/download-artifact/blob/37930b1c2abaa49bbe596cd826c3c89aef350131/action.yml#L5-L37)
- latest v8.0.1：按 Content-Type 判断是否解压，新增 `skip-decompress=false`；**digest mismatch 的默认由 warning 变为 error**，可能让过去带 warning 的 run 失败。这是安全默认的收紧，不应未经批准设为 warn/ignore 以追求绿灯。v8.0.1 还修复 CJK 文件名/名称和 Content-Type 的相关行为。[v8.0.0 release](https://github.com/actions/download-artifact/releases/tag/v8.0.0)、[v8.0.1 release](https://github.com/actions/download-artifact/releases/tag/v8.0.1)、[v8.0.1 action.yml:38–47](https://github.com/actions/download-artifact/blob/3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c/action.yml#L38-L47)；v7 warning 实现见 [src/download-artifact.ts:191–199](https://github.com/actions/download-artifact/blob/37930b1c2abaa49bbe596cd826c3c89aef350131/src/download-artifact.ts#L191-L199)。
- **延期结论**：第一批保留现有 v4.3.0 pin，不修改 release.yml。v7.0.0 只是已核验的后续最小 Node24 候选；owner 若选择 v8.0.1，需明确接受 digest enforcement 和解压行为的独立验证范围。

### 4. Artifact 跨版本边界

1. **官方已说明的硬边界**：v4+ download 不支持 upload-artifact v3 及更早生成的 artifact；两 Action 的 v4+ 架构不能被当成 v3/GHES 兼容替代。给定基线已经是 v4，因此无需为了此 Node24 维护重做 v3→v4 迁移。[download v7 README:49–68](https://github.com/actions/download-artifact/blob/37930b1c2abaa49bbe596cd826c3c89aef350131/README.md#L49-L68)、[upload v6 README:51–60](https://github.com/actions/upload-artifact/blob/b7c566a772e6b6bfb58ed0dc250532a479d7789f/README.md#L51-L60)
2. **有依据但未实跑的兼容性推论**：upload v6 保留 ZIP/v4 artifact backend；download v4.3.0 与 v7 均按该 backend 的 ID 下载。未发现官方要求这两个 Action 的 major 数字必须相同，因此不能仅因 upload 升 v6 就强迫 release-only download 同批升级。然而这不是本仓库 upload-v6→download-v4.3.0 的完整 release 演练证明；第一批范围收窄后，跨 workflow 消费链仍有验证欠账。
3. **新增格式不能交给旧消费端碰运气**：upload v7 的 `archive=false` 会生成 raw artifact；download v8 才新增 Content-Type/跳过解压支持。既有 ZIP 路线可以单独评审，新 raw 路线不能默认被旧 downloader 支持。依据 [upload v7 release](https://github.com/actions/upload-artifact/releases/tag/v7.0.0) 与 [download v8 release](https://github.com/actions/download-artifact/releases/tag/v8.0.0)，本次不启用该新格式。

### 5. 官方时间线：不是仍在未来的 Node20 退役

[GitHub 2026-09-23 最终公告](https://github.blog/changelog/2026-09-23-node-20-is-no-longer-available-in-github-actions/) 已明确 Node20 不再提供、JavaScript Actions 现在使用 Node24，临时不安全 runtime opt-out 已移除。因此截至 **2026-10-02**，不能继续把退役写成未来事件，也不能把 opt-out 列为回滚方式。

`action.yml` 的 node20 声明与某次 runner 实际执行的 Node 版本是不同证据层级：旧 SHA 声明 node20 本身不证明现在仍在跑 Node20，也不证明已有绿灯一定会立即失败。原生 pin 更新是减少隐式 runtime 兼容风险；官方声明、静态契约检查与新运行证据三者不可互相替代。runner-label/image、平台覆盖及支持状态由协调者另行审计，本文件只记录 Action 声明和官方 runner 最低版本。

### 6. Files found / Code patterns / Related specs

**已读取的本仓库规划与规范（仅相对路径）：**

| 文件 | 与本研究有关的内容 |
|---|---|
| `AGENTS.md` | 工具树不属于产品功能；任务、规范与正式证据的权威顺序；不得共享凭据、个人路径与本机状态。 |
| `.trellis/workflow.md:153–165,353–380,437–439` | 规划授权不等于实施授权；research 必须落盘；任务启动在 artifact review 之后。 |
| `.trellis/tasks/10-02-ci-runtime-compatibility/prd.md` | Node24 原生、最小 immutable SHA 更新、保留 release/输入输出/权限契约、待最终 scope 批准。 |
| `.trellis/tasks/10-02-ci-runtime-compatibility/task.json` | 此目标是 planning，不据过期注入信息切换别的任务。 |
| `.trellis/spec/guides/index.md:54–66` | reviewer 的行为变化或 bug 结论必须查实际代码，不能只信概括。 |
| `.trellis/spec/agentsessions-cli/backend/index.md:192–195,226–231` | installed-artifact 检查与证据诚实；绿 CI 不等于 clean-machine/minimum-OS/signed-release 验证。 |
| `.trellis/spec/agentsessions-cli/backend/quality-guidelines.md` | 当前仍是模板；不能从占位符臆造已存在的 CI 契约。 |

**已读取的上游目标源码及模式：**

| 固定版本中的文件 | 一行用途 / 已核验模式 |
|---|---|
| 五个候选各自的 `action.yml` | 明确 runtime、输入默认值、outputs、main/post；固定 SHA 与行号见 §2。 |
| `actions/checkout@v5.1.0/src/git-auth-helper.ts:275–318` | token 配置仍使用 `.git/config`，不是 v6 凭据文件方案。 |
| `actions/checkout@v5.1.0/src/unsafe-pr-checkout-helper.ts:13–80` | 按 event/fork/ref 判断危险 checkout；不可只看 major 名称猜兼容。 |
| `actions/setup-python@v6.3.0/src/cache-distributions/pip-cache.ts:65–75` | Linux distro 信息参与自动 cache key。 |
| `actions/cache@v5.1.0/src/saveImpl.ts:65–91` | save result 与 warning 处理，不将未写缓存当作成功写入。 |
| `actions/upload-artifact@v6.0.0/src/shared/search.ts:14–20,82–90,125–156` | 隐藏文件开关、glob、LCA 与单文件 root 规则。 |
| `actions/download-artifact@v4.3.0` / `@v7.0.0` 的 `src/download-artifact.ts:172–199` | 元数据相同仍可能有目录布局变化；v7 mismatch 仍是 warning。 |
| 各候选及 latest 的 `README.md`、各关键 major release body | runner 要求、迁移边界、行为变化，与 action.yml 相互核对。 |

协调者另提供既有 CLI workflow-contract unittest 会校验不可变完整 pins 和 `GH_REPO`；本研究没有重复扫描该测试或 workflow 使用点，也没有运行它。实施计划应复用该真实检查，而不是把本报告误当作测试通过凭据。

### 7. External references / 精确核验命令

所有本报告外部结论最终来自 **actions 官方 GitHub 仓库、GitHub Docs/GitHub Changelog**。Grok/Web 搜索仅用于发现官方来源，不以 AI 摘要作为版本、SHA 或 runtime 证据。已加载 gh、web-access、find-docs 技能；已执行 `web.run` 查询/打开官方页面。部分网页入口未返回正文，版本与 source 的证明由成功的 `gh api` 响应提供。

**实际执行的只读命令族：**下述变量取值均为公开仓库；未传递私有代码、仓库名、凭据或本机路径。分页列表、release body、ref object、contents API 分别独立核对，不从 README badge 或 floating major 反推版本。

```powershell
$repos = @(
  'actions/checkout', 'actions/setup-python', 'actions/cache',
  'actions/upload-artifact', 'actions/download-artifact'
)
foreach ($repo in $repos) {
  gh api "repos/$repo/releases/latest"
  gh api --paginate --slurp "repos/$repo/releases?per_page=100"
}

# Selected major/candidate release bodies; complete release bodies also come from the paginated list:
gh api "repos/$repo/releases/tags/$tag"
# Every one of the 21 refs in the ledger:
gh api "repos/$repo/git/ref/tags/$tag"

# After resolving the COMMIT SHA (never the tag-object SHA):
gh api "repos/$repo/contents/action.yml?ref=$sha"
gh api "repos/$repo/contents/README.md?ref=$sha"
gh api "repos/$repo/git/trees/${sha}?recursive=1"
gh api "repos/$repo/contents/${sourcePath}?ref=$sha"
```

实际 ref/runtime 核验中使用的解析流程如下；API 非零退出即停止，不以记忆填表：

```powershell
function Get-PublicJson([string]$Endpoint) {
  $raw = gh api $Endpoint
  if ($LASTEXITCODE -ne 0) { throw "GitHub API failed for $Endpoint" }
  $raw | ConvertFrom-Json
}
$ref = Get-PublicJson "repos/$repo/git/ref/tags/$tag"
$obj = $ref.object
while ($obj.type -eq 'tag') {
  # Annotated-tag peeling: not needed for any of this run's 21 direct-commit refs.
  $annotated = Get-PublicJson "repos/$repo/git/tags/$($obj.sha)"
  $obj = $annotated.object
}
if ($obj.type -ne 'commit') { throw 'Tag did not resolve to a commit' }
$sha = $obj.sha
$file = Get-PublicJson "repos/$repo/contents/action.yml?ref=$sha"
$text = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($file.content))
```

**ref 查询的全部 21 个实参：**

| 官方仓库 | 实际核验 tag（各完整 SHA 均已记录在 §2） |
|---|---|
| actions/checkout | `v4.2.2`, `v5.0.0`, `v5.1.0`, `v7.0.1` |
| actions/setup-python | `v5.6.0`, `v6.0.0`, `v6.3.0`, `v7.0.0` |
| actions/cache | `v4.3.0`, `v5.0.0`, `v5.1.0`, `v6.1.0` |
| actions/upload-artifact | `v4.6.2`, `v5.0.0`, `v6.0.0`, `v7.0.1` |
| actions/download-artifact | `v4.3.0`, `v5.0.0`, `v6.0.0`, `v7.0.0`, `v8.0.1` |

额外读取的跨-major release 说明包含 checkout `v6.0.0/v7.0.0`、cache `v6.0.0`、upload `v7.0.0`、download `v8.0.0`；这些是行为变化证据，不是额外候选 pin。本文没有解析这些额外 release 的 commit SHA，也不把它们当已核验的实施 pin。

公开 API 入口示例：[checkout latest](https://api.github.com/repos/actions/checkout/releases/latest)、[setup-python latest](https://api.github.com/repos/actions/setup-python/releases/latest)、[cache latest](https://api.github.com/repos/actions/cache/releases/latest)、[upload latest](https://api.github.com/repos/actions/upload-artifact/releases/latest)、[download latest](https://api.github.com/repos/actions/download-artifact/releases/latest)。可点击的版本 release 和固定 source 链接已随各表/结论给出。

官方公告正文另通过定向 `web_fetch` 获取：[Node20 最终退役公告](https://github.blog/changelog/2026-09-23-node-20-is-no-longer-available-in-github-actions/)、[checkout safer PR defaults](https://github.blog/changelog/2026-06-18-safer-pull_request_target-defaults-for-github-actions-checkout/)。后者正文仍残留 July 16，但 **2026-07-15 editor note 已改为 2026-07-20 生效**；本报告采用编辑注与实际 2026-07-20 release，不沿用旧日期。

## Caveats / Not Found

- **授权边界**：四个候选仍待 owner 最终范围审查；release.yml、download-artifact 和所有 release-only pins 不在第一批。此文件只研究，不实施、不启动任务、不修改任何运行指针。
- **未完成的验证**：没有 CI dispatch、artifact 上传下载、release rehearsal、runner 版本实测或私有 workflow use-site 审计。三个 workflow 的将来绿灯也不能补齐 release-only 上传/下载/打包/发布链路的证据。
- **来源冲突已经显式处理**：artifact 的“支持 Node24”旧 release 与实际 node20 声明不同；setup-python v7 README 概括与 `pip-install` 移除不同；checkout 公告保留旧 backport 日期。以固定 SHA 的源码、具体发布说明、更新注为准。
- **不是新平台支持认证**：runner 2.327.1 是 Action 官方最低 runtime 条件，不是当前 hosted image 的充分保证，也不是产品 minimum-OS 声明。未对未来 image 标签作推断。
- **不是永久安全/维护保证**：未执行全量上游 CVE/依赖树审计，没有声称首个原生 major 包含所有 newer-major 安全修复；这些是当前可追溯的最小候选，最终实施前须再次检查官方 release/ref/适用变更。
- **跨版本下载仍须实测**：ZIP/backend 层兼容性推论不等于本仓库消费链运行通过；raw direct-upload 不在该推论范围。
- **服务与工具限制**：本次 GitHub/Grok 成功响应中未遇到 API rate-limit；没有假造失败服务的验证结果。Context7 实际使用 **0/3** 次：任务是 GitHub release/ref/源码核验，使用 gh 直达原始证据；未为使用 Context7 安装工具或依赖，也未修改认证/用户配置。
- **未写入共享文件的内容**：没有保留本机绝对路径、credentials、查询身份、环境值、浏览器状态或运行时 task pointer。未读取 implement/check manifests；未修改代码、spec、workflow 或协调者 task artifacts。

### Remaining decisions

1. Owner 是否批准 **仅三个 workflow、四种 Action** 使用 §2.4 的确切 SHA，并保持既有输入、name/path、凭据、权限、触发器和产品依赖不变？
2. 协调者需把 checkout 安全回补适用性、Python cache 冷启动、artifact 文件布局/隐藏文件/名称和现有 workflow-contract unittest 纳入第一批验收；不把提案当通过结果。
3. release 安全演练路径另行授权后，再决定 release-only pins 和 download 的 v7 最小迁移，或 v8 的 digest/解压变化；不创建新 tag 绕开现有演练前提，不以降低 digest/unsafe/runtime 安全开关消除失败。
