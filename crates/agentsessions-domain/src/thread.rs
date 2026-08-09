//! Deterministic, placement-aware context selection over one validated Session
//! graph. The module is pure and performs no I/O.

use crate::{
    DomainError, DomainResult, Message, MessageEdge, MessagePlacement, SessionContextGraph,
};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

/// Session context selection policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextPolicy {
    Mainline,
    Full,
}

/// A selected contextual branch in root-to-leaf order.
#[derive(Debug, PartialEq)]
pub struct BranchSelection<'a> {
    pub leaf: &'a MessagePlacement,
    pub placements: Vec<&'a MessagePlacement>,
}

/// Select the deterministic contextual mainline.
///
/// Non-sidechain placements form the candidate graph. When every placement is
/// sidechain, all placements are candidates rather than inventing a mainline.
/// Parent Message IDs resolve within that candidate graph. Multiple matches are
/// accepted only when exactly one is in the child's source document.
pub fn select_mainline(graph: &SessionContextGraph) -> DomainResult<Option<BranchSelection<'_>>> {
    graph.validate()?;

    let messages = messages_by_id(graph);
    let edges: BTreeMap<&str, &MessageEdge> = graph
        .edges
        .iter()
        .map(|edge| (edge.child_placement_id.as_str(), edge))
        .collect();

    let all_placements: Vec<&MessagePlacement> = graph.placements.iter().collect();
    let mut candidates: Vec<&MessagePlacement> = all_placements
        .iter()
        .copied()
        .filter(|placement| !placement.is_sidechain)
        .collect();
    if candidates.is_empty() {
        candidates.extend(all_placements.iter().copied());
    }
    if candidates.is_empty() {
        return Ok(None);
    }

    let mut resolved_parents: BTreeMap<&str, Option<&MessagePlacement>> = BTreeMap::new();
    for child in &candidates {
        resolved_parents.insert(
            child.id.as_str(),
            resolve_parent(child, &all_placements, &edges)?,
        );
    }

    // A message referenced as a parent by any candidate's edge is internal to
    // the mainline graph: none of its occurrences are leaves, even when the
    // resolved parent happens to pick a different occurrence. Real transcripts
    // carry the same native message in several documents (cross-file copies),
    // so a stale occurrence of an internal message must not win the leaf
    // selection over the child that actually points at it.
    let parent_message_ids: BTreeSet<&str> = candidates
        .iter()
        .filter_map(|candidate| {
            edges
                .get(candidate.id.as_str())
                .map(|edge| edge.parent_message_id.as_str())
        })
        .collect();
    let leaves: Vec<&MessagePlacement> = candidates
        .iter()
        .copied()
        .filter(|candidate| !parent_message_ids.contains(candidate.message_id.as_str()))
        .collect();

    let leaf = leaves
        .into_iter()
        .max_by(|left, right| compare_placements(left, right, &messages))
        .or_else(|| {
            candidates
                .iter()
                .copied()
                .max_by(|left, right| compare_placements(left, right, &messages))
        });
    let Some(leaf) = leaf else {
        return Ok(None);
    };

    let mut placements = vec![leaf];
    let mut visited = BTreeSet::new();
    visited.insert(leaf.id.as_str());
    let mut current = leaf;
    loop {
        let parent = match resolved_parents.get(current.id.as_str()).copied() {
            Some(parent) => parent,
            None => resolve_parent(current, &all_placements, &edges)?,
        };
        let Some(parent) = parent else {
            break;
        };
        if !visited.insert(parent.id.as_str()) {
            break;
        }
        placements.push(parent);
        current = parent;
    }
    placements.reverse();

    Ok(Some(BranchSelection { leaf, placements }))
}

/// Return every placement in deterministic contextual order.
pub fn select_full(graph: &SessionContextGraph) -> DomainResult<Vec<&MessagePlacement>> {
    graph.validate()?;
    let messages = messages_by_id(graph);
    let mut placements: Vec<&MessagePlacement> = graph.placements.iter().collect();
    placements.sort_by(|left, right| compare_placements(left, right, &messages));
    Ok(placements)
}

fn messages_by_id(graph: &SessionContextGraph) -> BTreeMap<&str, &Message> {
    graph
        .messages
        .iter()
        .map(|message| (message.id.as_str(), message))
        .collect()
}

