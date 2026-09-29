use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::{Comparison, Input, Rendered, cases::synthetic_cases, compare, serialize_text};

fn exact_source_slice(input: &Input, output: &Rendered) -> bool {
    let range = output.source_range;
    let chars: Vec<_> = input.text.chars().collect();
    range.start <= range.end
        && range.end <= chars.len()
        && chars[range.start..range.end].iter().collect::<String>() == output.text
        && output.scalar_count == output.text.chars().count()
}

fn exact_wire(output: &Rendered) -> bool {
    serde_json::from_str::<Value>(&output.wire_json)
        .is_ok_and(|wire| wire == json!({"text": output.text}))
        && output.wire_json.len() == output.json_bytes
}

fn visible_spans(input: &Input, comparison: &Comparison, output: &Rendered) -> bool {
    let source: Vec<_> = input.text.chars().collect();
    let expected: Vec<_> = comparison
        .source_matches
        .iter()
        .filter(|hit| {
            output.source_range.start <= hit.range.start && hit.range.end <= output.source_range.end
        })
        .cloned()
        .collect();
    output.visible_source_matches == expected
        && output.visible_source_matches.iter().all(|hit| {
            hit.term_index < input.terms.len()
                && hit.range.start < hit.range.end
                && hit.range.end <= source.len()
        })
}

