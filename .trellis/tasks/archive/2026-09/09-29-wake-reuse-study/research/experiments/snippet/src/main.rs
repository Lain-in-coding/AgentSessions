use std::{env, fs, io::Write, process::ExitCode};

use wake_reuse_snippet::report::{build_report, validate_report};

const USAGE: &str = "Usage: wake-reuse-snippet report [output.json]\n       wake-reuse-snippet validate-report <report.json>";

fn run() -> Result<bool, Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    match args.as_slice() {
        [command] if command == "--help" || command == "-h" => {
            println!("{USAGE}");
            Ok(true)
        }
        [command, rest @ ..] if command == "report" && rest.len() <= 1 => {
            let report = build_report()?;
            let mut bytes = serde_json::to_vec_pretty(&report)?;
            bytes.push(b'\n');
            if let Some(path) = rest.first() {
                fs::write(path, bytes)?;
            } else {
                std::io::stdout().lock().write_all(&bytes)?;
            }
            Ok(report["status"] == "passed")
        }
        [command, path] if command == "validate-report" => {
            let report = serde_json::from_slice(&fs::read(path)?)?;
            validate_report(&report)?;
            println!("Report matches a fresh deterministic synthetic replay.");
            Ok(true)
        }
        _ => Err(USAGE.into()),
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => {
            eprintln!("Synthetic expectations failed; inspect the generated report.");
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
