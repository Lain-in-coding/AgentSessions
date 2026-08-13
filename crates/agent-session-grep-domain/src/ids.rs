//! Typed stable identifiers (RFC-0001).
//!
//! Every entity in the Canonical Model carries a *typed* stable id. Ids are
//! opaque strings with a versioned type prefix so that a raw id string alone
//! is self-describing and mistyping one id for another is a compile error.
//!
//! # Identity vs location
//!
//! A stable id encodes an entity's *identity* — what it is — and never its
//! *location* — where the bytes currently live on disk. Locations move
//! (files get renamed, data roots migrate); identity must survive that. The
//! hash inputs below are deliberately restricted to intrinsic, relocation-
//! invariant facts.
//!
//! # Stability tiers
//!
//! Not every provider gives us a durable native id. We record how much to
//! trust an id's stability via [`Stability`]:
//!
//! * [`Stability::Native`] — the provider emitted a durable id we adopt verbatim.
//! * [`Stability::Reconstructed`] — we derived a deterministic id from intrinsic
//!   content the provider *does* guarantee (e.g. a session's first-message
//!   timestamp + provider tag). Stable across re-ingest of the same source.
//! * [`Stability::Unstable`] — best-effort id derived from facts that may shift
//!   between runs. Callers must not persist cross-run references to these.

use serde::{Deserialize, Serialize};
use std::fmt;

/// The versioned type tag that prefixes every stable id string.
///
/// The `_v1_` infix is a format version: if the hashing scheme for a kind ever
/// changes, we bump to `_v2_` and both can coexist during migration.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum IdKind {
    /// A discovered source (a provider's on-disk history location).
    Source,
    /// A single history document within a source.
    Document,
    /// A conversation/session reconstructed from one or more documents.
    Session,
    /// A single message (turn) within a session.
    Message,
}

impl IdKind {
    /// The stable, wire-visible prefix for this kind, including the trailing `_`.
    pub const fn prefix(self) -> &'static str {
        match self {
            IdKind::Source => "src_v1_",
            IdKind::Document => "doc_v1_",
            IdKind::Session => "ses_v1_",
            IdKind::Message => "msg_v1_",
        }
    }
}

impl fmt::Display for IdKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            IdKind::Source => "source",
            IdKind::Document => "document",
            IdKind::Session => "session",
            IdKind::Message => "message",
        })
    }
}

/// How much an id's stability can be trusted across re-ingest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Stability {
    /// Adopted verbatim from a durable provider-native id.
    Native,
    /// Deterministically derived from intrinsic content; stable across re-ingest.
    Reconstructed,
    /// Best-effort; may shift between runs. Do not persist cross-run references.
    Unstable,
}

/// A typed, opaque, stable identifier.
///
/// The wire form is `<prefix><hex-blake3-digest>` for derived ids, or
/// `<prefix><adopted>` for native ids where `<adopted>` is a sanitized copy of
/// the provider's own id. Construct via [`StableId::native`] or
/// [`StableId::derive`]; never hand-assemble the string.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StableId {
    kind: IdKind,
    stability: Stability,
    /// Full wire string, prefix included.
    value: String,
}

/// Length (in hex chars) of the truncated BLAKE3 digest used in derived ids.
///
/// 32 hex chars = 128 bits. Collision-resistant far beyond the id population we
/// will ever hold, while keeping ids short enough to eyeball in logs.
const DIGEST_HEX_LEN: usize = 32;
const PLACEMENT_ID_PREFIX: &str = "plc_v1_";

/// Maximum length (in chars) of the adopted suffix of a native id.
///
/// Provider-native ids are trusted-but-unbounded strings; an extreme value
/// would pollute logs and JSON responses. Every known provider id is far
/// shorter (Claude Code uuids are 36 chars, Codex session ids shorter), so
/// the clamp never truncates a legitimate id and the lossless round-trip
/// contract holds for all real inputs.
const NATIVE_SUFFIX_MAX_CHARS: usize = 256;