fn resolve_parent<'a>(
    child: &'a MessagePlacement,
    candidates: &[&'a MessagePlacement],
    edges: &BTreeMap<&str, &MessageEdge>,
) -> DomainResult<Option<&'a MessagePlacement>> {
    let Some(edge) = edges.get(child.id.as_str()) else {
        return Ok(None);
    };

    let matching: Vec<&MessagePlacement> = candidates
        .iter()
        .copied()
        .filter(|placement| placement.message_id.as_str() == edge.parent_message_id.as_str())
        .collect();
    match matching.as_slice() {
        [] => Ok(None),
        [parent] => Ok(Some(*parent)),
        _ => {
            let same_document: Vec<&MessagePlacement> = matching
                .into_iter()
                .filter(|placement| {
                    placement.source_document_id.as_str() == child.source_document_id.as_str()
                })
                .collect();
            match same_document.as_slice() {
                [parent] => Ok(Some(*parent)),
                _ => Err(DomainError::AmbiguousGraph(format!(
                    "child placement {} has multiple possible parents",
                    child.id
                ))),
            }
        }
    }
}

/// Compare two timestamps in ISO-8601 UTC form.
///
/// Raw byte comparison reverses order when the two values disagree about the
/// fractional-second digits (`"00:04:00Z"` vs `"00:04:00.123Z"`: `'Z'` (0x5A)
/// sorts after `'.'` (0x2E) even though `.123Z` is later). Providers emit both
/// shapes, so the timestamps are normalized before comparison: a missing
/// fraction is zero-padded to the microsecond, matching the longer form's
/// length.
fn cmp_timestamps(left: &str, right: &str) -> std::cmp::Ordering {
    const FRACTION_LEN: usize = 6;
    fn normalize(value: &str) -> String {
        // A value without a fraction keeps its trailing `Z`; strip it first so
        // the base never carries the marker while a fractional value does.
        let value = value.trim_end_matches('Z');
        let (base, fraction) = match value.rsplit_once('.') {
            Some((base, fraction)) => (base, fraction),
            None => (value, ""),
        };
        let mut out = String::with_capacity(base.len() + 1 + FRACTION_LEN);
        out.push_str(base);
        out.push('.');
        out.push_str(&fraction[..fraction.len().min(FRACTION_LEN)]);
        for _ in 0..FRACTION_LEN.saturating_sub(fraction.len()) {
            out.push('0');
        }
        out
    }
    normalize(left).cmp(&normalize(right))
}

