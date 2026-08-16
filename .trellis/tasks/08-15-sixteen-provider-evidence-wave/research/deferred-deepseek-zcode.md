# Deferred 决策记录:DeepSeek Harness 与 ZCode (15/16、16/16)

> 日期:2026-08-16
> 任务:08-15-sixteen-provider-evidence-wave
> 决策者:owner(QIN)通过 GOAL 任务授权自主决策,按 PRD Acceptance Criteria 记录

## 决策

对 16-provider 冻结名单中的两个无证据 provider 记录 `deferred`:

| Provider | 状态 | 能力矩阵行 | 宣传口径 |
|---|---|---|---|
| DeepSeek Harness (15/16) | **deferred** | 保留,`maturity=unsupported`,variant 空,capability 全 unknown | 不宣传为已实现 |
| ZCode (16/16) | **deferred** | 保留,`maturity=unsupported`,variant 空,capability 全 unknown | 不宣传为已实现 |

两个 provider 均**不设 maturity target**(`target_for` 返回 None),与 PRD 要求
「deferred provider 仍保留 16 行,不得宣传为已实现」一致。

## 证据状态(2026-08-15 核查)

- `~/.deepseek` 不存在,无 `DEEPSEEK_HOME` 环境证据,无任何本地 transcript。
- `~/.zcode` 不存在,无任何本地 transcript。
- 12 个深读参考项目(deep-read-*.md 与固定 clone)中**没有任何** DeepSeek Harness
  或 ZCode 的 adapter/connector。
- 竞品借用矩阵(hstry/AgentRecall/cass/ctx)中两 provider 均无条目。

## 为什么 deferred 而不是猜测实现

1. PRD 硬约束:「无证据不实现、不宣传」「禁止猜测」。
2. 两个 provider 的 source root、format、identity、resume 全部 unknown;
   凭命名猜测 adapter 会污染 probe 消歧(AmbiguousVariant 原则)。
3. 猜测实现无法通过 golden fixture 验证,且会给用户错误的 resume/搜索结果。

## 解锁条件(恢复实现的前置)

- owner 提供任一真实 transcript fixture(文件头 20 行即可),或
- 官方文档/源码确认 source root 与行格式(需 provenance + version 记录)。

满足后按「证据 → fixture → probe → parse → golden → 矩阵晋级」路径补齐。
