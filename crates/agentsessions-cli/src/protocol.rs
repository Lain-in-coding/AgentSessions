//! Robot 协议层：统一 JSON envelope、错误目录（Error Catalog）与 Outcome。
//!
//! 遵循 `docs/contracts/CONTRACT-cli-robot-mcp-draft.md`、
//! `schemas/robot/v1/envelope.schema.json` 与 `schemas/robot/v1/error-catalog.json`：所有入口
//! 先把结果/错误归一到同一组版本化 DTO，再按同一映射投影到 exit code / JSON，
//! 不允许各命令各自决定语义。本模块目前是唯一消费者（CLI）；MCP 落地时再抽 crate。
//!
//! 约束：
//! - stdout 只输出协议数据；进程级诊断只走 stderr。
//! - 错误 envelope 携带稳定 `code` + 安全 `message` + `retryable` + 有界 `details`。
//! - `schema_version` 走 major.minor；未知 major 由调用方拒绝。

use agentsessions_application::AppError;
use agentsessions_application::cursor::CursorError;
use agentsessions_domain::DomainError;
use agentsessions_ports::{PortError, ProviderError};
use serde_json::{Value, json};

/// 当前协议 schema 版本（major.minor）。未知 major 必须拒绝，兼容 minor 按合同处理。
pub const SCHEMA_VERSION: &str = "1.0";

/// 业务结果层级。与进程错误分离：partial 绝不伪装成 success。
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Success,
    Partial,
}

/// 输出模式。切片期实现 human/json；jsonl 预留。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    /// 人类可读（默认）。当前退化为紧凑单行 JSON，待人类渲染器落地。
    Human,
    /// 单个 JSON envelope。
    Json,
    /// 每行一个完整协议 frame。
    Jsonl,
}

/// `schemas/robot/v1/error-catalog.json` 中错误目录的实现子集。
///
/// 每个 canonical code 固定映射到 exit code / retryable / redaction，
/// 新增错误必须先在此登记，任何入口才能返回。
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanonicalCode {
    /// 参数或请求校验失败 → exit 2。
    InvalidRequest,
    /// 请求对象不存在 → exit 4。
    NotFound,
    /// Source/File I/O 错误 → exit 5。
    SourceIo,
    /// 源在读取期间被改写（`source_changed`）→ exit 5。
    SourceChanged,
    /// 快照校验阶段无法完成（与已确认 source_changed 区分）。
    #[allow(dead_code)]
    SnapshotFailed,
    /// Catalog 或 Search Index 错误 → exit 6。
    CatalogError,
    /// Provider 格式或 Adapter 错误 → exit 7。
    ProviderError,
    /// writer lease 未取得（`writer_busy`）→ exit 6，可重试。
    WriterBusy,
    /// JSON/协议版本不兼容 → exit 9。
    SchemaIncompatible,
    /// Cursor 令牌无法解析/校验失败（`cursor_invalid`）→ exit 2。
    CursorInvalid,
    /// Cursor 超出 TTL（`cursor_expired`）→ exit 2。
    CursorExpired,
    /// Cursor 携带的 generation 与活动 generation 不一致 → exit 9。
    GenerationMismatch,
    /// 未分类内部错误（不变量违反等 bug 信号）→ exit 70。
    Internal,
}

impl CanonicalCode {
    /// 稳定 wire 字符串。对外契约的一部分，不可随意更名。
    pub fn as_str(self) -> &'static str {
        match self {
            CanonicalCode::InvalidRequest => "invalid_request",
            CanonicalCode::NotFound => "not_found",
            CanonicalCode::SourceIo => "source_io",
            CanonicalCode::SourceChanged => "source_changed",
            CanonicalCode::SnapshotFailed => "snapshot_failed",
            CanonicalCode::CatalogError => "catalog_error",
            CanonicalCode::ProviderError => "provider_error",
            CanonicalCode::WriterBusy => "writer_busy",
            CanonicalCode::SchemaIncompatible => "schema_incompatible",
            CanonicalCode::CursorInvalid => "cursor_invalid",
            CanonicalCode::CursorExpired => "cursor_expired",
            CanonicalCode::GenerationMismatch => "generation_mismatch",
            CanonicalCode::Internal => "internal",
        }
    }

    /// CLI exit code，必须与 Robot v1 error catalog 保持一致。
    pub fn exit_code(self) -> i32 {
        match self {
            CanonicalCode::InvalidRequest
            | CanonicalCode::CursorInvalid
            | CanonicalCode::CursorExpired => 2,
            CanonicalCode::NotFound => 4,
            CanonicalCode::SourceIo
            | CanonicalCode::SourceChanged
            | CanonicalCode::SnapshotFailed => 5,
            CanonicalCode::CatalogError | CanonicalCode::WriterBusy => 6,
            CanonicalCode::ProviderError => 7,
            CanonicalCode::SchemaIncompatible | CanonicalCode::GenerationMismatch => 9,
            CanonicalCode::Internal => 70,
        }
    }

    /// 是否值得重试（writer_busy 等瞬态错误为 true）。
    pub fn retryable(self) -> bool {
        matches!(
            self,
            CanonicalCode::WriterBusy | CanonicalCode::SourceChanged
        )
    }
}

