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

impl StableId {
    /// Adopt a provider-native id verbatim (tier [`Stability::Native`]).
    ///
    /// The raw id is sanitized (whitespace trimmed) but otherwise preserved so
    /// that round-tripping back to the provider is lossless.
    pub fn native(kind: IdKind, raw: &str) -> Self {
        let sanitized = raw.trim();
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
    /// stability tier. Returns `None` if no known prefix matches.
    pub fn from_wire(wire: &str) -> Option<Self> {
        for kind in [
            IdKind::Source,
            IdKind::Document,
            IdKind::Session,
            IdKind::Message,
        ] {
            if wire.starts_with(kind.prefix()) {
                return Some(StableId {
                    kind,
                    stability: Stability::Unstable,
                    value: wire.to_string(),
                });
            }
        }
        None
    }
}

impl fmt::Display for StableId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.value)
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
}
