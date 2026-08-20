//! Declaration-vs-behaviour assertions for `ProviderCapabilityMatrix::current()`.
//!
//! The two pre-existing matrix guards both compare a *declaration* to another
//! *declaration*: `provider_matrix.rs` compares `capability.rs` to the markdown
//! matrix/ledger, and the manifest test compares adapter manifests to ledger
//! columns. Neither one ever asks an adapter what it actually does, so the
//! matrix was able to certify itself — `aider` shipped
//! `tool_activity: Partial` with zero activity emissions under a green CI.
//!
//! This file closes that hole by driving each implemented adapter over its
//! committed golden fixture and comparing observed behaviour to the declared
//! capability level. It follows the shape of the existing good precedent,
//! `application/src/resume.rs::capability_matrix_resume_level_matches_builder_support`,
//! which asserts `resume == Derived` if and only if the resume-command builder
//! produces a command.

use agent_session_grep_ports::capability::{
    CapabilityLevel, ProviderCapabilityMatrix, ProviderMaturity,
};
use agent_session_grep_ports::{
    CanonicalEventSink, MessageEvent, PortResult, ProviderAdapter, ToolActivityEvent,
};
use agent_session_grep_provider_aider::AiderAdapter;
use agent_session_grep_provider_antigravity::AntigravityAdapter;
use agent_session_grep_provider_claude::ClaudeCodeAdapter;
use agent_session_grep_provider_cline::ClineAdapter;
use agent_session_grep_provider_codebuddy::CodeBuddyAdapter;
use agent_session_grep_provider_codex::CodexAdapter;
use agent_session_grep_provider_cursor::CursorAdapter;
use agent_session_grep_provider_grok::GrokBuildAdapter;
use agent_session_grep_provider_hermes::OpenHermesAdapter;
use agent_session_grep_provider_kimi::KimiCodeAdapter;
use agent_session_grep_provider_openclaw::OpenClawAdapter;
use agent_session_grep_provider_opencode::OpenCodeAdapter;
use agent_session_grep_provider_pi::PiAdapter;
use agent_session_grep_provider_qoder::QoderAdapter;

/// One implemented adapter paired with the golden fixture bytes committed
/// alongside it. `include_bytes!` binds the fixture at compile time, so a
/// renamed or deleted fixture is a build error rather than a skipped test.
struct GoldenSubject {
    adapter: Box<dyn ProviderAdapter>,
    fixture: &'static [u8],
}

/// Observed behaviour of one message event (only the fields the assertions
/// below need; `MessageEvent` borrows the input so it cannot be stored).
struct ObservedMessage {
    seq: u32,
    span: Option<(u64, u64)>,
}

/// Collects what an adapter actually emitted, including tool activities.
///
/// The `emit_activity` default in `CanonicalEventSink` is a silent no-op, which
/// is exactly why an adapter that never calls it can pass every other test.
/// Here the override counts them.
#[derive(Default)]
struct ObservingSink {
    messages: Vec<ObservedMessage>,
    activities: usize,
}

impl CanonicalEventSink for ObservingSink {
    fn emit_message(&mut self, event: MessageEvent<'_>) -> PortResult<()> {
        self.messages.push(ObservedMessage {
            seq: event.seq,
            span: event.span,
        });
        Ok(())
    }

    fn emit_activity(&mut self, _event: ToolActivityEvent<'_>) -> PortResult<()> {
        self.activities += 1;
        Ok(())
    }
}

