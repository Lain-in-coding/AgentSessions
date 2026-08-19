//! Deterministic Markdown projection of a Handoff Pack v1.
//!
//! `schemas/handoff/v1/pack.schema.json` states the contract this module
//! implements: *"JSON is authoritative; the Markdown form is a deterministic
//! projection of this structure"*. The projection lives beside the generator
//! ([`crate::handoff_pack`]) rather than in an entry point so that CLI, MCP and
//! any future surface render the same bytes from the same pack — the CLI/Robot/
//! MCP contract forbids the surfaces diverging in meaning.
//!
//! # Determinism
//!
//! [`render`] is a pure function of the pack: no clock reads, no environment
//! reads, no hash-map iteration (every pack collection is a `Vec`, and
//! `tool_activity` objects are projected through a fixed key list rather than by
//! iterating the JSON object). Line endings are always `\n` and every number is
//! formatted with an explicit, locale-independent format string. The same pack
//! therefore always renders to the same bytes, which — since the pack itself is
//! byte-reproducible for a given generation/query/budget — makes the Markdown
//! byte-reproducible too.
//!
//! # Privacy
//!
//! The pack is cross-boundary output (ADR-0009): evidence text is already
//! redacted by the generator and `tool_activity[].target` has already been
//! reduced to a basename. This projection adds **no** field of its own and reads
//! **no** filesystem state, so it can only ever show a subset of what the JSON
//! shows. `tool_activity` is deliberately rendered through a fixed key list
//! (`name`/`kind`/`actor`/`status`/`target`/`message_id`) instead of dumping the
//! object: a field added to that projection in future cannot leak through
//! Markdown without someone editing this list.

use agent_session_grep_ports::handoff::{
    ConfidenceLevel, GenerationMode, HandoffPack, RedactionMode, RedactionState, TruncationReason,
};
use std::fmt::Write as _;

/// Render a handoff pack as Markdown. Pure and byte-deterministic (see module
/// docs); the JSON pack stays authoritative.
pub fn render(pack: &HandoffPack) -> String {
    let mut out = String::new();
    render_header(&mut out, pack);
    render_matched_sessions(&mut out, pack);
    render_mainline(&mut out, pack);
    render_evidence(&mut out, pack);
    render_tool_activity(&mut out, pack);
    render_inference(&mut out, pack);
    render_source_locators(&mut out, pack);
    out
}

fn render_header(out: &mut String, pack: &HandoffPack) {
    let _ = writeln!(out, "# Handoff pack {}", inline_code(&pack.pack_id));
    out.push('\n');
    let _ = writeln!(
        out,
        "- pack schema: handoff-pack/v1 (version {})",
        inline_code(&pack.schema_version)
    );
    let _ = writeln!(
        out,
        "- created_at: {} (derived from catalog generation {}, never read from the clock)",
        inline_code(&pack.created_at),
        pack.catalog_generation
    );
    let _ = writeln!(
        out,
        "- generation mode: {}",
        match pack.generation_mode {
            GenerationMode::Deterministic => "deterministic (no model was called)",
            GenerationMode::LocalLlm => "local_llm (a local model produced the inference section)",
        }
    );
    let _ = writeln!(
        out,
        "- retrieval mode: {}",
        inline_code(pack.query.retrieval_mode.as_str())
    );
    if pack.query.terms.is_empty() {
        let _ = writeln!(out, "- query: none");
    } else {
        let terms: Vec<String> = pack.query.terms.iter().map(|t| inline_code(t)).collect();
        let _ = writeln!(out, "- query: {}", terms.join(", "));
    }
    if let Some(filters) = &pack.query.filters {
        let mut parts: Vec<String> = Vec::new();
        if !filters.providers.is_empty() {
            let providers: Vec<String> = filters.providers.iter().map(|p| inline_code(p)).collect();
            parts.push(format!("providers {}", providers.join(", ")));
        }
        if let Some(since) = &filters.since {
            parts.push(format!("since {}", inline_code(since)));
        }
        if let Some(until) = &filters.until {
            parts.push(format!("until {}", inline_code(until)));
        }
        if !parts.is_empty() {
            let _ = writeln!(out, "- filters: {}", parts.join("; "));
        }
    }
    if let Some(provenance) = &pack.provenance {
        let session = provenance
            .session_id
            .as_deref()
            .map(inline_code)
            .unwrap_or_else(|| "unknown".to_string());
        let provider = provenance
            .provider_id
            .as_deref()
            .map(inline_code)
            .unwrap_or_else(|| "unattributed".to_string());
        let _ = writeln!(out, "- provenance: session {session}, provider {provider}");
    }
    let _ = writeln!(
        out,
        "- confidence: {}",
        confidence_text(pack.confidence.overall)
    );
    let _ = writeln!(
        out,
        "- redaction: {} ({} mode, ruleset {}, {} span(s) redacted)",
        match pack.redaction.status {
            RedactionState::Applied => "applied",
            RedactionState::Partial => "partial",
            RedactionState::None => "nothing matched",
        },
        match pack.redaction.mode {
            RedactionMode::Default => "default",
            RedactionMode::Revealed => "revealed",
        },
        inline_code(&pack.redaction.ruleset_version),
        pack.redaction.redacted_count
    );
    let _ = writeln!(
        out,
        "- budget: {}/{} tokens, {}/{} bytes",
        pack.budget.used_tokens,
        pack.budget.max_tokens,
        pack.budget.used_bytes,
        pack.budget.max_bytes
    );
    if pack.truncation.truncated {
        let _ = writeln!(
            out,
            "- truncation: truncated ({}), {} entr(y/ies) dropped",
            truncation_reason_text(pack.truncation.reason),
            pack.truncation.dropped_count
        );
    } else {
        let _ = writeln!(out, "- truncation: none");
    }
    out.push('\n');
}

