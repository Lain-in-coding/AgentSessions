# 外部就绪门（External Readiness Gate）

> 治理记录
> - decision_id: R0-EXTERNAL-READINESS
> - status: Draft（待 R0 评审）
> - owner: （待指派）  approver: 项目最终验收人
> - due_milestone: R0；正式发布前必须再次执行一次完整 dry-run
> - evidence_path: docs/operations/ + CI 发布日志

本门是外部发布就绪要求的规范性清单。目标：正式发布依赖的外部凭据与渠道，在不依赖本地开发机状态的前提下，提前完成配置或无密钥 dry-run，避免发布当天才发现凭据缺失。平台承诺与构建策略见 `../adr/ADR-0002-platform-targets.md`。

## 1. 门项清单（每项需 owner + 状态 + 最后一次 dry-run 记录）

| 门项 | 说明 | 本机现状 |
|---|---|---|
| Windows 代码签名 | 证书就位或 dry-run | 需真实证书，本机无 |
| macOS 开发者签名 + 公证 | Apple 凭据 + macOS 主机 | 本机无 macOS |
| 制品透明日志签名 | cosign / OIDC | 本机缺 cosign，CI 阶段补 |
| 制品来源证明 | GitHub Artifact Attestation / provenance | CI 阶段补 |
| crates.io 可信发布 | OIDC + 维护责任人 | 待配置 |
| 下游渠道 | Homebrew Tap / Scoop bucket / 安装脚本 | 待配置，均为发布后 promotion |

## 2. 规则

- 凭据绝不写入仓库，不在 CI 日志或制品中出现；
- 任一外部渠道不可用时，权威 GitHub Release 仍可独立发布，渠道随后再 promotion；
- 门结果、证书有效期、责任人、回滚手册和最后一次 dry-run 时间戳作为发布证据保存；
- 正式发布前必须重新跑一次完整 dry-run，过期即视为未通过。

## 3. 与 spike 的关系

cross-platform-packaging spike 已实测本机可验证部分（干净构建、依赖审计、SBOM 生成、checksum-after-sign 顺序）。本门覆盖的是本机无法验证、需真实凭据与外部账户的部分。
