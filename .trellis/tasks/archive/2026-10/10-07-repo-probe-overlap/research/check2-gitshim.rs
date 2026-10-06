// B8 check2 git shim: logs each spawn attempt, then forwards to the real git
// with inherited stdio (so the caller's piped stdout flows through unchanged).
use std::io::Write;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Ok(log) = std::env::var("ASG_SHIM_LOG") {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&log) {
            let _ = writeln!(f, "{}", args.join(" "));
        }
    }
    let real = std::env::var("ASG_REAL_GIT").unwrap_or_else(|_| "git".to_string());
    let status = std::process::Command::new(real)
        .args(&args)
        .status()
        .expect("spawn real git");
    std::process::exit(status.code().unwrap_or(1));
}
