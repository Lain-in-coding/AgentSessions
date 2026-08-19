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
            if let Some(redacted) = agent_session_grep_ports::redact::redact_string(&s) {
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
///
/// Matching is by **key segment**, not substring. A substring rule fired on our
/// own protocol: `max_tokens` and `used_tokens` are declared integers in the
/// handoff pack budget, and `lower.contains("token")` rewrote both to the string
/// `"[redacted]"` — a published-schema violation and a corrupted budget in every
/// machine consumer, with no secret involved.
///
/// Two distinctions make the narrower rule principled rather than a patch for
/// one field. First, a credential is named in the singular (`token`,
/// `access_token`); the plural `tokens` is a count. Second, a `token` segment
/// paired with a counting word (`token_count`, `tokenCount`) is also a count.
/// Everything that actually names a credential still matches — see
/// `credential_key_names_still_redact_across_separator_styles`.
fn is_secret_key(key: &str) -> bool {
    // Single segments that name a credential on their own.
    const SECRET_TERMS: &[&str] = &[
        "secret",
        "password",
        "passwd",
        "token",
        "credential",
        "credentials",
        "authorization",
        "bearer",
        "apikey",
    ];
    // Adjacent segment pairs that name a credential together (`api_key`,
    // `apiKey`, `access-key`, …). Kept separate from the single-segment list so
    // a bare `key` — common and rarely a secret on its own — does not match.
    const SECRET_PAIRS: &[&str] = &[
        "apikey",
        "accesskey",
        "privatekey",
        "secretkey",
        "authtoken",
        "apitoken",
        "accesstoken",
        "refreshtoken",
        "sessiontoken",
    ];
    // Words that turn an adjacent credential noun into a quantity. `used_tokens`
    // is caught by the plural rule; `token_count` needs this one.
    const COUNTING_WORDS: &[&str] = &["count", "counts", "total", "totals", "limit", "limits"];

    let segments = key_segments(key);
    let counted = segments
        .iter()
        .any(|s| COUNTING_WORDS.contains(&s.as_str()));
    if !counted && segments.iter().any(|s| SECRET_TERMS.contains(&s.as_str())) {
        return true;
    }
    segments.windows(2).any(|pair| {
        let joined = format!("{}{}", pair[0], pair[1]);
        SECRET_PAIRS.contains(&joined.as_str())
    })
}

/// Split a JSON key into lowercase alphanumeric segments, breaking on both
/// punctuation (`api_key`, `api-key`) and camelCase boundaries (`apiKey`).
fn key_segments(key: &str) -> Vec<String> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut prev_lower = false;
    for ch in key.chars() {
        if !ch.is_alphanumeric() {
            if !current.is_empty() {
                segments.push(std::mem::take(&mut current));
            }
            prev_lower = false;
            continue;
        }
        // camelCase boundary: a lowercase/digit run followed by an uppercase.
        if ch.is_uppercase() && prev_lower && !current.is_empty() {
            segments.push(std::mem::take(&mut current));
        }
        prev_lower = ch.is_lowercase() || ch.is_numeric();
        current.extend(ch.to_lowercase());
    }
    if !current.is_empty() {
        segments.push(current);
    }
    segments
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
    fn token_count_fields_are_not_treated_as_secrets() {
        // 回归：`lower.contains("token")` 把 handoff budget 里声明为 integer 的
        // `max_tokens`/`used_tokens` 改写成字符串 `"[redacted]"` —— 既违反已发布
        // schema，也让每个机器消费者读到损坏的预算，而且根本没有秘密涉及。
        // 复数 `tokens` 是计数，单数 `token` 才是凭据名。
        let val = serde_json::json!({
            "budget": {"max_tokens": 8000, "used_tokens": 123, "max_bytes": 2_000_000},
            "used_token_count": 7,
        });
        let (redacted, status) = redact_value(val);
        assert_eq!(redacted["budget"]["max_tokens"], 8000);
        assert_eq!(redacted["budget"]["used_tokens"], 123);
        assert_eq!(redacted["used_token_count"], 7);
        assert_eq!(status.redacted_count, 0, "计数字段不得触发脱敏");
    }

    #[test]
    fn credential_key_names_still_redact_across_separator_styles() {
        // 收窄成按段匹配后，凭据字段仍必须全部命中——包括 camelCase。
        for key in [
            "token",
            "auth_token",
            "authToken",
            "access_token",
            "refreshToken",
            "api_key",
            "apiKey",
            "api-key",
            "access_key",
            "private_key",
            "privateKey",
            "secret",
            "client_secret",
            "password",
            "passwd",
            "credential",
            "credentials",
            "authorization",
            "bearer",
        ] {
            let val = serde_json::json!({ key: "any-value-here" });
            let (redacted, status) = redact_value(val);
            assert_eq!(redacted[key], "[redacted]", "must redact key {key}");
            assert_eq!(status.redacted_count, 1, "must count once for {key}");
        }
    }

    #[test]
    fn ordinary_keys_are_not_redacted_by_name() {
        // 按段匹配的另一半：普通字段不得因为"含有"某个片段而被误伤。
        // `key` 单独出现太常见（`session_key`/`sort_key`/`key` 本身），
        // 只在与 api/access/private/secret 相邻时才算凭据。
        for key in [
            "max_tokens",
            "used_tokens",
            "token_count",
            "tokens",
            "key",
            "sort_key",
            "keyword",
            "keywords",
            "secretary_note",
            "authorized_users",
            "bearing",
        ] {
            let val = serde_json::json!({ key: "plain value" });
            let (redacted, status) = redact_value(val);
            assert_eq!(redacted[key], "plain value", "must not redact key {key}");
            assert_eq!(status.redacted_count, 0, "must not count for {key}");
        }
    }

    #[test]
    fn key_segments_splits_on_punctuation_and_camel_case() {
        assert_eq!(key_segments("max_tokens"), vec!["max", "tokens"]);
        assert_eq!(key_segments("apiKey"), vec!["api", "key"]);
        assert_eq!(key_segments("api-key"), vec!["api", "key"]);
        assert_eq!(key_segments("HTTPToken"), vec!["httptoken"]);
        assert_eq!(key_segments("refreshToken2"), vec!["refresh", "token2"]);
    }
}