fn render_matched_sessions(out: &mut String, pack: &HandoffPack) {
    out.push_str("## Matched sessions\n\n");
    if pack.matched_sessions.is_empty() {
        out.push_str("No session matched this query.\n\n");
        return;
    }
    out.push_str("| session | provider | occurrences | score |\n");
    out.push_str("| --- | --- | --- | --- |\n");
    for session in &pack.matched_sessions {
        let _ = writeln!(
            out,
            "| {} | {} | {} | {:.4} |",
            cell(&session.session_id),
            session
                .provider_id
                .as_deref()
                .map(cell)
                .unwrap_or_else(|| "—".to_string()),
            session.occurrences,
            session.relevance_score
        );
    }
    out.push_str("\nScores are backend-relative (BM25 for the lexical path): comparable within\n");
    out.push_str("this pack only, not across queries or against a fixed threshold.\n\n");
}

fn render_mainline(out: &mut String, pack: &HandoffPack) {
    out.push_str("## Mainline\n\n");
    if pack.mainline.is_empty() {
        out.push_str("No mainline entry survived the budget.\n\n");
        return;
    }
    for entry in &pack.mainline {
        let sidechain = if entry.is_sidechain {
            " (subagent)"
        } else {
            ""
        };
        let preview = entry
            .text_preview
            .as_deref()
            .map(one_line)
            .unwrap_or_default();
        let _ = writeln!(
            out,
            "{}. **{}**{} {} — {}",
            entry.ordinal,
            escape_text(&entry.role),
            sidechain,
            inline_code(&entry.message_id),
            escape_text(&preview)
        );
    }
    out.push('\n');
}

fn render_evidence(out: &mut String, pack: &HandoffPack) {
    out.push_str("## Evidence\n\n");
    out.push_str("Verbatim message text, redacted per ADR-0009. Nothing here is inferred.\n\n");
    if pack.evidence.is_empty() {
        out.push_str("No evidence span survived the budget.\n\n");
        return;
    }
    for entry in &pack.evidence {
        let _ = writeln!(out, "### {}", inline_code(&entry.message_id));
        out.push('\n');
        let _ = writeln!(
            out,
            "- source document: {}",
            inline_code(&entry.source_document_id)
        );
        if let Some(session) = &entry.session_id {
            let _ = writeln!(out, "- session: {}", inline_code(session));
        }
        if let (Some(start), Some(end)) = (entry.span_start, entry.span_end) {
            let _ = writeln!(out, "- source bytes: {start}..{end}");
        }
        out.push('\n');
        push_fenced_block(out, &entry.text);
        out.push('\n');
    }
}

