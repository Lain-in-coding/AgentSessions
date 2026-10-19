"""Contract tests plus small native Windows processes; no Cargo build/install."""
import copy
import hashlib
import importlib.util
import json
import os
import sys
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

MODULE_PATH = Path(__file__).with_name("build_feedback_runner.py")
SPEC = importlib.util.spec_from_file_location("build_feedback_runner_tests", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
RUNNER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RUNNER)


def write_json(path, value):
    path.write_text(json.dumps(value), encoding="utf-8")


def fixture(root):
    source, live, allocated_before, allocated_after = [root / name for name in ("frozen", "live", "baseline-source", "candidate-source")]
    for path in (source, live, allocated_before, allocated_after):
        path.mkdir()
    entries = {"Cargo.lock": b"# frozen lock\n", RUNNER.DEFAULT_EDIT_PATH: b"pub fn value() -> u32 { 3 }\n", "schemas/example.json": b"{}\n"}
    manifest = []
    for name, contents in entries.items():
        for path in (source, allocated_before, allocated_after):
            full = path / name
            full.parent.mkdir(parents=True, exist_ok=True)
            full.write_bytes(contents)
        manifest.append({"path": name, "sha256": hashlib.sha256(contents).hexdigest(), "bytes": len(contents)})
    receipt = root / "source.json"
    write_json(receipt, {"schema_version": 1, "live_source_unchanged": True, "snapshot_commit": "a" * 40, "snapshot_tree": "b" * 40, "live_source": str(live), "source_identity": {"files": manifest}})
    baseline = root / "baseline.json"
    write_json(baseline, {"schema_version": 1, "complete": True, "errors": [], "allocated_bytes": 7 * RUNNER.GIB})
    chain = {}
    for name in ("cargo", "rustc", "compiler", "linker"):
        executable = root / (name + ".exe")
        executable.write_bytes(b"synthetic executable identity " + name.encode())
        chain[name] = {"path": str(executable), "sha256": RUNNER.sha256_file(executable), "version": name + " 1.97.1 (fixed hash date)"}
    inputs = {"source_root": str(source), "source_commit": "a" * 40, "source_tree": "b" * 40, "source_receipt_path": str(receipt), "baseline_receipt_path": str(baseline), "run_root": str(root / "run"), "toolchain": {"cargo": chain["cargo"], "rustc": chain["rustc"], "msvc": {"compiler": chain["compiler"], "linker": chain["linker"]}, "target": "x86_64-pc-windows-msvc"}, "candidate_config": {"debug": "line-tables-only", "layout": "3lane", "incremental": "on", "tool": {"kind": "native"}}, "inventory_roots": [str(root)], "predicted_growth_bytes": 2 * RUNNER.GIB, "allocations": [{"pair_index": 0, "attempt": 0, "baseline_source_root": str(allocated_before), "candidate_source_root": str(allocated_after)}]}
    return inputs


def git_identity(root, *arguments):
    if arguments == ("rev-parse", "--show-toplevel"):
        return str(root)
    if arguments == ("rev-parse", "HEAD"):
        return "a" * 40
    if arguments == ("rev-parse", "HEAD^{tree}"):
        return "b" * 40
    if arguments in (("diff", "--cached", "--name-only", "HEAD"), ("ls-files", "--others", "--exclude-standard")):
        return ""
    if arguments[:4] == ("-c", "core.autocrlf=true", "-c", "core.safecrlf=false"):
        edit = root / RUNNER.DEFAULT_EDIT_PATH
        return RUNNER.DEFAULT_EDIT_PATH if edit.read_bytes().endswith(RUNNER.EDIT_SUFFIX) else ""
    if arguments[0] == "status":
        edit = root / RUNNER.DEFAULT_EDIT_PATH
        return "M " + RUNNER.DEFAULT_EDIT_PATH if edit.read_bytes().endswith(RUNNER.EDIT_SUFFIX) else ""
    raise AssertionError(arguments)


def simple_budget(baseline, current, free, growth=0):
    project = baseline + 60 * RUNNER.GIB - current - growth
    disk = free - 80 * RUNNER.GIB - growth
    allowed = project > 0 and disk >= 0
    return {"allowed": allowed, "review_required": current + growth >= baseline + 40 * RUNNER.GIB, "reasons": [] if allowed else ["budget_exhausted"], "project_headroom_bytes": project, "disk_headroom_bytes": disk, "usable_headroom_bytes": min(project, disk)}


class ProtocolTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.fields = fixture(self.root)
        self.git_patch = patch.object(RUNNER, "_git", side_effect=git_identity)
        self.git_patch.start()
        self.addCleanup(self.git_patch.stop)

    def protocol(self):
        return RUNNER.make_protocol(self.fields)

    def test_fixed_jobs_offline_and_parallelism(self):
        protocol = self.protocol()
        self.assertEqual(protocol["pairs"], 5)
        self.assertEqual(len(RUNNER.JOBS), 6)
        self.assertEqual(len({job["lane"] for job in RUNNER.JOBS}), 3)
        for job in RUNNER.JOBS:
            command = RUNNER.job_command(protocol, job, self.root / "target")
            for argument in ("--locked", "--offline", "-j2", "--message-format=json"):
                self.assertIn(argument, command)
            self.assertNotIn("--features", command)
            self.assertNotIn("--no-default-features", command)
            if job["args"][0] == "test":
                self.assertEqual(command[-2:], ["--", "--test-threads=2"])
        RUNNER._validate_protocol(protocol)

    def test_unknown_mode_and_wrong_fixed_n_rejected(self):
        for key, value in (("pairs", 100), ("jobs", 8), ("test_threads", 8), ("phase", "short"), ("garbage", True)):
            fields = dict(self.fields, **{key: value})
            with self.subTest(key=key), self.assertRaises(RUNNER.ProtocolError):
                RUNNER.make_protocol(fields)
        for key, value in (("debug", "none"), ("incremental", "off"), ("layout", "automatic")):
            fields = copy.deepcopy(self.fields)
            fields["candidate_config"][key] = value
            with self.subTest(key=key), self.assertRaises(RUNNER.ProtocolError):
                RUNNER.make_protocol(fields)

    def test_shared_or_live_source_rejected(self):
        self.fields["source_root"] = str(self.root / "live")
        with self.assertRaises(RUNNER.ProtocolError):
            self.protocol()

    def test_runtime_dedicated_worktrees_remain_valid_after_coordinator_install(self):
        coordinator = self.root / "coordinator"
        runtime = coordinator / ".trellis" / ".runtime" / "synthetic-campaign"
        runtime.mkdir(parents=True)
        inputs = fixture(runtime)
        with patch.object(RUNNER, "__file__", str(coordinator / "scripts" / "evidence" / "build_feedback_runner.py")):
            protocol = RUNNER.make_protocol(inputs)
            self.assertEqual(protocol["source"]["root"], inputs["source_root"])
            self.assertEqual(protocol["run_root"], inputs["run_root"])
            def parent_git(root, *args):
                return str(coordinator) if args == ("rev-parse", "--show-toplevel") else git_identity(root, *args)
            with patch.object(RUNNER, "_git", side_effect=parent_git), self.assertRaisesRegex(RUNNER.ProtocolError, "dedicated_git_worktree_required"):
                RUNNER.make_protocol(inputs)
            unsafe = dict(inputs, run_root=str(coordinator / "crates" / "trial"))
            with self.assertRaisesRegex(RUNNER.ProtocolError, "run_root_must_use_ignored"):
                RUNNER.make_protocol(unsafe)

    def test_prewarm_nested_worktree_cannot_overlap_a_trial_source(self):
        executable = self.root / "mbx.exe"
        executable.write_bytes(b"synthetic local cache")
        store = self.root / "store"
        store.mkdir()
        prewarm = Path(self.fields["allocations"][0]["candidate_source_root"]) / "nested"
        prewarm.mkdir()
        receipt = self.root / "prewarm-overlap.json"
        write_json(receipt, {"schema_version": 1})
        fields = copy.deepcopy(self.fields)
        fields["store_roots"] = [str(store)]
        fields["candidate_config"]["tool"] = {"kind": "mbx", "path": str(executable), "sha256": RUNNER.sha256_file(executable), "version": "mbx 1.24.1", "store": str(store), "store_limit_bytes": 10 * RUNNER.GIB, "prewarm_source_root": str(prewarm), "prewarm_receipt_path": str(receipt), "prewarm_receipt_sha256": RUNNER.sha256_file(receipt), "views": False, "native_check_lane": False}
        with self.assertRaisesRegex(RUNNER.ProtocolError, "prewarm_worktree_must_be_distinct"):
            RUNNER.make_protocol(fields)

    def test_source_file_drift_and_untracked_status_rejected(self):
        protocol = self.protocol()
        source = Path(protocol["source"]["root"])
        (source / RUNNER.DEFAULT_EDIT_PATH).write_bytes(b"changed")
        with self.assertRaisesRegex(RUNNER.ProtocolError, "source_drift"):
            RUNNER._verify_source(source, protocol)
        (self.root / "other").mkdir()
        self.fields = fixture(self.root / "other")
        protocol = self.protocol()
        with patch.object(RUNNER, "_git", side_effect=lambda root, *args: "foreign.rs" if args[0] == "ls-files" else git_identity(root, *args)), self.assertRaisesRegex(RUNNER.ProtocolError, "untracked"):
            RUNNER._verify_source(Path(protocol["source"]["root"]), protocol)

    def test_source_receipt_and_binary_identity_drift_rejected(self):
        protocol = self.protocol()
        source_receipt = Path(self.fields["source_receipt_path"])
        source_receipt.write_text(source_receipt.read_text() + " ", encoding="utf-8")
        with self.assertRaisesRegex(RUNNER.ProtocolError, "source_receipt_changed"):
            RUNNER._validate_protocol(protocol)
        executable = Path(self.fields["toolchain"]["cargo"]["path"])
        executable.write_bytes(b"changed executable")
        with self.assertRaises(RUNNER.ProtocolError):
            RUNNER._identity(self.fields["toolchain"]["cargo"], "cargo")

    def test_incomplete_baseline_and_formal_without_pilot_rejected(self):
        write_json(Path(self.fields["baseline_receipt_path"]), {"schema_version": 1, "complete": False, "errors": ["scan failed"], "allocated_bytes": 3})
        with self.assertRaisesRegex(RUNNER.ProtocolError, "complete_baseline"):
            self.protocol()
        write_json(Path(self.fields["baseline_receipt_path"]), {"schema_version": 1, "complete": True, "errors": [], "allocated_bytes": 3})
        self.fields["phase"] = "formal"
        with self.assertRaisesRegex(RUNNER.ProtocolError, "pilot_receipt"):
            self.protocol()

    def test_balanced_schedule_deterministic_and_retry_same_order(self):
        for count in (5, 100):
            orders = RUNNER.balanced_order(count, 73)
            self.assertEqual(orders, RUNNER.balanced_order(count, 73))
            first = orders.count(["baseline", "candidate"])
            self.assertLessEqual(abs(first - (count - first)), 1)
        with self.assertRaises(RUNNER.ProtocolError):
            RUNNER.balanced_order(20, 73)

    def test_reservation_integer_rounding(self):
        self.assertEqual(RUNNER.cache_reservation_bytes(0), RUNNER.GIB)
        self.assertEqual(RUNNER.cache_reservation_bytes(4 * RUNNER.GIB), 6 * RUNNER.GIB)
        self.assertEqual(RUNNER.cache_reservation_bytes(4 * RUNNER.GIB + 1), 7 * RUNNER.GIB)

    def test_preview_rejects_partial_inventory_and_budget_projection(self):
        protocol = self.protocol()
        module = SimpleNamespace(budget_status=simple_budget)
        report = {"schema_version": 1, "complete": True, "errors": [], "allocated_bytes": 9 * RUNNER.GIB, "disk_free_bytes": 100 * RUNNER.GIB}
        with patch.object(RUNNER, "_inventory_module", return_value=module):
            self.assertTrue(RUNNER.preview_protocol(protocol, report)["allowed"])
            report["allocated_bytes"] = 66 * RUNNER.GIB
            self.assertFalse(RUNNER.preview_protocol(protocol, report)["allowed"])
            report["complete"] = False
            self.assertIn("inventory_incomplete", RUNNER.preview_protocol(protocol, report)["reasons"])
        self.assertFalse(Path(protocol["run_root"]).exists())

    def test_review_receipt_bound_to_exact_protocol_and_frozen_baseline(self):
        protocol = self.protocol()
        preview = {"allowed": True, "review_required": True, "reasons": [], "protocol_sha256": RUNNER.canonical_sha256(protocol), "allocated_bytes": 48 * RUNNER.GIB, "disk_free_bytes": 100 * RUNNER.GIB}
        with self.assertRaisesRegex(RUNNER.ProtocolError, "review_receipt_required"):
            RUNNER._check_review(protocol, preview, None)
        review = self.root / "review.json"
        write_json(review, {"schema_version": 1, "approved": True, "protocol_sha256": preview["protocol_sha256"], "baseline_receipt_sha256": protocol["baseline"]["receipt_sha256"], "max_allocated_bytes": 50 * RUNNER.GIB, "min_disk_free_bytes": 80 * RUNNER.GIB})
        RUNNER._check_review(protocol, preview, review)
        preview["protocol_sha256"] = "0" * 64
        with self.assertRaises(RUNNER.ProtocolError):
            RUNNER._check_review(protocol, preview, review)

    def test_explicit_review_is_validated_below_soft_threshold(self):
        protocol = self.protocol()
        preview = {"allowed": True, "review_required": False, "reasons": [], "protocol_sha256": RUNNER.canonical_sha256(protocol), "allocated_bytes": 9 * RUNNER.GIB, "disk_free_bytes": 100 * RUNNER.GIB}
        review = self.root / "review-below-soft.json"
        write_json(review, {"schema_version": 1, "approved": True, "protocol_sha256": preview["protocol_sha256"], "baseline_receipt_sha256": protocol["baseline"]["receipt_sha256"], "max_allocated_bytes": 20 * RUNNER.GIB, "min_disk_free_bytes": 90 * RUNNER.GIB})
        RUNNER._check_review(protocol, preview, review)
        preview["disk_free_bytes"] = 89 * RUNNER.GIB
        with self.assertRaisesRegex(RUNNER.ProtocolError, "review_free_space_mismatch"):
            RUNNER._check_review(protocol, preview, review)

    def test_environment_wrapper_stacking_cleared_and_edit_keeps_incremental(self):
        protocol = self.protocol()
        configuration = copy.deepcopy(protocol["configs"]["candidate"])
        configuration["incremental"] = "one-shot-off"
        with patch.dict(os.environ, {"CARGO_HOME": str(self.root), "RUSTC_WRAPPER": "old.exe", "RUSTC_WORKSPACE_WRAPPER": "other.exe", "RUSTFLAGS": "-O", "SCCACHE_BUCKET": "remote", "CARGO_PROFILE_RELEASE_DEBUG": "0"}):
            env = RUNNER.tool_environment(protocol, configuration)
            self.assertEqual(env["RUSTC_WRAPPER"], "")
            self.assertEqual(env["RUSTC_WORKSPACE_WRAPPER"], "")
            self.assertEqual(env["CARGO_INCREMENTAL"], "0")
            self.assertNotIn("SCCACHE_BUCKET", env)
            self.assertNotIn("CARGO_PROFILE_RELEASE_DEBUG", env)
            self.assertEqual(RUNNER.tool_environment(protocol, configuration, feedback=True)["CARGO_INCREMENTAL"], "1")

    def test_duplicate_json_and_proxy_toolchain_rejected(self):
        path = self.root / "duplicates.json"
        path.write_text('{"schema_version":1,"schema_version":2}', encoding="utf-8")
        with self.assertRaisesRegex(RUNNER.ProtocolError, "duplicate_json_key"):
            RUNNER._read_json(path)
        self.fields["toolchain"]["rustc"] = dict(self.fields["toolchain"]["cargo"], version="rustc 1.97.1 (fixed hash date)")
        with self.assertRaisesRegex(RUNNER.ProtocolError, "not_rustup_proxies"):
            self.protocol()

    def test_inventory_scope_cannot_shrink_from_frozen_baseline(self):
        baseline_path = Path(self.fields["baseline_receipt_path"])
        baseline = RUNNER._read_json(baseline_path)
        baseline["roots"] = [{"path": str(self.root)}, {"path": str(self.root / "other-runtime")}]
        write_json(baseline_path, baseline)
        protocol = self.protocol()
        report = {"schema_version": 1, "complete": True, "errors": [], "allocated_bytes": 9 * RUNNER.GIB, "disk_free_bytes": 100 * RUNNER.GIB, "roots": [{"path": str(self.root)}]}
        with patch.object(RUNNER, "_inventory_module", return_value=SimpleNamespace(budget_status=simple_budget)):
            preview = RUNNER.preview_protocol(protocol, report)
        self.assertFalse(preview["allowed"])
        self.assertIn("initial_inventory_scope_missing", preview["reasons"])

    def test_sccache_preview_blocks_without_non_eviction_guarantee(self):
        protocol = self.protocol()
        executable = self.root / "sccache.exe"
        executable.write_bytes(b"synthetic local cache tool")
        store = self.root / "local-store"
        store.mkdir()
        prewarm = self.root / "prewarm-source"
        prewarm.mkdir()
        receipt = self.root / "prewarm.json"
        write_json(receipt, {"schema_version": 1, "complete": True})
        protocol["configs"]["candidate"]["tool"] = {
            "kind": "sccache", "path": str(executable), "sha256": RUNNER.sha256_file(executable),
            "version": "synthetic cache version", "store": str(store),
            "store_limit_bytes": 100 * RUNNER.GIB, "prewarm_source_root": str(prewarm),
            "prewarm_receipt_path": str(receipt), "prewarm_receipt_sha256": RUNNER.sha256_file(receipt),
            "server_port": 40261,
        }
        protocol["store_roots"] = [str(store)]
        report = {"schema_version": 1, "complete": True, "errors": [],
                  "allocated_bytes": 9 * RUNNER.GIB, "disk_free_bytes": 100 * RUNNER.GIB}
        with patch.object(RUNNER, "_inventory_module", return_value=SimpleNamespace(budget_status=simple_budget)), self.assertRaisesRegex(RUNNER.ProtocolError, "sccache_warm_store_read_only_required"):
            RUNNER.preview_protocol(protocol, report)
        self.assertFalse(Path(protocol["run_root"]).exists())

    def test_initial_store_cannot_be_downgraded_to_discovery_root(self):
        store = self.root / "opaque-store"
        store.mkdir()
        (store / "cache-hash-without-extension").write_bytes(b"cached object")
        baseline_path = Path(self.fields["baseline_receipt_path"])
        baseline = RUNNER._read_json(baseline_path)
        baseline["roots"] = [{"path": str(self.root), "kind": "known_root"},
                             {"path": str(store), "kind": "store"}]
        write_json(baseline_path, baseline)
        protocol = self.protocol()
        report = {"schema_version": 1, "complete": True, "errors": [],
                  "allocated_bytes": 9 * RUNNER.GIB, "disk_free_bytes": 100 * RUNNER.GIB,
                  "roots": [{"path": str(self.root), "kind": "known_root"},
                            {"path": str(store), "kind": "known_root"}]}
        with patch.object(RUNNER, "_inventory_module", return_value=SimpleNamespace(budget_status=simple_budget)):
            narrowed = RUNNER.preview_protocol(protocol, report)
            self.assertFalse(narrowed["allowed"])
            self.assertIn("initial_store_scope_missing", narrowed["reasons"])
            report["roots"][1]["kind"] = "store"
            self.assertTrue(RUNNER.preview_protocol(protocol, report)["allowed"])

    def test_segment_versions_must_match_real_capture(self):
        protocol = self.protocol()
        root = self.root / "segment-output"
        root.mkdir()
        # This unit test checks banner equality; the real environment receipt
        # is validated by protocol construction and tool_environment.
        protocol["msvc_environment"] = {"receipt_path": "synthetic"}
        def captured(argv, **kwargs):
            label = kwargs["label"]
            output, error = kwargs["log_root"] / (label + ".stdout"), kwargs["log_root"] / (label + ".stderr")
            identity = protocol["toolchain"][label] if label in ("cargo", "rustc") else protocol["toolchain"]["msvc"]["compiler" if label == "msvc_compiler" else "linker"]
            output.write_text(identity["version"], encoding="utf-8")
            error.write_bytes(b"")
            return {"status": "ok", "stdout_path": str(output), "stderr_path": str(error)}
        with patch.object(RUNNER, "capture_command", side_effect=captured), patch.object(RUNNER, "tool_environment", return_value={}), patch.dict(os.environ, {"CARGO_HOME": str(self.root)}):
            receipt = RUNNER._verify_segment_toolchain(protocol, root)
        self.assertEqual(RUNNER.sha256_file(Path(receipt["path"])), receipt["sha256"])
        self.assertTrue(RUNNER._read_json(Path(receipt["path"]))["complete"])
        protocol["toolchain"]["rustc"]["version"] = "rustc 1.97.1 (mismatch)"
        def mismatched(argv, **kwargs):
            result = captured(argv, **kwargs)
            if kwargs["label"] == "rustc":
                Path(result["stdout_path"]).write_text("rustc 1.97.0 (old)", encoding="utf-8")
            return result
        with patch.object(RUNNER, "capture_command", side_effect=mismatched), patch.object(RUNNER, "tool_environment", return_value={}), patch.dict(os.environ, {"CARGO_HOME": str(self.root)}), self.assertRaisesRegex(RUNNER.ProtocolError, "version_mismatch"):
            RUNNER._verify_segment_toolchain(protocol, root)

    def test_duplicate_allocation_and_source_hardlinks_rejected(self):
        self.fields["allocations"].append(copy.deepcopy(self.fields["allocations"][0]))
        with self.assertRaises(RUNNER.ProtocolError):
            self.protocol()
        self.fields["allocations"].pop()
        protocol = self.protocol()
        file = Path(protocol["source"]["root"]) / RUNNER.DEFAULT_EDIT_PATH
        os.link(file, self.root / "alias.rs")
        with self.assertRaisesRegex(RUNNER.ProtocolError, "hardlink"):
            RUNNER._verify_source(Path(protocol["source"]["root"]), protocol)


