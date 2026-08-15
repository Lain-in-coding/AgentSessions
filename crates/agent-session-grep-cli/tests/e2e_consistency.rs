//! Consistency smoke test: shells the five-entry-point comparison script
//! (``scripts/rehearsal/compare_entrypoints.py``) in fixtures-only mode against
//! the freshly compiled binary. Asserts the report is well-formed, the overall
//! verdict is "consistent", and every not-yet-implemented entry point is an
//! explicit skip — never silently absent.
//!
//! Gated to run green TODAY: only CLI and MCP are compared; Web/TUI are
//! represented as explicit ``skipped: not implemented`` entries.

use std::path::PathBuf;
use std::process::{Command, Output};

/// The compiled binary path injected by Cargo at build time.
const BIN: &str = env!("CARGO_BIN_EXE_agent-session-grep");

/// Repo root: two levels up from this test's crate directory.
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crate dir must have a parent")
        .parent()
        .expect("workspace dir must have a parent")
        .to_path_buf()
}

fn consistency_script() -> PathBuf {
    repo_root()
        .join("scripts")
        .join("rehearsal")
        .join("compare_entrypoints.py")
}

fn run_script(args: &[&str]) -> Output {
    // Prefer `python` (Windows), fall back to `python3` (POSIX). Once an
    // interpreter successfully spawns, preserve its real exit/output instead
    // of allowing a later WindowsApps launcher stub to mask the failure.
    for interpreter in ["python", "python3"] {
        if let Ok(output) = Command::new(interpreter)
            .arg(consistency_script())
            .args(args)
            .output()
        {
            return output;
        }
    }
    panic!("failed to spawn python for compare_entrypoints.py")
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

#[test]
fn consistency_report_cli_mcp_agree_pending_skipped_explicit() {
    let script = consistency_script();
    assert!(
        script.is_file(),
        "consistency script must exist: {}",
        script.display()
    );

    let out = run_script(&["--binary", BIN, "--json"]);
    let code = out.status.code().unwrap_or(-1);
    let stderr_text = stderr(&out);
    assert!(
        out.status.success(),
        "compare_entrypoints.py must exit 0, got {code}\nstderr: {stderr_text}"
    );

    let report: serde_json::Value =
        serde_json::from_str(&stdout(&out)).expect("report must be valid JSON");

    // Schema version must match.
    assert_eq!(
        report["schema_version"].as_str().unwrap_or(""),
        "agent-session-grep.entrypoint-consistency/v1",
        "report schema_version mismatch"
    );

    // Overall verdict must be consistent.
    assert_eq!(
        report["overall_verdict"].as_str().unwrap_or(""),
        "consistent",
        "overall_verdict must be consistent, got: {}",
        serde_json::to_string_pretty(&report).unwrap_or_default()
    );

    // Every operation must be consistent.
    let operations = report["operations"]
        .as_array()
        .expect("report.operations must be an array");
    assert!(
        !operations.is_empty(),
        "report must have at least one operation"
    );
    for op in operations {
        assert_eq!(
            op["verdict"].as_str().unwrap_or(""),
            "consistent",
            "operation {} must be consistent",
            op["operation"]
        );
        // Every declared entry point must appear — compared, aliased, or
        // explicitly skipped.
        let compared: Vec<&str> = op["compared"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();
        let aliases: Vec<&str> = op["aliases"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v["entry_point"].as_str()).collect())
            .unwrap_or_default();
        let skipped: Vec<&str> = op["skipped"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v["entry_point"].as_str()).collect())
            .unwrap_or_default();
        for ep in ["cli", "mcp", "robot", "web", "tui"] {
            let in_compared = compared.contains(&ep);
            let in_aliases = aliases.contains(&ep);
            let in_skipped = skipped.contains(&ep);
            assert!(
                in_compared || in_aliases || in_skipped,
                "entry point {ep} must appear as compared, aliased, or skipped \
                 in operation {}",
                op["operation"]
            );
        }
        // Pending entry points must be explicitly skipped, not compared.
        if let Some(skips) = op["skipped"].as_array() {
            for skip in skips {
                assert_eq!(
                    skip["status"].as_str().unwrap_or(""),
                    "skipped",
                    "pending entry point must have status=skipped"
                );
                assert_eq!(
                    skip["reason"].as_str().unwrap_or(""),
                    "not implemented",
                    "pending entry point must say reason=not implemented"
                );
            }
        }
        // Aliased entry points must declare which entry point they alias.
        if let Some(alias_records) = op["aliases"].as_array() {
            for alias in alias_records {
                assert_eq!(
                    alias["status"].as_str().unwrap_or(""),
                    "alias",
                    "alias record must have status=alias"
                );
                assert!(
                    alias["alias_of"].is_string(),
                    "alias record must name alias_of"
                );
            }
        }
    }

    // Entry-point metadata must declare all five, with web/tui pending.
    let declared: Vec<&str> = report["entry_points"]["declared"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();
    assert_eq!(declared, vec!["cli", "mcp", "robot", "web", "tui"]);
    let pending: Vec<&str> = report["entry_points"]["pending"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();
    assert_eq!(pending, vec!["web", "tui"]);

    // Privacy fields must be asserted in the report.
    let privacy = &report["privacy"];
    assert_eq!(
        privacy["message_text"].as_str().unwrap_or(""),
        "never_compared"
    );
    assert_eq!(
        privacy["absolute_source_paths"].as_str().unwrap_or(""),
        "never_emitted"
    );
}

#[test]
fn consistency_script_help_runs() {
    let out = run_script(&["--help"]);
    assert!(
        out.status.success(),
        "--help must exit 0, got {:?}\nstderr: {}",
        out.status.code(),
        stderr(&out)
    );
    let text = stdout(&out);
    assert!(
        text.contains("--binary"),
        "--help must mention --binary, got: {text}"
    );
}