impl StableId {
    /// Adopt a provider-native id verbatim (tier [`Stability::Native`]).
    ///
    /// The raw id is sanitized (trimmed, control characters stripped, suffix
    /// clamped to [`NATIVE_SUFFIX_MAX_CHARS`]) so that hostile or pathological
    /// ids cannot break logs or JSON output; within those bounds the id is
    /// preserved so that round-tripping back to the provider is lossless.
    pub fn native(kind: IdKind, raw: &str) -> Self {
        let sanitized: String = raw
            .trim()
            .chars()
            .filter(|c| !c.is_control())
            .take(NATIVE_SUFFIX_MAX_CHARS)
            .collect();
        StableId {
            kind,
            stability: Stability::Native,
            value: format!("{}{sanitized}", kind.prefix()),
        }
    }

    /// Derive a deterministic id by hashing intrinsic identity facts.
    ///
    /// `facts` are hashed in order with an unambiguous length-prefixed framing
    /// so that `["a", "bc"]` and `["ab", "c"]` never collide. Pass only
    /// relocation-invariant inputs (never absolute paths, mtimes, or run-local
    /// counters) or the resulting id will not be reproducible.
    ///
    /// `stability` should be [`Stability::Reconstructed`] when every fact is
    /// provider-guaranteed, or [`Stability::Unstable`] otherwise.
    pub fn derive(kind: IdKind, stability: Stability, facts: &[&[u8]]) -> Self {
        let mut hasher = blake3::Hasher::new();
        // Domain-separate by kind so identical facts under different kinds
        // never produce the same digest.
        hasher.update(kind.prefix().as_bytes());
        for fact in facts {
            hasher.update(&(fact.len() as u64).to_le_bytes());
            hasher.update(fact);
        }
        let digest = hasher.finalize();
        let hex = digest.to_hex();
        let truncated = &hex.as_str()[..DIGEST_HEX_LEN];
        StableId {
            kind,
            stability,
            value: format!("{}{truncated}", kind.prefix()),
        }
    }

    /// The entity kind this id names.
    pub fn kind(&self) -> IdKind {
        self.kind
    }

    /// The stability tier of this id.
    pub fn stability(&self) -> Stability {
        self.stability
    }

    /// The full wire string, prefix included.
    pub fn as_str(&self) -> &str {
        &self.value
    }

    /// Reconstruct an id from a wire string received from outside (e.g. a CLI
    /// argument or an API caller echoing back an id we emitted).
    ///
    /// The `kind` is recovered from the versioned prefix. Stability is *not*
    /// encoded on the wire, so a reconstructed id is always [`Stability::Unstable`]:
    /// we cannot prove how the original was derived. This is honest — callers
    /// must not persist cross-run references built from a wire round-trip.
    ///
    /// Catalog lookups key on [`StableId::as_str`], which is preserved exactly,
    /// so a wire round-trip resolves the same stored entity regardless of the
    /// stability tier. Returns `None` if no known prefix matches, or if only a
    /// bare prefix is given (an empty adopted suffix is not a valid id —
    /// [`StableId::validate`] rejects it the same way).
    pub fn from_wire(wire: &str) -> Option<Self> {
        for kind in [
            IdKind::Source,
            IdKind::Document,
            IdKind::Session,
            IdKind::Message,
        ] {
            if let Some(suffix) = wire.strip_prefix(kind.prefix()) {
                if suffix.is_empty() {
                    return None;
                }
                return Some(StableId {
                    kind,
                    stability: Stability::Unstable,
                    value: wire.to_string(),
                });
            }
        }
        None
    }

    /// Verify the wire value is consistent with the declared kind.
    ///
    /// A `StableId` deserialized from untrusted JSON could pair an arbitrary
    /// kind with a mismatched value (e.g. kind `Message` but value
    /// `"ses_v1_..."`); nothing else in the codebase would catch that because
    /// entity validation only inspects `kind()`. Rejects empty adopted values
    /// too, so all-messages-with-empty-native-ids cannot collide on one id.
    pub fn validate(&self) -> bool {
        !self.value.is_empty()
            && self.value.starts_with(self.kind.prefix())
            && !self.value[self.kind.prefix().len()..].is_empty()
    }
}