class LiveBudgetTests(unittest.TestCase):
    def setUp(self):
        self.protocol = {"baseline": {"allocated_bytes": 7 * RUNNER.GIB}}
        self.before = {"allocated_bytes": 55 * RUNNER.GIB, "disk_free_bytes": 100 * RUNNER.GIB}
        self.inventory = SimpleNamespace(budget_status=simple_budget)

    def test_external_growth_before_monitor_start_consumes_original_allowance(self):
        with patch.object(RUNNER, "_free_bytes", return_value=81 * RUNNER.GIB), patch.object(RUNNER._LiveBudget, "_write_bound", return_value=0), patch.object(RUNNER, "_inventory_module", return_value=self.inventory):
            monitor = RUNNER._LiveBudget(self.protocol, self.before, None)
            sample = monitor()
        self.assertEqual(sample["physical_growth_bytes"], 19 * RUNNER.GIB)
        self.assertEqual(sample["allocated_bytes"], 74 * RUNNER.GIB)
        self.assertFalse(sample["budget"]["allowed"])

    def test_free_space_recovery_does_not_rebase_retained_consumption(self):
        with patch.object(RUNNER, "_free_bytes", side_effect=[95 * RUNNER.GIB, 98 * RUNNER.GIB, 94 * RUNNER.GIB]), patch.object(RUNNER._LiveBudget, "_write_bound", return_value=0), patch.object(RUNNER, "_inventory_module", return_value=self.inventory):
            monitor = RUNNER._LiveBudget(self.protocol, self.before, None)
            samples = [monitor() for _ in range(3)]
        self.assertEqual([row["physical_growth_bytes"] for row in samples], [5 * RUNNER.GIB, 5 * RUNNER.GIB, 6 * RUNNER.GIB])

    def test_review_free_space_floor_stops_before_global_minimum(self):
        with patch.object(RUNNER, "_free_bytes", return_value=89 * RUNNER.GIB), patch.object(RUNNER._LiveBudget, "_write_bound", return_value=0), patch.object(RUNNER, "_inventory_module", return_value=self.inventory):
            monitor = RUNNER._LiveBudget(self.protocol, self.before, None, review_floor_bytes=90 * RUNNER.GIB)
            sample = monitor()
        self.assertFalse(sample["budget"]["allowed"])
        self.assertIn("budget_review_free_space_floor_exceeded", sample["budget"]["reasons"])

    def test_mbx_store_has_independent_live_reservation(self):
        config = {"tool": {"kind": "mbx", "store_limit_bytes": 12 * RUNNER.GIB}}
        with patch.object(RUNNER, "_free_bytes", return_value=96 * RUNNER.GIB), patch.object(RUNNER._LiveBudget, "_write_bound", return_value=0), patch.object(RUNNER, "_inventory_module", return_value=self.inventory):
            monitor = RUNNER._LiveBudget(self.protocol, self.before, None, 9 * RUNNER.GIB, config)
            sample = monitor()
        self.assertFalse(sample["budget"]["allowed"])
        self.assertIn("store_reservation_write_bound_reached", sample["budget"]["reasons"])


