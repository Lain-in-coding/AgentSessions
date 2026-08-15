//! Resume command descriptor and dry-run preview (#5).
//!
//! Builds the resume command from `SessionResumeMetadata`. Default is
//! dry-run preview (prints the command, does not execute). `--yes` / config
//! opt-in is required for actual execution; first install forces preview
//! once regardless.
//!
//! This module owns the resume descriptor/preview; the actual provider process
//! spawn is deferred to the execution layer (requires owner authorization per
//! CLAUDE.md). Handoff pack (#4) only consumes the descriptor, never executes.

use agent_session_grep_ports::SessionResumeMetadata;

/// Provider resume command descriptor: the command + args + cwd + permission
/// mode that would restore the session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumeDescriptor {
    /// Provider binary name (e.g. `claude`, `codex`, `pi`).
    pub provider_binary: String,
    /// Command arguments (e.g. `["--resume", "<session-id>"]`).
    pub args: Vec<String>,
    /// Original working directory to restore (if known).
    pub working_directory: Option<String>,
    /// Permission/approval mode flag (e.g. `--dangerously-skip-permissions`).
    /// `None` means default (no yolo/auto mode).
    pub permission_mode: Option<String>,
}

/// Dry-run preview result: what would be executed, without executing it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumePreview {
    pub descriptor: ResumeDescriptor,
    /// Full command as a displayable string (e.g. `claude --resume abc-123`).
    pub command_string: String,
    /// Whether the session is resumable at all.
    pub available: bool,
    /// Why the session is not resumable (when `available` is false).
    pub unavailable_reason: Option<String>,
}

/// Build a resume descriptor from session metadata.
///
/// Unknown/unverified providers return `available:false` with a reason —
/// never fabricates a command. Only providers with known resume commands
/// produce a descriptor.
pub fn build_resume_descriptor(metadata: &SessionResumeMetadata) -> ResumePreview {
    if !metadata.resume_available {
        return ResumePreview {
            descriptor: ResumeDescriptor {
                provider_binary: String::new(),
                args: Vec::new(),
                working_directory: metadata.original_working_directory.clone(),
                permission_mode: None,
            },
            command_string: String::new(),
            available: false,
            unavailable_reason: metadata
                .unavailable_reason
                .clone()
                .or_else(|| Some("resume not available".to_string())),
        };
    }

    let provider_id = metadata.provider_id.as_deref().unwrap_or("");
    let session_id = metadata.provider_session_id.as_deref().unwrap_or("");

    let (binary, args) = match provider_id {
        "claude-code" => (
            "claude",
            vec!["--resume".to_string(), session_id.to_string()],
        ),
        "codex" => ("codex", vec!["resume".to_string(), session_id.to_string()]),
        "pi" => ("pi", vec!["--session".to_string(), session_id.to_string()]),
        "grok-build" => ("grok", vec!["--resume".to_string(), session_id.to_string()]),
        // Unknown/unverified providers: resume command is null/— (not fabricated).
        // These include: opencode, antigravity, hermes, kimi-code (version conflicts),
        // and all not-yet-implemented providers.
        _ => {
            return ResumePreview {
                descriptor: ResumeDescriptor {
                    provider_binary: String::new(),
                    args: Vec::new(),
                    working_directory: metadata.original_working_directory.clone(),
                    permission_mode: None,
                },
                command_string: String::new(),
                available: false,
                unavailable_reason: Some(format!(
                    "resume command for provider '{provider_id}' is unverified or unsupported"
                )),
            };
        }
    };

    let mut full_args = args.clone();
    if let Some(mode) = &metadata_provider_permission_hint(provider_id) {
        full_args.push(mode.clone());
    }

    let command_string = format_command(binary, &full_args, &metadata.original_working_directory);

    ResumePreview {
        descriptor: ResumeDescriptor {
            provider_binary: binary.to_string(),
            args: full_args,
            working_directory: metadata.original_working_directory.clone(),
            permission_mode: metadata_provider_permission_hint(provider_id),
        },
        command_string,
        available: true,
        unavailable_reason: None,
    }
}