fn render_tool_activity(out: &mut String, pack: &HandoffPack) {
    if pack.tool_activity.is_empty() {
        return;
    }
    out.push_str("## Tool activity\n\n");
    out.push_str(
        "Targets are reduced to a basename: the pack never carries a directory chain.\n\n",
    );
    out.push_str("| tool | kind | actor | status | target | message |\n");
    out.push_str("| --- | --- | --- | --- | --- | --- |\n");
    for activity in &pack.tool_activity {
        // Fixed key list, not object iteration: a new field cannot leak through
        // Markdown without an edit here (see module docs).
        let _ = writeln!(
            out,
            "| {} | {} | {} | {} | {} | {} |",
            activity_field(activity, "name"),
            activity_field(activity, "kind"),
            activity_field(activity, "actor"),
            activity_field(activity, "status"),
            activity_field(activity, "target"),
            activity_field(activity, "message_id")
        );
    }
    out.push('\n');
}

fn render_inference(out: &mut String, pack: &HandoffPack) {
    out.push_str("## Inference\n\n");
    if pack.inference.is_empty() {
        out.push_str(
            "None. This pack was generated deterministically, so no model was called and there\n\
             is nothing to infer — an empty section is the truthful value, not a missing summary.\n\n",
        );
        return;
    }
    for entry in &pack.inference {
        let _ = writeln!(
            out,
            "- **{}** (inferred, source {}{}): {}",
            format_args!("{:?}", entry.kind).to_string().to_lowercase(),
            format_args!("{:?}", entry.source)
                .to_string()
                .to_lowercase(),
            entry
                .model_id
                .as_deref()
                .map(|m| format!(", model {}", inline_code(m)))
                .unwrap_or_default(),
            escape_text(&one_line(&entry.text))
        );
    }
    out.push('\n');
}

fn render_source_locators(out: &mut String, pack: &HandoffPack) {
    if pack.source_locators.is_empty() {
        return;
    }
    out.push_str("## Source locators\n\n");
    out.push_str("Opaque back-tracing references. Feed a cursor to `asg get-message`.\n\n");
    for locator in &pack.source_locators {
        match &locator.cursor {
            Some(cursor) => {
                let _ = writeln!(
                    out,
                    "- {} @ {}",
                    inline_code(&locator.source_document_id),
                    inline_code(cursor)
                );
            }
            None => {
                let _ = writeln!(out, "- {}", inline_code(&locator.source_document_id));
            }
        }
    }
    out.push('\n');
}

fn confidence_text(level: ConfidenceLevel) -> &'static str {
    match level {
        ConfidenceLevel::High => "high",
        ConfidenceLevel::Medium => "medium",
        ConfidenceLevel::Low => "low",
    }
}

fn truncation_reason_text(reason: TruncationReason) -> &'static str {
    match reason {
        TruncationReason::BudgetExceeded => "budget_exceeded",
        TruncationReason::MaxItems => "max_items",
        TruncationReason::MaxEvidence => "max_evidence",
        TruncationReason::MaxBytes => "max_bytes",
        TruncationReason::None => "none",
    }
}

/// One `tool_activity` field as a table cell, or an em dash when absent or not a
/// string. Never renders a nested object: those are not part of the projection.
fn activity_field(activity: &serde_json::Value, key: &str) -> String {
    activity
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(cell)
        .unwrap_or_else(|| "—".to_string())
}

/// Inline code span that survives backticks in the content: the delimiter is one
/// backtick longer than the longest run inside, and content touching a backtick
/// gets the space padding CommonMark requires.
fn inline_code(text: &str) -> String {
    let flat = one_line(text);
    let fence = "`".repeat(longest_backtick_run(&flat) + 1);
    let pad = if flat.starts_with('`') || flat.ends_with('`') {
        " "
    } else {
        ""
    };
    format!("{fence}{pad}{flat}{pad}{fence}")
}

/// Fenced block whose fence is always longer than any backtick run inside, so
/// evidence text containing a fence cannot break out of the block.
fn push_fenced_block(out: &mut String, text: &str) {
    let fence = "`".repeat(longest_backtick_run(text).max(2) + 1);
    out.push_str(&fence);
    out.push('\n');
    out.push_str(text);
    if !text.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&fence);
    out.push('\n');
}

fn longest_backtick_run(text: &str) -> usize {
    let mut longest = 0usize;
    let mut current = 0usize;
    for c in text.chars() {
        if c == '`' {
            current += 1;
            longest = longest.max(current);
        } else {
            current = 0;
        }
    }
    longest
}

/// Collapse every run of ASCII whitespace (including newlines) into one space so
/// a value can sit inside a table cell or a list item.
fn one_line(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_space = false;
    for c in text.chars() {
        if c.is_whitespace() {
            in_space = true;
        } else {
            if in_space && !out.is_empty() {
                out.push(' ');
            }
            in_space = false;
            out.push(c);
        }
    }
    out
}