impl fmt::Display for StableId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.value)
    }
}

/// A deterministic identity for one contextual occurrence of a Message.
///
/// A placement is not a canonical entity and therefore does not use
/// [`StableId`] or a stability tier. Its identity is derived only from logical,
/// path-independent context: session, source document, message, and the
/// source-local ordinal.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PlacementId(String);

impl PlacementId {
    /// Derive the occurrence identity from relocation-invariant context.
    pub fn derive(
        session_id: &StableId,
        source_document_id: &StableId,
        message_id: &StableId,
        source_ordinal: u32,
    ) -> Self {
        let ordinal = source_ordinal.to_le_bytes();
        let facts = [
            session_id.as_str().as_bytes(),
            source_document_id.as_str().as_bytes(),
            message_id.as_str().as_bytes(),
            ordinal.as_slice(),
        ];
        let mut hasher = blake3::Hasher::new();
        hasher.update(PLACEMENT_ID_PREFIX.as_bytes());
        for fact in facts {
            hasher.update(&(fact.len() as u64).to_le_bytes());
            hasher.update(fact);
        }
        let digest = hasher.finalize();
        let hex = digest.to_hex();
        Self(format!(
            "{PLACEMENT_ID_PREFIX}{}",
            &hex.as_str()[..DIGEST_HEX_LEN]
        ))
    }

