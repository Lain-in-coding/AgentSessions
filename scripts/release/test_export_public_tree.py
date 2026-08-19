#!/usr/bin/env python3

from __future__ import annotations

import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("export_public_tree.py")
SPEC = importlib.util.spec_from_file_location("export_public_tree", SCRIPT)
assert SPEC and SPEC.loader
export_public_tree = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(export_public_tree)

# Reuse the exporter's fragment-assembled tracker token so this test never
# contains a literal internal reference of its own.
TRACKER = export_public_tree._TRACKER


class PublicTreeExportTests(unittest.TestCase):
    def test_tracked_paths_exclude_internal_records(self) -> None:
        with tempfile.TemporaryDirectory() as temp_name:
            repo = Path(temp_name)
            import subprocess

            subprocess.run(["git", "init", "-q", str(repo)], check=True)
            subprocess.run(["git", "-C", str(repo), "config", "user.email", "test@example.invalid"], check=True)
            subprocess.run(["git", "-C", str(repo), "config", "user.name", "Test"], check=True)
            for relative in [
                "README.md",
                f".{TRACKER}/tasks/x.md",
                ".codex/hook.py",
                "scripts/evidence/out/report.json",
                "spikes/probe/EVIDENCE.md",
                "src/lib.rs",
            ]:
                path = repo / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(relative, encoding="utf-8")
            subprocess.run(["git", "-C", str(repo), "add", "."], check=True)
            subprocess.run(["git", "-C", str(repo), "commit", "-qm", "fixture"], check=True)
            paths = export_public_tree.tracked_paths(repo, "HEAD")
            # spikes/ ships: the ADRs and RFCs cite its EVIDENCE.md files.
            self.assertEqual(paths, ["README.md", "spikes/probe/EVIDENCE.md", "src/lib.rs"])

    def test_tracked_paths_exclude_exact_files_without_their_directories(self) -> None:
        """Each excluded file drops out while its siblings still ship."""
        with tempfile.TemporaryDirectory() as temp_name:
            repo = Path(temp_name)
            import subprocess

            subprocess.run(["git", "init", "-q", str(repo)], check=True)
            subprocess.run(["git", "-C", str(repo), "config", "user.email", "test@example.invalid"], check=True)
            subprocess.run(["git", "-C", str(repo), "config", "user.name", "Test"], check=True)
            siblings = [
                "scripts/evidence/core_beta_benchmark.py",
                "scripts/release/build-manifest.py",
                "docs/release/rehearsal-runbook.md",
                "docs/operations/INSTALL-AND-UPGRADE.md",
                "docs/architecture/RFC-0001.md",
            ]
            for relative in [*export_public_tree.EXCLUDED_FILES, *siblings]:
                path = repo / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(relative, encoding="utf-8")
            subprocess.run(["git", "-C", str(repo), "add", "."], check=True)
            subprocess.run(["git", "-C", str(repo), "commit", "-qm", "fixture"], check=True)
            paths = export_public_tree.tracked_paths(repo, "HEAD")
            for excluded in export_public_tree.EXCLUDED_FILES:
                self.assertNotIn(excluded, paths)
            self.assertEqual(sorted(paths), sorted(siblings))

    def test_every_dated_go_no_go_record_is_excluded(self) -> None:
        """A go/no-go record is excluded by date prefix, not by being listed.

        The exclusion list previously named one dated record explicitly, so every
        later rehearsal would have published its own decision draft. The failure
        is silent -- the export succeeds and the internal record ships -- so the
        rule is pinned here rather than relying on someone remembering to extend
        a list.
        """
        with tempfile.TemporaryDirectory() as temp_name:
            repo = Path(temp_name)
            import subprocess

            subprocess.run(["git", "init", "-q", str(repo)], check=True)
            subprocess.run(
                ["git", "-C", str(repo), "config", "user.email", "test@example.invalid"],
                check=True,
            )
            subprocess.run(["git", "-C", str(repo), "config", "user.name", "Test"], check=True)
            dated = [
                "docs/release/go-no-go.2026-08-16.md",
                "docs/release/go-no-go.2026-08-19.md",
                "docs/release/go-no-go.2031-12-31.md",
            ]
            keep = ["docs/release/rehearsal-runbook.md"]
            for relative in [*dated, *keep]:
                path = repo / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(relative, encoding="utf-8")
            subprocess.run(["git", "-C", str(repo), "add", "."], check=True)
            subprocess.run(["git", "-C", str(repo), "commit", "-qm", "fixture"], check=True)
            paths = export_public_tree.tracked_paths(repo, "HEAD")
            for record in dated:
                self.assertNotIn(record, paths, f"dated record shipped: {record}")
            self.assertEqual(paths, keep)

    def test_scan_export_uses_the_public_profile(self) -> None:
        # An internal tracker reference is invisible to the default profile but
        # must fail the export, so the exporter has to select `public`.
        repo_root = Path(__file__).resolve().parents[2]
        with tempfile.TemporaryDirectory() as temp_name:
            destination = Path(temp_name)
            (destination / "notes.md").write_text(
                f"see .{TRACKER}/tasks/x/prd.md\n", encoding="utf-8"
            )
            self.assertEqual(export_public_tree.scan_export(repo_root, destination), 1)
            (destination / "notes.md").write_text("no internal refs\n", encoding="utf-8")
            self.assertEqual(export_public_tree.scan_export(repo_root, destination), 0)

    def test_scan_export_exempts_no_file(self) -> None:
        """A manifest-named file inside the export is scanned like any other.

        The exporter used to skip its own manifest, which made the only file it
        wrote the only file it never checked. The manifest now lands outside the
        destination, so nothing under it is exempt.
        """
        repo_root = Path(__file__).resolve().parents[2]
        with tempfile.TemporaryDirectory() as temp_name:
            destination = Path(temp_name)
            (destination / export_public_tree.MANIFEST_NAME).write_text(
                f'{{"note": "see .{TRACKER}/tasks/x/prd.md"}}\n', encoding="utf-8"
            )
            self.assertEqual(export_public_tree.scan_export(repo_root, destination), 1)

    def test_export_manifest_is_sorted_and_path_free(self) -> None:
        with tempfile.TemporaryDirectory() as temp_name:
            root = Path(temp_name)
            repo = root / "repo"
            destination = root / "public"
            manifest_path = root / export_public_tree.MANIFEST_NAME
            import subprocess

            subprocess.run(["git", "init", "-q", str(repo)], check=True)
            subprocess.run(["git", "-C", str(repo), "config", "user.email", "test@example.invalid"], check=True)
            subprocess.run(["git", "-C", str(repo), "config", "user.name", "Test"], check=True)
            (repo / "b.txt").write_text("b", encoding="utf-8")
            (repo / "a.txt").write_text("a", encoding="utf-8")
            subprocess.run(["git", "-C", str(repo), "add", "."], check=True)
            subprocess.run(["git", "-C", str(repo), "commit", "-qm", "fixture"], check=True)
            commit = export_public_tree.git(repo, "rev-parse", "HEAD")
            manifest = export_public_tree.export_tree(
                repo, destination, commit, manifest_path
            )
            self.assertEqual([entry["path"] for entry in manifest["files"]], ["a.txt", "b.txt"])
            parsed = json.loads(manifest_path.read_text(encoding="utf-8"))
            self.assertEqual(parsed["source_commit"], commit)
            self.assertNotIn(str(root), json.dumps(parsed))

    def test_manifest_is_never_written_into_the_export(self) -> None:
        """The manifest stays a local artifact, so the tree cannot ship it."""
        with tempfile.TemporaryDirectory() as temp_name:
            root = Path(temp_name)
            repo = root / "repo"
            destination = root / "public"
            manifest_path = root / export_public_tree.MANIFEST_NAME
            import subprocess

            subprocess.run(["git", "init", "-q", str(repo)], check=True)
            subprocess.run(["git", "-C", str(repo), "config", "user.email", "test@example.invalid"], check=True)
            subprocess.run(["git", "-C", str(repo), "config", "user.name", "Test"], check=True)
            (repo / "a.txt").write_text("a", encoding="utf-8")
            subprocess.run(["git", "-C", str(repo), "add", "."], check=True)
            subprocess.run(["git", "-C", str(repo), "commit", "-qm", "fixture"], check=True)
            commit = export_public_tree.git(repo, "rev-parse", "HEAD")
            export_public_tree.export_tree(repo, destination, commit, manifest_path)
            self.assertTrue(manifest_path.is_file())
            self.assertEqual(
                [path.name for path in destination.rglob("*") if path.is_file()],
                ["a.txt"],
            )

    def test_export_refuses_a_manifest_inside_the_destination(self) -> None:
        with tempfile.TemporaryDirectory() as temp_name:
            root = Path(temp_name)
            repo = root / "repo"
            destination = root / "public"
            import subprocess

            subprocess.run(["git", "init", "-q", str(repo)], check=True)
            subprocess.run(["git", "-C", str(repo), "config", "user.email", "test@example.invalid"], check=True)
            subprocess.run(["git", "-C", str(repo), "config", "user.name", "Test"], check=True)
            (repo / "a.txt").write_text("a", encoding="utf-8")
            subprocess.run(["git", "-C", str(repo), "add", "."], check=True)
            subprocess.run(["git", "-C", str(repo), "commit", "-qm", "fixture"], check=True)
            commit = export_public_tree.git(repo, "rev-parse", "HEAD")
            with self.assertRaises(ValueError):
                export_public_tree.export_tree(
                    repo,
                    destination,
                    commit,
                    destination / export_public_tree.MANIFEST_NAME,
                )


if __name__ == "__main__":
    unittest.main()
