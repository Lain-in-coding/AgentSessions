//! Shared cross-boundary secret redaction (ADR-0009).
//!
//! High-confidence secret detection + redaction for plain strings. This is the
//! shared engine that both the CLI envelope redactor (`redact_value` JSON-tree
//! walker in the CLI crate) and the Handoff Pack builder delegate to, so a pack
//! always carries the same default-mode redaction as any other cross-boundary
//! output. Status projection (`RedactionMode`/`RedactionState`) stays in the
//! calling layer; this module only redacts text and reports span counts.

/// Ruleset version — bumped when detection patterns change.
pub const RULESET_VERSION: &str = "v1.1";

/// Detect and replace high-confidence secrets in a plain string.
///
/// Returns the redacted text and `1` when at least one secret span was
/// replaced, `0` otherwise. `redacted_count` is the number of redacted
/// entries, not the number of spans inside one entry (ADR-0009 field shape).
pub fn redact_text(s: &str) -> (String, u64) {
    match redact_string(s) {
        Some(redacted) => (redacted, 1),
        None => (s.to_string(), 0),
    }
}

/// Detect and replace high-confidence secrets in a plain string.
///
/// Returns `None` when nothing matched, so callers can distinguish "no secret"
/// from "redacted to an empty/placeholder value". Patterns are deliberately
/// conservative: only high-confidence, structured secret formats are matched to
/// avoid false positives that would erode trust. Matches both standalone
/// secrets (the whole string is a secret) and secrets embedded inside prose
/// ("the key is AKIA...") — cross-boundary output must not leak either form
/// (ADR-0009).
pub fn redact_string(s: &str) -> Option<String> {
    if s.is_empty() || s.len() < 8 {
        return None;
    }
    // Standalone (whole-string) match first: the value is exactly one secret.
    if let Some(marker) = standalone_secret(s) {
        return Some(marker.to_string());
    }
    // Embedded match: scan for each known secret shape inside the string and
    // replace matched spans with their redaction marker.
    let mut redacted = s.to_string();
    let mut count = 0u64;
    for shape in embedded_shapes() {
        replace_spans(&mut redacted, &mut count, |text| find_embedded(text, shape));
    }
    // The AWS secret key shape carries no prefix to anchor on, so it gets its
    // own boundary-anchored pass (the standalone rule only covers a value that
    // *is* the key; an embedded one must be redacted too).
    replace_spans(&mut redacted, &mut count, find_embedded_aws_secret);
    // JWTs carry no fixed prefix either: the `eyJ` anchor is the base64url
    // encoding of `{"`, so it gets its own boundary-anchored pass. Runs after
    // the prefix shapes so a "Bearer <jwt>" already replaced is not rescanned.
    replace_spans(&mut redacted, &mut count, find_embedded_jwt);
    if count == 0 { None } else { Some(redacted) }
}