class ProcessOwnershipTests(unittest.TestCase):
    def test_owned_and_confirmed_exited_processes_are_not_contamination(self):
        identities = [{"pid": 1}, {"pid": 2}, {"pid": 3}]
        job = SimpleNamespace(owns_process_identity=lambda value: {1: True, 2: None, 3: False}[value["pid"]])
        with patch.object(RUNNER, "build_process_snapshot", return_value=identities):
            self.assertEqual(RUNNER._external_build_processes(job), [identities[2]])

    def test_ownership_query_failure_blocks_instead_of_discarding_process(self):
        def unavailable(identity):
            raise RUNNER.ProtocolError("build_process_membership_unknown")
        with patch.object(RUNNER, "build_process_snapshot", return_value=[{"pid": 1}]), self.assertRaisesRegex(RUNNER.ProtocolError, "membership_unknown"):
            RUNNER._external_build_processes(SimpleNamespace(owns_process_identity=unavailable))


class CacheReservationTests(unittest.TestCase):
    setUp = ProtocolTests.setUp
    protocol = ProtocolTests.protocol

    def test_read_only_store_does_not_reserve_entire_pair_target_forecast(self):
        protocol = self.protocol()
        protocol["predicted_growth_bytes"] = 39 * RUNNER.GIB
        prewarm = self.root / "prewarm"
        prewarm.mkdir()
        store = self.root / "store"
        store.mkdir()
        receipt = self.root / "prewarm-valid.json"
        tool = {"kind": "sccache", "sha256": "c" * 64, "store": str(store), "store_limit_bytes": 10 * RUNNER.GIB, "prewarm_source_root": str(prewarm), "prewarm_receipt_path": str(receipt), "local_rw_mode": "READ_ONLY"}
        jobs = [{"job": fixed["name"], "status": "ok", "returncode": 0, "process_tree_capture_complete": True} for fixed in RUNNER.JOBS]
        write_json(receipt, {"schema_version": 1, "complete": True, "offline": True, "errors": [], "source_tree": protocol["source"]["tree"], "source_commit": protocol["source"]["commit"], "source_root": str(prewarm), "tool_sha256": tool["sha256"], "toolchain_sha256": RUNNER.canonical_sha256(protocol["toolchain"]), "store": str(store), "cold_store_duration_ms": 100, "jobs": jobs, "jobs_limit": 2, "test_threads": 2, "logical_bytes": 9 * RUNNER.GIB})
        tool["prewarm_receipt_sha256"] = RUNNER.sha256_file(receipt)
        module = SimpleNamespace(inventory=lambda *args, **kwargs: {"complete": True, "errors": [], "allocated_bytes": 9 * RUNNER.GIB})
        with patch.object(RUNNER, "_verify_source"), patch.object(RUNNER, "_inventory_module", return_value=module):
            RUNNER._verify_cache_prewarm(protocol, {"tool": tool}, Path(self.fields["source_root"]))
            tool["store_limit_bytes"] = 9 * RUNNER.GIB
            with self.assertRaisesRegex(RUNNER.ProtocolError, "store_reservation_blocks_batch"):
                RUNNER._verify_cache_prewarm(protocol, {"tool": tool}, Path(self.fields["source_root"]))

    def test_missing_verified_msvc_environment_blocks_before_capture(self):
        protocol = self.protocol()
        with patch.object(RUNNER, "capture_command") as capture, self.assertRaisesRegex(RUNNER.ProtocolError, "verified_msvc_environment_required"):
            RUNNER._verify_segment_toolchain(protocol, self.root / "toolchain-run")
        capture.assert_not_called()


class CargoArtifactTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.target = self.root / "target"
        self.target.mkdir()
        self.executable = self.target / "selected.exe"
        self.executable.write_bytes(b"selected executable")
        self.message = {"reason": "compiler-artifact", "package_id": "test#0.1.0", "manifest_path": str(self.root / "Cargo.toml"), "target": {"name": "test", "kind": ["bin"]}, "profile": {"test": True}, "features": [], "filenames": [str(self.executable)], "executable": str(self.executable), "fresh": False}

    def parse(self, message=None, tail='{"reason":"build-finished","success":true}\n'):
        stdout = (json.dumps(message or self.message) + "\n" + tail).encode()
        return RUNNER.parse_cargo_json(stdout, self.target, self.root)

    def test_only_invocation_selected_artifacts_and_hashes(self):
        (self.target / "stale.exe").write_bytes(b"unselected stale file")
        result = self.parse(tail='{"reason":"build-finished","success":true}\nrunning 4 tests\n{test text after Cargo}\n')
        self.assertTrue(result["build_success"])
        self.assertEqual(result["errors"], [])
        self.assertEqual(len(result["artifacts"][0]["files"]), 1)
        receipt = result["artifacts"][0]["files"][0]
        RUNNER.verify_artifact_receipt(receipt)
        self.executable.write_bytes(b"replaced executable")
        with self.assertRaisesRegex(RUNNER.ProtocolError, "artifact_changed"):
            RUNNER.verify_artifact_receipt(receipt)

    def test_outside_target_missing_finish_and_duplicate_finish_rejected(self):
        outside = self.root / "outside.exe"
        outside.write_bytes(b"foreign")
        message = dict(self.message, filenames=[str(outside)], executable=str(outside))
        with self.assertRaisesRegex(RUNNER.ProtocolError, "outside_invocation"):
            self.parse(message)
        self.assertIn("missing_build_finished", self.parse(tail="")["errors"])
        self.assertIn("invalid_build_finished", self.parse(tail='{"reason":"build-finished","success":true}\n{"reason":"build-finished","success":true}\n')["errors"])

    def test_fresh_cargo_selected_lane_artifact_is_valid(self):
        self.message["fresh"] = True
        result = self.parse()
        self.assertTrue(result["artifacts"][0]["fresh"])
        self.assertTrue(result["build_success"])

    def test_file_line_diagnostics_preserved(self):
        diagnostic = {"reason": "compiler-message", "message": {"level": "error", "message": "intentional probe", "spans": [{"file_name": "src/lib.rs", "line_start": 9, "column_start": 4, "is_primary": True}], "rendered": "error at src/lib.rs:9:4"}}
        result = self.parse(tail=json.dumps(diagnostic) + '\n{"reason":"build-finished","success":false}\n')
        self.assertFalse(result["build_success"])
        self.assertEqual(result["diagnostics"][0]["spans"][0]["line_start"], 9)


class ResumeTests(unittest.TestCase):
    def test_changed_order_and_incomplete_state_fail_closed(self):
        protocol = {"phase": "pilot", "pairs": 5, "order": RUNNER.balanced_order(5, 8), "run_root": "/tmp/nonexistent"}
        state = {"schema_version": 1, "protocol_sha256": "c" * 64, "phase": "pilot", "planned_pairs": 5, "completed_valid_pairs": 0, "pairs": [{"pair_index": 0, "attempt": 0, "order": protocol["order"][0], "status": "contaminated", "arms": {}, "arm_receipts": {}}]}
        state["integrity_sha256"] = RUNNER.canonical_sha256(state)
        RUNNER._validate_resume(state, protocol, "c" * 64)
        state["completed_valid_pairs"] = 1
        with self.assertRaisesRegex(RUNNER.ProtocolError, "resume_state_corrupt"):
            RUNNER._validate_resume(state, protocol, "c" * 64)
        state["integrity_sha256"] = RUNNER.canonical_sha256({k: v for k, v in state.items() if k != "integrity_sha256"})
        with self.assertRaisesRegex(RUNNER.ProtocolError, "completed_count"):
            RUNNER._validate_resume(state, protocol, "c" * 64)