pub fn build_report() -> serde_json::Result<Value> {
    let cases = synthetic_cases();
    let fixture_bytes = serde_json::to_vec(&serde_json::to_value(&cases)?)?;
    let dataset_hash = format!("{:x}", Sha256::digest(&fixture_bytes));
    let json_floor = serialize_text("")?.len();
    let mut results = Vec::new();
    let mut passed = 0;
    let mut prefix_visible_cases = 0;
    let mut candidate_visible_cases = 0;
    let mut raw_prefix_over_byte_budget = 0;
    for case in cases {
        let actual = compare(&case.input)?;
        let bounded = actual.bounded_prefix.as_ref();
        let candidate = actual.candidate.output.as_ref();
        let all_outputs: Vec<_> = std::iter::once(&actual.raw_prefix)
            .chain(bounded)
            .chain(candidate)
            .collect();
        let budgeted_outputs: Vec<_> = bounded.into_iter().chain(candidate).collect();
        let checks = json!({
            "evidence": actual.evidence == case.expected.evidence,
            "source_matches": actual.source_matches == case.expected.source_matches,
            "raw_prefix_golden": actual.raw_prefix.text == case.expected.raw_prefix_text,
            "raw_prefix_expression": actual.raw_prefix.text == case.input.text.chars()
                .take(case.input.budget.max_snippet_chars).collect::<String>(),
            "bounded_prefix_golden": bounded.map(|out| &out.text) == case.expected.bounded_prefix_text.as_ref(),
            "candidate_golden": candidate.map(|out| &out.text) == case.expected.candidate_text.as_ref(),
            "candidate_strategy": actual.candidate.strategy == case.expected.candidate_strategy,
            "character_budgets": all_outputs.iter().all(|out| out.scalar_count <= case.input.budget.max_snippet_chars),
            "local_json_byte_budgets": budgeted_outputs.iter().all(|out| out.json_bytes <= case.input.budget.max_json_bytes),
            "omission_only_below_json_floor": bounded.is_none() == (case.input.budget.max_json_bytes < json_floor)
                && candidate.is_none() == bounded.is_none(),
            "original_source_slices": all_outputs.iter().all(|out| exact_source_slice(&case.input, out)),
            "serialized_json_round_trip": all_outputs.iter().all(|out| exact_wire(out)),
            "visible_spans_from_original_evidence": all_outputs.iter().all(|out| visible_spans(&case.input, &actual, out)),
        });
        let case_passed = checks
            .as_object()
            .is_some_and(|map| map.values().all(|value| value == true));
        passed += usize::from(case_passed);
        prefix_visible_cases +=
            usize::from(bounded.is_some_and(|out| !out.visible_source_matches.is_empty()));
        candidate_visible_cases +=
            usize::from(candidate.is_some_and(|out| !out.visible_source_matches.is_empty()));
        raw_prefix_over_byte_budget +=
            usize::from(actual.raw_prefix.json_bytes > case.input.budget.max_json_bytes);
        results.push(json!({
            "id": case.id,
            "input": case.input,
            "expected": case.expected,
            "actual": actual,
            "checks": checks,
            "passed": case_passed,
        }));
    }
    let case_count = results.len();
    Ok(json!({
        "schema_version": "agent-session-grep.wake-reuse-snippet/v1",
        "experiment": "A",
        "status": if passed == case_count { "passed" } else { "failed" },
        "evidence_status": "isolated_synthetic_correctness_prototype",
        "reuse_classification": "adapt",
        "product_integration": false,
        "performance_claims": false,
        "fixture_revision": "snippet-synthetic-v1",
        "contains_real_transcripts": false,
        "dataset_hash": {
            "algorithm": "SHA-256",
            "value": dataset_hash,
            "encoding": "UTF-8 compact JSON of the ordered {id,input,expected} case array, recursively sorted object keys, no ASCII escaping of Unicode, including independent expectations"
        },
        "upstream": {
            "repository": "https://github.com/iAmCorey/Wake",
            "tag": "v0.8.5",
            "commit": "71aeca67ec80f8645d1f9d5199290c2c732036ce",
            "path": "crates/wake-core/src/db.rs",
            "blob_sha1": "d0152507eba4fcc470f57538ae2661f0dcc9a799",
            "symbol": "make_like_snippet",
            "line_range": [3302, 3345],
            "license": "MIT",
            "copyright": "Copyright (c) 2026 Corey Chiu",
            "license_blob_sha1": "77ad15a2bbf1194c9728572cbbe7393b8295954e",
            "execution": "not_executed; source inspection only"
        },
        "baseline": {
            "commit": "2b8f89562e43bd1f68ad3c8e5cf8ccc9281f9af2",
            "path": "crates/agent-session-grep-application/src/lib.rs",
            "blob_sha1": "5cce489f96bb4b1692a263569f07128143f0a832",
            "symbol": "assemble_search_hit",
            "line_range": [970, 1009],
            "reproduced_expression": "text.chars().take(max_snippet_chars).collect()",
            "unmodified_product_executed": false,
            "scope": "Only the text-prefix expression is reproduced. raw_prefix has no local byte gate; bounded_prefix is an experimental wrapper, not product behavior."
        },
        "method": {
            "character_unit": "Rust Unicode scalar values, not grapheme clusters or UTF-8 byte offsets",
            "case_matching": "Per-scalar char::to_lowercase on text and terms; expanded positions map to whole original scalars",
            "literal_terms": "Explicit terms only; no query parser, CJK bigram transform, normalization, regex, SQL, FTS or semantic model",
            "final_star": "Remove exactly one final *; match its literal stem only at an original scalar/token start. Start of text, whitespace or ASCII punctuation except _ opens a token. All other wildcard characters remain literal.",
            "hit_selection": "All overlapping occurrences are recorded; earliest original start wins, then term index, then end. Only one window is displayed.",
            "window": "Preserve complete anchor, add two right scalars then one left; at most 40 before and 80 after. Stop a side when its adjacent scalar cannot fit; no skipped source text.",
            "no_match": "Empty/nonmatching/semantic-only terms produce a bounded prefix with no literal evidence. Semantic-only mode is an explicit test flag, not actual semantic search.",
            "small_budget": "When an anchor cannot fit, emit empty text and retain source evidence separately. Below the empty payload byte floor, emit no snippet payload.",
            "plain_text": "Original contiguous source slice only; no highlighting, markup or ellipsis characters",
            "json_budget_scope": "Exactly compact UTF-8 {\"text\":<string>} after serde_json escaping, including braces, key, quotes and all escapes. The diagnostic report and the product SearchHit/Robot envelope are NOT under this budget.",
            "empty_payload_json_bytes": json_floor,
            "validation": "Rebuild all synthetic cases, recompute SHA-256/outputs/checks/counts, and require exact JSON-value equality; object key order is irrelevant."
        },
        "summary": {
            "case_count": case_count,
            "passed": passed,
            "failed": case_count - passed,
            "bounded_prefix_cases_with_visible_literal_match": prefix_visible_cases,
            "candidate_cases_with_visible_literal_match": candidate_visible_cases,
            "raw_prefix_cases_over_local_json_byte_budget": raw_prefix_over_byte_budget,
            "count_interpretation": "Descriptive counts on this hand-authored synthetic suite, NOT recall, ranking quality, latency, throughput or representative product metrics."
        },
        "limitations": [
            "This is not Wake execution, product integration, provider certification, legal approval, a benchmark or a production recommendation.",
            "Only explicit text and terms are studied. Product CJK bigram term derivation, payload-leaf guidance, why_matched, final-star FTS semantics, ranking, cursor, occurrence merging and full serialized envelopes remain unchanged and untested here.",
            "Prefix boundaries are intentionally conservative and not equivalent to SQLite tokenization. Repeated final stars differ from the product sanitizer, which trims every trailing star; interior * remains literal here.",
            "Scalar lowercase is neither full case folding nor Unicode normalization; sharp-s, decomposed accents and contextual Greek sigma counterexamples are retained. A match inside a lowercase expansion maps to the entire original scalar.",
            "UTF-8 boundaries are safe, but grapheme clusters, combining marks and emoji ZWJ sequences can be split at snippet edges.",
            "One earliest-hit window need not show later or more useful matches. An over-budget anchor yields empty display, not a fabricated shorter match.",
            "The prototype materializes full scalar/lowercase maps and all occurrences, and repeatedly serializes candidates. No streaming, memory bound or performance improvement is claimed.",
            "The JSON cap applies only to the isolated text payload. A future product change must account for all real SearchHit/guidance/evidence/envelope bytes before claiming product budget integration.",
            "Replay validation detects accidental or intentional report edits but is not a cryptographic signature or independent implementation oracle; the hand-authored golden cases and focused tests provide separate checks."
        ],
        "cases": results
    }))
}

pub fn validate_report(report: &Value) -> Result<(), Box<dyn std::error::Error>> {
    let replay = build_report()?;
    if replay["status"] != "passed" {
        return Err(
            "Current synthetic expectations fail; no report can be certified by this prototype."
                .into(),
        );
    }
    if *report != replay {
        return Err("Report differs from deterministic replay (inputs, expectations, outputs, metadata or counts).".into());
    }
    Ok(())
}