/// Format a resume command as a displayable string for dry-run preview.
fn format_command(binary: &str, args: &[String], cwd: &Option<String>) -> String {
    let mut parts = vec![binary.to_string()];
    parts.extend(args.iter().cloned());
    let cmd = parts.join(" ");
    if let Some(dir) = cwd {
        format!("(cd {dir} && {cmd})")
    } else {
        cmd
    }
}

/// Provider-specific permission mode hint (none by default — user must opt-in).
/// Returns `None` for all providers: resume never auto-carries yolo/full-auto.
fn metadata_provider_permission_hint(_provider_id: &str) -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_session_grep_domain::StableId;
    use agent_session_grep_ports::SessionResumeMetadata;

    fn metadata(
        provider: &str,
        available: bool,
        session_id: &str,
        cwd: Option<&str>,
    ) -> SessionResumeMetadata {
        SessionResumeMetadata {
            session_id: StableId::from_wire("ses_v1_test").unwrap(),
            provider_id: Some(provider.to_string()),
            resume_available: available,
            provider_session_id: if available {
                Some(session_id.to_string())
            } else {
                None
            },
            original_working_directory: cwd.map(|s| s.to_string()),
            unavailable_reason: if !available {
                Some("no resume metadata claims".to_string())
            } else {
                None
            },
        }
    }

    #[test]
    fn builds_claude_resume_command() {
        let m = metadata("claude-code", true, "abc-123", Some("/home/user/proj"));
        let preview = build_resume_descriptor(&m);
        assert!(preview.available);
        assert_eq!(preview.descriptor.provider_binary, "claude");
        assert_eq!(preview.descriptor.args, vec!["--resume", "abc-123"]);
        assert!(preview.command_string.contains("claude --resume abc-123"));
        assert!(preview.command_string.contains("/home/user/proj"));
    }

    #[test]
    fn builds_codex_resume_command() {
        let m = metadata("codex", true, "sess-456", None);
        let preview = build_resume_descriptor(&m);
        assert!(preview.available);
        assert_eq!(preview.descriptor.provider_binary, "codex");
        assert_eq!(preview.descriptor.args, vec!["resume", "sess-456"]);
    }

    #[test]
    fn builds_pi_resume_command() {
        let m = metadata("pi", true, "uuid-789", None);
        let preview = build_resume_descriptor(&m);
        assert!(preview.available);
        assert_eq!(preview.descriptor.provider_binary, "pi");
        assert_eq!(preview.descriptor.args, vec!["--session", "uuid-789"]);
    }

    #[test]
    fn unverified_provider_returns_unavailable() {
        let m = metadata("kimi-code", true, "k-sess", None);
        let preview = build_resume_descriptor(&m);
        assert!(!preview.available);
        assert!(
            preview
                .unavailable_reason
                .as_deref()
                .unwrap()
                .contains("unverified")
        );
    }

    #[test]
    fn not_available_returns_unavailable() {
        let m = metadata("claude-code", false, "", None);
        let preview = build_resume_descriptor(&m);
        assert!(!preview.available);
        assert!(preview.unavailable_reason.is_some());
        assert!(preview.command_string.is_empty());
    }

    #[test]
    fn no_permission_mode_by_default() {
        let m = metadata("claude-code", true, "abc", None);
        let preview = build_resume_descriptor(&m);
        // Resume never auto-carries yolo/full-auto — user must opt-in.
        assert!(preview.descriptor.permission_mode.is_none());
    }

    #[test]
    fn grok_build_resume_command() {
        let m = metadata("grok-build", true, "grok-sess", None);
        let preview = build_resume_descriptor(&m);
        assert!(preview.available);
        assert_eq!(preview.descriptor.provider_binary, "grok");
        assert_eq!(preview.descriptor.args, vec!["--resume", "grok-sess"]);
    }
}