@unittest.skipUnless(os.name == "nt" and sys.maxsize > 2 ** 32, "native Windows 64-bit capture")
class NativeWindowsTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.snapshot = patch.object(RUNNER, "build_process_snapshot", return_value=[])
        self.snapshot_mock = self.snapshot.start()
        self.addCleanup(self.snapshot.stop)

    def capture(self, code, label="native", **kwargs):
        return RUNNER.capture_command([sys.executable, "-c", code], cwd=self.root, env=dict(os.environ), log_root=self.root, label=label, **kwargs)

    def test_process_capture_and_committed_memory(self):
        result = self.capture('import sys; print("captured"); print("error stream",file=sys.stderr)')
        self.assertEqual(result["status"], "ok")
        self.assertEqual(result["returncode"], 0)
        self.assertGreater(result["peak_commit_bytes"], 0)
        self.assertTrue(result["process_tree_capture_complete"])
        self.assertIn(b"captured", Path(result["stdout_path"]).read_bytes())
        self.assertIn(b"error stream", Path(result["stderr_path"]).read_bytes())

    def test_live_identity_uses_job_membership_and_rejects_pid_reuse(self):
        with RUNNER.WindowsJob(128 * 1024**2) as job:
            process = job.start([sys.executable, "-c", "import time;time.sleep(10)"], self.root, dict(os.environ), self.root, "owned-identity")
            self.snapshot.stop()
            try:
                # The normal scanner selects build tool names, so include
                # this synthetic Python process for native ownership proof.
                with patch.object(RUNNER, "BUILD_NAMES", RUNNER.BUILD_NAMES | {Path(sys.executable).name.lower()}):
                    observed = next(row for row in RUNNER.build_process_snapshot() if row["pid"] == process["pid"])
            finally:
                self.snapshot_mock = self.snapshot.start()
            self.assertTrue(job.owns_process_identity(observed))
            with self.assertRaisesRegex(RUNNER.ProtocolError, "identity_changed"):
                job.owns_process_identity(dict(observed, created_filetime=observed["created_filetime"] + 1))
            job.terminate()
            self.assertIsNone(job.owns_process_identity(observed))

    def test_timeout_kills_descendant_tree(self):
        pidfile = self.root / "child.pid"
        code = 'import subprocess,sys,time,pathlib; p=subprocess.Popen([sys.executable,"-c","import time;time.sleep(15)"]);pathlib.Path(' + repr(str(pidfile)) + ').write_text(str(p.pid));time.sleep(15)'
        result = self.capture(code, timeout_seconds=0.4)
        self.assertEqual(result["status"], "timeout")
        self.assertLess(result["duration_ms"], 5500)
        self.assertTrue(pidfile.exists())
        pid = int(pidfile.read_text())
        kernel = RUNNER.ctypes.WinDLL("kernel32", use_last_error=True)
        kernel.OpenProcess.argtypes, kernel.OpenProcess.restype = [RUNNER.DWORD, RUNNER.wintypes.BOOL, RUNNER.DWORD], RUNNER.HANDLE
        kernel.WaitForSingleObject.argtypes, kernel.WaitForSingleObject.restype = [RUNNER.HANDLE, RUNNER.DWORD], RUNNER.DWORD
        kernel.CloseHandle.argtypes = [RUNNER.HANDLE]
        handle = kernel.OpenProcess(0x100000, False, pid)
        if handle:
            try:
                self.assertEqual(kernel.WaitForSingleObject(handle, 0), 0)
            finally:
                kernel.CloseHandle(handle)
        else:
            self.assertEqual(RUNNER.ctypes.get_last_error(), 87)

    def test_memory_limit_recorded_separately(self):
        result = self.capture('x=bytearray(128*1024**2)', memory_limit_bytes=32 * 1024 ** 2)
        self.assertEqual(result["status"], "memory_limited")
        self.assertEqual(result["memory_limit_bytes"], 32 * 1024 ** 2)
        self.assertNotEqual(result["returncode"], 0)

    def test_campaign_lock_refuses_second_owner(self):
        lock_path = self.root / "exclusive.lock"
        with RUNNER._CampaignLock(lock_path), self.assertRaisesRegex(RUNNER.ProtocolError, "other_runner"), RUNNER._CampaignLock(lock_path):
            self.fail("second lock acquired")
        with RUNNER._CampaignLock(lock_path):
            self.assertTrue(lock_path.is_file())

    def test_budget_exhaustion_and_external_activity_kill_only_own_job(self):
        monitor = lambda: {"allocated_bytes": 1, "disk_free_bytes": 1, "budget": {"allowed": False, "reasons": ["disk_free_floor"]}}
        result = self.capture('import time;time.sleep(15)', monitor=monitor)
        self.assertEqual(result["status"], "budget_limited")
        self.snapshot_mock.return_value = [{"pid": 987654321, "image": "foreign.exe", "created_filetime": 1}]
        with patch.object(RUNNER.WindowsJob, "owns_process_identity", return_value=False):
            result = self.capture('import time;time.sleep(15)', label="polluted")
        self.assertEqual(result["status"], "contaminated")
        self.assertEqual(result["external_processes"][0]["pid"], 987654321)