/// Replace every span a finder reports, bounded to 16 spans per value.
///
/// The finder returns `(start, span_len, marker)` relative to the slice it was
/// given; scanning resumes after the inserted marker so a marker is never
/// re-scanned.
fn replace_spans(
    text: &mut String,
    count: &mut u64,
    finder: impl Fn(&str) -> Option<(usize, usize, &'static str)>,
) {
    let mut search_from = 0usize;
    while let Some((start, span_len, marker)) = finder(&text[search_from..]) {
        let abs_start = search_from + start;
        text.replace_range(abs_start..abs_start + span_len, marker);
        search_from = abs_start + marker.len();
        *count += 1;
        if *count >= 16 {
            break; // bounded: never rewrite more than 16 spans per value
        }
    }
}

/// Whole-string secret match: the value itself is a single secret token.
fn standalone_secret(s: &str) -> Option<&'static str> {
    // JWT: three dot-separated base64url segments whose header decodes to JSON
    // carrying an "alg" claim. Checked first so a bare JWT (no "Bearer "
    // prefix) is caught — agent transcripts often log raw access tokens.
    if is_jwt(s) {
        return Some("[redacted:jwt]");
    }
    // AWS access key: AKIA + 16 uppercase alphanumeric
    if s.starts_with("AKIA") && s.len() >= 20 && s[..20].chars().all(|c| c.is_ascii_alphanumeric())
    {
        return Some("[redacted:aws_access_key]");
    }
    // AWS secret key: 40-char base64-ish (heuristic, only if it looks like a standalone token)
    if s.len() == 40
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=')
    {
        return Some("[redacted:aws_secret_key]");
    }
    // GitHub tokens: fine-grained PAT (github_pat_) plus the classic
    // ghp_/gho_/ghs_/ghu_/ghr_ family. Mirrors `embedded_shapes`.
    for &(prefix, min_len) in &[
        ("github_pat_", 20),
        ("ghp_", 40),
        ("gho_", 40),
        ("ghs_", 40),
        ("ghu_", 40),
        ("ghr_", 40),
    ] {
        if s.starts_with(prefix) && s.len() >= min_len {
            return Some("[redacted:github_token]");
        }
    }
    // Slack tokens: xoxb- (bot), xoxp- (user), xoxa- (app), xoxe- (refresh)
    for prefix in &["xoxb-", "xoxp-", "xoxa-", "xoxe-"] {
        if s.starts_with(prefix) && s.len() >= 20 {
            return Some("[redacted:slack_token]");
        }
    }
    // Google API key: AIza + 35 chars
    if s.starts_with("AIza") && s.len() >= 39 {
        return Some("[redacted:google_api_key]");
    }
    // Stripe secret/restricted keys, before the generic sk- rule below so a
    // sk_live_ value is reported as a Stripe key rather than a generic one.
    for prefix in &["sk_live_", "sk_test_", "rk_live_", "rk_test_"] {
        if s.starts_with(prefix) && s.len() >= 20 {
            return Some("[redacted:stripe_key]");
        }
    }
    // GitLab PAT: glpat- + 20 = 26 chars
    if s.starts_with("glpat-") && s.len() >= 26 {
        return Some("[redacted:gitlab_token]");
    }
    // Generic API key patterns: sk- (OpenAI), sk-ant- (Anthropic), xai- (xAI)
    for prefix in &["sk-ant-", "sk-", "xai-"] {
        if s.starts_with(prefix) && s.len() >= 20 {
            return Some("[redacted:api_key]");
        }
    }
    // Bearer token in a string value
    if s.starts_with("Bearer ") && s.len() > 10 {
        return Some("[redacted:bearer_token]");
    }
    // Private key header (PEM)
    if s.contains("-----BEGIN ") && s.contains("PRIVATE KEY-----") {
        return Some("[redacted:private_key]");
    }
    None
}

/// Embedded secret shape: (prefix, minimum total length, redaction marker).
type SecretShape = (&'static str, usize, &'static str);

/// The known secret shapes, longest prefix first so more specific shapes
/// (sk-ant- before sk-) win the first-match.
fn embedded_shapes() -> Vec<SecretShape> {
    vec![
        // AWS access key: AKIA + 16 alphanumeric = 20 chars
        ("AKIA", 20, "[redacted:aws_access_key]"),
        // GitHub fine-grained PAT: github_pat_ + 9 = 20 chars minimum
        ("github_pat_", 20, "[redacted:github_token]"),
        // GitHub PAT: ghp_ + 36 = 40 chars
        ("ghp_", 40, "[redacted:github_token]"),
        ("gho_", 40, "[redacted:github_token]"),
        ("ghs_", 40, "[redacted:github_token]"),
        ("ghu_", 40, "[redacted:github_token]"),
        ("ghr_", 40, "[redacted:github_token]"),
        // Slack bot/user/app/refresh tokens
        ("xoxb-", 20, "[redacted:slack_token]"),
        ("xoxp-", 20, "[redacted:slack_token]"),
        ("xoxa-", 20, "[redacted:slack_token]"),
        ("xoxe-", 20, "[redacted:slack_token]"),
        // Google API key: AIza + 35 = 39 chars
        ("AIza", 39, "[redacted:google_api_key]"),
        // Stripe secret/restricted keys, before the generic sk- rule below so
        // an embedded sk_live_ value reports as a Stripe key.
        ("sk_live_", 20, "[redacted:stripe_key]"),
        ("sk_test_", 20, "[redacted:stripe_key]"),
        ("rk_live_", 20, "[redacted:stripe_key]"),
        ("rk_test_", 20, "[redacted:stripe_key]"),
        // GitLab PAT: glpat- + 20 = 26 chars
        ("glpat-", 26, "[redacted:gitlab_token]"),
        // Anthropic before generic sk-
        ("sk-ant-", 20, "[redacted:api_key]"),
        // OpenAI / xAI
        ("sk-", 20, "[redacted:api_key]"),
        ("xai-", 20, "[redacted:api_key]"),
        // Bearer tokens in prose
        ("Bearer ", 11, "[redacted:bearer_token]"),
    ]
}