/// Escape the Markdown-significant characters that can appear in prose emitted
/// outside a code span (role names, previews).
fn escape_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '\\' | '`' | '*' | '_' | '[' | ']' | '|' | '<' | '>') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Table-cell text: single line, pipes escaped so a value cannot forge a column.
fn cell(text: &str) -> String {
    escape_text(&one_line(text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_session_grep_ports::handoff::{
        EvidenceEntry, HandoffBudget, HandoffFilters, HandoffQuery, MainlineEntry, MatchedSession,
        PackConfidence, Provenance, RedactionStatus, RetrievalMode, SessionConfidence,
        SourceLocator, TruncationStatus,
    };

    /// A pack that exercises every risky field: a provider-attributed session,
    /// evidence text with a Markdown fence in it, tool activity for all three
    /// path shapes, and a source locator.
    fn fixture_pack() -> HandoffPack {
        HandoffPack {
            schema_version: "1.0".into(),
            pack_id: "pack_v1_0123456789abcdef".into(),
            catalog_generation: 7,
            generation_mode: GenerationMode::Deterministic,
            query: HandoffQuery {
                terms: vec!["backup config".into()],
                retrieval_mode: RetrievalMode::Lexical,
                filters: Some(HandoffFilters {
                    providers: vec!["claude-code".into()],
                    since: Some("1786838400.000000000Z".into()),
                    until: None,
                }),
            },
            created_at: "2026-08-16T00:00:07Z".into(),
            matched_sessions: vec![MatchedSession {
                session_id: "ses_v1_abc".into(),
                provider_id: Some("claude-code".into()),
                title: None,
                relevance_score: 1.25,
                occurrences: 2,
            }],
            mainline: vec![MainlineEntry {
                message_id: "msg_v1_aaa".into(),
                session_id: Some("ses_v1_abc".into()),
                role: "user".into(),
                ordinal: 0,
                text_preview: Some("first\nline and second".into()),
                is_sidechain: false,
            }],
            evidence: vec![EvidenceEntry {
                message_id: "msg_v1_aaa".into(),
                source_document_id: "doc_v1_zzz".into(),
                session_id: Some("ses_v1_abc".into()),
                span_start: Some(0),
                span_end: Some(42),
                text: "before\n```\nfenced\n```\nafter".into(),
            }],
            inference: Vec::new(),
            target: None,
            time_window: None,
            provenance: Some(Provenance {
                provider_id: Some("claude-code".into()),
                session_id: Some("ses_v1_abc".into()),
            }),
            budget: HandoffBudget {
                max_tokens: 8000,
                max_bytes: 2_000_000,
                used_tokens: 12,
                used_bytes: 900,
                max_evidence: Some(20),
                context_lines: None,
            },
            truncation: TruncationStatus {
                truncated: false,
                reason: TruncationReason::None,
                dropped_count: 0,
                dropped_locators: Vec::new(),
            },
            redaction: RedactionStatus {
                mode: RedactionMode::Default,
                status: RedactionState::Applied,
                ruleset_version: "v1".into(),
                redacted_count: 1,
                audit_id: None,
            },
            confidence: PackConfidence {
                overall: ConfidenceLevel::High,
                per_session: vec![SessionConfidence {
                    session_id: "ses_v1_abc".into(),
                    confidence: ConfidenceLevel::High,
                }],
            },
            tool_activity: vec![
                serde_json::json!({
                    "activity_id": "act-1", "message_id": "msg_v1_aaa", "kind": "file",
                    "actor": "assistant", "name": "Read", "status": "success",
                    "target": "README.md",
                }),
                serde_json::json!({
                    "activity_id": "act-2", "message_id": "msg_v1_aaa", "kind": "command",
                    "actor": "assistant", "name": "Bash", "status": "success",
                    "target": "cargo test --workspace",
                }),
            ],
            source_locators: vec![SourceLocator {
                source_document_id: "doc_v1_zzz".into(),
                cursor: Some("msg_v1_aaa".into()),
            }],
        }
    }

    /// The determinism contract: the same pack renders to the same bytes.
    #[test]
    fn rendering_twice_produces_identical_bytes() {
        let pack = fixture_pack();
        let first = render(&pack);
        let second = render(&pack);
        assert_eq!(first.as_bytes(), second.as_bytes());
        // And a separately constructed but equal pack renders identically too —
        // nothing is carried over from the first render.
        assert_eq!(render(&fixture_pack()).as_bytes(), first.as_bytes());
    }

    /// Line endings are `\n` on every platform: a CRLF would make the byte
    /// comparison above platform-dependent.
    #[test]
    fn rendering_uses_lf_line_endings_only() {
        let markdown = render(&fixture_pack());
        assert!(!markdown.contains('\r'), "{markdown}");
    }

    #[test]
    fn header_carries_the_packs_own_identity_and_time() {
        let markdown = render(&fixture_pack());
        assert!(markdown.contains("pack_v1_0123456789abcdef"), "{markdown}");
        assert!(markdown.contains("2026-08-16T00:00:07Z"), "{markdown}");
        assert!(markdown.contains("catalog generation 7"), "{markdown}");
        assert!(markdown.contains("deterministic"), "{markdown}");
    }

    #[test]
    fn evidence_text_is_rendered_verbatim_inside_a_longer_fence() {
        let markdown = render(&fixture_pack());
        // The evidence contains a ``` fence; the block must open with a longer
        // fence so the text cannot break out.
        assert!(
            markdown.contains("````\nbefore\n```\nfenced\n```\nafter\n````"),
            "{markdown}"
        );
    }

    #[test]
    fn empty_inference_is_explained_rather_than_left_blank() {
        let markdown = render(&fixture_pack());
        assert!(markdown.contains("## Inference"), "{markdown}");
        assert!(markdown.contains("no model was called"), "{markdown}");
    }

    #[test]
    fn empty_pack_still_renders_every_section_honestly() {
        let mut pack = fixture_pack();
        pack.matched_sessions.clear();
        pack.mainline.clear();
        pack.evidence.clear();
        pack.tool_activity.clear();
        pack.source_locators.clear();
        pack.query.terms.clear();
        pack.provenance = None;
        pack.query.filters = None;
        let markdown = render(&pack);
        assert!(
            markdown.contains("No session matched this query."),
            "{markdown}"
        );
        assert!(
            markdown.contains("No evidence span survived the budget."),
            "{markdown}"
        );
        assert!(markdown.contains("- query: none"), "{markdown}");
        // Absent sections are omitted rather than printed empty.
        assert!(!markdown.contains("## Tool activity"), "{markdown}");
        assert!(!markdown.contains("## Source locators"), "{markdown}");
    }

    #[test]
    fn truncation_is_reported_with_its_reason() {
        let mut pack = fixture_pack();
        pack.truncation = TruncationStatus {
            truncated: true,
            reason: TruncationReason::MaxBytes,
            dropped_count: 3,
            dropped_locators: Vec::new(),
        };
        let markdown = render(&pack);
        assert!(
            markdown.contains("truncated (max_bytes), 3 entr"),
            "{markdown}"
        );
    }

    #[test]
    fn tool_activity_renders_only_the_fixed_key_list() {
        let mut pack = fixture_pack();
        pack.tool_activity = vec![serde_json::json!({
            "activity_id": "act-1",
            "message_id": "msg_v1_aaa",
            "kind": "file",
            "actor": "assistant",
            "name": "Read",
            "status": "success",
            "target": "README.md",
            // A field a later store version might add. It must not appear.
            "raw_input_path": "/placeholder/home/user/project/README.md",
        })];
        let markdown = render(&pack);
        assert!(
            markdown.contains("| Read | file | assistant | success | README.md |"),
            "{markdown}"
        );
        assert!(!markdown.contains("raw_input_path"), "{markdown}");
        assert!(!markdown.contains("placeholder"), "{markdown}");
    }

    #[test]
    fn table_cells_cannot_forge_a_column() {
        let mut pack = fixture_pack();
        pack.matched_sessions[0].provider_id = Some("evil | injected".into());
        let markdown = render(&pack);
        assert!(markdown.contains(r"evil \| injected"), "{markdown}");
    }

    #[test]
    fn mainline_preview_is_collapsed_to_one_line() {
        let markdown = render(&fixture_pack());
        assert!(markdown.contains("first line and second"), "{markdown}");
    }

    #[test]
    fn inline_code_survives_backticks_in_the_value() {
        assert_eq!(inline_code("plain"), "`plain`");
        assert_eq!(inline_code("a`b"), "``a`b``");
        assert_eq!(inline_code("`lead"), "`` `lead ``");
    }

    #[test]
    fn one_line_collapses_every_whitespace_run() {
        assert_eq!(one_line("  a \r\n\t b  "), "a b");
        assert_eq!(one_line(""), "");
    }
}