fn compare_placements(
    left: &MessagePlacement,
    right: &MessagePlacement,
    messages: &BTreeMap<&str, &Message>,
) -> Ordering {
    let left_timestamp = messages
        .get(left.message_id.as_str())
        .and_then(|message| message.timestamp.as_deref());
    let right_timestamp = messages
        .get(right.message_id.as_str())
        .and_then(|message| message.timestamp.as_deref());

    match (left_timestamp, right_timestamp) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Less,
        (Some(_), None) => Ordering::Greater,
        (Some(left), Some(right)) => cmp_timestamps(left, right),
    }
    .then_with(|| {
        left.source_document_id
            .as_str()
            .as_bytes()
            .cmp(right.source_document_id.as_str().as_bytes())
    })
    .then_with(|| left.source_ordinal.cmp(&right.source_ordinal))
    .then_with(|| {
        left.id
            .as_str()
            .as_bytes()
            .cmp(right.id.as_str().as_bytes())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{IdKind, MessageRelation, PlacementId, Role, SourceDocument, Stability, StableId};

    fn id(kind: IdKind, tag: &str) -> StableId {
        StableId::derive(kind, Stability::Reconstructed, &[tag.as_bytes()])
    }

    fn message(tag: &str, timestamp: Option<&str>) -> Message {
        Message {
            id: id(IdKind::Message, tag),
            role: Role::User,
            text: tag.to_string(),
            timestamp: timestamp.map(str::to_string),
        }
    }

    fn document(tag: &str) -> SourceDocument {
        SourceDocument {
            id: id(IdKind::Document, tag),
            provider_id: "test-provider".into(),
            variant_id: "test-provider/v1".into(),
            fingerprint: format!("fingerprint-{tag}"),
            len: 1024,
        }
    }

    fn placement(
        session_id: &StableId,
        document_id: &StableId,
        message_id: &StableId,
        source_ordinal: u32,
        is_sidechain: bool,
    ) -> MessagePlacement {
        MessagePlacement::new(
            session_id.clone(),
            document_id.clone(),
            message_id.clone(),
            source_ordinal,
            is_sidechain,
            None,
        )
    }

    fn edge(child: &MessagePlacement, parent: &Message) -> MessageEdge {
        MessageEdge {
            child_placement_id: child.id.clone(),
            parent_message_id: parent.id.clone(),
            parent_native_id: None,
            relation: MessageRelation::Reply,
        }
    }

    fn graph(
        session_id: StableId,
        messages: Vec<Message>,
        source_documents: Vec<SourceDocument>,
        placements: Vec<MessagePlacement>,
        edges: Vec<MessageEdge>,
    ) -> SessionContextGraph {
        SessionContextGraph {
            session_id,
            messages,
            source_documents,
            placements,
            edges,
        }
    }

    fn ordinals(placements: &[&MessagePlacement]) -> Vec<u32> {
        placements
            .iter()
            .map(|placement| placement.source_ordinal)
            .collect()
    }

    #[test]
    fn stale_occurrence_of_internal_message_is_not_a_leaf() {
        // BLOCKER-1 regression: the same message M appears in documents A and
        // B; child C in document B has an edge to M. M is internal to the
        // graph, so the stale occurrence M_a must not be chosen as the leaf
        // over C. All timestamps are None, so ordering alone cannot save us:
        // the message-level exclusion must.
        let session = id(IdKind::Session, "session");
        let doc_a = document("doc-a");
        let doc_b = document("doc-b");
        let m = message("m", None);
        let c = message("c", None);
        let m_id = m.id.clone();
        let c_id = c.id.clone();
        let m_a = placement(&session, &doc_a.id, &m_id, 0, false);
        let m_b = placement(&session, &doc_b.id, &m_id, 0, false);
        let c_b = placement(&session, &doc_b.id, &c_id, 1, false);
        let m_b_id = m_b.id.clone();
        let c_b_id = c_b.id.clone();
        let edge_cb_m = edge(&c_b, &m);
        let graph = graph(
            session,
            vec![m, c],
            vec![doc_a, doc_b],
            vec![m_a, m_b, c_b],
            vec![edge_cb_m],
        );
        let branch = select_mainline(&graph).unwrap().unwrap();
        assert_eq!(
            branch
                .placements
                .iter()
                .map(|p| p.id.as_str())
                .collect::<Vec<_>>(),
            vec![m_b_id.as_str(), c_b_id.as_str()],
            "mainline must run through the document-B occurrence of M"
        );
        assert_eq!(branch.leaf.id, c_b_id);
    }

    #[test]
    fn timestamp_cmp_normalizes_fractional_digits() {
        assert_eq!(
            cmp_timestamps("2026-01-01T00:04:00Z", "2026-01-01T00:04:00.123Z"),
            Ordering::Less
        );
        assert_eq!(
            cmp_timestamps("2026-01-01T00:04:00.123Z", "2026-01-01T00:04:00Z"),
            Ordering::Greater
        );
        assert_eq!(
            cmp_timestamps("2026-01-01T00:04:00.5Z", "2026-01-01T00:04:00.500000Z"),
            Ordering::Equal
        );
        assert_eq!(
            cmp_timestamps("2026-01-01T00:04:01Z", "2026-01-01T00:04:00.999Z"),
            Ordering::Greater
        );
    }

    #[test]
    fn linear_chain_returns_root_to_leaf_placements() {
        let session = id(IdKind::Session, "session");
        let document = document("document");
        let root = message("root", Some("2026-07-28T00:00:00Z"));
        let middle = message("middle", Some("2026-07-28T00:00:01Z"));
        let leaf = message("leaf", Some("2026-07-28T00:00:02Z"));
        let root_placement = placement(&session, &document.id, &root.id, 0, false);
        let middle_placement = placement(&session, &document.id, &middle.id, 1, false);
        let leaf_placement = placement(&session, &document.id, &leaf.id, 2, false);
        let graph = graph(
            session,
            vec![root.clone(), middle.clone(), leaf],
            vec![document],
            vec![
                root_placement,
                middle_placement.clone(),
                leaf_placement.clone(),
            ],
            vec![
                edge(&middle_placement, &root),
                edge(&leaf_placement, &middle),
            ],
        );

        let selected = select_mainline(&graph).unwrap().unwrap();
        assert_eq!(selected.leaf.id, leaf_placement.id);
        assert_eq!(ordinals(&selected.placements), vec![0, 1, 2]);
    }

    #[test]
    fn divergent_contextual_parents_select_per_session() {
        let parent_a = message("parent-a", None);
        let parent_b = message("parent-b", None);
        let shared_child = message("shared-child", None);

        let session_a = id(IdKind::Session, "session-a");
        let document_a = document("document-a");
        let parent_a_placement = placement(&session_a, &document_a.id, &parent_a.id, 0, false);
        let child_a_placement = placement(&session_a, &document_a.id, &shared_child.id, 1, false);
        let graph_a = graph(
            session_a,
            vec![parent_a.clone(), shared_child.clone()],
            vec![document_a],
            vec![parent_a_placement.clone(), child_a_placement.clone()],
            vec![edge(&child_a_placement, &parent_a)],
        );

        let session_b = id(IdKind::Session, "session-b");
        let document_b = document("document-b");
        let parent_b_placement = placement(&session_b, &document_b.id, &parent_b.id, 0, false);
        let child_b_placement = placement(&session_b, &document_b.id, &shared_child.id, 1, false);
        let graph_b = graph(
            session_b,
            vec![parent_b.clone(), shared_child],
            vec![document_b],
            vec![parent_b_placement.clone(), child_b_placement.clone()],
            vec![edge(&child_b_placement, &parent_b)],
        );

        let selected_a = select_mainline(&graph_a).unwrap().unwrap();
        let selected_b = select_mainline(&graph_b).unwrap().unwrap();
        assert_eq!(selected_a.placements[0].message_id, parent_a.id);
        assert_eq!(selected_b.placements[0].message_id, parent_b.id);
        assert_ne!(selected_a.leaf.id, selected_b.leaf.id);
    }

    #[test]
    fn same_document_parent_disambiguates_repeated_message() {
        let session = id(IdKind::Session, "session");
        let document_a = document("a");
        let document_b = document("b");
        let parent = message("parent", None);
        let child = message("child", Some("2026-07-28T00:00:00Z"));
        let parent_a = placement(&session, &document_a.id, &parent.id, 0, false);
        let parent_b = placement(&session, &document_b.id, &parent.id, 0, false);
        let child_b = placement(&session, &document_b.id, &child.id, 1, false);
        let graph = graph(
            session,
            vec![parent.clone(), child],
            vec![document_a, document_b],
            vec![parent_a, parent_b.clone(), child_b.clone()],
            vec![edge(&child_b, &parent)],
        );

        let selected = select_mainline(&graph).unwrap().unwrap();
        assert_eq!(selected.leaf.id, child_b.id);
        assert_eq!(selected.placements[0].id, parent_b.id);
        assert_eq!(selected.placements.len(), 2);
    }

    #[test]
    fn same_document_parent_resolution_considers_sidechain_placements() {
        let session = id(IdKind::Session, "session");
        let document_a = document("a");
        let document_b = document("b");
        let root = message("root", Some("2026-07-28T00:00:00Z"));
        let parent = message("parent", Some("2026-07-28T00:00:01Z"));
        let child = message("child", Some("2026-07-28T00:00:02Z"));
        let root_b = placement(&session, &document_b.id, &root.id, 0, false);
        let parent_a = placement(&session, &document_a.id, &parent.id, 0, false);
        let parent_b = placement(&session, &document_b.id, &parent.id, 1, true);
        let child_b = placement(&session, &document_b.id, &child.id, 2, false);
        let graph = graph(
            session,
            vec![root.clone(), parent.clone(), child],
            vec![document_a, document_b],
            vec![root_b.clone(), parent_a, parent_b.clone(), child_b.clone()],
            vec![edge(&parent_b, &root), edge(&child_b, &parent)],
        );

        let selected = select_mainline(&graph).unwrap().unwrap();
        assert_eq!(selected.leaf.id, child_b.id);
        assert_eq!(
            selected
                .placements
                .iter()
                .map(|placement| placement.id.clone())
                .collect::<Vec<_>>(),
            vec![root_b.id, parent_b.id, child_b.id]
        );
    }

    #[test]
    fn unresolved_repeated_parent_is_explicitly_ambiguous() {
        let session = id(IdKind::Session, "session");
        let document_a = document("a");
        let document_b = document("b");
        let document_c = document("c");
        let parent = message("parent", None);
        let child = message("child", None);
        let parent_a = placement(&session, &document_a.id, &parent.id, 0, false);
        let parent_b = placement(&session, &document_b.id, &parent.id, 0, false);
        let child_c = placement(&session, &document_c.id, &child.id, 0, false);
        let graph = graph(
            session,
            vec![parent.clone(), child],
            vec![document_a, document_b, document_c],
            vec![parent_a, parent_b, child_c.clone()],
            vec![edge(&child_c, &parent)],
        );

        let error = select_mainline(&graph).unwrap_err();
        assert_eq!(error.code(), "ambiguous_graph");
    }

    #[test]
    fn orphan_parent_stops_walk_without_invalidating_graph() {
        let session = id(IdKind::Session, "session");
        let document = document("document");
        let child = message("child", None);
        let orphan = message("orphan", None);
        let child_placement = placement(&session, &document.id, &child.id, 0, false);
        let graph = graph(
            session,
            vec![child],
            vec![document],
            vec![child_placement.clone()],
            vec![edge(&child_placement, &orphan)],
        );

        let selected = select_mainline(&graph).unwrap().unwrap();
        assert_eq!(selected.leaf.id, child_placement.id);
        assert_eq!(selected.placements, vec![&child_placement]);
    }

    #[test]
    fn cycles_have_deterministic_fallback_and_terminate() {
        let session = id(IdKind::Session, "session");
        let document = document("document");
        let a = message("a", None);
        let b = message("b", None);
        let a_placement = placement(&session, &document.id, &a.id, 0, false);
        let b_placement = placement(&session, &document.id, &b.id, 1, false);
        let graph = graph(
            session,
            vec![a.clone(), b.clone()],
            vec![document],
            vec![a_placement.clone(), b_placement.clone()],
            vec![edge(&a_placement, &b), edge(&b_placement, &a)],
        );

        let selected = select_mainline(&graph).unwrap().unwrap();
        assert_eq!(selected.leaf.id, b_placement.id);
        assert_eq!(ordinals(&selected.placements), vec![0, 1]);
    }

    #[test]
    fn sidechains_are_excluded_and_all_sidechains_fall_back_honestly() {
        let session = id(IdKind::Session, "session");
        let document = document("document");
        let root = message("root", None);
        let main = message("main", None);
        let side = message("side", Some("2026-07-28T00:00:00Z"));
        let root_placement = placement(&session, &document.id, &root.id, 0, false);
        let main_placement = placement(&session, &document.id, &main.id, 1, false);
        let side_placement = placement(&session, &document.id, &side.id, 2, true);
        let mixed = graph(
            session.clone(),
            vec![root.clone(), main.clone(), side.clone()],
            vec![document.clone()],
            vec![
                root_placement.clone(),
                main_placement.clone(),
                side_placement.clone(),
            ],
            vec![edge(&main_placement, &root), edge(&side_placement, &main)],
        );

        let selected = select_mainline(&mixed).unwrap().unwrap();
        assert_eq!(selected.leaf.id, main_placement.id);
        assert_eq!(ordinals(&selected.placements), vec![0, 1]);
        assert_eq!(select_full(&mixed).unwrap().len(), 3);

        let side_root = placement(&session, &document.id, &root.id, 0, true);
        let side_leaf = placement(&session, &document.id, &side.id, 1, true);
        let all_sidechain = graph(
            session,
            vec![root.clone(), side],
            vec![document],
            vec![side_root.clone(), side_leaf.clone()],
            vec![edge(&side_leaf, &root)],
        );
        let selected = select_mainline(&all_sidechain).unwrap().unwrap();
        assert_eq!(selected.leaf.id, side_leaf.id);
        assert_eq!(ordinals(&selected.placements), vec![0, 1]);
    }

    #[test]
    fn true_leaf_wins_even_when_missing_timestamp_sorts_first() {
        let session = id(IdKind::Session, "session");
        let document = document("document");
        let internal = message("internal", Some("9999-12-31T23:59:59Z"));
        let leaf = message("leaf", None);
        let internal_placement = placement(&session, &document.id, &internal.id, 99, false);
        let leaf_placement = placement(&session, &document.id, &leaf.id, 0, false);
        let graph = graph(
            session,
            vec![internal.clone(), leaf],
            vec![document],
            vec![internal_placement.clone(), leaf_placement.clone()],
            vec![edge(&leaf_placement, &internal)],
        );

        let full = select_full(&graph).unwrap();
        assert_eq!(full[0].id, leaf_placement.id);
        assert_eq!(full[1].id, internal_placement.id);

        let selected = select_mainline(&graph).unwrap().unwrap();
        assert_eq!(selected.leaf.id, leaf_placement.id);
        assert_eq!(
            selected.placements,
            vec![&internal_placement, &leaf_placement]
        );
    }

    #[test]
    fn full_order_is_timestamp_document_ordinal_then_placement_id() {
        let session = id(IdKind::Session, "session");
        let mut document_a = document("a");
        document_a.id = StableId::native(IdKind::Document, "a");
        let mut document_b = document("b");
        document_b.id = StableId::native(IdKind::Document, "b");
        let missing_a_1 = message("missing-a-1", None);
        let missing_a_3 = message("missing-a-3", None);
        let missing_b = message("missing-b", None);
        let earlier = message("earlier", Some("2025-01-01T00:00:00Z"));
        let later = message("later", Some("2026-01-01T00:00:00Z"));
        let p_missing_a_1 = placement(&session, &document_a.id, &missing_a_1.id, 1, false);
        let p_missing_a_3 = placement(&session, &document_a.id, &missing_a_3.id, 3, false);
        let p_missing_b = placement(&session, &document_b.id, &missing_b.id, 0, false);
        let p_earlier = placement(&session, &document_b.id, &earlier.id, 1, false);
        let p_later = placement(&session, &document_a.id, &later.id, 2, false);
        let graph = graph(
            session,
            vec![later, missing_b, missing_a_3, earlier, missing_a_1],
            vec![document_b, document_a],
            vec![
                p_later.clone(),
                p_missing_b.clone(),
                p_missing_a_3.clone(),
                p_earlier.clone(),
                p_missing_a_1.clone(),
            ],
            Vec::new(),
        );

        let selected = select_full(&graph).unwrap();
        assert_eq!(
            selected
                .iter()
                .map(|placement| placement.id.clone())
                .collect::<Vec<PlacementId>>(),
            vec![
                p_missing_a_1.id,
                p_missing_a_3.id,
                p_missing_b.id,
                p_earlier.id,
                p_later.id,
            ]
        );
    }

    #[test]
    fn empty_graph_returns_empty_selections() {
        let graph = graph(
            id(IdKind::Session, "session"),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        assert!(select_mainline(&graph).unwrap().is_none());
        assert!(select_full(&graph).unwrap().is_empty());
    }

    #[test]
    fn selector_rejects_invalid_graph_before_selection() {
        let session = id(IdKind::Session, "session");
        let document = document("document");
        let message = message("message", None);
        let mut invalid = placement(&session, &document.id, &message.id, 0, false);
        invalid.id = PlacementId::derive(&session, &document.id, &message.id, 1);
        let graph = graph(
            session,
            vec![message],
            vec![document],
            vec![invalid],
            Vec::new(),
        );

        assert_eq!(
            select_mainline(&graph).unwrap_err().code(),
            "invariant_violation"
        );
        assert_eq!(
            select_full(&graph).unwrap_err().code(),
            "invariant_violation"
        );
    }

    #[test]
    fn context_policy_serde_is_snake_case() {
        assert_eq!(
            serde_json::to_string(&ContextPolicy::Mainline).unwrap(),
            "\"mainline\""
        );
        assert_eq!(
            serde_json::from_str::<ContextPolicy>("\"full\"").unwrap(),
            ContextPolicy::Full
        );
    }
}
