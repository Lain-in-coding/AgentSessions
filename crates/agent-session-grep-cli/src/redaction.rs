//! Cross-boundary output redaction (ADR-0009).
//!
//! Secret detection + redaction for machine/cross-boundary outputs
//! (Robot JSON/JSONL, MCP, HTTP API, Handoff Pack, Web UI). Human CLI/TUI
//! output stays unredacted per ADR-0004.
//!
//! The redactor recursively walks a `serde_json::Value` tree and replaces
//! string values matching high-confidence secret patterns with a mode
//! reference. The redaction status is carried in the Robot envelope's
//! `redaction` field.

use agent_session_grep_ports::{RedactionMode, RedactionState, RedactionStatus};

/// Ruleset version — bumped when detection patterns change.
pub const RULESET_VERSION: &str = "v1.0";

/// Detect and redact secrets in a JSON value tree.
///
/// Returns the redacted value and a `RedactionStatus` summarizing what was
/// changed. The walk is recursive: every string leaf is checked against
/// high-confidence secret patterns. Unknown structure is left intact
/// (forward-compatible).
pub fn redact_value(value: serde_json::Value) -> (serde_json::Value, RedactionStatus) {
    let mut count = 0u64;
    let redacted = redact_value_inner(value, &mut count);
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

fn redact_value_inner(value: serde_json::Value, count: &mut u64) -> serde_json::Value {
    match value {
        serde_json::Value::String(s) => {
            if let Some(redacted) = redact_string(&s) {
                *count += 1;
                serde_json::Value::String(redacted)
            } else {
                serde_json::Value::String(s)
            }
        }
        serde_json::Value::Array(arr) => serde_json::Value::Array(
            arr.into_iter()
                .map(|v| redact_value_inner(v, count))
                .collect(),
        ),
        serde_json::Value::Object(map) => {
            let mut new_map = serde_json::Map::with_capacity(map.len());
            for (key, val) in map {
                // Redact values; also check if the key itself signals a secret
                // (e.g. "api_key", "password", "token", "secret").
                let redacted_val = if is_secret_key(&key) {
                    redact_secret_value(val, count)
                } else {
                    redact_value_inner(val, count)
                };
                new_map.insert(key, redacted_val);
            }
            serde_json::Value::Object(new_map)
        }
        other => other,
    }
}

/// When the key name signals a secret, replace the entire value with a
/// redaction marker (even if the value is non-string, e.g. a number or null).
/// Empty strings and null are left as-is (no secret to leak).
fn redact_secret_value(value: serde_json::Value, count: &mut u64) -> serde_json::Value {
    match &value {
        serde_json::Value::String(s) if s.is_empty() => value,
        serde_json::Value::Null => value,
        _ => {
            *count += 1;
            serde_json::Value::String("[redacted]".into())
        }
    }
}

/// Check if a JSON key name indicates a secret field.
fn is_secret_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
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
    SECRET_KEY_FRAGMENTS.iter().any(|frag| lower.contains(frag))
}

/// Check a string value for high-confidence secret patterns and return a
/// redacted replacement if matched.
///
/// Patterns are deliberately conservative: only high-confidence, structured
/// secret formats are matched to avoid false positives that would erode trust.
fn redact_string(s: &str) -> Option<String> {
    if s.is_empty() || s.len() < 8 {
        return None;
    }
    // AWS access key: AKIA + 16 uppercase alphanumeric
    if s.starts_with("AKIA") && s.len() >= 20 && s[..20].chars().all(|c| c.is_ascii_alphanumeric())
    {
        return Some("[redacted:aws_access_key]".to_string());
    }
    // AWS secret key: 40-char base64-ish (heuristic, only if it looks like a standalone token)
    if s.len() == 40
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=')
    {
        return Some("[redacted:aws_secret_key]".to_string());
    }
    // GitHub PAT: ghp_ / gho_ / ghs_ / ghu_ / gsr_ + 36 chars
    for prefix in &["ghp_", "gho_", "ghs_", "ghu_", "ghr_", "ghs_"] {
        if s.starts_with(prefix) && s.len() >= 40 {
            return Some("[redacted:github_token]".to_string());
        }
    }
    // Generic API key patterns: sk- (OpenAI), sk-ant- (Anthropic), xai- (xAI)
    for prefix in &["sk-ant-", "sk-", "xai-"] {
        if s.starts_with(prefix) && s.len() >= 20 {
            return Some("[redacted:api_key]".to_string());
        }
    }
    // Bearer token in a string value
    if s.starts_with("Bearer ") && s.len() > 10 {
        return Some("[redacted:bearer_token]".to_string());
    }
    // Private key header (PEM)
    if s.contains("-----BEGIN ") && s.contains("PRIVATE KEY-----") {
        return Some("[redacted:private_key]".to_string());
    }
    None
}

/// Redact a plain string (non-JSON) for warning/error channels.
pub fn redact_text(s: &str) -> (String, RedactionStatus) {
    let mut count = 0u64;
    let redacted = redact_string(s).unwrap_or_else(|| s.to_string());
    if redacted != s {
        count = 1;
    }
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
        assert_eq!(redacted["token"], "[redacted]");
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
    fn empty_value_not_redacted() {
        let val = serde_json::json!({"api_key": ""});
        let (redacted, status) = redact_value(val);
        assert_eq!(redacted["api_key"], "");
        assert_eq!(status.redacted_count, 0);
    }
}