/// 归一后的协议错误：稳定 code + 安全 message。
#[derive(Debug, Clone)]
pub struct ProtocolError {
    pub code: CanonicalCode,
    pub message: String,
}

impl ProtocolError {
    pub fn new(code: CanonicalCode, message: impl Into<String>) -> Self {
        ProtocolError {
            code,
            message: message.into(),
        }
    }
}

impl From<DomainError> for ProtocolError {
    fn from(e: DomainError) -> Self {
        let code = match &e {
            DomainError::NotFound(_) => CanonicalCode::NotFound,
            DomainError::InvalidRequest(_) => CanonicalCode::InvalidRequest,
            DomainError::InvariantViolation(_) => CanonicalCode::Internal,
            DomainError::UnstableIdentity(_) => CanonicalCode::InvalidRequest,
        };
        ProtocolError::new(code, e.to_string())
    }
}

impl From<AppError> for ProtocolError {
    fn from(error: AppError) -> Self {
        match error {
            AppError::Domain(error) => error.into(),
            AppError::Port(error) => error.into(),
            AppError::Provider(error) => error.into(),
            // cursor 错误族有专属 canonical code；contract major 不符归 schema_incompatible。
            AppError::Cursor(error) => {
                let code = match &error {
                    CursorError::Invalid(_) => CanonicalCode::CursorInvalid,
                    CursorError::Expired(_) => CanonicalCode::CursorExpired,
                    CursorError::GenerationMismatch { .. } => CanonicalCode::GenerationMismatch,
                    CursorError::ContractMismatch { .. } => CanonicalCode::SchemaIncompatible,
                };
                ProtocolError::new(code, error.to_string())
            }
            // 预算过小是请求校验失败（CONTRACT §3）。
            AppError::Budget(error) => {
                ProtocolError::new(CanonicalCode::InvalidRequest, error.to_string())
            }
        }
    }
}

impl From<ProviderError> for ProtocolError {
    fn from(error: ProviderError) -> Self {
        ProtocolError::new(CanonicalCode::ProviderError, error.to_string())
    }
}

impl From<PortError> for ProtocolError {
    fn from(e: PortError) -> Self {
        let code = match &e {
            PortError::Backend(_) => CanonicalCode::CatalogError,
            PortError::SourceIo(_) => CanonicalCode::SourceIo,
            PortError::SchemaIncompatible(_) => CanonicalCode::SchemaIncompatible,
            PortError::NotFound(_) => CanonicalCode::NotFound,
            PortError::SnapshotChanged(_) => CanonicalCode::SourceChanged,
            PortError::WriterBusy(_) => CanonicalCode::WriterBusy,
        };
        ProtocolError::new(code, e.to_string())
    }
}

/// 从参数中解析输出模式：`--robot` 等价稳定 JSON；`--output human|json|jsonl`。
///
/// `--robot` 优先级最高（等价 `--output json` + 无色 + 无进度）。缺省为 Human。
/// 未知或缺少 `--output` 值属于请求错误，不能静默降级为另一种协议。
pub fn parse_output_mode(args: &[String]) -> Result<OutputMode, String> {
    if args.iter().any(|a| a == "--robot") {
        return Ok(OutputMode::Json);
    }
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--output" {
            return match it.next().map(|s| s.as_str()) {
                Some("human") => Ok(OutputMode::Human),
                Some("json") => Ok(OutputMode::Json),
                Some("jsonl") => Ok(OutputMode::Jsonl),
                Some(value) => Err(format!("unsupported output mode: {value}")),
                None => Err("--output requires human|json|jsonl".into()),
            };
        }
    }
    Ok(OutputMode::Human)
}

fn request_id() -> String {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    format!("cli-{}-{millis}", std::process::id())
}