/// Find one embedded secret span in `s` starting at the given offset.
///
/// Returns (start, end, marker) where end is the matched span length —
/// prefix + the trailing run of token characters (alphanumerics, '_', '-',
/// '+', '/', '='). The span is bounded to the shape's minimum length to
/// avoid over-consuming trailing prose.
fn find_embedded(s: &str, shape: SecretShape) -> Option<(usize, usize, &'static str)> {
    let (prefix, min_len, marker) = shape;
    let mut search = 0usize;
    while let Some(rel) = s[search..].find(prefix) {
        let start = search + rel;
        let after = start + prefix.len();
        // Require a non-alphanumeric boundary before the prefix (avoid
        // matching inside a longer identifier like "myAKIA...").
        let boundary_ok = start == 0
            || !s[..start]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
        if boundary_ok {
            let mut end = after;
            for c in s[after..].chars() {
                if c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '+' | '/' | '=') {
                    end += c.len_utf8();
                } else {
                    break;
                }
            }
            let span_len = end - start;
            if span_len >= min_len {
                return Some((start, span_len, marker));
            }
        }
        search = after;
    }
    None
}

/// Find one embedded AWS secret access key span in `s`.
///
/// An AWS secret key carries no prefix to anchor on — the shape is a 40-char
/// token from the base64 alphabet (`[A-Za-z0-9+/]`, never padded at that
/// length). The span is therefore anchored the other way round: a maximal run
/// of those characters, exactly 40 long, whose preceding character is a
/// boundary (not `_`/`-`, so a slice of a longer identifier never matches).
///
/// The run must additionally mix upper and lower case, which a random base64
/// secret always does and a 40-character hex digest (a git object id, quoted
/// constantly in real transcripts) never does.
fn find_embedded_aws_secret(s: &str) -> Option<(usize, usize, &'static str)> {
    const AWS_SECRET_LEN: usize = 40;
    let bytes = s.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        if !is_base64_token_byte(bytes[index]) {
            index += 1;
            continue;
        }
        let start = index;
        while index < bytes.len() && is_base64_token_byte(bytes[index]) {
            index += 1;
        }
        let span = &s[start..index];
        let boundary_ok = start == 0 || !matches!(bytes[start - 1], b'_' | b'-');
        if boundary_ok
            && span.len() == AWS_SECRET_LEN
            && span.bytes().any(|b| b.is_ascii_uppercase())
            && span.bytes().any(|b| b.is_ascii_lowercase())
        {
            return Some((start, AWS_SECRET_LEN, "[redacted:aws_secret_key]"));
        }
    }
    None
}

fn is_base64_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/')
}

/// Whether `s` is exactly one JWT: three dot-separated base64url segments
/// whose header decodes to JSON carrying an `alg` claim.
///
/// Requiring `alg` in the decoded header keeps a three-segment value that
/// merely happens to use base64url characters (e.g. an `a.b.c` version
/// string) from being flagged.
fn is_jwt(s: &str) -> bool {
    let mut parts = s.split('.');
    let (Some(header), Some(body), Some(sig), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    if header.is_empty() || body.is_empty() || sig.is_empty() {
        return false;
    }
    if ![header, body, sig]
        .iter()
        .all(|seg| seg.bytes().all(is_b64url_byte))
    {
        return false;
    }
    // A JWT header is base64url-encoded JSON, and `{"` encodes to `eyJ`, so
    // every real header starts with it. Cheap reject before the full decode.
    if !header.starts_with("eyJ") {
        return false;
    }
    base64url_decode(header)
        .and_then(|decoded| String::from_utf8(decoded).ok())
        .is_some_and(|json| json.contains("alg"))
}

/// Find one embedded JWT span, anchored on the `eyJ` header prefix.
///
/// Returns `(start, span_len, marker)` for [`replace_spans`]. A non-token
/// boundary is required before `eyJ` so a JWT-looking run inside a longer
/// identifier is not matched.
fn find_embedded_jwt(s: &str) -> Option<(usize, usize, &'static str)> {
    let bytes = s.as_bytes();
    let mut search = 0usize;
    while let Some(rel) = s[search..].find("eyJ") {
        let start = search + rel;
        let boundary_before = start == 0 || !is_b64url_byte(bytes[start - 1]);
        if boundary_before {
            // Consume exactly three dot-separated base64url runs.
            let mut end = 0usize;
            let mut dots = 0usize;
            for (idx, byte) in s[start..].bytes().enumerate() {
                if byte == b'.' {
                    if dots == 2 {
                        break;
                    }
                    dots += 1;
                    end = idx + 1;
                    continue;
                }
                if !is_b64url_byte(byte) {
                    break;
                }
                end = idx + 1;
            }
            if dots == 2 && is_jwt(&s[start..start + end]) {
                return Some((start, end, "[redacted:jwt]"));
            }
        }
        search = start + 3;
    }
    None
}

fn is_b64url_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')
}

