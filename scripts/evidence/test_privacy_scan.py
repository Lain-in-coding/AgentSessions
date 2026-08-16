import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

MODULE_PATH = Path(__file__).with_name("privacy_scan.py")
SPEC = importlib.util.spec_from_file_location("privacy_scan", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
SCANNER = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = SCANNER
SPEC.loader.exec_module(SCANNER)


class RuleTests(unittest.TestCase):
    def test_flags_personal_absolute_paths(self) -> None:
        lines = [
            "checkout root " + "C:/" + "Users/Alice/project was scanned",
            "worktree " + "/Users/" + "alice/repo also appears",
            "home directory " + "/home/" + "alice/repo",
            "escaped " + "C:" + "\\\\Users\\\\Alice\\\\transcript.jsonl",
        ]
        findings = SCANNER.scan_lines("notes.md", lines, frozenset())
        self.assertEqual(
            [(f.line, f.rule) for f in findings],
            [(1, "user-home"), (2, "user-home"), (3, "user-home"), (4, "user-home")],
        )

    def test_flags_non_ascii_username_machine_roots_and_worktrees(self) -> None:
        private_username = chr(0x5C0F) + "Q"
        findings = SCANNER.scan_lines(
            "research.md",
            [
                "cache at " + "C:/" + "Users/用户/.cache/tool",
                "checkout at " + "C:/" + "AgentSessions/crates",
                "reference at " + "D:" + "\\AgentHub\\project",
                "coordinate " + "." + "claude/worktrees/agent-abc123",
                "branch " + "worktree-" + "08-15-privacy",
                "private account " + private_username,
            ],
            frozenset(),
        )
        self.assertEqual(
            [f.rule for f in findings],
            [
                "user-home",
                "machine-root",
                "machine-root",
                "agent-coordinate",
                "agent-coordinate",
                "personal-username",
            ],
        )

    def test_allowlist_accepts_only_exact_synthetic_entries(self) -> None:
        findings = SCANNER.scan_lines(
            "crates/agent-session-grep-cli/src/protocol.rs",
            ['error "cannot open ' + "C:/" + 'Users/secret/transcript.jsonl"'],
        )
        self.assertEqual(findings, [])

        # Same path shape in a file that is not allowlisted must still flag.
        findings = SCANNER.scan_lines(
            "docs/notes.md",
            ['error "cannot open ' + "C:/" + 'Users/secret/transcript.jsonl"'],
        )
        self.assertEqual(len(findings), 1)

    def test_synthetic_non_personal_roots_do_not_match(self) -> None:
        # C:/data and C:/profiles style fixtures are not user homes; the
        # scanner deliberately does not reject generic drive roots.
        findings = SCANNER.scan_lines(
            "docs/runbook.md",
            ["use --db C:/data/example.db and fixture C:/profiles/one"],
            frozenset(),
        )
        self.assertEqual(findings, [])

    def test_decode_text_skips_binary_and_utf16(self) -> None:
        self.assertIsNone(SCANNER.decode_text(b"a\x00b"))
        self.assertIsNone(SCANNER.decode_text("abc".encode("utf-16-le")))
        self.assertEqual(SCANNER.decode_text("abc".encode("utf-8")), "abc")


class RepoScanTests(unittest.TestCase):
    def test_scan_repo_reads_only_tracked_files(self) -> None:
        with tempfile.TemporaryDirectory() as name:
            repo = Path(name)
            (repo / "tracked.md").write_text(
                "cache at " + "C:/" + "Users/Alice/.cache/tool\n", encoding="utf-8"
            )
            (repo / "untracked.md").write_text(
                "cache at " + "C:/" + "Users/Bob/.cache/tool\n", encoding="utf-8"
            )
            (repo / "image.bin").write_bytes(b"\x00\x01\x02")

            with mock.patch.object(
                SCANNER, "tracked_files", return_value=["tracked.md", "image.bin"]
            ):
                findings = SCANNER.scan_repo(repo)

        self.assertEqual(len(findings), 1)
        self.assertEqual(findings[0].path, "tracked.md")
        self.assertEqual(findings[0].match, "C:/" + "Users/Alice")

    def test_main_exit_codes(self) -> None:
        with mock.patch.object(SCANNER, "scan_repo", return_value=[]):
            self.assertEqual(SCANNER.main(["--repo", "."]), 0)
        finding = SCANNER.Finding(
            path="x.md", line=1, rule="user-home", match="/" + "home/alice", excerpt=""
        )
        with mock.patch.object(SCANNER, "scan_repo", return_value=[finding]):
            self.assertEqual(SCANNER.main(["--repo", "."]), 1)


if __name__ == "__main__":
    unittest.main()