/// The 14 implemented adapters with their golden fixtures, in the same order as
/// `provider_matrix.rs::implemented_adapters` (which mirrors the CLI registry).
fn golden_subjects() -> Vec<GoldenSubject> {
    vec![
        GoldenSubject {
            adapter: Box::new(ClaudeCodeAdapter::new()),
            fixture: include_bytes!(
                "../../agent-session-grep-provider-claude/tests/golden/basic.jsonl"
            ),
        },
        GoldenSubject {
            adapter: Box::new(AiderAdapter::new()),
            fixture: include_bytes!(
                "../../agent-session-grep-provider-aider/tests/golden/basic.md"
            ),
        },
        GoldenSubject {
            adapter: Box::new(CodexAdapter::new()),
            fixture: include_bytes!(
                "../../agent-session-grep-provider-codex/tests/golden/basic.jsonl"
            ),
        },
        GoldenSubject {
            adapter: Box::new(GrokBuildAdapter::new()),
            fixture: include_bytes!(
                "../../agent-session-grep-provider-grok/tests/golden/basic.jsonl"
            ),
        },
        GoldenSubject {
            adapter: Box::new(PiAdapter::new()),
            fixture: include_bytes!(
                "../../agent-session-grep-provider-pi/tests/golden/basic.jsonl"
            ),
        },
        GoldenSubject {
            adapter: Box::new(QoderAdapter::new()),
            fixture: include_bytes!(
                "../../agent-session-grep-provider-qoder/tests/golden/basic.jsonl"
            ),
        },
        GoldenSubject {
            adapter: Box::new(KimiCodeAdapter::new()),
            fixture: include_bytes!(
                "../../agent-session-grep-provider-kimi/tests/golden/basic.jsonl"
            ),
        },
        GoldenSubject {
            adapter: Box::new(OpenClawAdapter::new()),
            fixture: include_bytes!(
                "../../agent-session-grep-provider-openclaw/tests/golden/basic.jsonl"
            ),
        },
        GoldenSubject {
            adapter: Box::new(OpenCodeAdapter::new()),
            fixture: include_bytes!(
                "../../agent-session-grep-provider-opencode/tests/golden/basic.db"
            ),
        },
        GoldenSubject {
            adapter: Box::new(CodeBuddyAdapter::new()),
            fixture: include_bytes!(
                "../../agent-session-grep-provider-codebuddy/tests/golden/basic.jsonl"
            ),
        },
        GoldenSubject {
            adapter: Box::new(ClineAdapter::new()),
            fixture: include_bytes!(
                "../../agent-session-grep-provider-cline/tests/golden/basic.json"
            ),
        },
        GoldenSubject {
            adapter: Box::new(AntigravityAdapter::new()),
            fixture: include_bytes!(
                "../../agent-session-grep-provider-antigravity/tests/golden/basic.jsonl"
            ),
        },
        GoldenSubject {
            adapter: Box::new(OpenHermesAdapter::new()),
            fixture: include_bytes!(
                "../../agent-session-grep-provider-hermes/tests/golden/basic.json"
            ),
        },
        GoldenSubject {
            adapter: Box::new(CursorAdapter::new()),
            fixture: include_bytes!(
                "../../agent-session-grep-provider-cursor/tests/golden/basic.db"
            ),
        },
    ]
}

/// Parse one subject's golden fixture and return what it emitted.
fn observe(subject: &GoldenSubject) -> ObservingSink {
    let mut sink = ObservingSink::default();
    subject
        .adapter
        .parse(subject.fixture, &mut sink)
        .unwrap_or_else(|error| {
            panic!(
                "{}: golden fixture parse must succeed, got {error:?}",
                subject.adapter.provider_id()
            )
        });
    sink
}

/// Look up the authoritative declaration for a provider id.
fn declared(provider_id: &str) -> agent_session_grep_ports::capability::ProviderCapability {
    ProviderCapabilityMatrix::current()
        .find(provider_id)
        .unwrap_or_else(|| panic!("{provider_id} must have a row in the capability matrix"))
        .clone()
}

#[test]
fn behaviour_subjects_cover_every_implemented_provider() {
    // Guard the guard: if a provider is added to the matrix without a golden
    // subject here, the behavioural assertions below would silently not apply
    // to it. That is the exact failure mode this file exists to prevent.
    let matrix = ProviderCapabilityMatrix::current();
    let implemented: Vec<&str> = matrix
        .providers
        .iter()
        .filter(|p| p.maturity != ProviderMaturity::Unsupported)
        .map(|p| p.provider_id.as_str())
        .collect();
    let subjects = golden_subjects();
    let covered: Vec<&str> = subjects
        .iter()
        .map(|s| s.adapter.provider_id())
        .collect::<Vec<_>>();

    for provider_id in &implemented {
        assert!(
            covered.contains(provider_id),
            "{provider_id} is implemented in the capability matrix but has no golden \
             behaviour subject; add one so its declarations stay evidence-backed"
        );
    }
    assert_eq!(
        covered.len(),
        implemented.len(),
        "behaviour subjects ({covered:?}) must match the implemented providers ({implemented:?})"
    );
}

/// Supplementary behavioural evidence for providers whose committed golden
/// fixture contains no tool-call records at all, so the golden alone cannot
/// witness a declared `tool_activity` capability.
///
/// This is **not** an escape hatch: the sample listed here must itself make the
/// adapter emit at least one activity (asserted below), so a provider that
/// genuinely extracts nothing — aider folds its blockquote tool output into
/// assistant text — can never be laundered through this list. Its only effect
/// is to name, in code, which golden fixtures still owe tool-call coverage.
///
/// The list is currently empty: `codex` was the last entry, and its golden
/// fixture now carries a paired and an unpaired `custom_tool_call`
/// (fixture_revision 2), so the golden itself witnesses the pairing path. Keep
/// the hook rather than the entry — a future provider whose real format puts
/// tool calls out of reach of a minimal golden can register here, and
/// `supplementary_activity_samples_are_all_still_needed` will delete it again
/// as soon as the golden covers them.
fn supplementary_activity_sample(_provider_id: &str) -> Option<&'static [u8]> {
    None
}

