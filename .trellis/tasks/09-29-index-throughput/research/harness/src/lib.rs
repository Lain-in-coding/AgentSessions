//! Independent search experiment, not a product adapter or a Wake build.
pub mod bench;
pub mod corpus;
pub mod query;

use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub fn sha256_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 65_536];
    loop {
        let length = file.read(&mut buffer)?;
        if length == 0 {
            break;
        }
        hash.update(&buffer[..length]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

pub fn runtime() -> Result<serde_json::Value> {
    let conn = rusqlite::Connection::open_in_memory()?;
    let source_id: String = conn.query_row("SELECT sqlite_source_id()", [], |row| row.get(0))?;
    let options = conn
        .prepare("PRAGMA compile_options")?
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(serde_json::json!({
        "kind": "linked_rust_probe",
        "rusqlite_version": "0.40.2",
        "sqlite_version": rusqlite::version(),
        "sqlite_source_id": source_id,
        "compile_options": options,
        "bundled": true,
        "default_features": false,
        "cache_feature": cfg!(feature = "cache"),
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH
    }))
}