/// 分页元数据（envelope `page` 字段）：续读令牌 + 是否还有后续页。
#[derive(Debug, Clone, Default)]
pub struct Page {
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

/// 统一输出一个成功结果。所有模式当前都发单个 envelope（人类渲染器待落地；
/// Human 在切片期退化为紧凑 JSON envelope，与既有 Robot-JSON 雏形一致）。
pub fn emit(
    command: &str,
    _mode: OutputMode,
    outcome: Outcome,
    data: Value,
    duration_ms: u64,
    page: &Page,
) {
    println!(
        "{}",
        success_envelope(command, outcome, data, duration_ms, page)
    );
}

/// 成功 envelope。`data` 是已验证的 JSON Value，不接受未校验字符串片段。
pub fn success_envelope(
    command: &str,
    outcome: Outcome,
    data: Value,
    duration_ms: u64,
    page: &Page,
) -> String {
    let outcome_str = match outcome {
        Outcome::Success => "success",
        Outcome::Partial => "partial",
    };
    let generation = data.get("generation").cloned().unwrap_or(Value::Null);
    json!({
        "schema_version": SCHEMA_VERSION,
        "frame_type": "response",
        "command": command,
        "request_id": request_id(),
        "ok": true,
        "outcome": outcome_str,
        "data": data,
        "warnings": [],
        "page": {
            "next_cursor": page.next_cursor.as_deref().map_or(Value::Null, |c| json!(c)),
            "has_more": page.has_more,
        },
        "meta": {
            "duration_ms": duration_ms,
            "generation": generation,
        },
    })
    .to_string()
}

/// 错误 envelope。stdout 只有这一个对象；细节安全、有界。
pub fn error_envelope(command: &str, err: &ProtocolError) -> String {
    json!({
        "schema_version": SCHEMA_VERSION,
        "frame_type": "error",
        "command": command,
        "request_id": request_id(),
        "ok": false,
        "outcome": "failure",
        "error": {
            "code": err.code.as_str(),
            "message": err.message,
            "retryable": err.code.retryable(),
            "details": {},
        },
        "warnings": [],
        "page": {
            "next_cursor": Value::Null,
            "has_more": false,
        },
        "meta": {
            "duration_ms": 0,
            "generation": Value::Null,
        },
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_codes_follow_contract() {
        assert_eq!(CanonicalCode::InvalidRequest.exit_code(), 2);
        assert_eq!(CanonicalCode::NotFound.exit_code(), 4);
        assert_eq!(CanonicalCode::SourceIo.exit_code(), 5);
        assert_eq!(CanonicalCode::SourceChanged.exit_code(), 5);
        assert_eq!(CanonicalCode::CatalogError.exit_code(), 6);
        assert_eq!(CanonicalCode::WriterBusy.exit_code(), 6);
        assert_eq!(CanonicalCode::ProviderError.exit_code(), 7);
        assert_eq!(CanonicalCode::SchemaIncompatible.exit_code(), 9);
        assert_eq!(CanonicalCode::Internal.exit_code(), 70);
    }

    #[test]
    fn writer_busy_maps_from_port_error() {
        let e: ProtocolError = PortError::WriterBusy("held".into()).into();
        assert_eq!(e.code, CanonicalCode::WriterBusy);
        assert!(e.code.retryable());
    }

    #[test]
    fn port_error_categories_map_to_catalog() {
        let e: ProtocolError = PortError::SnapshotChanged("mtime".into()).into();
        assert_eq!(e.code, CanonicalCode::SourceChanged);

        let e: ProtocolError = PortError::SourceIo("cannot open source".into()).into();
        assert_eq!(e.code, CanonicalCode::SourceIo);

        let e: ProtocolError = PortError::SchemaIncompatible("newer schema".into()).into();
        assert_eq!(e.code, CanonicalCode::SchemaIncompatible);
    }

    #[test]
    fn invariant_violation_is_internal() {
        let e: ProtocolError = DomainError::InvariantViolation("bug".into()).into();
        assert_eq!(e.code, CanonicalCode::Internal);
        assert_eq!(e.code.exit_code(), 70);
    }

    #[test]
    fn cursor_and_budget_errors_map_to_dedicated_codes() {
        let e: ProtocolError = AppError::Cursor(CursorError::Invalid("x".into())).into();
        assert_eq!(e.code, CanonicalCode::CursorInvalid);
        assert_eq!(e.code.exit_code(), 2);

        let e: ProtocolError = AppError::Cursor(CursorError::Expired("x".into())).into();
        assert_eq!(e.code, CanonicalCode::CursorExpired);
        assert_eq!(e.code.exit_code(), 2);

        let e: ProtocolError = AppError::Cursor(CursorError::GenerationMismatch {
            cursor: 1,
            active: 2,
        })
        .into();
        assert_eq!(e.code, CanonicalCode::GenerationMismatch);
        assert_eq!(e.code.exit_code(), 9);

        let e: ProtocolError = AppError::Cursor(CursorError::ContractMismatch {
            cursor: 2,
            supported: 1,
        })
        .into();
        assert_eq!(e.code, CanonicalCode::SchemaIncompatible);

        let e: ProtocolError = AppError::Budget(
            agentsessions_application::budget::BudgetError::TooSmall("x".into()),
        )
        .into();
        assert_eq!(e.code, CanonicalCode::InvalidRequest);
    }

    #[test]
    fn success_envelope_is_well_formed() {
        let s = success_envelope(
            "status",
            Outcome::Success,
            json!({ "catalog_count": 3, "generation": 5 }),
            42,
            &Page::default(),
        );
        assert!(s.contains("\"schema_version\":\"1.0\""));
        assert!(s.contains("\"frame_type\":\"response\""));
        assert!(s.contains("\"command\":\"status\""));
        assert!(s.contains("\"ok\":true"));
        assert!(s.contains("\"outcome\":\"success\""));
        assert!(s.contains("\"data\":{\"catalog_count\":3"));
        assert!(s.contains("\"duration_ms\":42"));
        assert!(s.contains("\"generation\":5"));
        assert!(s.contains("\"next_cursor\":null"));
        assert!(s.contains("\"has_more\":false"));
    }

    #[test]
    fn success_envelope_carries_page_cursor() {
        let s = success_envelope(
            "search",
            Outcome::Partial,
            json!({ "hits": [] }),
            1,
            &Page {
                next_cursor: Some("tok.abc".into()),
                has_more: true,
            },
        );
        assert!(s.contains("\"outcome\":\"partial\""));
        assert!(s.contains("\"next_cursor\":\"tok.abc\""));
        assert!(s.contains("\"has_more\":true"));
    }

    #[test]
    fn published_error_catalog_matches_runtime_mapping() {
        let catalog: Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../schemas/robot/v1/error-catalog.json"
        )))
        .expect("published error catalog must be valid JSON");
        let published = catalog["errors"]
            .as_array()
            .expect("error catalog must contain errors");
        let runtime = [
            CanonicalCode::InvalidRequest,
            CanonicalCode::NotFound,
            CanonicalCode::SourceIo,
            CanonicalCode::SourceChanged,
            CanonicalCode::SnapshotFailed,
            CanonicalCode::CatalogError,
            CanonicalCode::ProviderError,
            CanonicalCode::WriterBusy,
            CanonicalCode::SchemaIncompatible,
            CanonicalCode::CursorInvalid,
            CanonicalCode::CursorExpired,
            CanonicalCode::GenerationMismatch,
            CanonicalCode::Internal,
        ];
        assert_eq!(published.len(), runtime.len());
        for code in runtime {
            let entry = published
                .iter()
                .find(|entry| entry["code"] == code.as_str())
                .unwrap_or_else(|| panic!("missing published error {}", code.as_str()));
            assert_eq!(entry["cli_exit_code"], code.exit_code());
            assert_eq!(entry["retryable"], code.retryable());
            assert_eq!(entry["robot_ok"], false);
        }
    }

