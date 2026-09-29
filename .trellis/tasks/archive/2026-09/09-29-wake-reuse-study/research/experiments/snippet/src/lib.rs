// Independent adaptation of Wake's scalar-origin mapping/window idea.
// Upstream: Copyright (c) 2026 Corey Chiu, MIT; full notice in LICENSE-WAKE.
// See PROVENANCE.md for the pinned source and deliberate semantic differences.

pub mod cases;
pub mod report;

use serde::Serialize;

const BEFORE_CHARS: usize = 40;
const AFTER_CHARS: usize = 80;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Lexical,
    SemanticOnly,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct Budget {
    pub max_snippet_chars: usize,
    pub max_json_bytes: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Input {
    pub text: String,
    pub terms: Vec<String>,
    pub mode: Mode,
    pub budget: Budget,
}

/// Half-open Unicode scalar offsets into the original, unmodified input.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct CharRange {
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct MatchSpan {
    pub term_index: usize,
    pub range: CharRange,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Evidence {
    Literal,
    NoMatch,
    EmptyTerms,
    SemanticOnly,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Rendered {
    pub text: String,
    pub source_range: CharRange,
    pub scalar_count: usize,
    /// The *entire* compact prototype payload, not just the escaped string.
    pub wire_json: String,
    pub json_bytes: usize,
    pub visible_source_matches: Vec<MatchSpan>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Strategy {
    HitCentered,
    PrefixWithoutLiteralEvidence,
    EmptyAnchorExceedsBudget,
    OmittedJsonFloor,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Candidate {
    pub strategy: Strategy,
    /// None means even {"text":""} cannot fit; it is not a serialized null.
    pub output: Option<Rendered>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Comparison {
    pub evidence: Evidence,
    pub source_matches: Vec<MatchSpan>,
    pub focus: Option<MatchSpan>,
    /// Exact chars().take(max_snippet_chars) baseline, WITHOUT a byte gate.
    pub raw_prefix: Rendered,
    /// Local byte gate for an apples-to-apples display comparison only.
    pub bounded_prefix: Option<Rendered>,
    pub candidate: Candidate,
}

#[derive(Serialize)]
struct Wire<'a> {
    text: &'a str,
}

pub fn serialize_text(text: &str) -> serde_json::Result<String> {
    serde_json::to_string(&Wire { text })
}

fn lowercase_scalars(text: &str) -> Vec<char> {
    text.chars().flat_map(char::to_lowercase).collect()
}

struct Needle {
    term_index: usize,
    lower: Vec<char>,
    prefix: bool,
}

fn needles(terms: &[String]) -> Vec<Needle> {
    terms
        .iter()
        .enumerate()
        .filter_map(|(term_index, term)| {
            // Only the last star is an operator. Earlier stars stay literal.
            let (literal, prefix) = term
                .strip_suffix('*')
                .map_or((term.as_str(), false), |stem| (stem, true));
            if literal.trim().is_empty() {
                return None;
            }
            Some(Needle {
                term_index,
                lower: lowercase_scalars(literal),
                prefix,
            })
        })
        .collect()
}

/// Deliberately conservative, NOT SQLite unicode61/FTS tokenization.
/// Non-ASCII non-whitespace scalars (including combining marks) do not open
/// a new token. In particular, neither "network" nor "_work" matches work*.
fn prefix_boundary(chars: &[char], start: usize) -> bool {
    start == 0
        || chars[start - 1].is_whitespace()
        || (chars[start - 1].is_ascii_punctuation() && chars[start - 1] != '_')
}

fn locate(chars: &[char], needles: &[Needle]) -> Vec<MatchSpan> {
    let mut lower = Vec::new();
    let mut origins = Vec::new();
    for (source_index, ch) in chars.iter().enumerate() {
        for folded in ch.to_lowercase() {
            lower.push(folded);
            origins.push(source_index);
        }
    }
    let mut matches = Vec::new();
    for needle in needles {
        for (offset, window) in lower.windows(needle.lower.len()).enumerate() {
            if window != needle.lower {
                continue;
            }
            let start = origins[offset];
            // Starting at the second scalar of İ -> i + dot is not a token
            // start, even though both transformed scalars map to source 0.
            let begins_source_scalar = offset == 0 || origins[offset - 1] != start;
            if needle.prefix && (!begins_source_scalar || !prefix_boundary(chars, start)) {
                continue;
            }
            matches.push(MatchSpan {
                term_index: needle.term_index,
                range: CharRange {
                    start,
                    end: origins[offset + needle.lower.len() - 1] + 1,
                },
            });
        }
    }
    matches.sort_by_key(|hit| (hit.range.start, hit.term_index, hit.range.end));
    matches.dedup();
    matches
}

fn render(chars: &[char], range: CharRange, matches: &[MatchSpan]) -> serde_json::Result<Rendered> {
    let text: String = chars[range.start..range.end].iter().collect();
    let wire_json = serialize_text(&text)?;
    Ok(Rendered {
        scalar_count: range.end - range.start,
        json_bytes: wire_json.len(),
        wire_json,
        text,
        source_range: range,
        // Use original, already verified matches, not a re-search of the
        // truncated display which could manufacture a prefix boundary.
        visible_source_matches: matches
            .iter()
            .filter(|hit| range.start <= hit.range.start && hit.range.end <= range.end)
            .cloned()
            .collect(),
    })
}

fn fits(output: &Rendered, budget: Budget) -> bool {
    output.scalar_count <= budget.max_snippet_chars && output.json_bytes <= budget.max_json_bytes
}

fn bounded_prefix(
    chars: &[char],
    matches: &[MatchSpan],
    budget: Budget,
) -> serde_json::Result<Option<Rendered>> {
    let mut best = render(chars, CharRange { start: 0, end: 0 }, matches)?;
    if !fits(&best, budget) {
        return Ok(None);
    }
    for end in 1..=chars.len().min(budget.max_snippet_chars) {
        let next = render(chars, CharRange { start: 0, end }, matches)?;
        if !fits(&next, budget) {
            break;
        }
        best = next;
    }
    Ok(Some(best))
}

fn hit_centered(
    chars: &[char],
    matches: &[MatchSpan],
    focus: &MatchSpan,
    budget: Budget,
) -> serde_json::Result<Candidate> {
    let mut best = render(chars, focus.range, matches)?;
    if !fits(&best, budget) {
        // No partial-anchor claim and no invented substitute match. The
        // report keeps the source hit separately from its display visibility.
        return Ok(Candidate {
            strategy: Strategy::EmptyAnchorExceedsBudget,
            output: Some(render(
                chars,
                CharRange {
                    start: focus.range.start,
                    end: focus.range.start,
                },
                matches,
            )?),
        });
    }
    let left_limit = focus.range.start.saturating_sub(BEFORE_CHARS);
    let right_limit = focus.range.end.saturating_add(AFTER_CHARS).min(chars.len());
    let mut left_open = best.source_range.start > left_limit;
    let mut right_open = best.source_range.end < right_limit;
    // A deterministic 2-after / 1-before schedule preserves the anchor.
    // If one adjacent scalar is too expensive, only the other side can grow;
    // no source character is skipped. Limits are 40 before / 80 after.
    while left_open || right_open {
        for after in [true, true, false] {
            if (after && !right_open) || (!after && !left_open) {
                continue;
            }
            let mut range = best.source_range;
            if after {
                range.end += 1;
            } else {
                range.start -= 1;
            }
            let next = render(chars, range, matches)?;
            if fits(&next, budget) {
                best = next;
                left_open &= best.source_range.start > left_limit;
                right_open &= best.source_range.end < right_limit;
            } else if after {
                right_open = false;
            } else {
                left_open = false;
            }
        }
    }
    Ok(Candidate {
        strategy: Strategy::HitCentered,
        output: Some(best),
    })
}

pub fn compare(input: &Input) -> serde_json::Result<Comparison> {
    let chars: Vec<char> = input.text.chars().collect();
    let terms = needles(&input.terms);
    let source_matches = if input.mode == Mode::SemanticOnly {
        Vec::new()
    } else {
        locate(&chars, &terms)
    };
    let evidence = if input.mode == Mode::SemanticOnly {
        Evidence::SemanticOnly
    } else if terms.is_empty() {
        Evidence::EmptyTerms
    } else if source_matches.is_empty() {
        Evidence::NoMatch
    } else {
        Evidence::Literal
    };
    let focus = source_matches.first().cloned();
    let raw_prefix = render(
        &chars,
        CharRange {
            start: 0,
            end: chars.len().min(input.budget.max_snippet_chars),
        },
        &source_matches,
    )?;
    let bounded_prefix = bounded_prefix(&chars, &source_matches, input.budget)?;
    let candidate = match (&bounded_prefix, &focus) {
        (None, _) => Candidate {
            strategy: Strategy::OmittedJsonFloor,
            output: None,
        },
        (Some(_), Some(hit)) => hit_centered(&chars, &source_matches, hit, input.budget)?,
        (Some(prefix), None) => Candidate {
            strategy: Strategy::PrefixWithoutLiteralEvidence,
            output: Some(prefix.clone()),
        },
    };
    Ok(Comparison {
        evidence,
        source_matches,
        focus,
        raw_prefix,
        bounded_prefix,
        candidate,
    })
}