    /// Reconstruct a placement id from its wire representation.
    pub fn from_wire(wire: &str) -> Option<Self> {
        let digest = wire.strip_prefix(PLACEMENT_ID_PREFIX)?;
        if digest.len() == DIGEST_HEX_LEN && digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            Some(Self(wire.to_string()))
        } else {
            None
        }
    }

    /// The full wire string, including the versioned `plc_v1_` prefix.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PlacementId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_preserves_and_prefixes() {
        let id = StableId::native(IdKind::Source, "  claude-code  ");
        assert_eq!(id.as_str(), "src_v1_claude-code");
        assert_eq!(id.kind(), IdKind::Source);
        assert_eq!(id.stability(), Stability::Native);
    }

    #[test]
    fn native_strips_control_characters_and_clamps_oversized_suffixes() {
        // 控制字符(换行/DEL/BEL)会破坏日志与 JSON,必须剥离。
        let control = StableId::native(IdKind::Message, "ab\nc\u{7f}d\u{0007}e");
        assert_eq!(control.as_str(), "msg_v1_abcde");
        assert!(control.validate());

        // 超长 native id 截断到上限;已知 provider id 都远短于此。
        let oversized = StableId::native(IdKind::Message, &"x".repeat(300));
        assert_eq!(
            oversized.as_str().len(),
            "msg_v1_".len() + NATIVE_SUFFIX_MAX_CHARS
        );
        assert!(oversized.validate());
    }

    #[test]
    fn derive_is_deterministic() {
        let a = StableId::derive(
            IdKind::Session,
            Stability::Reconstructed,
            &[b"claude", b"2026-07-21T00:00:00Z"],
        );
        let b = StableId::derive(
            IdKind::Session,
            Stability::Reconstructed,
            &[b"claude", b"2026-07-21T00:00:00Z"],
        );
        assert_eq!(a, b);
        assert!(a.as_str().starts_with("ses_v1_"));
    }

    #[test]
    fn framing_prevents_boundary_collision() {
        let a = StableId::derive(IdKind::Message, Stability::Reconstructed, &[b"a", b"bc"]);
        let b = StableId::derive(IdKind::Message, Stability::Reconstructed, &[b"ab", b"c"]);
        assert_ne!(a, b);
    }

    #[test]
    fn kind_domain_separation() {
        let s = StableId::derive(IdKind::Session, Stability::Reconstructed, &[b"x"]);
        let m = StableId::derive(IdKind::Message, Stability::Reconstructed, &[b"x"]);
        assert_ne!(s.as_str()[7..].to_string(), m.as_str()[7..].to_string());
    }

    #[test]
    fn derived_digest_length() {
        let id = StableId::derive(IdKind::Document, Stability::Reconstructed, &[b"doc"]);
        assert_eq!(id.as_str().len(), "doc_v1_".len() + DIGEST_HEX_LEN);
    }

    #[test]
    fn from_wire_roundtrips_kind_and_value() {
        let original = StableId::derive(IdKind::Message, Stability::Reconstructed, &[b"a", b"b"]);
        let reparsed = StableId::from_wire(original.as_str()).unwrap();
        // Value (the catalog lookup key) is preserved exactly, so the round-trip
        // resolves the same stored entity.
        assert_eq!(reparsed.as_str(), original.as_str());
        assert_eq!(reparsed.kind(), IdKind::Message);
        // Stability is not on the wire; a reconstructed id is always Unstable.
        assert_eq!(reparsed.stability(), Stability::Unstable);
    }

    #[test]
    fn from_wire_rejects_unknown_prefix() {
        assert!(StableId::from_wire("bogus_v1_deadbeef").is_none());
        assert!(StableId::from_wire("").is_none());
    }

    #[test]
    fn from_wire_rejects_bare_prefix_with_empty_suffix() {
        // 裸前缀不是合法 id——与 validate() 的结论一致(空 adopted 后缀)。
        assert!(StableId::from_wire("msg_v1_").is_none());
        assert!(StableId::from_wire("ses_v1_").is_none());
        assert!(StableId::from_wire("doc_v1_").is_none());
        assert!(StableId::from_wire("src_v1_").is_none());
        // 非空后缀(含 native 形态)仍可反解。
        assert!(StableId::from_wire("msg_v1_legacy").is_some());
    }

    #[test]
    fn placement_id_is_deterministic_and_path_independent() {
        let session = StableId::derive(IdKind::Session, Stability::Reconstructed, &[b"session"]);
        let document = StableId::derive(
            IdKind::Document,
            Stability::Reconstructed,
            &[b"document-bytes"],
        );
        let message = StableId::native(IdKind::Message, "native-message");

        let at_original_path = PlacementId::derive(&session, &document, &message, 7);
        let after_source_move = PlacementId::derive(&session, &document, &message, 7);

        assert_eq!(at_original_path, after_source_move);
        assert!(at_original_path.as_str().starts_with("plc_v1_"));
        assert_eq!(
            at_original_path.as_str().len(),
            PLACEMENT_ID_PREFIX.len() + DIGEST_HEX_LEN
        );
    }

    #[test]
    fn placement_id_changes_with_each_identity_component() {
        let session = StableId::derive(IdKind::Session, Stability::Reconstructed, &[b"session"]);
        let other_session = StableId::derive(
            IdKind::Session,
            Stability::Reconstructed,
            &[b"other-session"],
        );
        let document = StableId::derive(IdKind::Document, Stability::Reconstructed, &[b"document"]);
        let other_document = StableId::derive(
            IdKind::Document,
            Stability::Reconstructed,
            &[b"other-document"],
        );
        let message = StableId::native(IdKind::Message, "message");
        let other_message = StableId::native(IdKind::Message, "other-message");
        let base = PlacementId::derive(&session, &document, &message, 1);

        assert_ne!(
            base,
            PlacementId::derive(&other_session, &document, &message, 1)
        );
        assert_ne!(
            base,
            PlacementId::derive(&session, &other_document, &message, 1)
        );
        assert_ne!(
            base,
            PlacementId::derive(&session, &document, &other_message, 1)
        );
        assert_ne!(base, PlacementId::derive(&session, &document, &message, 2));
    }

    #[test]
    fn placement_id_wire_roundtrip_is_strict() {
        let session = StableId::native(IdKind::Session, "session");
        let document = StableId::native(IdKind::Document, "document");
        let message = StableId::native(IdKind::Message, "message");
        let original = PlacementId::derive(&session, &document, &message, 0);

        assert_eq!(
            PlacementId::from_wire(original.as_str()).as_ref(),
            Some(&original)
        );
        assert!(PlacementId::from_wire("plc_v1_short").is_none());
        assert!(PlacementId::from_wire("plc_v1_zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz").is_none());
        assert!(PlacementId::from_wire("msg_v1_00000000000000000000000000000000").is_none());
    }
}
