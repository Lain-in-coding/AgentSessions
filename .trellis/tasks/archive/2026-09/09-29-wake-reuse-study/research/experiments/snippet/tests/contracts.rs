use std::{collections::BTreeSet, process::Command};

use serde_json::{Value, json};
use wake_reuse_snippet::{
    CharRange, Evidence, Mode, Strategy,
    cases::{input, synthetic_cases},
    compare,
    report::{build_report, validate_report},
    serialize_text,
};

#[test]
fn all_independently_authored_goldens_match() {
    let report = build_report().unwrap();
    for case in report["cases"].as_array().unwrap() {
        assert_eq!(
            case["passed"], true,
            "case {}: {}",
            case["id"], case["checks"]
        );
    }
    assert_eq!(report["status"], "passed");
    assert_eq!(report["summary"]["failed"], 0);
    let cases = synthetic_cases();
    let unique: BTreeSet<_> = cases.iter().map(|case| case.id).collect();
    assert_eq!(unique.len(), cases.len());
}

#[test]
fn json_byte_cost_is_final_serialized_payload_not_raw_text() {
    let text = "\"\\\n\t\r\u{8}\u{c}\0\u{1}\u{1f}";
    let wire = serialize_text(text).unwrap();
    assert_eq!(wire, r#"{"text":"\"\\\n\t\r\b\f\u0000\u0001\u001f"}"#);
    assert_eq!(wire.len(), 43);
    assert_eq!(serialize_text("").unwrap(), r#"{"text":""}"#);
    assert_eq!(serialize_text("").unwrap().len(), 11);
    for scalar in '\0'..='\u{1f}' {
        let text = scalar.to_string();
        let wire = serialize_text(&text).unwrap();
        assert!(wire.bytes().all(|byte| byte >= 0x20));
        assert_eq!(
            serde_json::from_str::<Value>(&wire).unwrap(),
            json!({ "text": text })
        );
        let exact = wire.len();
        let exact_result = compare(&input(&text, &[], 1, exact)).unwrap();
        assert_eq!(exact_result.bounded_prefix.unwrap().text, text);
        let short_result = compare(&input(&text, &[], 1, exact - 1)).unwrap();
        assert_eq!(short_result.bounded_prefix.unwrap().text, "");
    }
}

#[test]
fn small_budget_grid_preserves_utf8_json_caps_and_original_evidence() {
    let text = "a\"\\\0\n\t🙂e\u{301}İz";
    let original: Vec<char> = text.chars().collect();
    for char_cap in 0..=13 {
        for byte_cap in 0..=48 {
            for (mode, terms) in [
                (Mode::Lexical, vec!["🙂", "i"]),
                (Mode::Lexical, vec!["missing"]),
                (Mode::Lexical, vec![]),
                (Mode::SemanticOnly, vec!["🙂"]),
            ] {
                let mut request = input(text, &terms, char_cap, byte_cap);
                request.mode = mode;
                let result = compare(&request).unwrap();
                assert_eq!(
                    result.raw_prefix.text,
                    text.chars().take(char_cap).collect::<String>()
                );
                for output in [&result.bounded_prefix, &result.candidate.output] {
                    let Some(output) = output else {
                        assert!(byte_cap < 11);
                        continue;
                    };
                    assert!(byte_cap >= 11);
                    assert!(output.text.chars().count() <= char_cap);
                    assert_eq!(output.wire_json.len(), output.json_bytes);
                    assert!(output.json_bytes <= byte_cap);
                    assert_eq!(
                        serde_json::from_str::<Value>(&output.wire_json).unwrap(),
                        json!({"text": output.text})
                    );
                    assert_eq!(
                        output.text,
                        original[output.source_range.start..output.source_range.end]
                            .iter()
                            .collect::<String>()
                    );
                    for hit in &output.visible_source_matches {
                        assert!(result.source_matches.contains(hit));
                        assert!(output.source_range.start <= hit.range.start);
                        assert!(hit.range.end <= output.source_range.end);
                    }
                }
                if result.candidate.strategy == Strategy::HitCentered {
                    let focus = result.focus.unwrap();
                    assert!(
                        result
                            .candidate
                            .output
                            .unwrap()
                            .visible_source_matches
                            .contains(&focus)
                    );
                }
                if mode == Mode::SemanticOnly || terms.is_empty() || terms == ["missing"] {
                    assert!(result.source_matches.is_empty());
                }
            }
        }
    }
}

#[test]
fn expansion_offsets_use_original_scalars_not_transformed_offsets() {
    let result = compare(&input("AİB", &["i\u{307}", "B", "\u{307}"], 10, 100)).unwrap();
    assert_eq!(result.source_matches.len(), 3);
    assert_eq!(
        result.source_matches[0].range,
        CharRange { start: 1, end: 2 }
    );
    assert_eq!(
        result.source_matches[1].range,
        CharRange { start: 1, end: 2 }
    );
    assert_eq!(
        result.source_matches[2].range,
        CharRange { start: 2, end: 3 }
    );
    let short = compare(&input("İ", &["i"], 1, 12)).unwrap();
    assert_eq!(short.evidence, Evidence::Literal);
    assert_eq!(short.candidate.output.unwrap().text, "");
    assert_eq!(
        compare(&input("I", &["i\u{307}"], 10, 100))
            .unwrap()
            .evidence,
        Evidence::NoMatch
    );
}

#[test]
fn final_star_requires_original_prefix_boundary() {
    for (text, start) in [
        ("workshop", 0),
        ("work", 0),
        ("/work", 1),
        (" work", 1),
        ("\u{3000}work", 1),
        (".work", 1),
    ] {
        let result = compare(&input(text, &["work*"], 20, 100)).unwrap();
        assert_eq!(result.source_matches.len(), 1, "{text}");
        assert_eq!(
            result.source_matches[0].range,
            CharRange {
                start,
                end: start + 4
            }
        );
    }
    for text in [
        "network",
        "rework",
        "_work",
        "e\u{301}work",
        "éwork",
        "中work",
        "🙂work",
    ] {
        let result = compare(&input(text, &["work*"], 4, 100)).unwrap();
        assert_eq!(result.evidence, Evidence::NoMatch, "{text}");
        assert!(result.source_matches.is_empty());
        assert!(
            result
                .candidate
                .output
                .unwrap()
                .visible_source_matches
                .is_empty()
        );
    }
    assert_eq!(
        compare(&input("İ", &["\u{307}*"], 10, 100))
            .unwrap()
            .evidence,
        Evidence::NoMatch
    );
}

#[test]
fn no_globs_regex_or_sql_wildcards_are_interpreted_elsewhere() {
    for term in ["a?b", "a%b", "a_b", "a*b", "a.b", "[ab]", "*ab", "ab**"] {
        assert!(
            compare(&input("aZZb ab aXb", &[term], 50, 100))
                .unwrap()
                .source_matches
                .is_empty(),
            "{term}"
        );
    }
    assert_eq!(
        compare(&input("foo*barley", &["foo*bar*"], 20, 100))
            .unwrap()
            .source_matches[0]
            .range,
        CharRange { start: 0, end: 7 }
    );
    assert_eq!(
        compare(&input("work*bench", &["work**"], 20, 100))
            .unwrap()
            .source_matches[0]
            .range,
        CharRange { start: 0, end: 5 }
    );
}

#[test]
fn multiple_matches_remain_source_facts_not_display_claims() {
    let result = compare(&input("banana", &["ana"], 3, 100)).unwrap();
    assert_eq!(result.source_matches.len(), 2);
    let output = result.candidate.output.unwrap();
    assert_eq!(output.text, "ana");
    assert_eq!(output.source_range, CharRange { start: 1, end: 4 });
    assert_eq!(
        output.visible_source_matches,
        vec![result.source_matches[0].clone()]
    );
}

#[test]
fn semantic_empty_and_nonmatching_terms_cannot_fabricate_literal_evidence() {
    let mut semantic = input("needle here", &["needle"], 20, 100);
    semantic.mode = Mode::SemanticOnly;
    for request in [
        semantic,
        input("needle here", &[], 20, 100),
        input("needle here", &["other"], 20, 100),
        input("needle here", &["", " \t", "*"], 20, 100),
    ] {
        let result = compare(&request).unwrap();
        assert!(result.source_matches.is_empty());
        assert!(result.focus.is_none());
        assert_eq!(
            result.candidate.strategy,
            Strategy::PrefixWithoutLiteralEvidence
        );
        assert!(
            result
                .candidate
                .output
                .unwrap()
                .visible_source_matches
                .is_empty()
        );
    }
}

#[test]
fn large_caps_do_not_overflow_window_arithmetic() {
    let result = compare(&input("🙂x", &["🙂"], usize::MAX, usize::MAX)).unwrap();
    assert_eq!(result.candidate.output.unwrap().text, "🙂x");
}

#[test]
fn deterministic_replay_rejects_tampered_results_inputs_expectations_and_counts() {
    let report = build_report().unwrap();
    validate_report(&report).unwrap();
    let pointers = [
        "/status",
        "/summary/case_count",
        "/summary/passed",
        "/summary/candidate_cases_with_visible_literal_match",
        "/dataset_hash/value",
        "/cases/0/input/text",
        "/cases/0/expected/candidate_text",
        "/cases/0/actual/candidate/output/text",
        "/cases/0/actual/candidate/output/json_bytes",
        "/cases/0/actual/candidate/output/wire_json",
        "/cases/0/checks/evidence",
        "/cases/0/passed",
        "/upstream/commit",
        "/limitations/0",
    ];
    for pointer in pointers {
        let mut changed = report.clone();
        *changed.pointer_mut(pointer).unwrap() = json!("tampered");
        assert!(validate_report(&changed).is_err(), "{pointer}");
    }
    let mut changed = report.clone();
    changed["summary"]["case_count"] = json!(report["summary"]["case_count"].as_u64().unwrap() + 1);
    assert!(validate_report(&changed).is_err());
    let mut changed = report.clone();
    changed["unrecognized_claim"] = json!(true);
    assert!(validate_report(&changed).is_err());
}

#[test]
fn cli_emits_json_to_stdout_and_rejects_invalid_commands() {
    let output = Command::new(env!("CARGO_BIN_EXE_wake-reuse-snippet"))
        .arg("report")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = serde_json::from_slice(&output.stdout).unwrap();
    validate_report(&report).unwrap();
    let invalid = Command::new(env!("CARGO_BIN_EXE_wake-reuse-snippet"))
        .arg("invalid")
        .output()
        .unwrap();
    assert!(!invalid.status.success());
    assert!(invalid.stdout.is_empty());
}

#[test]
fn fixture_hash_can_be_recomputed_from_report_without_private_struct_order() {
    use sha2::{Digest, Sha256};

    let report = build_report().unwrap();
    let cases: Vec<_> = report["cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|case| json!({"id": case["id"], "input": case["input"], "expected": case["expected"]}))
        .collect();
    let bytes = serde_json::to_vec(&cases).unwrap();
    let recomputed = format!("{:x}", Sha256::digest(bytes));
    assert_eq!(report["dataset_hash"]["value"], recomputed);
}