class CampaignExecutionTests(unittest.TestCase):
    """Exercise scheduling and receipt preservation without building Cargo."""
    protocol = ProtocolTests.protocol

    def setUp(self):
        ProtocolTests.setUp(self)
        for index in range(1, 5):
            before, after = self.root / f"pair-{index}-baseline", self.root / f"pair-{index}-candidate"
            for source in (before, after):
                source.mkdir()
                for name in ("Cargo.lock", RUNNER.DEFAULT_EDIT_PATH, "schemas/example.json"):
                    target = source / name
                    target.parent.mkdir(parents=True, exist_ok=True)
                    target.write_bytes((self.root / "frozen" / name).read_bytes())
            self.fields["allocations"].append({"pair_index": index, "attempt": 0, "baseline_source_root": str(before), "candidate_source_root": str(after)})
        self.protocol_value = self.protocol()
        self.protocol_path = self.root / "protocol.json"
        write_json(self.protocol_path, self.protocol_value)
        self.sample = {"schema_version": 1, "complete": True, "errors": [], "allocated_bytes": 9 * RUNNER.GIB, "disk_free_bytes": 100 * RUNNER.GIB, "budget": simple_budget(7 * RUNNER.GIB, 9 * RUNNER.GIB, 100 * RUNNER.GIB)}
        self.stubs = [patch.object(RUNNER, "_verify_segment_toolchain", side_effect=self.fake_segment), patch.object(RUNNER, "build_process_snapshot", return_value=[]), patch.object(RUNNER, "_free_bytes", return_value=self.sample["disk_free_bytes"]), patch.object(RUNNER, "_budget_sample", return_value=self.sample), patch.object(RUNNER, "_inventory_module", return_value=SimpleNamespace(budget_status=simple_budget))]
        for stub in self.stubs:
            stub.start()
            self.addCleanup(stub.stop)

    @staticmethod
    def fake_segment(protocol, root):
        path = root / ("segment-" + str(len(list(root.glob("segment-*.json")))) + ".json")
        write_json(path, {"complete": True, "protocol_sha256": RUNNER.canonical_sha256(protocol)})
        return {"path": str(path), "sha256": RUNNER.sha256_file(path)}

    @staticmethod
    def fake_arm(protocol, arm, source, arm_root, review):
        arm_root.mkdir(parents=True)
        result = {"schema_version": 1, "arm": arm, "source_commit": protocol["source"]["commit"], "source_tree": protocol["source"]["tree"], "status": "ok", "duration_ms": 3000, "retained_growth_bytes": RUNNER.GIB, "allocated_before_bytes": 4 * RUNNER.GIB, "peak_allocated_bytes": 6 * RUNNER.GIB, "jobs": [], "edit_jobs": [], "reasons": []}
        write_json(arm_root / "arm.json", result)
        return result

    def test_quick_disk_refusal_precedes_inventory_and_target_creation(self):
        minimum = 80 * RUNNER.GIB + self.protocol_value["predicted_growth_bytes"]
        with (
            patch.object(RUNNER, "_free_bytes", return_value=minimum - 1),
            patch.object(RUNNER, "_budget_sample", side_effect=AssertionError("must refuse before full scan")),
            self.assertRaisesRegex(RUNNER.ProtocolError, "projected_disk_free_below_minimum"),
        ):
            RUNNER.run_protocol(self.protocol_path)
        self.assertFalse(Path(self.protocol_value["run_root"]).exists())
        paths = list(self.root.glob("admission-disk-refused-*.json"))
        self.assertEqual(len(paths), 1)
        receipt = RUNNER._read_json(paths[0])
        self.assertFalse(receipt["allowed"])
        self.assertEqual(receipt["disk_free_bytes"], minimum - 1)
        self.assertEqual(receipt["minimum_free_bytes"], 80 * RUNNER.GIB)

    def test_segment_resume_and_fixed_five_completed_pairs(self):
        with patch.object(RUNNER, "_run_arm", side_effect=self.fake_arm) as arms:
            state = RUNNER.run_protocol(self.protocol_path, pair_limit=2)
            self.assertEqual(state["status"], "segment_complete")
            self.assertEqual(state["planned_pairs"], 5)
            self.assertEqual(state["completed_valid_pairs"], 2)
            self.assertEqual(arms.call_count, 4)
            for index, pair in enumerate(state["pairs"]):
                self.assertEqual(pair["order"], self.protocol_value["order"][index])
            state = RUNNER.run_protocol(self.protocol_path, pair_limit=3)
            self.assertEqual(state["status"], "complete")
            self.assertTrue(state["complete"])
            self.assertEqual(state["completed_valid_pairs"], 5)
            self.assertEqual(state["peak_pair_growth_bytes"], 4 * RUNNER.GIB)
            self.assertEqual(arms.call_count, 10)
            RUNNER._validate_resume(state, self.protocol_value, RUNNER.canonical_sha256(self.protocol_value))
            RUNNER.run_protocol(self.protocol_path)
            self.assertEqual(arms.call_count, 10)

    def test_failed_pair_retained_and_cannot_be_skipped_on_resume(self):
        def failed(*args):
            result = self.fake_arm(*args)
            result["status"] = "memory_limited"
            write_json(args[3] / "arm.json", result)
            return result
        with patch.object(RUNNER, "_run_arm", side_effect=failed) as arms:
            state = RUNNER.run_protocol(self.protocol_path)
            self.assertEqual(state["status"], "failed")
            self.assertFalse(state["complete"])
            self.assertEqual(state["pairs"][0]["status"], "memory_limited")
            state = RUNNER.run_protocol(self.protocol_path)
            self.assertEqual(state["completed_valid_pairs"], 0)
            self.assertEqual(arms.call_count, 1)

    def test_contamination_requires_fresh_full_pair_and_original_order(self):
        allocation = copy.deepcopy(self.fields["allocations"][0])
        allocation["attempt"] = 1
        for arm in ("baseline", "candidate"):
            source = self.root / ("retry-" + arm)
            source.mkdir()
            for name in ("Cargo.lock", RUNNER.DEFAULT_EDIT_PATH, "schemas/example.json"):
                path = source / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes((self.root / "frozen" / name).read_bytes())
            allocation[arm + "_source_root"] = str(source)
        self.fields["allocations"].append(allocation)
        self.protocol_value = self.protocol()
        write_json(self.protocol_path, self.protocol_value)
        calls = 0
        def polluted_once(*args):
            nonlocal calls
            calls += 1
            result = self.fake_arm(*args)
            if calls == 1:
                result["status"] = "contaminated"
                result["reasons"] = ["external_build_process"]
                write_json(args[3] / "arm.json", result)
            return result
        with patch.object(RUNNER, "_run_arm", side_effect=polluted_once):
            state = RUNNER.run_protocol(self.protocol_path, pair_limit=1)
        self.assertEqual([entry["status"] for entry in state["pairs"]], ["contaminated", "ok"])
        self.assertEqual(state["pairs"][0]["order"], state["pairs"][1]["order"])
        self.assertEqual(calls, 3)
        self.assertEqual(state["completed_valid_pairs"], 1)
        self.assertTrue((self.root / "run/pairs/pair-0000-attempt-00").exists())
        self.assertTrue((self.root / "run/pairs/pair-0000-attempt-01").exists())

    def test_resume_rejects_changed_immutable_arm_receipt(self):
        with patch.object(RUNNER, "_run_arm", side_effect=self.fake_arm):
            state = RUNNER.run_protocol(self.protocol_path, pair_limit=1)
        receipt = next(iter(state["pairs"][0]["arm_receipts"].values()))
        Path(receipt["path"]).write_text("{}", encoding="utf-8")
        with self.assertRaisesRegex(RUNNER.ProtocolError, "arm_receipt_changed"):
            RUNNER.run_protocol(self.protocol_path)

    def test_inflight_interruption_preserves_and_stops_campaign(self):
        with patch.object(RUNNER, "_run_arm", side_effect=KeyboardInterrupt), self.assertRaises(KeyboardInterrupt):
            RUNNER.run_protocol(self.protocol_path)
        state = RUNNER.run_protocol(self.protocol_path)
        self.assertEqual(state["status"], "interrupted")
        self.assertFalse(state["complete"])
        self.assertEqual(state["pairs"][0]["status"], "interrupted")
        self.assertIn("raw_root", state["pairs"][0])


