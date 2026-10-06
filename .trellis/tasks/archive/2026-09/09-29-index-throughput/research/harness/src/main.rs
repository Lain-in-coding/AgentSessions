use std::{collections::BTreeMap, path::PathBuf};
use wake_reuse_search::{Result, bench, runtime};

fn main() {
    if let Err(error) = run() {
        eprintln!(
            "{}",
            serde_json::json!({"kind":"error", "error": error.to_string()})
        );
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let command = args
        .next()
        .ok_or("expected runtime or run; see README.md")?;
    if command == "runtime" {
        if args.next().is_some() {
            return Err("runtime takes no arguments".into());
        }
        println!("{}", runtime()?);
        return Ok(());
    }
    if command != "run" {
        return Err("expected runtime or run".into());
    }
    let mut options = BTreeMap::new();
    while let Some(key) = args.next() {
        if !["--db", "--messages", "--samples"].contains(&key.as_str()) {
            return Err(format!("unknown option: {key}").into());
        }
        let value = args.next().ok_or("option requires a value")?;
        if options.insert(key, value).is_some() {
            return Err("duplicate option".into());
        }
    }
    let db = PathBuf::from(options.get("--db").ok_or("--db is required")?);
    let messages = options
        .get("--messages")
        .ok_or("--messages is required")?
        .parse()?;
    let samples = options
        .get("--samples")
        .ok_or("--samples is required")?
        .parse()?;
    let report = bench::run(&db, messages, samples)?;
    println!(
        "{}",
        serde_json::json!({"kind": "result", "report": report})
    );
    Ok(())
}
