use crate::{Result, sha256_bytes};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub const FIXTURE: &str = include_str!("../fixtures/beacons.json");
pub const HASH_METHOD: &str = "sha256(concat(u64le(id),u64le(utf8_bytes),utf8_text)), id=1..N";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fixture {
    pub revision: String,
    pub repeat: u64,
    pub queries: Vec<Query>,
    pub beacons: Vec<Beacon>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Query {
    pub id: String,
    pub category: String,
    pub text: String,
    pub note: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Beacon {
    pub text: String,
    pub relevant_for: Vec<String>,
}

impl Fixture {
    pub fn load() -> Result<Self> {
        let fixture: Self = serde_json::from_str(FIXTURE)?;
        let ids: BTreeSet<_> = fixture.queries.iter().map(|q| q.id.as_str()).collect();
        if ids.len() != fixture.queries.len()
            || fixture.queries.is_empty()
            || fixture.beacons.is_empty()
            || fixture.repeat == 0
            || fixture
                .beacons
                .iter()
                .any(|b| b.relevant_for.iter().any(|id| !ids.contains(id.as_str())))
        {
            return Err("invalid beacon fixture".into());
        }
        Ok(fixture)
    }

    pub fn minimum_messages(&self) -> u64 {
        self.beacons.len() as u64 * self.repeat
    }

    pub fn text(&self, id: u64) -> String {
        assert!(id > 0);
        if id <= self.minimum_messages() {
            let index = (id - 1) / self.repeat;
            format!(
                "synthetic beacon {index:02} copy {} {}",
                (id - 1) % self.repeat,
                self.beacons[index as usize].text
            )
        } else {
            let noise = id
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407)
                % 1_000_003;
            format!("synthetic filler {id:08} quiet amber stone meadow packet {noise:07}")
        }
    }

    pub fn qrels(&self, qid: &str) -> Vec<u64> {
        self.beacons
            .iter()
            .enumerate()
            .filter(|(_, b)| b.relevant_for.iter().any(|id| id == qid))
            .flat_map(|(index, _)| {
                let first = index as u64 * self.repeat + 1;
                first..first + self.repeat
            })
            .collect()
    }

    pub fn hash(&self, count: u64) -> String {
        let mut hash = Sha256::new();
        for id in 1..=count {
            update_hash(&mut hash, id, &self.text(id));
        }
        format!("{:x}", hash.finalize())
    }

    pub fn fixture_hash(&self) -> String {
        sha256_bytes(FIXTURE.as_bytes())
    }
}

pub fn update_hash(hash: &mut Sha256, id: u64, text: &str) {
    hash.update(id.to_le_bytes());
    hash.update((text.len() as u64).to_le_bytes());
    hash.update(text.as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qrels_are_beacon_only_and_hash_is_sensitive() {
        let f = Fixture::load().unwrap();
        assert_eq!(f.minimum_messages(), 96);
        assert!(f.qrels("negative").is_empty());
        assert_eq!(f.qrels("cjk_two"), [1, 2, 3, 88, 89, 90, 91, 92, 93]);
        assert_eq!(
            f.text(97),
            "synthetic filler 00000097 quiet amber stone meadow packet 0908647"
        );
        assert_eq!(f.hash(100), f.hash(100));
        assert_ne!(f.hash(100), f.hash(101));
        for query in &f.queries {
            assert!((97..=1000).all(|id| !f.text(id).contains(&query.text)));
        }
    }
}
