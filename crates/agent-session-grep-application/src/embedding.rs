//! Deterministic embedding model stub (#3).
//!
//! This is a placeholder embedding model that produces deterministic vectors
//! from text using BLAKE3 hashing. It is NOT a real semantic embedding — it
//! does not capture meaning, only exact/near-exact string similarity via
//! hashing. It exists to make the semantic search pipeline end-to-end
//! testable before a real model (ONNX Runtime / candle) is integrated.
//!
//! When a real model is available, replace this with an `EmbeddingModel`
//! implementation that loads and runs the actual model weights. The
//! `SemanticIndex` and `hybrid::fuse` code does not need to change.
//!
//! The stub produces 384-dimensional vectors (matching multilingual-e5-small)
//! and uses query/passage prefixes per the E5 convention.

use agent_session_grep_ports::{EmbeddingManifest, EmbeddingModel, PortResult};

/// The dimension of the stub embedding (matches multilingual-e5-small).
pub const STUB_DIMENSION: usize = 384;

/// Manifest for the deterministic stub model.
pub fn stub_manifest() -> EmbeddingManifest {
    EmbeddingManifest {
        model_id: "stub-hash-v1".to_string(),
        file_hash: "none (deterministic stub, no model file)".to_string(),
        dimension: STUB_DIMENSION,
        license: "MIT OR Apache-2.0".to_string(),
    }
}

/// A deterministic embedding model stub.
///
/// Produces 384-dim vectors from BLAKE3(text). Same text → same vector.
/// Two texts with shared substrings produce vectors with some shared
/// dimensions (via bigram hashing), giving approximate similarity.
pub struct StubEmbeddingModel {
    manifest: EmbeddingManifest,
}

impl StubEmbeddingModel {
    pub fn new() -> Self {
        Self {
            manifest: stub_manifest(),
        }
    }
}

impl Default for StubEmbeddingModel {
    fn default() -> Self {
        Self::new()
    }
}

impl EmbeddingModel for StubEmbeddingModel {
    fn embed(&self, text: &str, is_query: bool) -> PortResult<Vec<f32>> {
        // E5 convention: prefix query/passage.
        let prefixed = if is_query {
            format!("query: {text}")
        } else {
            format!("passage: {text}")
        };
        Ok(hash_to_vector(&prefixed, STUB_DIMENSION))
    }

    fn dimension(&self) -> usize {
        STUB_DIMENSION
    }

    fn manifest(&self) -> &EmbeddingManifest {
        &self.manifest
    }
}

/// Hash text to a fixed-dimensional L2-normalized vector.
///
/// Uses BLAKE3 to produce a deterministic byte stream, then maps it to
/// float dimensions. For approximate similarity, we also hash bigrams of
/// the text and accumulate their contributions into the same vector space
/// — texts sharing bigrams will have correlated dimensions.
fn hash_to_vector(text: &str, dim: usize) -> Vec<f32> {
    let mut vec = vec![0.0f32; dim];

    // Full-text hash → seeds the vector.
    let full_hash = blake3::hash(text.as_bytes());
    for (i, byte) in full_hash.as_bytes().iter().enumerate() {
        let idx = (i * 7 + *byte as usize) % dim;
        vec[idx] += (*byte as f32 / 255.0) * 0.5;
    }

    // Bigram hashing for approximate similarity.
    let chars: Vec<char> = text.chars().collect();
    for window in chars.windows(2) {
        let bigram: String = window.iter().collect();
        let h = blake3::hash(bigram.as_bytes());
        let bytes = h.as_bytes();
        let idx = (u16::from_le_bytes([bytes[0], bytes[1]]) as usize) % dim;
        vec[idx] += 1.0;
    }

    // L2 normalize.
    let norm: f32 = vec.iter().map(|v| v * v).sum::<f32>().sqrt();
    if norm > 0.0 {
        for v in &mut vec {
            *v /= norm;
        }
    }

    vec
}

/// Cosine similarity between two vectors (dot product of L2-normalized vectors).
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_produces_correct_dimension() {
        let model = StubEmbeddingModel::new();
        let emb = model.embed("hello world", false).unwrap();
        assert_eq!(emb.len(), STUB_DIMENSION);
        assert_eq!(model.dimension(), STUB_DIMENSION);
    }

    #[test]
    fn same_text_produces_same_vector() {
        let model = StubEmbeddingModel::new();
        let a = model.embed("hello world", false).unwrap();
        let b = model.embed("hello world", false).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn different_text_produces_different_vector() {
        let model = StubEmbeddingModel::new();
        let a = model.embed("hello world", false).unwrap();
        let b = model.embed("goodbye universe", false).unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn query_and_passage_prefixes_differ() {
        let model = StubEmbeddingModel::new();
        let q = model.embed("hello", true).unwrap();
        let p = model.embed("hello", false).unwrap();
        assert_ne!(q, p);
    }

    #[test]
    fn similar_texts_have_higher_similarity_than_dissimilar() {
        let model = StubEmbeddingModel::new();
        let a = model.embed("the quick brown fox", false).unwrap();
        let b = model.embed("the quick brown dog", false).unwrap();
        let c = model.embed("completely different text", false).unwrap();
        let sim_ab = cosine_similarity(&a, &b);
        let sim_ac = cosine_similarity(&a, &c);
        // Texts sharing bigrams ("the ", "he q", " qu", etc.) should be
        // more similar than completely different texts.
        assert!(
            sim_ab >= sim_ac,
            "sim(a,b)={sim_ab} should be >= sim(a,c)={sim_ac}"
        );
    }

    #[test]
    fn vectors_are_l2_normalized() {
        let model = StubEmbeddingModel::new();
        let emb = model.embed("some text here for testing", false).unwrap();
        let norm: f32 = emb.iter().map(|v| v * v).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 0.01, "norm should be ~1.0, got {norm}");
    }

    #[test]
    fn cosine_similarity_identical_is_one() {
        let model = StubEmbeddingModel::new();
        let a = model.embed("identical text", false).unwrap();
        let sim = cosine_similarity(&a, &a);
        assert!(
            (sim - 1.0).abs() < 0.01,
            "self-similarity should be ~1.0, got {sim}"
        );
    }

    #[test]
    fn manifest_is_stamped() {
        let model = StubEmbeddingModel::new();
        let m = model.manifest();
        assert_eq!(m.model_id, "stub-hash-v1");
        assert_eq!(m.dimension, STUB_DIMENSION);
    }

    #[test]
    fn empty_text_does_not_panic() {
        let model = StubEmbeddingModel::new();
        let emb = model.embed("", false).unwrap();
        assert_eq!(emb.len(), STUB_DIMENSION);
    }

    #[test]
    fn different_dimensions_dont_match() {
        let sim = cosine_similarity(&[1.0, 0.0], &[1.0, 0.0, 0.0]);
        assert_eq!(sim, 0.0);
    }
}
