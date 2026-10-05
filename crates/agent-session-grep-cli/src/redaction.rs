//! Cross-boundary output redaction (ADR-0009).
//!
//! Secret detection + redaction for machine/cross-boundary outputs
//! (Robot JSON/JSONL, MCP, HTTP API, Handoff Pack, Web UI). Human CLI/TUI
//! output stays unredacted per ADR-0004.
//!
//! The redactor recursively walks a `serde_json::Value` tree and replaces
//! string values matching high-confidence secret patterns with a mode
//! reference. The redaction status is carried in the Robot envelope's
//! `redaction` field. The string-level engine lives in
//! [`agent_session_grep_ports::redact`] so the Handoff Pack builder shares the
//! same default-mode redaction; this module adds the JSON-tree walker.

use agent_session_grep_ports::{RedactionMode, RedactionState, RedactionStatus};

/// Ruleset version — shared with the ports engine (bump in `ports::redact`).
pub const RULESET_VERSION: &str = agent_session_grep_ports::redact::RULESET_VERSION;

/// Detect and redact secrets in a JSON value tree.
///
/// Returns the redacted value and a `RedactionStatus` summarizing what was
/// changed. The walk is recursive: every string leaf is checked against
/// high-confidence secret patterns. Unknown structure is left intact
/// (forward-compatible).
pub fn redact_value(value: serde_json::Value) -> (serde_json::Value, RedactionStatus) {
    redact_with_identity_paths(value, &[])
}

/// Redact a command's data projection, preserving only its typed identity leaves.
/// The command is supplied by dispatch, never inferred from untrusted JSON.
/// Opaque payloads and unknown commands retain the conservative default scan.
pub fn redact_command_data(
    command: &str,
    data: serde_json::Value,
) -> (serde_json::Value, RedactionStatus) {
    redact_with_identity_paths(data, command_identity_paths(command))
}

/// Redact an internally constructed MCP/HTTP success payload: data/page/warnings.
/// This is not a JSON-RPC frame or a Robot envelope; their correlation fields are
/// already handled outside this boundary. Return counts before callers add their
/// redaction metadata, and do not scan the returned payload a second time.
pub fn redact_command_response(
    command: &str,
    response: serde_json::Value,
) -> (serde_json::Value, RedactionStatus) {
    let mut paths: Vec<Vec<&str>> = command_identity_paths(command)
        .iter()
        .map(|path| {
            std::iter::once("data")
                .chain(path.iter().copied())
                .collect()
        })
        .collect();
    paths.push(vec!["page", "next_cursor"]);
    let paths: Vec<&[&str]> = paths.iter().map(Vec::as_slice).collect();
    redact_with_identity_paths(response, &paths)
}