class ArmLifecycleTests(unittest.TestCase):
    setUp = ProtocolTests.setUp
    protocol = ProtocolTests.protocol

    def test_edit_feedback_uses_retained_incremental_target_and_restores_own_edit(self):
        protocol = self.protocol()
        source = Path(self.fields["allocations"][0]["candidate_source_root"])
        original = (source / RUNNER.DEFAULT_EDIT_PATH).read_bytes()
        record = []
        class FakeJob:
            def account(self):
                return {"peak_commit_bytes": 100}
            def close(self):
                pass
        def parallel(job, protocol, config, source, targets, logs, phase, environment, monitor, **kwargs):
            rows = []
            for fixed in RUNNER.JOBS:
                target = targets / ("primary" if phase == "primary" or config["incremental"] == "on" else "feedback") / fixed["lane"]
                argv = RUNNER.job_command(protocol, fixed, target)
                label = phase + "-" + fixed["name"]
                output, error = logs / (label + ".stdout"), logs / (label + ".stderr")
                output.write_bytes(b'{"reason":"build-finished","success":true}\n')
                error.write_bytes(b"")
                record.append((label, argv, environment["CARGO_INCREMENTAL"]))
                rows.append({"job": fixed["name"], "status": "ok", "reasons": [], "returncode": 0, "duration_ms": 10, "peak_commit_bytes": 100, "process_tree_capture_complete": True, "external_processes": [], "resource_samples": [], "argv": argv, "stdout_path": str(output), "stderr_path": str(error)})
            return rows, 20
        sample = {"schema_version": 1, "complete": True, "errors": [], "allocated_bytes": 9 * RUNNER.GIB, "disk_free_bytes": 100 * RUNNER.GIB, "budget": simple_budget(7 * RUNNER.GIB, 9 * RUNNER.GIB, 100 * RUNNER.GIB)}
        module = SimpleNamespace(budget_status=simple_budget, inventory=lambda *args, **kwargs: {"schema_version": 1, "complete": True, "errors": [], "allocated_bytes": 123})
        with patch.object(RUNNER, "build_process_snapshot", return_value=[]), patch.object(RUNNER, "WindowsJob", FakeJob), patch.object(RUNNER, "_run_parallel_phase", side_effect=parallel), patch.object(RUNNER, "_LiveBudget", return_value=lambda: sample), patch.object(RUNNER, "_memory_status", return_value={"physical_available": 10 * RUNNER.GIB, "commit_available": 30 * RUNNER.GIB}), patch.object(RUNNER, "_budget_sample", return_value=sample), patch.object(RUNNER, "_inventory_module", return_value=module), patch.dict(os.environ, {"CARGO_HOME": str(self.root)}):
            result = RUNNER._run_arm(protocol, "candidate", source, self.root / "arm", None)
        self.assertEqual(result["status"], "ok", result.get("reasons"))
        self.assertTrue(result["own_edit_restored"])
        self.assertEqual((source / RUNNER.DEFAULT_EDIT_PATH).read_bytes(), original)
        self.assertEqual([len(result[key]) for key in ("jobs", "feedback_warm_jobs", "edit_jobs")], [6, 6, 6])
        for label, argv, incremental in record:
            target = argv[argv.index("--target-dir") + 1]
            if label.startswith("primary-"):
                self.assertIn("primary", Path(target).parts)
            else:
                self.assertIn("primary", Path(target).parts)
                self.assertEqual(incremental, "1")
        self.assertTrue(result["owned_allocation_receipt"]["complete"])
        self.assertEqual(result["owned_allocation_receipt"]["storage_scope"], "owned_targets_and_stores")



@unittest.skipUnless(os.name == "nt", "Windows native process-tree test")
class ConcurrentPhaseTests(unittest.TestCase):
    def test_six_roots_and_three_lanes_have_real_distinct_concurrency(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            config = {"layout": "isolated", "tool": {"kind": "native"}}
            protocol = {"timeout_seconds": 20, "poll_seconds": 0.05}
            def command(protocol, fixed, target):
                return [sys.executable, "-c", "import time;time.sleep(0.25);print('done')"]
            monitor = lambda: {"allocated_bytes": 0, "disk_free_bytes": 100 * RUNNER.GIB, "budget": {"allowed": True, "reasons": []}}
            counts = []
            with patch.object(RUNNER, "job_command", side_effect=command), patch.object(RUNNER, "build_process_snapshot", return_value=[]), patch.object(RUNNER, "parse_cargo_json", return_value={"build_success": True, "errors": [], "artifacts": []}):
                for layout in ("isolated", "3lane"):
                    config["layout"] = layout
                    logs = root / layout
                    logs.mkdir()
                    with RUNNER.WindowsJob(128 * 1024**2) as job:
                        rows, duration = RUNNER._run_parallel_phase(job, protocol, config, root, logs / "targets", logs, "primary", dict(os.environ), monitor)
                    self.assertEqual([row["job"] for row in rows], [fixed["name"] for fixed in RUNNER.JOBS])
                    self.assertTrue(all(row["status"] == "ok" and row["returncode"] == 0 for row in rows))
                    maximum = max(sample["active_root_commands"] for row in rows for sample in row["resource_samples"])
                    counts.append(maximum)
                    self.assertGreater(duration, 0)
            self.assertEqual(counts, [6, 3])

    def test_midphase_budget_refusal_kills_all_six_roots(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            monitor = lambda: {"allocated_bytes": 100, "disk_free_bytes": 79 * RUNNER.GIB, "budget": {"allowed": False, "reasons": ["disk_free_below_minimum"]}}
            with patch.object(RUNNER, "job_command", return_value=[sys.executable, "-c", "import time;time.sleep(30)"]), patch.object(RUNNER, "build_process_snapshot", return_value=[]), RUNNER.WindowsJob(128 * 1024**2) as job:
                rows, _ = RUNNER._run_parallel_phase(job, {"timeout_seconds": 10, "poll_seconds": 0.05}, {"layout": "isolated", "tool": {"kind": "native"}}, root, root / "targets", root, "primary", dict(os.environ), monitor)
                self.assertEqual(job.account()["active_processes"], 0)
            self.assertEqual(len(rows), 6)
            self.assertTrue(all(row["status"] == "budget_limited" for row in rows))


    def test_invalid_cargo_receipt_keeps_all_launched_job_evidence(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            monitor = lambda: {"allocated_bytes": 0, "disk_free_bytes": 100 * RUNNER.GIB, "budget": {"allowed": True, "reasons": []}}
            calls = 0
            def parse(stdout, target, source):
                nonlocal calls
                calls += 1
                if calls == 1:
                    raise RUNNER.ProtocolError("artifact_outside_invocation_target")
                return {"build_success": True, "errors": [], "artifacts": []}
            with patch.object(RUNNER, "job_command", return_value=[sys.executable, "-c", "print('done')"]), patch.object(RUNNER, "build_process_snapshot", return_value=[]), patch.object(RUNNER, "parse_cargo_json", side_effect=parse), RUNNER.WindowsJob(128 * 1024**2) as job:
                rows, _ = RUNNER._run_parallel_phase(job, {"timeout_seconds": 10, "poll_seconds": 0.05}, {"layout": "isolated", "tool": {"kind": "native"}}, root, root / "targets", root, "primary", dict(os.environ), monitor)
            self.assertEqual([row["job"] for row in rows], [fixed["name"] for fixed in RUNNER.JOBS])
            self.assertEqual(rows[0]["status"], "capture_failed")
            self.assertEqual(rows[0]["reasons"], ["artifact_outside_invocation_target"])
            self.assertTrue(all((root / ("primary-" + fixed["name"] + ".json")).is_file() for fixed in RUNNER.JOBS))


if __name__ == "__main__":
    unittest.main()
