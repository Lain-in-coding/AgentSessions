//! Structured provider adapter declarations required by RFC-0002 §6.

use crate::capability::{ProviderCapability, ProviderCapabilityMatrix, ProviderMaturity};

/// Machine-readable metadata for one implemented provider adapter.
///
/// The capability row, provider id, current maturity, and current variant are
/// projected from [`ProviderCapabilityMatrix::current`] by [`manifest_for`].
/// Adapters supply only evidence that is not part of the capability matrix.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AdapterManifest {
    /// Canonical provider id.
    pub provider_id: String,
    /// Variants supported by this adapter implementation.
    pub supported_variants: Vec<String>,
    /// Current evidence-backed maturity, independent from target maturity.
    pub maturity: ProviderMaturity,
    /// Authoritative per-field capability row for this provider.
    pub capabilities: ProviderCapability,
    /// Revision from a provider golden fixture's `PROVENANCE.md`, if one exists.
    pub fixture_revision: Option<u32>,
    /// Named targets from successful certification workflow runs.
    pub last_certified_targets: Vec<String>,
    /// Concise, user-relevant limits of the current adapter implementation.
    pub known_limitations: Vec<String>,
}

/// Build an adapter manifest from the authoritative capability matrix row.
///
/// This intentionally has no unknown-provider fallback: every implemented
/// adapter must have an explicit matrix row before it can expose a manifest.
pub fn manifest_for(
    provider_id: &str,
    fixture_revision: Option<u32>,
    known_limitations: &[&str],
) -> AdapterManifest {
    let capabilities = ProviderCapabilityMatrix::current()
        .providers
        .into_iter()
        .find(|capability| capability.provider_id == provider_id)
        .unwrap_or_else(|| panic!("provider `{provider_id}` has no capability matrix entry"));
    let supported_variants = if capabilities.variant_id.is_empty() {
        Vec::new()
    } else {
        vec![capabilities.variant_id.clone()]
    };

    AdapterManifest {
        provider_id: capabilities.provider_id.clone(),
        supported_variants,
        maturity: capabilities.maturity,
        capabilities,
        fixture_revision,
        last_certified_targets: Vec::new(),
        known_limitations: known_limitations
            .iter()
            .map(|limitation| (*limitation).to_string())
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const IMPLEMENTED_PROVIDERS: [&str; 14] = [
        "claude-code",
        "codex",
        "grok-build",
        "antigravity",
        "opencode",
        "pi",
        "hermes",
        "cursor",
        "kimi-code",
        "openclaw",
        "qoder",
        "tencent-codebuddy",
        "cline",
        "aider",
    ];

    fn fixture_revision(provider_id: &str) -> Option<u32> {
        match provider_id {
            "claude-code" | "codex" => Some(1),
            _ => None,
        }
    }

    #[test]
    fn implemented_manifests_match_authoritative_capability_rows() {
        let matrix = ProviderCapabilityMatrix::current();

        for provider_id in IMPLEMENTED_PROVIDERS {
            let manifest = manifest_for(provider_id, fixture_revision(provider_id), &[]);
            let capability = matrix
                .find(provider_id)
                .unwrap_or_else(|| panic!("missing matrix row for {provider_id}"));

            assert_eq!(manifest.provider_id, capability.provider_id);
            assert_eq!(
                manifest.supported_variants.as_slice(),
                std::slice::from_ref(&capability.variant_id)
            );
            assert_eq!(manifest.maturity, capability.maturity);
            assert_eq!(&manifest.capabilities, capability);
            assert_eq!(manifest.maturity, ProviderMaturity::Experimental);
            assert!(manifest.last_certified_targets.is_empty());
        }
    }

    #[test]
    fn only_provenance_backed_fixture_revisions_are_declared() {
        for provider_id in IMPLEMENTED_PROVIDERS {
            let manifest = manifest_for(provider_id, fixture_revision(provider_id), &[]);
            let expected = match provider_id {
                "claude-code" | "codex" => Some(1),
                _ => None,
            };
            assert_eq!(manifest.fixture_revision, expected, "{provider_id}");
        }
    }

    #[test]
    fn json_serialization_contains_every_rfc_field() {
        let manifest = manifest_for(
            "claude-code",
            Some(1),
            &["tool activity extraction is partial"],
        );
        let json = serde_json::to_value(&manifest).expect("manifest must serialize");

        for field in [
            "provider_id",
            "supported_variants",
            "maturity",
            "capabilities",
            "fixture_revision",
            "last_certified_targets",
            "known_limitations",
        ] {
            assert!(json.get(field).is_some(), "missing JSON field `{field}`");
        }

        let round_trip: AdapterManifest =
            serde_json::from_value(json).expect("manifest must deserialize");
        assert_eq!(round_trip, manifest);
    }
}