/// Exact paths from `render`, ResumeMetadata, HandoffPack and the fixed
/// `tool_activities_for_messages` projection. `*` matches an array element only.
/// In particular, `session`, `entity`, every message `payload`, and unknown
/// tool fields are opaque JSON, not additional sources of trusted identities.
fn command_identity_paths(command: &str) -> &'static [&'static [&'static str]] {
    match command {
        "index" => &[&["id"]],
        "search" | "search_sessions" => &[&["hits", "*", "id"], &["hits", "*", "session_id"]],
        "list" | "list_sessions" => &[&["entries", "*", "id"]],
        "get-session-resume" | "get_session_resume" => &[&["session_id"], &["provider_session_id"]],
        "resume" => &[&["session_id"]],
        "get-message" | "get_message" => &[
            &["message_id"],
            &["session_id"],
            &["anchor_placement_id"],
            &["messages", "*", "id"],
            &["messages", "*", "placement_id"],
            &["messages", "*", "message_id"],
        ],
        "context" | "get_session_context" => &[
            &["session_id"],
            &["branch_leaf"],
            &["branch_leaf_placement_id"],
            &["messages", "*", "id"],
            &["messages", "*", "placement_id"],
            &["messages", "*", "message_id"],
            &["evidence", "*", "occurrence_id"],
            &["evidence", "*", "message_id"],
            &["evidence", "*", "source_document_id"],
            &["tool_activities", "*", "activity_id"],
            &["tool_activities", "*", "message_id"],
            &["talks", "*", "user_message", "id"],
            &["talks", "*", "user_message", "placement_id"],
            &["talks", "*", "user_message", "message_id"],
            &["talks", "*", "following_messages", "*", "id"],
            &["talks", "*", "following_messages", "*", "placement_id"],
            &["talks", "*", "following_messages", "*", "message_id"],
            &["summary", "first_user_message", "id"],
            &["summary", "first_user_message", "placement_id"],
            &["summary", "first_user_message", "message_id"],
            &["hint", "session_id"],
        ],
        "handoff" | "generate_handoff" => &[
            &["pack_id"],
            &["matched_sessions", "*", "session_id"],
            &["mainline", "*", "message_id"],
            &["mainline", "*", "session_id"],
            &["evidence", "*", "message_id"],
            &["evidence", "*", "session_id"],
            &["evidence", "*", "source_document_id"],
            &["provenance", "session_id"],
            &["tool_activity", "*", "activity_id"],
            &["tool_activity", "*", "message_id"],
            &["source_locators", "*", "source_document_id"],
            &["source_locators", "*", "cursor"],
            &["truncation", "dropped_locators", "*", "source_document_id"],
            &["truncation", "dropped_locators", "*", "cursor"],
            &["confidence", "per_session", "*", "session_id"],
        ],
        _ => &[],
    }
}

fn redact_with_identity_paths(
    value: serde_json::Value,
    identity_paths: &[&[&str]],
) -> (serde_json::Value, RedactionStatus) {
    let mut count = 0u64;
    let redacted = redact_value_inner(value, &mut count, identity_paths);
    let status = if count == 0 {
        RedactionStatus {
            mode: RedactionMode::Default,
            status: RedactionState::None,
            ruleset_version: RULESET_VERSION.to_string(),
            redacted_count: 0,
            audit_id: None,
        }
    } else {
        RedactionStatus {
            mode: RedactionMode::Default,
            status: RedactionState::Applied,
            ruleset_version: RULESET_VERSION.to_string(),
            redacted_count: count,
            audit_id: None,
        }
    };
    (redacted, status)
}

fn redact_value_inner(
    value: serde_json::Value,
    count: &mut u64,
    identity_paths: &[&[&str]],
) -> serde_json::Value {
    // A path exemption is scalar-only. A container in an identity position
    // must still be scanned, rather than trusting attacker-controlled children.
    if value.is_string() && identity_paths.iter().any(|path| path.is_empty()) {
        return value;
    }
    match value {
        serde_json::Value::String(s) => {
            if let Some(redacted) = agent_session_grep_ports::redact::redact_string(&s) {
                *count += 1;
                serde_json::Value::String(redacted)
            } else {
                serde_json::Value::String(s)
            }
        }
        serde_json::Value::Array(arr) => {
            let paths: Vec<&[&str]> = identity_paths
                .iter()
                .filter_map(|path| path.strip_prefix(&["*"]))
                .collect();
            serde_json::Value::Array(
                arr.into_iter()
                    .map(|v| redact_value_inner(v, count, &paths))
                    .collect(),
            )
        }
        serde_json::Value::Object(map) => {
            let mut new_map = serde_json::Map::with_capacity(map.len());
            for (key, val) in map {
                // Redact values; also check if the key itself signals a secret
                // (e.g. "api_key", "password", "token", "secret").
                let redacted_val = if is_secret_key(&key) {
                    redact_secret_value(val, count)
                } else {
                    let paths: Vec<&[&str]> = identity_paths
                        .iter()
                        .filter_map(|path| match path.split_first() {
                            Some((head, tail)) if *head != "*" && *head == key => Some(tail),
                            _ => None,
                        })
                        .collect();
                    redact_value_inner(val, count, &paths)
                };
                new_map.insert(key, redacted_val);
            }
            serde_json::Value::Object(new_map)
        }
        other => other,
    }
}

