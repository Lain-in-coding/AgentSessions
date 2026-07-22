# EVIDENCE：cross-platform-packaging spike

> 可丢弃探针证据。对应计划 §12.4（发布证据顺序）、审查 #12（checksum-after-sign）、
> Selection Gate 硬门「全部正式 target 干净构建 + 安全构建约束」。
> 探针脚本：`verify-release-pipeline.ps1`（本机实跑）。

## 环境
- os/arch = windows / x86_64
- 已装工具：cargo-audit、cargo-cyclonedx、cargo-deny、gh 2.83.0
- 缺失工具：cosign（签名）、syft（备选 SBOM）；仅装 x86_64-pc-windows-msvc target

## 本机可验证步骤（实测 PASS）
| step | 结论 |
|---|---|
| 1 干净构建（release，本 target） | PASS：`cargo build --release` 成功 |
| 2 供应链审计 | PASS：cargo-audit 无已知漏洞 |
| 3 SBOM (CycloneDX) | PASS：cargo-cyclonedx 生成真实 SBOM（约 48KB） |
| 4 checksum 顺序（审查#12） | PASS：先（模拟）签名 → 再算 checksum；改字节后旧 checksum 立即失效，证明「签名后算 checksum」顺序正确 |

## 关键结论
1. **审查 #12 成立**：checksum 必须在签名/公证/最终打包之后、对最终字节计算；任何后续字节变更都会使旧 checksum 失效。探针实测「改字节 → 旧 checksum 不匹配」，坐实此顺序。
2. **供应链证据可自动化**：cargo-audit + cargo-cyclonedx 在本机即可产出漏洞与 SBOM 证据，可进 PR/Release Gate。
3. **cargo-deny 对 tantivy 报 unlicensed**（ort-sys 传递依赖，见 search-backend EVIDENCE），FTS5 依赖树（rusqlite bundled）更干净——从供应链维度支持 FTS5 默认。

## 需 External Readiness Gate（本机不验证）
- Windows Authenticode 签名：需真实证书；
- macOS Notarization：需 Apple 凭据 + macOS 主机；
- 多 target 交叉构建：需安装 target 或 CI runner（当前仅 windows-msvc）；
- cosign / syft：本机缺，CI 或 External Readiness 阶段补齐。

## 对计划的意义
- §12.4 发布顺序（build → SBOM → 签名/notarize → 最终字节 checksum → provenance → 验证 → Release）中，本机可验证的前段与 checksum 顺序已实测成立；
- 签名/公证/多平台交叉构建确认为 External Readiness Gate 职责，不阻断本机 R0；
- 安全构建约束（无联网、无 protoc/模型下载）在 FTS5 路径下天然满足；Tantivy 路径因 ort-sys 需额外审查。
