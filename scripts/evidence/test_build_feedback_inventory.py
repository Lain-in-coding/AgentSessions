"""Fail-closed inventory contracts and synthetic NTFS smoke tests; no Cargo."""
from __future__ import annotations

import importlib.util
import json
import os
import subprocess
import tempfile
import unittest
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from typing import ClassVar
from unittest.mock import patch

if os.name != "nt":
    raise unittest.SkipTest("Windows build-feedback evidence suite")

MODULE_PATH = Path(__file__).with_name("build_feedback_inventory.py")
SPEC = importlib.util.spec_from_file_location("build_feedback_inventory_tests", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
INVENTORY = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(INVENTORY)


class SyntheticMetadata:
    """Controlled identity/allocation capture, while using actual directory entries."""
    volumes: ClassVar[dict[str, dict[str, str]]] = {"test-volume": {"filesystem": "NTFS"}}

    def read(self, path, include_streams=True):
        path = Path(path)
        info = path.stat()
        directory = path.is_dir()
        allocated = 0 if directory else ((info.st_size + 4095) // 4096) * 4096
        return INVENTORY._Metadata(
            ("test-volume", str(info.st_ino)), directory, allocated,
            0 if directory else info.st_size, info.st_mtime_ns, info.st_ctime_ns,
            0, str(path.resolve()), 0 if directory else 1, info.st_nlink,
        )

    def free_bytes(self):
        return 200 * INVENTORY.GIB


class InventoryContracts(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="inventory-contract-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.reader = SyntheticMetadata()
        capture = patch.object(INVENTORY, "_new_metadata_reader", return_value=self.reader)
        capture.start()
        self.addCleanup(capture.stop)

    def test_store_declaration_survives_same_path_discovery_root(self):
        (self.root / "opaque-blob").write_bytes(b"cache data")
        standalone = INVENTORY.inventory([], store_roots=[self.root])
        combined = INVENTORY.inventory([self.root], store_roots=[self.root])
        self.assertTrue(combined["complete"])
        self.assertEqual(combined["allocated_bytes"], 4096)
        self.assertEqual(combined["allocated_bytes"], standalone["allocated_bytes"])
        self.assertEqual(len(combined["roots"]), 1)
        self.assertEqual(combined["roots"][0]["kind"], "store")

    def test_missing_optional_store_and_missing_known_store_are_distinct(self):
        missing = self.root / "missing-store"
        optional = INVENTORY.inventory([], store_roots=[missing])
        self.assertTrue(optional["complete"])
        self.assertEqual(optional["roots"][0]["status"], "absent")
        known = INVENTORY.inventory([missing], store_roots=[missing])
        self.assertFalse(known["complete"])
        self.assertFalse(known["admission_allowed"])
        self.assertTrue(known["errors"])

    def test_explicit_source_git_and_download_roots_block_admission(self):
        paths = (self.root / ".git", self.root / "src", self.root / ".cargo" / "registry")
        for path in paths:
            path.mkdir(parents=True)
            (path / "unrelated").write_bytes(b"not build cache")
            for use_store in (False, True):
                for discover in (False, True):
                    with self.subTest(path_name=path.name, use_store=use_store, discover=discover):
                        report = INVENTORY.inventory(
                            [] if use_store else [path],
                            store_roots=[path] if use_store else [], discover=discover,
                        )
                        self.assertFalse(report["complete"])
                        self.assertFalse(report["admission_allowed"])
                        self.assertEqual(report["allocated_bytes"], 0)
                        self.assertTrue(report["errors"])

    def test_store_cannot_overlap_a_source_worktree(self):
        (self.root / "Cargo.toml").write_text("[workspace]\n", encoding="utf-8")
        (self.root / "opaque-blob").write_bytes(b"not cache")
        report = INVENTORY.inventory([], store_roots=[self.root])
        self.assertFalse(report["complete"])
        self.assertFalse(report["admission_allowed"])
        self.assertEqual(report["allocated_bytes"], 0)

    def test_explicit_nonartifact_file_is_not_a_cache(self):
        source = self.root / "source.rs"
        source.write_text("fn main() {}\n", encoding="utf-8")
        for discover in (False, True):
            with self.subTest(discover=discover):
                report = INVENTORY.inventory([source], discover=discover)
                self.assertFalse(report["complete"])
                self.assertEqual(report["allocated_bytes"], 0)

    def test_parent_child_and_hardlink_allocations_are_globally_unique(self):
        child = self.root / "debug"
        child.mkdir()
        first = child / "sample.rlib"
        first.write_bytes(b"artifact")
        os.link(first, self.root / "alias.rlib")
        (self.root / "independent.pdb").write_bytes(b"symbols")
        report = INVENTORY.inventory([self.root, child], discover=False)
        self.assertTrue(report["complete"])
        self.assertEqual(report["allocated_bytes"], 8192)
        self.assertEqual(report["logical_bytes"], len(b"artifact") + len(b"symbols"))
        self.assertEqual(report["file_count"], 2)
        self.assertEqual(sum(root["allocated_bytes"] for root in report["roots"]), 8192)
        self.assertEqual(sum(cache["allocated_bytes"] for cache in report["caches"]), 8192)
        self.assertTrue(any(alias["kind"] == "file" for alias in report["aliases"]))
        self.assertTrue(any(relation["kind"] == "covered_by" for relation in report["relationships"]))

    def test_directory_alias_mutation_is_not_silently_covered(self):
        first, alias = self.root / "a-cache", self.root / "z-cache"
        first.mkdir()
        alias.mkdir()
        (first / "artifact.rlib").write_bytes(b"artifact")
        original = self.reader.read

        def metadata(path, include_streams=True):
            if Path(path) == alias:
                value = original(first, include_streams)
                return value._replace(change_time=value.change_time + 1)
            return original(path, include_streams)

        for discover in (False, True):
            with self.subTest(discover=discover), patch.object(self.reader, "read", side_effect=metadata):
                report = INVENTORY.inventory([first, alias], discover=discover)
            self.assertFalse(report["complete"])
            self.assertFalse(report["admission_allowed"])
            self.assertTrue(any("changed" in error["message"] for error in report["errors"]))

    def test_runtime_target_with_nonstandard_name_is_discovered(self):
        target = self.root / "opaque-output"
        target.mkdir()
        (target / ".rustc_info.json").write_text("{}", encoding="utf-8")
        (target / "artifact.rlib").write_bytes(b"artifact")
        excluded = self.root / "src"
        excluded.mkdir()
        (excluded / "source.rs").write_bytes(b"x" * 8193)
        report = INVENTORY.inventory([self.root])
        self.assertTrue(report["complete"])
        self.assertEqual(len(report["caches"]), 1)
        self.assertEqual(report["caches"][0]["path"], str(target))
        self.assertEqual(report["allocated_bytes"], 8192)

    def test_registered_worktree_targets_are_discovered_without_scanning_git(self):
        coordinator, linked = self.root / "coordinator", self.root / "linked"
        coordinator.mkdir()
        linked.mkdir()
        (coordinator / ".git").mkdir()
        (coordinator / ".git" / "private-object").write_bytes(b"x" * 8193)
        (coordinator / "Cargo.toml").write_text("[workspace]\n", encoding="utf-8")
        target = linked / "target"
        target.mkdir()
        (target / "artifact.rlib").write_bytes(b"artifact")
        result = subprocess.CompletedProcess(
            ["git"], 0,
            stdout=("worktree " + str(coordinator) + "\0\0worktree " + str(linked) + "\0\0").encode("utf-8"),
            stderr=b"",
        )
        with patch.object(INVENTORY.subprocess, "run", return_value=result) as discovery:
            report = INVENTORY.inventory([coordinator])
        self.assertTrue(report["complete"])
        self.assertEqual(report["allocated_bytes"], 4096)
        self.assertEqual({root["path"] for root in report["roots"]}, {str(coordinator), str(linked)})
        self.assertEqual(report["caches"][0]["path"], str(target))
        discovery.assert_called_once()
        self.assertEqual(discovery.call_args.kwargs["env"]["GIT_OPTIONAL_LOCKS"], "0")

    def test_nested_source_fixture_is_excluded_but_its_targets_are_scanned(self):
        target = self.root / "target"
        target.mkdir()
        (target / "artifact.rlib").write_bytes(b"outer build")
        source = target / "fixtures" / "nested-worktree"
        source.mkdir(parents=True)
        (source / "Cargo.toml").write_text("[workspace]\n", encoding="utf-8")
        (source / ".git").mkdir()
        (source / "fixture.exe").write_bytes(b"source fixture" * 1024)
        (source / "src").mkdir()
        (source / "src" / "lib.rs").write_bytes(b"source bytes" * 1024)
        nested = source / "target"
        nested.mkdir()
        (nested / "artifact.rlib").write_bytes(b"nested build")
        for discover in (False, True):
            with self.subTest(discover=discover), patch.object(
                INVENTORY.subprocess, "run", side_effect=AssertionError("local target discovery must not query other worktrees")
            ):
                report = INVENTORY.inventory([target], discover=discover)
            self.assertTrue(report["complete"], report["errors"])
            self.assertTrue(report["admission_allowed"])
            self.assertEqual(report["allocated_bytes"], 8192)
            self.assertEqual(report["logical_bytes"], len(b"outer build") + len(b"nested build"))
            self.assertEqual(report["file_count"], 2)
            self.assertEqual({cache["path"] for cache in report["caches"]}, {str(target), str(nested)})
            self.assertEqual(sum(cache["allocated_bytes"] for cache in report["caches"]), 8192)
            self.assertTrue(any(item["path"] == str(source) and item["reason"] == "source_worktree"
                                for item in report["exclusions"]))

    def test_generated_source_named_target_directories_are_not_under_counted(self):
        target = self.root / "target"
        target.mkdir()
        (target / "artifact.rlib").write_bytes(b"build")
        names = ("src", "fixtures", "resources", "tests", "docs")
        for name in names:
            generated = target / name
            generated.mkdir()
            (generated / "generated.rs").write_bytes(b"source" * 2048)
            (generated / "generated.exe").write_bytes(b"fixture" * 2048)
        report = INVENTORY.inventory([target])
        self.assertTrue(report["complete"], report["errors"])
        self.assertEqual(report["allocated_bytes"], 147456)
        self.assertEqual(report["file_count"], 11)
        self.assertFalse(report["exclusions"])

    def test_embedded_cargo_registry_and_git_downloads_are_excluded(self):
        target = self.root / "target"
        target.mkdir()
        (target / "artifact.rlib").write_bytes(b"build")
        cargo_home = target / "custom-cargo-home"
        registry = cargo_home / "registry"
        package = registry / "src" / "index.example" / "fixture-crate"
        package.mkdir(parents=True)
        (package / "Cargo.toml").write_text("[package]\n", encoding="utf-8")
        (package / "fixture.exe").write_bytes(b"download" * 2048)
        for name in ("cache", "index"):
            download = registry / name
            download.mkdir()
            (download / "blob.rlib").write_bytes(b"download" * 2048)
        cargo_git = cargo_home / "git"
        for name in ("db", "checkouts"):
            download = cargo_git / name
            download.mkdir(parents=True)
            (download / "blob.rlib").write_bytes(b"download" * 2048)
        for discover in (False, True):
            with self.subTest(discover=discover):
                report = INVENTORY.inventory([target], discover=discover)
            self.assertTrue(report["complete"], report["errors"])
            self.assertEqual(report["allocated_bytes"], 4096)
            self.assertEqual(report["logical_bytes"], len(b"build"))
            self.assertEqual(report["file_count"], 1)
            self.assertEqual({item["path"] for item in report["exclusions"]
                              if item["reason"] == "cargo_download"}, {str(registry), str(cargo_git)})

    def test_explicit_embedded_cargo_download_roots_still_block_admission(self):
        registry = self.root / "custom-cargo-home" / "registry"
        cargo_git = self.root / "custom-cargo-home" / "git"
        for path in (registry / "src", cargo_git / "checkouts"):
            path.mkdir(parents=True)
            (path / "download.rlib").write_bytes(b"download")
        for path in (registry, registry / "src", cargo_git, cargo_git / "checkouts"):
            for use_store in (False, True):
                for discover in (False, True):
                    with self.subTest(path=str(path), use_store=use_store, discover=discover):
                        report = INVENTORY.inventory(
                            [] if use_store else [path], store_roots=[path] if use_store else [],
                            discover=discover,
                        )
                        self.assertFalse(report["complete"])
                        self.assertFalse(report["admission_allowed"])
                        self.assertEqual(report["allocated_bytes"], 0)
                        self.assertTrue(report["errors"])

    def test_git_internal_registered_worktrees_are_informational_only(self):
        coordinator, linked = self.root / "coordinator", self.root / "linked"
        coordinator.mkdir()
        linked.mkdir()
        internal = coordinator / ".git" / "modules" / "submod"
        internal.mkdir(parents=True)
        (internal / "private-object.rlib").write_bytes(b"git object" * 2048)
        target = linked / "target"
        target.mkdir()
        (target / "artifact.rlib").write_bytes(b"build")
        result = subprocess.CompletedProcess(
            ["git"], 0,
            stdout=("worktree " + str(coordinator) + "\0\0worktree " + str(internal) +
                    "\0\0worktree " + str(linked) + "\0\0").encode("utf-8"), stderr=b"",
        )
        with patch.object(INVENTORY.subprocess, "run", return_value=result):
            report = INVENTORY.inventory([coordinator])
        self.assertTrue(report["complete"], report["errors"])
        self.assertEqual({root["path"] for root in report["roots"]}, {str(coordinator), str(linked)})
        self.assertEqual(report["allocated_bytes"], 4096)
        self.assertTrue(any(item["path"] == str(internal) and item["reason"] == "git_internal"
                            for item in report["exclusions"]))
        with patch.object(INVENTORY.subprocess, "run", return_value=result):
            explicit = INVENTORY.inventory([coordinator, internal])
        self.assertFalse(explicit["complete"])
        self.assertFalse(explicit["admission_allowed"])
        self.assertTrue(any(error["path"] == str(internal) for error in explicit["errors"]))

    def test_explicit_nested_source_is_not_covered_by_parent_exclusion(self):
        target = self.root / "target"
        target.mkdir()
        (target / "artifact.rlib").write_bytes(b"build")
        source = target / "source-fixture"
        source.mkdir()
        (source / "Cargo.toml").write_text("[workspace]\n", encoding="utf-8")
        (source / "fixture.exe").write_bytes(b"fixture")
        report = INVENTORY.inventory([target, source], discover=False)
        self.assertFalse(report["complete"])
        self.assertFalse(report["admission_allowed"])
        self.assertEqual(report["allocated_bytes"], 4096)
        self.assertTrue(any(error["path"] == str(source) for error in report["errors"]))

    def test_nested_exclusions_do_not_hide_metadata_failures_or_directory_drift(self):
        target = self.root / "target"
        target.mkdir()
        artifact = target / "artifact.rlib"
        artifact.write_bytes(b"build")
        source = target / "source-fixture"
        source.mkdir()
        (source / "Cargo.toml").write_text("[workspace]\n", encoding="utf-8")
        original = self.reader.read
        for fault in ("read", "parent_drift", "source_drift"):
            calls = {}

            def metadata(path, include_streams=True, *, fault=fault, calls=calls):
                path = Path(path)
                if fault == "read" and path == artifact:
                    raise PermissionError("synthetic included artifact failure")
                value = original(path, include_streams)
                calls[path] = calls.get(path, 0) + 1
                changed = target if fault == "parent_drift" else source
                if fault != "read" and path == changed and calls[path] > 1:
                    return value._replace(change_time=value.change_time + 1)
                return value

            with self.subTest(fault=fault), patch.object(self.reader, "read", side_effect=metadata):
                report = INVENTORY.inventory([target], discover=False)
            self.assertFalse(report["complete"])
            self.assertFalse(report["admission_allowed"])
            self.assertTrue(report["exclusions"])
            self.assertTrue(any(("included artifact failure" if fault == "read" else "changed during")
                                in error["message"] for error in report["errors"]))

    def test_read_failure_and_unknown_free_space_preserve_partial_evidence(self):
        first, unreadable = self.root / "a.rlib", self.root / "z.rlib"
        first.write_bytes(b"artifact")
        unreadable.write_bytes(b"artifact")
        original = self.reader.read

        def metadata(path, include_streams=True):
            if Path(path) == unreadable:
                raise PermissionError("synthetic metadata unavailable")
            return original(path, include_streams)

        with patch.object(self.reader, "read", side_effect=metadata), patch.object(
            self.reader, "free_bytes", side_effect=OSError("synthetic free space unavailable")
        ):
            report = INVENTORY.inventory([self.root], discover=False)
        self.assertFalse(report["complete"])
        self.assertFalse(report["admission_allowed"])
        self.assertEqual(report["allocated_bytes"], 4096)
        self.assertIsNone(report["free_bytes"])
        self.assertIn("inventory_incomplete", report["admission_reasons"])
        self.assertIn("disk_free_unknown", report["admission_reasons"])

    def test_approved_scan_floor_is_explicit_and_default_remains_strict(self):
        (self.root / "artifact.rlib").write_bytes(b"artifact")
        gib = INVENTORY.GIB
        with patch.object(self.reader, "free_bytes", return_value=56 * gib):
            legacy = INVENTORY.inventory([self.root], discover=False)
            bounded = INVENTORY.inventory([self.root], discover=False, minimum_free_bytes=40 * gib)
        self.assertTrue(legacy["complete"])
        self.assertTrue(bounded["complete"])
        self.assertFalse(legacy["admission_allowed"])
        self.assertEqual(legacy["minimum_free_bytes"], 80 * gib)
        self.assertEqual(legacy["admission_reasons"], ["disk_free_below_minimum"])
        self.assertTrue(bounded["admission_allowed"])
        self.assertEqual(bounded["minimum_free_bytes"], 40 * gib)
        self.assertEqual(legacy["allocated_bytes"], bounded["allocated_bytes"])
        self.assertIn("protocol reservations checked separately", bounded["admission_scope"])
        with patch.object(self.reader, "free_bytes", return_value=40 * gib):
            exact = INVENTORY.inventory([self.root], discover=False, minimum_free_bytes=40 * gib)
        self.assertTrue(exact["admission_allowed"])
        with patch.object(self.reader, "free_bytes", side_effect=[40 * gib, 40 * gib - 1]):
            short = INVENTORY.inventory([self.root], discover=False, minimum_free_bytes=40 * gib)
        self.assertFalse(short["admission_allowed"])
        self.assertEqual(short["free_bytes"], 40 * gib - 1)

    def test_invalid_scan_floor_fails_before_metadata_capture(self):
        for floor in (None, True, -1, 0, 40.0 * INVENTORY.GIB,
                      "40", 40 * INVENTORY.GIB - 1, 80 * INVENTORY.GIB + 1):
            with self.subTest(floor=floor), patch.object(INVENTORY, "_new_metadata_reader") as capture:
                with self.assertRaises(ValueError):
                    INVENTORY.inventory([self.root], discover=False, minimum_free_bytes=floor)
                capture.assert_not_called()

    def test_retained_outputs_and_recycle_pending_paths_remain_allocated(self):
        target, evidence, store = (self.root / name for name in ("target", "evidence", "store"))
        for directory in (target, evidence, store):
            directory.mkdir()
        artifact = target / "artifact.rlib"
        artifact.write_bytes(b"a" * 8193)
        os.link(artifact, evidence / "retained.rlib")
        (evidence / "capture.json").write_bytes(b"raw evidence")
        (store / "opaque-blob").write_bytes(b"store")
        before = INVENTORY.inventory([target, evidence], store_roots=[store], discover=False,
                                     minimum_free_bytes=40 * INVENTORY.GIB)
        # Model an exact same-volume recycle payload, without touching the
        # system bin. A move is not a release of the original allocation.
        pending = self.root / "recycle-pending-payload"
        self.assertEqual(target.resolve().parent, self.root.resolve())
        self.assertEqual(pending.resolve().parent, self.root.resolve())
        target.rename(pending)
        after = INVENTORY.inventory([pending, evidence], store_roots=[store], discover=False,
                                    minimum_free_bytes=40 * INVENTORY.GIB)
        self.assertTrue(before["complete"])
        self.assertTrue(after["complete"])
        self.assertEqual(before["allocated_bytes"], 20480)
        self.assertEqual(after["allocated_bytes"], before["allocated_bytes"])
        self.assertEqual(after["file_count"], 3)
        self.assertTrue(after["aliases"])
        self.assertEqual(sum(cache["allocated_bytes"] for cache in after["caches"]), 20480)
        self.assertEqual(sum(root["allocated_bytes"] for root in after["roots"]), 20480)

    def test_unsupported_capture_never_uses_file_lengths(self):
        (self.root / "artifact.rlib").write_bytes(b"artifact")
        with patch.object(INVENTORY, "_new_metadata_reader", side_effect=INVENTORY.UnsupportedMeasurement("synthetic unsupported filesystem")):
            report = INVENTORY.inventory([self.root], discover=False)
        self.assertFalse(report["complete"])
        self.assertFalse(report["admission_allowed"])
        self.assertEqual(report["allocated_bytes"], 0)


class BudgetAndReceiptContracts(unittest.TestCase):
    def test_fixed_baseline_thresholds_and_projected_disk_floor(self):
        gib = INVENTORY.GIB
        baseline = 9 * gib
        below = INVENTORY.budget_status(baseline, baseline + 40 * gib - 1, 200 * gib)
        self.assertTrue(below["allowed"])
        self.assertFalse(below["review_required"])
        review = INVENTORY.budget_status(baseline, baseline + 40 * gib, 200 * gib)
        self.assertTrue(review["allowed"])
        self.assertTrue(review["review_required"])
        hard = INVENTORY.budget_status(baseline, baseline + 60 * gib, 200 * gib)
        self.assertFalse(hard["allowed"])
        self.assertIn("project_hard_limit_reached", hard["reasons"])
        predicted = INVENTORY.budget_status(baseline, baseline + 59 * gib, 200 * gib, gib)
        self.assertFalse(predicted["allowed"])
        self.assertIn("projected_project_hard_limit_reached", predicted["reasons"])
        exact_free = INVENTORY.budget_status(baseline, baseline, 81 * gib, gib)
        self.assertTrue(exact_free["allowed"])
        self.assertEqual(exact_free["disk_headroom_bytes"], 0)
        short = INVENTORY.budget_status(baseline, baseline, 81 * gib - 1, gib)
        self.assertFalse(short["allowed"])
        self.assertIn("projected_disk_free_below_minimum", short["reasons"])

    def test_invalid_or_unknown_sizes_fail_closed(self):
        for position in range(4):
            for invalid in (None, True, -1, 1.5, "1"):
                with self.subTest(position=position, invalid=invalid):
                    values = [0, 0, 200 * INVENTORY.GIB, 0]
                    values[position] = invalid
                    self.assertFalse(INVENTORY.budget_status(*values)["allowed"])

    def test_v2_initial_probe_requires_exact_56_gib_free(self):
        gib = INVENTORY.GIB
        baseline = 156436568120
        keywords = {"policy_version": 2, "minimum_free_bytes": 40 * gib,
                    "external_growth_allowance_bytes": 8 * gib}
        exact = INVENTORY.budget_status(baseline, baseline, 56 * gib, 8 * gib, **keywords)
        self.assertTrue(exact["allowed"])
        self.assertFalse(exact["review_required"])
        self.assertEqual(exact["policy_version"], 2)
        self.assertEqual(exact["minimum_start_free_bytes"], 56 * gib)
        self.assertEqual(exact["minimum_free_bytes"], 40 * gib)
        self.assertEqual(exact["reserved_disk_growth_bytes"], 16 * gib)
        self.assertEqual(exact["projected_free_bytes"], 40 * gib)
        self.assertEqual(exact["projected_free_before_external_allowance_bytes"], 48 * gib)
        self.assertEqual(exact["disk_headroom_bytes"], 0)
        self.assertEqual(exact["usable_headroom_bytes"], 0)
        self.assertEqual(exact["projected_bytes"], baseline + 8 * gib)
        short = INVENTORY.budget_status(baseline, baseline, 56 * gib - 1, 8 * gib, **keywords)
        self.assertFalse(short["allowed"])
        self.assertEqual(short["disk_headroom_bytes"], -1)
        self.assertIn("projected_disk_free_below_minimum", short["reasons"])

    def test_v2_fixed_baseline_accounts_reclaimed_and_observed_project_growth(self):
        gib = INVENTORY.GIB
        baseline = 156436568120
        keywords = {"policy_version": 2, "external_growth_allowance_bytes": 8 * gib}
        reclaimed = INVENTORY.budget_status(baseline, baseline - 12 * gib, 200 * gib, 8 * gib, **keywords)
        self.assertTrue(reclaimed["allowed"])
        self.assertEqual(reclaimed["baseline_bytes"], baseline)
        self.assertEqual(reclaimed["growth_since_baseline_bytes"], -12 * gib)
        self.assertEqual(reclaimed["projected_growth_since_baseline_bytes"], -4 * gib)
        self.assertEqual(reclaimed["project_headroom_bytes"], 64 * gib)
        below = INVENTORY.budget_status(baseline, baseline + 32 * gib - 1, 200 * gib, 8 * gib, **keywords)
        review = INVENTORY.budget_status(baseline, baseline + 32 * gib, 200 * gib, 8 * gib, **keywords)
        self.assertFalse(below["review_required"])
        self.assertTrue(review["allowed"])
        self.assertTrue(review["review_required"])
        self.assertEqual(review["review_threshold_bytes"], baseline + 40 * gib)
        projected_hard = INVENTORY.budget_status(baseline, baseline + 52 * gib, 200 * gib, 8 * gib, **keywords)
        self.assertFalse(projected_hard["allowed"])
        self.assertIn("projected_project_hard_limit_reached", projected_hard["reasons"])
        already_hard = INVENTORY.budget_status(baseline, baseline + 60 * gib, 200 * gib, **keywords)
        self.assertFalse(already_hard["allowed"])
        self.assertIn("project_hard_limit_reached", already_hard["reasons"])
        self.assertEqual(already_hard["hard_threshold_bytes"], baseline + 60 * gib)

    def test_v1_cannot_opt_into_bounded_policy_by_changing_arguments(self):
        gib = INVENTORY.GIB
        default = INVENTORY.budget_status(0, 0, 119 * gib, 39 * gib)
        explicit = INVENTORY.budget_status(0, 0, 119 * gib, 39 * gib, policy_version=1,
                                          minimum_free_bytes=80 * gib, external_growth_allowance_bytes=0)
        self.assertEqual(default, explicit)
        self.assertTrue(default["allowed"])
        self.assertEqual(default["minimum_start_free_bytes"], 119 * gib)
        self.assertEqual(default["disk_headroom_bytes"], 0)
        self.assertFalse(INVENTORY.budget_status(0, 0, 119 * gib - 1, 39 * gib)["allowed"])
        for arguments in ({"minimum_free_bytes": 40 * gib},
                          {"external_growth_allowance_bytes": 8 * gib},
                          {"minimum_free_bytes": 40 * gib, "external_growth_allowance_bytes": 8 * gib}):
            with self.subTest(arguments=arguments):
                value = INVENTORY.budget_status(0, 0, 200 * gib, **arguments)
                self.assertFalse(value["allowed"])
                self.assertTrue(any("policy_mismatch" in reason for reason in value["reasons"]))

    def test_invalid_or_unapproved_budget_policies_fail_closed(self):
        gib = INVENTORY.GIB
        cases = [
            {"policy_version": version}
            for version in (None, True, False, 0, -1, 3, 1.0, 2.0, "2")
        ]
        cases.extend({"minimum_free_bytes": floor} for floor in (True, -1, 0, 80.0 * gib, "80"))
        cases.extend({"external_growth_allowance_bytes": allowance}
                     for allowance in (None, True, -1, 0.0, "0"))
        cases.extend((
            {"policy_version": 2},
            {"policy_version": 2, "minimum_free_bytes": 80 * gib, "external_growth_allowance_bytes": 8 * gib},
            {"policy_version": 2, "minimum_free_bytes": 40 * gib - 1, "external_growth_allowance_bytes": 8 * gib},
            {"policy_version": 2, "external_growth_allowance_bytes": 8 * gib - 1},
            {"policy_version": 2, "external_growth_allowance_bytes": 8 * gib + 1},
            {"policy_version": 2, "external_growth_allowance_bytes": 8.0 * gib},
        ))
        for arguments in cases:
            with self.subTest(arguments=arguments):
                value = INVENTORY.budget_status(0, 0, 200 * gib, **arguments)
                self.assertFalse(value["allowed"])
                self.assertFalse(value["review_required"])
                self.assertTrue(value["reasons"])
                self.assertIsNone(value["project_headroom_bytes"])
                self.assertIsNone(value["disk_headroom_bytes"])
                self.assertIsNone(value["usable_headroom_bytes"])

    def test_receipts_are_immutable_under_concurrent_publication(self):
        with tempfile.TemporaryDirectory(prefix="inventory-receipt-") as temporary:
            root = Path(temporary)
            destination = root / "baseline.json"

            def publish(index):
                try:
                    INVENTORY.atomic_json(destination, {"generation": index})
                    return True
                except FileExistsError:
                    return False

            with ThreadPoolExecutor(max_workers=2) as pool:
                successes = list(pool.map(publish, (1, 2)))
            self.assertEqual(sum(successes), 1)
            before = destination.read_bytes()
            with self.assertRaises(FileExistsError):
                INVENTORY.atomic_json(destination, {"generation": 3})
            self.assertEqual(destination.read_bytes(), before)
            self.assertIn(json.loads(before)["generation"], (1, 2))
            self.assertEqual([path.name for path in root.iterdir()], ["baseline.json"])

    def test_invalid_json_does_not_create_a_receipt_directory(self):
        with tempfile.TemporaryDirectory(prefix="inventory-invalid-receipt-") as temporary:
            destination = Path(temporary) / "unused" / "baseline.json"
            with self.assertRaises(ValueError):
                INVENTORY.atomic_json(destination, {"invalid": float("nan")})
            self.assertFalse(destination.parent.exists())


@unittest.skipUnless(os.name == "nt", "native Windows NTFS capture")
class NativeNtfsSmoke(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="inventory-native-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)

    def test_real_allocations_hardlinks_and_parent_child_coverage(self):
        child = self.root / "debug"
        child.mkdir()
        first = child / "artifact.rlib"
        first.write_bytes(b"a" * 8193)
        ordinary = INVENTORY.inventory([self.root], discover=False)
        self.assertTrue(ordinary["complete"], ordinary["errors"])
        self.assertGreater(ordinary["allocated_bytes"], ordinary["logical_bytes"])
        os.link(first, self.root / "alias.rlib")
        combined = INVENTORY.inventory([self.root, child], discover=False)
        self.assertTrue(combined["complete"], combined["errors"])
        self.assertEqual(combined["allocated_bytes"], ordinary["allocated_bytes"])
        self.assertEqual(combined["logical_bytes"], 8193)
        self.assertEqual(combined["file_count"], 1)

    def test_nested_source_and_download_exclusions_keep_real_target_allocations(self):
        target = self.root / "target"
        target.mkdir()
        artifact = target / "outer.rlib"
        artifact.write_bytes(b"a" * 8193)
        source = target / "fixtures" / "nested-worktree"
        source.mkdir(parents=True)
        (source / "Cargo.toml").write_text("[workspace]\n", encoding="utf-8")
        (source / "fixture.exe").write_bytes(b"source" * 4096)
        nested = source / "target"
        nested.mkdir()
        nested_artifact = nested / "inner.rlib"
        nested_artifact.write_bytes(b"b" * 8193)
        registry = target / "custom-cargo-home" / "registry" / "src"
        registry.mkdir(parents=True)
        (registry / "download.rlib").write_bytes(b"download" * 4096)
        reader = INVENTORY._new_metadata_reader()
        expected = sum(reader.read(path).allocated_bytes for path in (artifact, nested_artifact))
        report = INVENTORY.inventory([target])
        self.assertTrue(report["complete"], report["errors"])
        self.assertEqual(report["allocated_bytes"], expected)
        self.assertEqual(report["logical_bytes"], 2 * 8193)
        self.assertEqual(report["file_count"], 2)
        self.assertTrue(report["exclusions"])

    def test_file_and_directory_named_streams_are_counted(self):
        file = self.root / "artifact.rlib"
        file.write_bytes(b"a" * 8193)
        initial = INVENTORY.inventory([self.root], discover=False)
        Path(str(file) + ":review-data").write_bytes(b"b" * 8193)
        Path(str(self.root) + ":review-data").write_bytes(b"c" * 8193)
        report = INVENTORY.inventory([self.root], discover=False)
        self.assertTrue(report["complete"], report["errors"])
        self.assertEqual(report["logical_bytes"], 3 * 8193)
        self.assertGreater(report["allocated_bytes"], initial["allocated_bytes"])
        self.assertEqual(report["file_count"], 1)

    def test_compressed_file_reports_allocation_not_logical_size(self):
        file = self.root / "artifact.rlib"
        file.write_bytes(b"a" * (1024 * 1024))
        subprocess.run(["compact.exe", "/c", "/i", "/q", "/a", str(file)],
                       capture_output=True, check=True, timeout=20)
        self.assertTrue(file.stat().st_file_attributes & INVENTORY._COMPRESSED)
        report = INVENTORY.inventory([self.root], discover=False)
        self.assertTrue(report["complete"], report["errors"])
        self.assertEqual(report["logical_bytes"], 1024 * 1024)
        self.assertLess(report["allocated_bytes"], report["logical_bytes"])

    def test_sparse_file_reports_allocated_clusters(self):
        file = self.root / "artifact.rlib"
        file.write_bytes(b"x")
        subprocess.run(["fsutil.exe", "sparse", "setflag", str(file)],
                       capture_output=True, check=True, timeout=20)
        with file.open("r+b") as handle:
            handle.seek(8 * 1024 * 1024)
            handle.write(b"x")
        report = INVENTORY.inventory([self.root], discover=False)
        self.assertTrue(report["complete"], report["errors"])
        self.assertEqual(report["logical_bytes"], 8 * 1024 * 1024 + 1)
        self.assertLess(report["allocated_bytes"], report["logical_bytes"])

    def test_junction_alias_and_loop_do_not_duplicate_allocations(self):
        target = self.root / "a-target"
        target.mkdir()
        (target / "artifact.rlib").write_bytes(b"a" * 8193)
        alias = self.root / "z-alias"
        loop = target / "loop"
        for link, destination in ((alias, target), (loop, self.root)):
            subprocess.run(["cmd.exe", "/d", "/c", "mklink", "/j", str(link), str(destination)],
                           capture_output=True, check=True, timeout=20)
        report = INVENTORY.inventory([self.root, alias], discover=False)
        self.assertTrue(report["complete"], report["errors"])
        self.assertEqual(report["logical_bytes"], 8193)
        self.assertEqual(report["file_count"], 1)
        self.assertGreaterEqual(len(report["aliases"]), 2)


if __name__ == "__main__":
    unittest.main()