    #[test]
    fn published_envelope_schema_contains_runtime_contract() {
        let schema: Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../schemas/robot/v1/envelope.schema.json"
        )))
        .expect("published envelope schema must be valid JSON");
        assert_eq!(
            schema["$defs"]["success"]["properties"]["schema_version"]["const"],
            SCHEMA_VERSION
        );
        assert_eq!(
            schema["$defs"]["error"]["properties"]["schema_version"]["const"],
            SCHEMA_VERSION
        );
        let codes = schema["$defs"]["errorBody"]["properties"]["code"]["enum"]
            .as_array()
            .expect("schema must enumerate canonical error codes");
        assert_eq!(codes.len(), 13);
        for field in [
            "schema_version",
            "frame_type",
            "command",
            "request_id",
            "ok",
            "outcome",
            "warnings",
            "page",
            "meta",
        ] {
            let required = schema["$defs"]["success"]["required"]
                .as_array()
                .expect("success required must be an array");
            assert!(
                required.iter().any(|value| value == field),
                "missing {field}"
            );
        }
    }

    #[test]
    fn output_mode_rejects_unknown_or_missing_value() {
        assert!(parse_output_mode(&["--output".into(), "yaml".into()]).is_err());
        assert!(parse_output_mode(&["--output".into()]).is_err());
        assert_eq!(
            parse_output_mode(&["--robot".into(), "--output".into(), "yaml".into()]),
            Ok(OutputMode::Json)
        );
    }

    #[test]
    fn error_envelope_carries_code_and_retryable() {
        let err = ProtocolError::new(CanonicalCode::WriterBusy, "another writer holds the lease");
        let s = error_envelope("sync", &err);
        assert!(s.contains("\"frame_type\":\"error\""));
        assert!(s.contains("\"ok\":false"));
        assert!(s.contains("\"outcome\":\"failure\""));
        assert!(s.contains("\"code\":\"writer_busy\""));
        assert!(s.contains("\"retryable\":true"));
        assert!(s.contains("\"duration_ms\":0"));
    }
}