/// Decode an unpadded base64url string. Returns `None` on any byte outside
/// the base64url alphabet.
fn base64url_decode(input: &str) -> Option<Vec<u8>> {
    let mut buf = 0u32;
    let mut bits = 0u32;
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    for byte in input.bytes() {
        let value = match byte {
            b'A'..=b'Z' => u32::from(byte - b'A'),
            b'a'..=b'z' => u32::from(byte - b'a') + 26,
            b'0'..=b'9' => u32::from(byte - b'0') + 52,
            b'-' => 62,
            b'_' => 63,
            _ => return None,
        };
        buf = (buf << 6) | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_aws_access_key() {
        let (redacted, count) = redact_text("AKIAIOSFODNN7EXAMPLE");
        assert_eq!(redacted, "[redacted:aws_access_key]");
        assert_eq!(count, 1);
    }

    #[test]
    fn redacts_openai_key() {
        let (redacted, count) = redact_text("sk-proj-abcdef1234567890");
        assert_eq!(redacted, "[redacted:api_key]");
        assert_eq!(count, 1);
    }

    #[test]
    fn redacts_anthropic_key() {
        let (redacted, count) = redact_text("sk-ant-api03-1234567890abcdef");
        assert_eq!(redacted, "[redacted:api_key]");
        assert_eq!(count, 1);
    }

    #[test]
    fn redacts_github_pat() {
        let (redacted, count) = redact_text("ghp_1234567890abcdefghijklmnopqrstuvwxyz");
        assert_eq!(redacted, "[redacted:github_token]");
        assert_eq!(count, 1);
    }

    #[test]
    fn redacts_bearer_token() {
        let (redacted, count) = redact_text("Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9");
        assert_eq!(redacted, "[redacted:bearer_token]");
        assert_eq!(count, 1);
    }

    #[test]
    fn redacts_private_key() {
        let key =
            "-----BEGIN RSA PRIVATE KEY-----\nMIIEpAIIBAAKCAQEA...\n-----END RSA PRIVATE KEY-----";
        let (redacted, count) = redact_text(key);
        assert_eq!(redacted, "[redacted:private_key]");
        assert_eq!(count, 1);
    }

    #[test]
    fn does_not_redact_normal_text() {
        let (redacted, count) = redact_text("hello world");
        assert_eq!(redacted, "hello world");
        assert_eq!(count, 0);
    }

    #[test]
    fn redacts_embedded_secret_in_prose() {
        let (redacted, count) = redact_text(
            "config with key AKIAIOSFODNN7EXAMPLE and token ghp_1234567890abcdefghijklmnopqrstuvwxyz trailing",
        );
        assert_eq!(
            redacted,
            "config with key [redacted:aws_access_key] and token [redacted:github_token] trailing"
        );
        assert_eq!(count, 1); // one entry, two spans
    }

    #[test]
    fn does_not_redact_embedded_like_prefixes() {
        assert_eq!(
            redact_text("myAKIAIOSFODNN7EXAMPLE-suffix").0,
            "myAKIAIOSFODNN7EXAMPLE-suffix"
        );
        assert_eq!(
            redact_text("token ghp_tooshort trailing").0,
            "token ghp_tooshort trailing"
        );
        assert_eq!(
            redact_text("plain sk- text with nothing after").0,
            "plain sk- text with nothing after"
        );
    }

    #[test]
    fn short_strings_not_redacted() {
        let (redacted, count) = redact_text("AKIA");
        assert_eq!(redacted, "AKIA");
        assert_eq!(count, 0);
    }
    #[test]
    fn redacts_github_fine_grained_pat() {
        let (redacted, count) = redact_text("github_pat_11ABCDEFG0abcdefghijklmnopqrstuvwxyz");
        assert_eq!(redacted, "[redacted:github_token]");
        assert_eq!(count, 1);
    }

    #[test]
    fn redacts_embedded_github_fine_grained_pat_in_prose() {
        let (redacted, count) =
            redact_text("set GITHUB_TOKEN=github_pat_11ABCDEFG0abcdefghijrstuvwxyz before running");
        assert_eq!(
            redacted,
            "set GITHUB_TOKEN=[redacted:github_token] before running"
        );
        assert_eq!(count, 1);
    }

    #[test]
    fn redacts_embedded_aws_secret_key_in_prose() {
        let (redacted, count) = redact_text(
            "aws_secret_access_key = wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY trailing",
        );
        assert_eq!(
            redacted,
            "aws_secret_access_key = [redacted:aws_secret_key] trailing"
        );
        assert_eq!(count, 1);
    }

    #[test]
    fn does_not_redact_forty_char_hex_git_sha() {
        let (redacted, count) =
            redact_text("commit 1234567890abcdef1234567890abcdef12345678 landed");
        assert_eq!(
            redacted,
            "commit 1234567890abcdef1234567890abcdef12345678 landed"
        );
        assert_eq!(count, 0);
    }

    #[test]
    fn redacts_slack_token() {
        let (redacted, count) = redact_text("xoxb-1234567890-abcdef");
        assert_eq!(redacted, "[redacted:slack_token]");
        assert_eq!(count, 1);
    }

    #[test]
    fn redacts_slack_token_embedded_in_prose() {
        let (redacted, count) = redact_text("my slack token is xoxb-1234567890-abcdef here");
        assert_eq!(redacted, "my slack token is [redacted:slack_token] here");
        assert_eq!(count, 1);
    }

    #[test]
    fn redacts_google_api_key() {
        // AIza + 35 chars
        let (redacted, count) = redact_text("AIzaSyA1234567890abcdefghijklmnopqrstuv");
        assert_eq!(redacted, "[redacted:google_api_key]");
        assert_eq!(count, 1);
    }

    #[test]
    fn redacts_stripe_restricted_key() {
        let (redacted, count) = redact_text("rk_live_abcdef1234567890xyz");
        assert_eq!(redacted, "[redacted:stripe_key]");
        assert_eq!(count, 1);
    }

    #[test]
    fn redacts_stripe_secret_key_before_generic_api_key() {
        // `sk_live_` must report as a Stripe key, not the generic `sk-` api_key
        // marker — the audit type has to survive the shared engine.
        let (redacted, count) = redact_text("sk_live_abcdef1234567890xyz");
        assert_eq!(redacted, "[redacted:stripe_key]");
        assert_eq!(count, 1);
    }

    #[test]
    fn redacts_gitlab_pat() {
        let (redacted, count) = redact_text("glpat-1234567890abcdefghij");
        assert_eq!(redacted, "[redacted:gitlab_token]");
        assert_eq!(count, 1);
    }

    #[test]
    fn redacts_bare_jwt_standalone() {
        let jwt = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c";
        let (redacted, count) = redact_text(jwt);
        assert_eq!(redacted, "[redacted:jwt]");
        assert_eq!(count, 1);
    }

    #[test]
    fn redacts_bare_jwt_embedded_in_prose() {
        let jwt = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c";
        let (redacted, count) = redact_text(&format!("token {jwt} trailing"));
        assert_eq!(redacted, "token [redacted:jwt] trailing");
        assert_eq!(count, 1);
    }

    #[test]
    fn does_not_redact_non_jwt_eyj_value() {
        // `eyJ` alone or a two-segment value is not a complete JWT.
        assert_eq!(redact_text("eyJ").0, "eyJ");
        assert_eq!(redact_text("eyJhbGc.eyJzdWI").0, "eyJhbGc.eyJzdWI");
    }

    #[test]
    fn does_not_redact_dotted_version_string_as_jwt() {
        // Three dot-separated base64url-legal runs that are not a JWT: the
        // header must decode to JSON carrying `alg`, so a version string is safe.
        assert_eq!(
            redact_text("release 1.20.3 shipped").0,
            "release 1.20.3 shipped"
        );
    }

    #[test]
    fn ruleset_version_is_v1_1() {
        assert_eq!(RULESET_VERSION, "v1.1");
    }
}
