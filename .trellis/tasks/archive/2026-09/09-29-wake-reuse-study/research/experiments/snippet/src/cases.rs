//! Independently authored synthetic strings. No upstream fixtures/transcripts.

use serde::Serialize;

use crate::{Budget, CharRange, Evidence, Input, MatchSpan, Mode, Strategy};

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Expected {
    pub evidence: Evidence,
    pub source_matches: Vec<MatchSpan>,
    pub raw_prefix_text: String,
    pub bounded_prefix_text: Option<String>,
    pub candidate_text: Option<String>,
    pub candidate_strategy: Strategy,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SyntheticCase {
    pub id: &'static str,
    pub input: Input,
    pub expected: Expected,
}

pub fn input(text: impl Into<String>, terms: &[&str], chars: usize, bytes: usize) -> Input {
    Input {
        text: text.into(),
        terms: terms.iter().map(|term| (*term).to_string()).collect(),
        mode: Mode::Lexical,
        budget: Budget {
            max_snippet_chars: chars,
            max_json_bytes: bytes,
        },
    }
}

fn hit(term_index: usize, start: usize, end: usize) -> MatchSpan {
    MatchSpan {
        term_index,
        range: CharRange { start, end },
    }
}

fn expected(
    evidence: Evidence,
    matches: Vec<MatchSpan>,
    raw: &str,
    bounded: Option<&str>,
    centered: Option<&str>,
    strategy: Strategy,
) -> Expected {
    Expected {
        evidence,
        source_matches: matches,
        raw_prefix_text: raw.to_string(),
        bounded_prefix_text: bounded.map(str::to_string),
        candidate_text: centered.map(str::to_string),
        candidate_strategy: strategy,
    }
}

fn case(id: &'static str, input: Input, expected: Expected) -> SyntheticCase {
    SyntheticCase {
        id,
        input,
        expected,
    }
}

fn whole(id: &'static str, text: &str, terms: &[&str], matches: Vec<MatchSpan>) -> SyntheticCase {
    let evidence = if terms.is_empty() {
        Evidence::EmptyTerms
    } else if matches.is_empty() {
        Evidence::NoMatch
    } else {
        Evidence::Literal
    };
    let strategy = if matches.is_empty() {
        Strategy::PrefixWithoutLiteralEvidence
    } else {
        Strategy::HitCentered
    };
    case(
        id,
        input(text, terms, 512, 4096),
        expected(evidence, matches, text, Some(text), Some(text), strategy),
    )
}

pub fn synthetic_cases() -> Vec<SyntheticCase> {
    use Evidence::{EmptyTerms, Literal, NoMatch, SemanticOnly};
    use Strategy::{
        EmptyAnchorExceedsBudget, HitCentered, OmittedJsonFloor, PrefixWithoutLiteralEvidence,
    };
    let long_tail = format!("{} needle {}", "a".repeat(200), "z".repeat(100));
    let long_center = format!("{} needle {}", "a".repeat(10), "z".repeat(22));
    let context_limit = format!("{}needle{}", "a".repeat(70), "b".repeat(100));
    let fixed_window = format!("{}needle{}", "a".repeat(40), "b".repeat(80));
    let multi = format!("first {} last", "x".repeat(60));
    let multi_prefix = format!("first {}", "x".repeat(14));
    let later_term = format!("{} match!", "z".repeat(50));
    let cjk = format!("{}库{}", "前".repeat(30), "后".repeat(30));
    let emoji = format!("{}🚀 launch {}", "🙂".repeat(20), "🙂".repeat(10));
    let combining = format!("{} e\u{301} {}", "x".repeat(10), "y".repeat(10));
    let mut cases = vec![
        whole(
            "ascii_head",
            "hello needle world",
            &["needle"],
            vec![hit(0, 6, 12)],
        ),
        case(
            "long_tail",
            input(long_tail, &["needle"], 40, 4096),
            expected(
                Literal,
                vec![hit(0, 201, 207)],
                &"a".repeat(40),
                Some(&"a".repeat(40)),
                Some(&long_center),
                HitCentered,
            ),
        ),
        case(
            "wake_40_before_80_after_limit",
            input(&context_limit, &["needle"], 500, 4096),
            expected(
                Literal,
                vec![hit(0, 70, 76)],
                &context_limit,
                Some(&context_limit),
                Some(&fixed_window),
                HitCentered,
            ),
        ),
        case(
            "earliest_hit_not_query_order",
            input(multi, &["last", "first"], 20, 4096),
            expected(
                Literal,
                vec![hit(1, 0, 5), hit(0, 67, 71)],
                &multi_prefix,
                Some(&multi_prefix),
                Some(&multi_prefix),
                HitCentered,
            ),
        ),
        case(
            "absent_first_term_present_second",
            input(later_term, &["absent", "match"], 16, 4096),
            expected(
                Literal,
                vec![hit(1, 51, 56)],
                &"z".repeat(16),
                Some(&"z".repeat(16)),
                Some(&format!("{} match!", "z".repeat(9))),
                HitCentered,
            ),
        ),
        whole(
            "overlapping_occurrences",
            "banana",
            &["ana"],
            vec![hit(0, 1, 4), hit(0, 3, 6)],
        ),
        case(
            "cjk_one_scalar",
            input(cjk, &["库"], 9, 4096),
            expected(
                Literal,
                vec![hit(0, 30, 31)],
                &"前".repeat(9),
                Some(&"前".repeat(9)),
                Some(&format!("{}库{}", "前".repeat(2), "后".repeat(6))),
                HitCentered,
            ),
        ),
        whole(
            "cjk_two_scalars",
            "开头 数据库 结束",
            &["数据"],
            vec![hit(0, 3, 5)],
        ),
        whole(
            "cjk_overlapping_terms",
            "数据库",
            &["数据", "据库"],
            vec![hit(0, 0, 2), hit(1, 1, 3)],
        ),
        whole(
            "windows_path",
            r"open C:\synthetic\src\lib.rs now",
            &[r"C:\synthetic\src"],
            vec![hit(0, 5, 21)],
        ),
        whole(
            "posix_path_and_code",
            "/tmp/demo/src/lib.rs module::Symbol snake_case",
            &["/tmp/demo/src", "module::Symbol", "snake_case"],
            vec![hit(0, 0, 13), hit(1, 21, 35), hit(2, 36, 46)],
        ),
        case(
            "emoji_window",
            input(emoji, &["🚀"], 5, 100),
            expected(
                Literal,
                vec![hit(0, 20, 21)],
                &"🙂".repeat(5),
                Some(&"🙂".repeat(5)),
                Some("🙂🚀 la"),
                HitCentered,
            ),
        ),
        case(
            "combining_sequence",
            input(combining, &["e\u{301}"], 5, 100),
            expected(
                Literal,
                vec![hit(0, 11, 13)],
                "xxxxx",
                Some("xxxxx"),
                Some(" e\u{301} y"),
                HitCentered,
            ),
        ),
        case(
            "scalar_not_grapheme_contract",
            input("e\u{301}", &[], 1, 100),
            expected(
                EmptyTerms,
                vec![],
                "e",
                Some("e"),
                Some("e"),
                PrefixWithoutLiteralEvidence,
            ),
        ),
        case(
            "expansions_before_hit",
            input("İİ ΩK target!", &["target"], 7, 100),
            expected(
                Literal,
                vec![hit(0, 6, 12)],
                "İİ ΩK t",
                Some("İİ ΩK t"),
                Some("target!"),
                HitCentered,
            ),
        ),
        case(
            "expansion_inside_hit",
            input("ab İSTANBUL cd", &["i\u{307}stan"], 5, 100),
            expected(
                Literal,
                vec![hit(0, 3, 8)],
                "ab İS",
                Some("ab İS"),
                Some("İSTAN"),
                HitCentered,
            ),
        ),
        case(
            "partial_expansion_maps_whole_source_scalar",
            input("İ", &["i"], 1, 13),
            expected(
                Literal,
                vec![hit(0, 0, 1)],
                "İ",
                Some("İ"),
                Some("İ"),
                HitCentered,
            ),
        ),
        whole(
            "prefix_cannot_start_inside_expansion",
            "İz",
            &["\u{307}*"],
            vec![],
        ),
        whole("no_unicode_normalization", "café", &["cafe\u{301}"], vec![]),
        whole("no_full_case_folding", "Straße", &["STRASSE"], vec![]),
        whole(
            "scalar_lowercase_not_contextual_sigma",
            "ΟΣ",
            &["ος"],
            vec![],
        ),
        whole(
            "quotes_backslashes_all_json_short_escapes",
            "\"\\\n\t\r\u{8}\u{c}\0\u{1}\u{1f} needle",
            &["needle"],
            vec![hit(0, 11, 17)],
        ),
        case(
            "quote_backslash_byte_gate",
            input("\"\\x", &[], 10, 15),
            expected(
                EmptyTerms,
                vec![],
                "\"\\x",
                Some("\"\\"),
                Some("\"\\"),
                PrefixWithoutLiteralEvidence,
            ),
        ),
        case(
            "control_escape_too_expensive",
            input("\u{1}x", &[], 10, 16),
            expected(
                EmptyTerms,
                vec![],
                "\u{1}x",
                Some(""),
                Some(""),
                PrefixWithoutLiteralEvidence,
            ),
        ),
        case(
            "escaped_anchor_exact_fit",
            input("xx \"\\ yy", &["\"\\"], 2, 15),
            expected(
                Literal,
                vec![hit(0, 3, 5)],
                "xx",
                Some("xx"),
                Some("\"\\"),
                HitCentered,
            ),
        ),
        case(
            "escaped_anchor_one_byte_short",
            input("xx \"\\ yy", &["\"\\"], 2, 14),
            expected(
                Literal,
                vec![hit(0, 3, 5)],
                "xx",
                Some("xx"),
                Some(""),
                EmptyAnchorExceedsBudget,
            ),
        ),
        case(
            "emoji_byte_gate_keeps_anchor",
            input("a🙂b", &["🙂"], 3, 15),
            expected(
                Literal,
                vec![hit(0, 1, 2)],
                "a🙂b",
                Some("a"),
                Some("🙂"),
                HitCentered,
            ),
        ),
        case(
            "minimum_json_payload",
            input("needle", &["needle"], 8, 11),
            expected(
                Literal,
                vec![hit(0, 0, 6)],
                "needle",
                Some(""),
                Some(""),
                EmptyAnchorExceedsBudget,
            ),
        ),
        case(
            "below_json_floor",
            input("x", &["x"], 1, 10),
            expected(
                Literal,
                vec![hit(0, 0, 1)],
                "x",
                None,
                None,
                OmittedJsonFloor,
            ),
        ),
        case(
            "zero_json_budget",
            input("x", &["x"], 1, 0),
            expected(
                Literal,
                vec![hit(0, 0, 1)],
                "x",
                None,
                None,
                OmittedJsonFloor,
            ),
        ),
        case(
            "zero_character_budget",
            input("needle", &["needle"], 0, 100),
            expected(
                Literal,
                vec![hit(0, 0, 6)],
                "",
                Some(""),
                Some(""),
                EmptyAnchorExceedsBudget,
            ),
        ),
        case(
            "anchor_longer_than_char_budget",
            input("begin needle end", &["needle"], 3, 100),
            expected(
                Literal,
                vec![hit(0, 6, 12)],
                "beg",
                Some("beg"),
                Some(""),
                EmptyAnchorExceedsBudget,
            ),
        ),
        whole("empty_text", "", &["needle"], vec![]),
        case(
            "empty_terms",
            input("synthetic body", &[], 9, 100),
            expected(
                EmptyTerms,
                vec![],
                "synthetic",
                Some("synthetic"),
                Some("synthetic"),
                PrefixWithoutLiteralEvidence,
            ),
        ),
        case(
            "blank_and_bare_star_terms",
            input("synthetic * text", &["", " \t", "*"], 40, 100),
            expected(
                EmptyTerms,
                vec![],
                "synthetic * text",
                Some("synthetic * text"),
                Some("synthetic * text"),
                PrefixWithoutLiteralEvidence,
            ),
        ),
        case(
            "nonmatching_terms",
            input("only apples and pears", &["banana"], 11, 100),
            expected(
                NoMatch,
                vec![],
                "only apples",
                Some("only apples"),
                Some("only apples"),
                PrefixWithoutLiteralEvidence,
            ),
        ),
        whole(
            "prefix_boundary_positive_and_internal_negative",
            "network workbench /workstation",
            &["work*"],
            vec![hit(0, 8, 12), hit(0, 19, 23)],
        ),
        whole(
            "prefix_counterexamples",
            "network rework _work éwork e\u{301}work 中work",
            &["work*"],
            vec![],
        ),
        case(
            "truncation_cannot_create_prefix_boundary",
            input("xworkstation", &["work*"], 4, 100),
            expected(
                NoMatch,
                vec![],
                "xwor",
                Some("xwor"),
                Some("xwor"),
                PrefixWithoutLiteralEvidence,
            ),
        ),
        whole(
            "unicode_whitespace_prefix_boundary",
            "net\u{3000}WORKshop",
            &["work*"],
            vec![hit(0, 4, 8)],
        ),
        whole(
            "single_cjk_prefix_at_start",
            "工作",
            &["工*"],
            vec![hit(0, 0, 1)],
        ),
        whole(
            "interior_star_stays_literal",
            "fooxbar foo*barley",
            &["foo*bar*"],
            vec![hit(0, 8, 15)],
        ),
        whole(
            "only_last_repeated_star_is_operator",
            "workstation work*bench",
            &["work**"],
            vec![hit(0, 12, 17)],
        ),
        whole(
            "repeated_star_not_product_fts_equivalence",
            "workstation",
            &["work**"],
            vec![],
        ),
        whole(
            "no_other_wildcards_negative",
            "aXb ab aZZb",
            &["a?b", "a%b", "a_b", "a*b"],
            vec![],
        ),
        whole(
            "all_other_wildcards_are_literal",
            "a?b a%b a_b a*b",
            &["a?b", "a%b", "a_b", "a*b"],
            vec![hit(0, 0, 3), hit(1, 4, 7), hit(2, 8, 11), hit(3, 12, 15)],
        ),
        whole("leading_star_is_literal", "workstation", &["*work"], vec![]),
    ];
    let mut semantic_input = input("needle appears", &["needle"], 6, 100);
    semantic_input.mode = Mode::SemanticOnly;
    cases.push(case(
        "semantic_only_even_if_term_spelling_occurs",
        semantic_input,
        expected(
            SemanticOnly,
            vec![],
            "needle",
            Some("needle"),
            Some("needle"),
            PrefixWithoutLiteralEvidence,
        ),
    ));
    let mut semantic_input = input("oranges", &["citrus"], 20, 100);
    semantic_input.mode = Mode::SemanticOnly;
    cases.push(case(
        "semantic_only_no_literal_evidence",
        semantic_input,
        expected(
            SemanticOnly,
            vec![],
            "oranges",
            Some("oranges"),
            Some("oranges"),
            PrefixWithoutLiteralEvidence,
        ),
    ));
    cases
}