/// When the key name signals a secret, replace the entire value with a
/// redaction marker. Empty strings and null are left as-is (no secret to
/// leak), and so are numbers and booleans: a JSON number cannot carry a
/// secret shape, while several contract fields are numeric counters whose
/// names contain a secret-looking fragment (`max_tokens` / `used_tokens` in
/// `handoff-pack/v1`, whose published schema declares them as integers).
/// Blanking those would emit a string where the schema promises an integer
/// and would destroy the budget accounting the pack exists to report.
/// Arrays and objects still recurse, so a secret-named container is scanned
/// rather than trusted.
///
/// For non-empty string values, first try the value-pattern engine: if the
/// value matches a known secret shape, keep its specific marker (e.g.
/// `[redacted:api_key]`) so the audit type is preserved. Only fall back to the
/// generic `[redacted]` when the key name is the sole signal.
fn redact_secret_value(value: serde_json::Value, count: &mut u64) -> serde_json::Value {
    match &value {
        serde_json::Value::String(s) if s.is_empty() => value,
        serde_json::Value::Null => value,
        serde_json::Value::Number(_) | serde_json::Value::Bool(_) => value,
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
            redact_value_inner(value, count, &[])
        }
        serde_json::Value::String(s) => {
            if let Some(redacted) = agent_session_grep_ports::redact::redact_string(s) {
                *count += 1;
                serde_json::Value::String(redacted)
            } else {
                *count += 1;
                serde_json::Value::String("[redacted]".into())
            }
        }
    }
}

/// Check if a JSON key name indicates a secret field.
fn is_secret_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    // usage 维度的计量键（v15 usage_events 投影的稳定输出键）：名称含
    // "token" 片段但值是 SQL 求和的 u64 计数，绝不携带秘密——显式排除，
    // 否则 status 的用量汇总会被整值涂红（覆盖标记"真 0 vs 未知"也被抹掉）。
    // 排除只跳过"按键整值涂红"路径：值仍走 value-pattern 引擎，真正的秘密
    // 形态（如 sk-…）照常被识别。
    const USAGE_METRIC_KEYS: [&str; 5] = [
        "input_tokens",
        "output_tokens",
        "cache_read_tokens",
        "cache_write_tokens",
        "reasoning_tokens",
    ];
    if USAGE_METRIC_KEYS.contains(&lower.as_str()) {
        return false;
    }
    const SECRET_KEY_FRAGMENTS: &[&str] = &[
        "api_key",
        "apikey",
        "api-key",
        "secret",
        "password",
        "passwd",
        "token",
        "access_key",
        "accesskey",
        "private_key",
        "privatekey",
        "credential",
        "auth_token",
        "authorization",
        "bearer",
    ];
    if SECRET_KEY_FRAGMENTS.iter().any(|frag| lower.contains(frag)) {
        return true;
    }
    // Bare "...key" / "key..." names (e.g. STRIPE_RESTRICTED_KEY, signing_key,
    // encryption_key) carry secrets often enough to redact — but only when the
    // name is not explicitly public. Public-key-like fields are excluded so a
    // genuine public key is not silently blanked.
    const PUBLIC_KEY_HINTS: &[&str] = &[
        "public", "primary", "foreign", "unique", "count", "index", "name", "sort",
    ];
    let has_key = lower.ends_with("_key")
        || lower.starts_with("key_")
        || lower == "key"
        || lower.contains("_key_")
        || lower.contains("-key-");
    has_key && !PUBLIC_KEY_HINTS.iter().any(|hint| lower.contains(hint))
}