#[test]
fn declared_tool_activity_matches_emitted_activities() {
    // The assertion that catches a self-certifying matrix. Two directions:
    //
    // * declares Unsupported  => parsing must emit zero activities (a provider
    //   quietly emitting while declaring nothing is real drift too);
    // * declares anything else => the adapter must demonstrably emit at least
    //   one ToolActivityEvent on its behavioural evidence — its golden fixture,
    //   or the named supplementary sample when the golden carries no tool-call
    //   records (see `supplementary_activity_sample`).
    //
    // Shaped after the resume precedent in application/src/resume.rs, which
    // asserts resume == Derived iff the resume-command builder yields a command.
    for subject in golden_subjects() {
        let provider_id = subject.adapter.provider_id().to_string();
        let capability = declared(&provider_id);
        let golden_activities = observe(&subject).activities;

        if capability.tool_activity == CapabilityLevel::Unsupported {
            assert_eq!(
                golden_activities, 0,
                "provider {provider_id}: capability matrix declares tool_activity=Unsupported but \
                 parsing its golden fixture emitted {golden_activities} tool activities — the \
                 declaration understates the adapter."
            );
            continue;
        }

        let (source, evidence) = match supplementary_activity_sample(&provider_id) {
            Some(sample) => (sample, "supplementary sample"),
            None => (subject.fixture, "golden fixture"),
        };
        let mut sink = ObservingSink::default();
        subject
            .adapter
            .parse(source, &mut sink)
            .unwrap_or_else(|error| panic!("{provider_id}: {evidence} parse failed: {error:?}"));
        assert!(
            sink.activities > 0,
            "provider {provider_id}: capability matrix declares tool_activity={:?} but parsing its \
             {evidence} emitted zero tool activities. Either implement the extraction or downgrade \
             the declaration to Unsupported — the matrix must not certify itself.",
            capability.tool_activity
        );
    }
}

#[test]
fn supplementary_activity_samples_are_all_still_needed() {
    // Keep the supplementary list honest in both directions: an entry may only
    // exist while the provider's golden fixture really emits nothing (otherwise
    // the golden now covers tool calls and the entry is stale and should be
    // deleted), and the entry must actually witness the capability.
    for subject in golden_subjects() {
        let provider_id = subject.adapter.provider_id().to_string();
        let Some(sample) = supplementary_activity_sample(&provider_id) else {
            continue;
        };
        assert_eq!(
            observe(&subject).activities,
            0,
            "provider {provider_id}: golden fixture now emits tool activities, so the \
             supplementary sample entry is stale — delete it and let the golden carry the evidence."
        );
        let mut sink = ObservingSink::default();
        subject
            .adapter
            .parse(sample, &mut sink)
            .unwrap_or_else(|error| {
                panic!("{provider_id}: supplementary sample must parse, got {error:?}")
            });
        assert!(
            sink.activities > 0,
            "provider {provider_id}: supplementary sample emitted zero activities, so it witnesses \
             nothing — this list must never be used to excuse a non-extracting adapter."
        );
    }
}

#[test]
fn declared_source_span_is_backed_by_byte_precise_spans() {
    // A declared source_span level other than Unsupported must be backed by a
    // real byte range on every emitted message: present, non-empty, and inside
    // the fixture bytes. `Derived` (aider) is still a byte range — it is an
    // approximation of the record boundary, not an absent one.
    //
    // Deliberately one-directional: this catches overclaims (declaring a span
    // capability without producing spans) and leaves underclaims (producing
    // spans while declaring Unsupported) to be reported and promoted with
    // evidence, per RFC-0002 §6 (maturity moves on evidence, not on code
    // existence).
    for subject in golden_subjects() {
        let provider_id = subject.adapter.provider_id().to_string();
        let capability = declared(&provider_id);
        if capability.source_span == CapabilityLevel::Unsupported {
            continue;
        }
        let fixture_len = subject.fixture.len() as u64;
        let observed = observe(&subject);
        assert!(
            !observed.messages.is_empty(),
            "provider {provider_id}: golden fixture must emit messages to back a \
             source_span={:?} declaration",
            capability.source_span
        );
        for message in &observed.messages {
            let (start, end) = message.span.unwrap_or_else(|| {
                panic!(
                    "provider {provider_id}: capability matrix declares source_span={:?} but \
                     message seq={} carries no span. Either produce byte spans or downgrade the \
                     declaration to Unsupported.",
                    capability.source_span, message.seq
                )
            });
            assert!(
                start < end,
                "provider {provider_id}: message seq={} span ({start}, {end}) must be a \
                 non-empty byte range",
                message.seq
            );
            assert!(
                end <= fixture_len,
                "provider {provider_id}: message seq={} span end {end} exceeds the {fixture_len} \
                 fixture bytes — a span must address real captured bytes",
                message.seq
            );
        }
    }
}