/// Redact a plain string (non-JSON) for warning/error channels.
///
/// Delegates to the shared engine in `ports::redact` so the Handoff Pack and
/// the envelope use identical patterns and ruleset version.
pub fn redact_text(s: &str) -> (String, RedactionStatus) {
    let (redacted, count) = agent_session_grep_ports::redact::redact_text(s);
    let status = if count == 0 {
        RedactionStatus {
            mode: RedactionMode::Default,
            status: RedactionState::None,
            ruleset_version: RULESET_VERSION.to_string(),
            redacted_count: 0,
            audit_id: None,
        }
    } else {
        RedactionStatus {
            mode: RedactionMode::Default,
            status: RedactionState::Applied,
            ruleset_version: RULESET_VERSION.to_string(),
            redacted_count: count,
            audit_id: None,
        }
    };
    (redacted, status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_profiles_preserve_only_typed_leaves_and_count_changes() {
        let secret = "sk_live_abcdef1234567890xyz";
        let id = format!("msg_v1_part:{secret}");
        let message = serde_json::json!({
            "id": id, "message_id": id, "placement_id": format!("plc_v1_part:{secret}"),
            "payload": {"id": id, "session_id": secret, "token": "opaque-secret"}
        });
        let data = serde_json::json!({
            "session_id": format!("ses_v1_part:{secret}"), "branch_leaf": id,
            "messages": [message.clone()],
            "talks": [{"user_message": message.clone(), "following_messages": [message.clone()]}],
            "summary": {"first_user_message": message},
            "tool_activities": [{"activity_id": id, "message_id": id, "target": secret,
                "payload": {"activity_id": id}}],
            "session": {"message_id": id}, "unknown": {"messages": [{"id": id}]}
        });
        for command in ["context", "get_session_context"] {
            let (redacted, status) = redact_command_data(command, data.clone());
            assert_eq!(redacted["branch_leaf"], id);
            for message in [
                &redacted["messages"][0],
                &redacted["talks"][0]["user_message"],
                &redacted["talks"][0]["following_messages"][0],
                &redacted["summary"]["first_user_message"],
            ] {
                assert_eq!(message["id"], id);
                assert_eq!(message["message_id"], id);
                assert!(!message["payload"].to_string().contains(secret));
                assert_eq!(message["payload"]["token"], "[redacted]");
            }
            assert_eq!(redacted["tool_activities"][0]["message_id"], id);
            assert_eq!(redacted["tool_activities"][0]["activity_id"], id);
            assert!(
                !redacted["tool_activities"][0]["payload"]
                    .to_string()
                    .contains(secret)
            );
            assert!(!redacted["session"].to_string().contains(secret));
            assert!(!redacted["unknown"].to_string().contains(secret));
            // Four opaque message copies (three leaves each), two tool leaves,
            // opaque session and unknown node; typed identity leaves add zero.
            assert_eq!(status.redacted_count, 16);
            assert_eq!(status.status, RedactionState::Applied);
        }
    }

    #[test]
    fn command_response_paths_preserve_cursor_without_trusting_nested_page_or_ids() {
        let secret = "sk_live_abcdef1234567890xyz";
        let id = format!("msg_v1_part:{secret}");
        let cursor = format!("cursor:{secret}");
        let response = serde_json::json!({
            "command": "search", "outcome": "success",
            "data": {"hits": [{"id": id, "session_id": id, "text": secret, "token": "opaque-secret"}],
                "page": {"next_cursor": cursor}},
            "page": {"next_cursor": cursor, "has_more": true, "unknown": {"id": secret}},
            "warnings": [secret], "id": secret
        });
        for command in ["search", "search_sessions"] {
            let (redacted, status) = redact_command_response(command, response.clone());
            assert_eq!(redacted["command"], "search");
            assert_eq!(redacted["data"]["hits"][0]["id"], id);
            assert_eq!(redacted["data"]["hits"][0]["session_id"], id);
            assert_eq!(redacted["page"]["next_cursor"], cursor);
            assert_eq!(redacted["page"]["has_more"], true);
            assert!(!redacted["data"]["page"].to_string().contains(secret));
            assert_eq!(redacted["data"]["hits"][0]["token"], "[redacted]");
            assert!(!redacted["warnings"].to_string().contains(secret));
            assert_eq!(status.redacted_count, 6);
        }
    }

    #[test]
    fn identity_exemptions_do_not_match_wrong_shapes_or_unknown_commands() {
        let secret = "sk_live_abcdef1234567890xyz";
        for value in [
            serde_json::json!({"hits": {"*": {"id": secret}}}),
            serde_json::json!({"hits": [[{"id": secret}]]}),
            serde_json::json!({"hits": [{"id": {"id": secret}}]}),
            serde_json::json!({"hits": [{"id": [secret]}]}),
            serde_json::json!({"unknown": {"hits": [{"id": secret}]}}),
        ] {
            let (redacted, status) = redact_command_data("search", value);
            assert!(!redacted.to_string().contains(secret), "{redacted}");
            assert_eq!(status.redacted_count, 1);
        }
        let data = serde_json::json!({"hits": [{"id": secret}], "token": "opaque-secret"});
        for command in ["unknown", "show", "get", "doctor", "get_status"] {
            let (redacted, status) = redact_command_data(command, data.clone());
            assert!(
                !redacted.to_string().contains(secret),
                "{command}: {redacted}"
            );
            assert_eq!(redacted["token"], "[redacted]");
            assert_eq!(status.redacted_count, 2);
        }
    }

    #[test]
    fn native_metadata_identity_is_not_a_global_key_exemption() {
        let secret = "sk_live_abcdef1234567890xyz";
        let data = serde_json::json!({
            "session_id": format!("ses_v1_part:{secret}"), "provider_session_id": secret,
            "original_working_directory": secret,
            "unknown": {"provider_session_id": secret}
        });
        for command in ["get-session-resume", "get_session_resume"] {
            let (redacted, status) = redact_command_data(command, data.clone());
            assert_eq!(redacted["provider_session_id"], secret);
            assert!(redacted["session_id"].as_str().unwrap().contains(secret));
            assert_eq!(
                redacted["original_working_directory"],
                "[redacted:stripe_key]"
            );
            assert!(!redacted["unknown"].to_string().contains(secret));
            assert_eq!(status.redacted_count, 2);
        }
        let (redacted, status) = redact_value(data);
        assert!(!redacted.to_string().contains(secret));
        assert_eq!(status.redacted_count, 4);
    }
    #[test]
    fn redacts_aws_access_key() {
        let val = serde_json::json!({"text": "AKIAIOSFODNN7EXAMPLE"});
        let (redacted, status) = redact_value(val);
        assert_eq!(redacted["text"], "[redacted:aws_access_key]");
        assert_eq!(status.redacted_count, 1);
        assert_eq!(status.status, RedactionState::Applied);
    }

    #[test]
    fn redacts_openai_key() {
        let val = serde_json::json!({"key": "sk-proj-abcdef1234567890"});
        let (redacted, status) = redact_value(val);
        assert_eq!(redacted["key"], "[redacted:api_key]");
        assert_eq!(status.redacted_count, 1);
    }

    #[test]
    fn redacts_anthropic_key() {
        let val = serde_json::json!({"text": "sk-ant-api03-1234567890abcdef"});
        let (redacted, status) = redact_value(val);
        assert_eq!(redacted["text"], "[redacted:api_key]");
        assert_eq!(status.redacted_count, 1);
    }

    #[test]
    fn redacts_github_pat() {
        let val = serde_json::json!({"token": "ghp_1234567890abcdefghijklmnopqrstuvwxyz"});
        let (redacted, status) = redact_value(val);
        // Key name "token" signals a secret; the value also matches the GitHub
        // PAT shape, so the specific marker is preserved rather than the bare
        // [redacted] fallback.
        assert_eq!(redacted["token"], "[redacted:github_token]");
        assert_eq!(status.redacted_count, 1);
    }

    #[test]
    fn redacts_bearer_token() {
        let val = serde_json::json!({"auth": "Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9"});
        let (redacted, status) = redact_value(val);
        assert_eq!(redacted["auth"], "[redacted:bearer_token]");
        assert_eq!(status.redacted_count, 1);
    }

    #[test]
    fn redacts_private_key() {
        let key =
            "-----BEGIN RSA PRIVATE KEY-----\nMIIEpAIIBAAKCAQEA...\n-----END RSA PRIVATE KEY-----";
        let val = serde_json::json!({"key": key});
        let (redacted, status) = redact_value(val);
        assert_eq!(redacted["key"], "[redacted:private_key]");
        assert_eq!(status.redacted_count, 1);
    }

    #[test]
    fn does_not_redact_normal_text() {
        let val = serde_json::json!({"text": "hello world", "count": 42});
        let (redacted, status) = redact_value(val);
        assert_eq!(redacted["text"], "hello world");
        assert_eq!(status.redacted_count, 0);
        assert_eq!(status.status, RedactionState::None);
    }

    #[test]
    fn does_not_redact_usage_metric_keys() {
        // usage 维度（v15）：input_tokens 等计量键的值是 SQL 求和计数，绝不
        // 按键涂红；但值层面的秘密形态仍被 value-pattern 引擎识别。
        let val = serde_json::json!({
            "usage": {
                "sessions": 2,
                "input_tokens": 108,
                "output_tokens": 53,
                "cache_read_tokens": 32,
                "cache_write_tokens": 20,
                "reasoning_tokens": 1,
                "observed_events": 1,
                "derived_events": 1,
            },
        });
        let (redacted, status) = redact_value(val);
        assert_eq!(redacted["usage"]["input_tokens"], 108);
        assert_eq!(redacted["usage"]["output_tokens"], 53);
        assert_eq!(redacted["usage"]["cache_read_tokens"], 32);
        assert_eq!(status.redacted_count, 0);
        assert_eq!(status.status, RedactionState::None);

        // 值层面：秘密形态照常红（排除只跳过"按键整值涂红"路径）。
        let val = serde_json::json!({"input_tokens": "sk-ant-api03-1234567890abcdef"});
        let (redacted, _) = redact_value(val);
        assert_eq!(redacted["input_tokens"], "[redacted:api_key]");

        // 相邻秘密键不受影响：api_token 仍按键整值涂红。
        let val = serde_json::json!({"api_token": "ghp_1234567890abcdef"});
        let (redacted, status) = redact_value(val);
        assert_eq!(redacted["api_token"], "[redacted]");
        assert_eq!(status.redacted_count, 1);
    }

    #[test]
    fn key_name_redaction_leaves_numeric_and_boolean_contract_fields_intact() {
        // 真实缺陷（dogfooding 在 17 万条语料上用 MCP generate_handoff 发现）：
        // handoff-pack/v1 的 `budget.max_tokens` / `used_tokens` 是整数预算计数，
        // 键名含 "token" 片段却被按键整值涂红成字符串 "[redacted]" ——
        // 既违反 `schemas/handoff/v1/pack.schema.json`（声明 integer），
        // 又抹掉了 pack 存在意义所在的预算账目。
        // 根因修法：键名涂红不作用于数字/布尔（数字不可能是秘密形态）。
        let val = serde_json::json!({
            "budget": {
                "max_tokens": 4000,
                "max_bytes": 6000,
                "used_tokens": 1234,
                "used_bytes": 4951,
                "max_evidence": 2,
            },
            "token_verified": false,
        });
        let (redacted, status) = redact_value(val);
        assert_eq!(redacted["budget"]["max_tokens"], 4000);
        assert_eq!(redacted["budget"]["used_tokens"], 1234);
        assert_eq!(redacted["budget"]["max_bytes"], 6000);
        assert_eq!(redacted["token_verified"], false);
        assert_eq!(status.redacted_count, 0);
        assert_eq!(status.status, RedactionState::None);

        // 秘密名下的字符串照常涂红，容器仍递归扫描（不因本修法被信任放行）。
        let val = serde_json::json!({
            "access_token": "ghp_1234567890abcdef",
            "secrets": {"nested_token": "ghp_abcdef1234567890", "retries": 3},
        });
        let (redacted, status) = redact_value(val);
        assert_eq!(redacted["access_token"], "[redacted]");
        assert_eq!(redacted["secrets"]["nested_token"], "[redacted]");
        assert_eq!(redacted["secrets"]["retries"], 3);
        assert_eq!(status.redacted_count, 2);
    }

    #[test]
    fn redacts_recursively_in_nested_arrays() {
        let val = serde_json::json!({
            "hits": [
                {"text": "sk-ant-api03-1234567890abcdef"},
                {"text": "normal message"}
            ]
        });
        let (redacted, status) = redact_value(val);
        assert_eq!(redacted["hits"][0]["text"], "[redacted:api_key]");
        assert_eq!(redacted["hits"][1]["text"], "normal message");
        assert_eq!(status.redacted_count, 1);
    }

    #[test]
    fn redacts_secret_key_names() {
        let val = serde_json::json!({
            "api_key": "any-value-here",
            "password": "hunter2",
            "normal_field": "keep me"
        });
        let (redacted, status) = redact_value(val);
        assert_eq!(redacted["api_key"], "[redacted]");
        assert_eq!(redacted["password"], "[redacted]");
        assert_eq!(redacted["normal_field"], "keep me");
        assert_eq!(status.redacted_count, 2);
    }

    #[test]
    fn short_strings_not_redacted() {
        let val = serde_json::json!({"text": "AKIA"});
        let (redacted, status) = redact_value(val);
        assert_eq!(redacted["text"], "AKIA");
        assert_eq!(status.redacted_count, 0);
    }

    #[test]
    fn redact_text_plain_string() {
        let (redacted, status) = redact_text("Bearer my-secret-token-here");
        assert_eq!(redacted, "[redacted:bearer_token]");
        assert_eq!(status.redacted_count, 1);

        let (redacted, status) = redact_text("normal warning text");
        assert_eq!(redacted, "normal warning text");
        assert_eq!(status.redacted_count, 0);
    }

    #[test]
    fn redacts_embedded_secret_in_prose() {
        let val = serde_json::json!({
            "text": "config with key AKIAIOSFODNN7EXAMPLE and token ghp_1234567890abcdefghijklmnopqrstuvwxyz trailing"
        });
        let (redacted, status) = redact_value(val);
        assert_eq!(
            redacted["text"],
            "config with key [redacted:aws_access_key] and token [redacted:github_token] trailing"
        );
        assert_eq!(status.redacted_count, 1); // one value, two spans
    }

    #[test]
    fn does_not_redact_embedded_like_prefixes() {
        // Prefix inside a longer identifier must not match; too-short spans must
        // not match either.
        let val = serde_json::json!({
            "a": "myAKIAIOSFODNN7EXAMPLE-suffix",
            "b": "token ghp_tooshort trailing",
            "c": "plain sk- text with nothing after"
        });
        let (redacted, status) = redact_value(val);
        assert_eq!(redacted["a"], "myAKIAIOSFODNN7EXAMPLE-suffix");
        assert_eq!(redacted["b"], "token ghp_tooshort trailing");
        assert_eq!(redacted["c"], "plain sk- text with nothing after");
        assert_eq!(status.redacted_count, 0);
    }

    #[test]
    fn redacts_embedded_bearer_token() {
        let val = serde_json::json!({
            "text": "Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9 with payload"
        });
        let (redacted, status) = redact_value(val);
        assert_eq!(
            redacted["text"],
            "Authorization: [redacted:bearer_token] with payload"
        );
        assert_eq!(status.redacted_count, 1);
    }

    #[test]
    fn empty_value_not_redacted() {
        let val = serde_json::json!({"api_key": ""});
        let (redacted, status) = redact_value(val);
        assert_eq!(redacted["api_key"], "");
        assert_eq!(status.redacted_count, 0);
    }

    #[test]
    fn redacts_bare_secret_key_named_value() {
        // A secret-shaped value under a non-"secret/token" key name is still
        // caught by the value pattern (H1 fix — Slack/Stripe/etc. prefixes).
        let val = serde_json::json!({"STRIPE_RESTRICTED_KEY": "rk_live_abcdef1234567890xyz"});
        let (redacted, status) = redact_value(val);
        assert_eq!(redacted["STRIPE_RESTRICTED_KEY"], "[redacted:stripe_key]");
        assert_eq!(status.redacted_count, 1);
    }

    #[test]
    fn redacts_non_secret_named_bare_key_value() {
        // A bare "key"-named field whose value does not match any value
        // pattern is still redacted by key name, but only for non-public keys.
        let val = serde_json::json!({"signing_key": "some-opaque-value-here"});
        let (redacted, status) = redact_value(val);
        assert_eq!(redacted["signing_key"], "[redacted]");
        assert_eq!(status.redacted_count, 1);
    }

    #[test]
    fn does_not_redact_public_key_named_fields() {
        let val = serde_json::json!({
            "public_key": "-----BEGIN PUBLIC KEY-----\n...\n-----END PUBLIC KEY-----",
            "primary_key": "id",
            "unique_key": "uuid"
        });
        let (redacted, status) = redact_value(val);
        assert_eq!(
            redacted["public_key"],
            "-----BEGIN PUBLIC KEY-----\n...\n-----END PUBLIC KEY-----"
        );
        assert_eq!(redacted["primary_key"], "id");
        assert_eq!(redacted["unique_key"], "uuid");
        assert_eq!(status.redacted_count, 0);
    }

    #[test]
    fn redacts_slack_token_value() {
        let val = serde_json::json!({"text": "xoxb-1234567890-abcdef"});
        let (redacted, status) = redact_value(val);
        assert_eq!(redacted["text"], "[redacted:slack_token]");
        assert_eq!(status.redacted_count, 1);
    }

    #[test]
    fn redacts_every_span_in_a_single_string_leaf() {
        // 真实泄漏（安全审计复现）：共享引擎曾在同一条字符串里替换满 16 段后
        // 停手，第 17 段起原样进 envelope，于是 Robot JSON / MCP
        // structuredContent / Web `/api/*` 三个机器边界同时漏出真实密钥。
        // 这里从 JSON 走树层再钉一次：单个 string leaf 的段数不设上限。
        let secrets: Vec<String> = (0..25).map(|index| format!("AKIA{index:016}X")).collect();
        let val = serde_json::json!({
            "hits": [{ "text": format!("dotenv dump {} tail", secrets.join(" ")) }]
        });
        let (redacted, status) = redact_value(val);
        let text = redacted["hits"][0]["text"].as_str().expect("text");
        for secret in &secrets {
            assert!(!text.contains(secret.as_str()), "`{secret}` 未脱敏：{text}");
        }
        assert_eq!(text.matches("[redacted:aws_access_key]").count(), 25);
        // 一条字符串 = 一个 redaction 条目（ADR-0009 字段口径），不是段数。
        assert_eq!(status.redacted_count, 1);
        assert_eq!(status.status, RedactionState::Applied);
    }

    #[test]
    fn redacts_bare_jwt_value() {
        let jwt = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c";
        let val = serde_json::json!({"auth": jwt});
        let (redacted, status) = redact_value(val);
        assert_eq!(redacted["auth"], "[redacted:jwt]");
        assert_eq!(status.redacted_count, 1);
    }
}
