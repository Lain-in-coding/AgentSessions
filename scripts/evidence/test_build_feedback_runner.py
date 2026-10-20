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
    if arguments in (("diff", "--cached", "--name-only", "HEAD"), ("ls-files", "--others", "--exclude-standard"),
                      ("ls-files", "--others", "--ignored", "--exclude-standard", "-z")):
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



def bounded_fields(fields, *, phase="capacity"):
    """Tiny identity-bound plans; synthetic observations are not real admission."""
    fields = copy.deepcopy(fields)
    legacy = RUNNER.make_protocol(fields)
    root = Path(fields["source_receipt_path"]).parent
    fields.update(protocol_version=2, phase=phase,
                  budget_policy={"policy_version": 2, "minimum_free_bytes": 40 * RUNNER.GIB,
                                 "external_growth_allowance_bytes": 8 * RUNNER.GIB})
    if phase == "capacity":
        probe_source = root / "capacity-source"
        probe_source.mkdir(exist_ok=True)
        for entry in RUNNER._manifest(RUNNER._read_json(Path(fields["source_receipt_path"]))):
            output = probe_source / entry["path"]
            output.parent.mkdir(parents=True, exist_ok=True)
            output.write_bytes((Path(fields["source_root"]) / entry["path"]).read_bytes())
        fields.update(allocations=[], predicted_growth_bytes=8 * RUNNER.GIB,
                      capacity_probe={"source_root": str(probe_source), "package": "agent-session-grep-domain",
                                      "command": "check", "stage": "initial", "owned_output_cap_bytes": 8 * RUNNER.GIB},
                      reservation_provenance={"kind": "initial-capacity-cap"})
        disposable = [str(Path(fields["run_root"]) / "capacity" / "targets")]
    else:
        observation = root / "reservation-observation.json"
        write_json(observation, {"kind": "build-feedback-arm", "status": "ok", "allocation_complete": True,
                                 "source_verified": True, "process_tree_drained": True,
                                 "source_commit": fields["source_commit"], "source_tree": fields["source_tree"],
                                 "owned_allocated_bytes": RUNNER.GIB})
        reservation = root / "reservation.json"
        write_json(reservation, {"schema_version": 2, "kind": "build-feedback-reservation-evidence", "complete": True,
                                "source_receipt_sha256": legacy["source"]["receipt_sha256"],
                                "baseline_receipt_sha256": legacy["baseline"]["receipt_sha256"],
                                "toolchain_sha256": RUNNER.canonical_sha256(legacy["toolchain"]),
                                "workload_scope": "full-arm-and-qualification", "observed_peak_bytes": RUNNER.GIB,
                                "observations": [{"path": str(observation), "sha256": RUNNER.sha256_file(observation)}]})
        observations = []
        for arm, config in legacy["configs"].items():
            record = root / ("reservation-observation-" + arm + ".json")
            write_json(record, {**RUNNER._read_json(observation), "config_sha256": RUNNER.canonical_sha256(config)})
            observations.append({"path": str(record), "sha256": RUNNER.sha256_file(record)})
        value = RUNNER._read_json(reservation)
        value["observations"] = observations
        write_json(reservation, value)
        fields["reservation_provenance"] = {"kind": "measured-capacity", "margin_bytes": RUNNER.GIB,
                                             "receipt": {"path": str(reservation), "sha256": RUNNER.sha256_file(reservation)}}
        disposable = [str(Path(fields["run_root"]) / "pairs" / f"pair-{entry['pair_index']:04d}-attempt-{entry['attempt']:02d}" / arm / "targets") for entry in fields["allocations"] for arm in ("baseline", "candidate")]
    checker = MODULE_PATH.with_name("build_feedback_retention.py").resolve()
    tools = {}
    for name in ("runner", "statistics", "benchmark", "inventory"):
        tool = MODULE_PATH.with_name("build_feedback_" + name + ".py").resolve()
        tools[name] = {"path": str(tool), "sha256": RUNNER.sha256_file(tool), "bytes": tool.stat().st_size}
    key = root / "synthetic-local-integrity.key"
    key.write_bytes(b"k" * 32)
    source_roots = [fields["source_root"], *[entry[arm + "_source_root"] for entry in fields["allocations"] for arm in ("baseline", "candidate")]]
    if phase == "capacity":
        source_roots.append(fields["capacity_probe"]["source_root"])
    source_receipt = RUNNER._read_json(Path(fields["source_receipt_path"]))
    approval = root / "retention-approval.json"
    write_json(approval, {"schema_version": 2, "kind": "build-feedback-retention-approval", "approved": True,
                         "source_receipt_sha256": legacy["source"]["receipt_sha256"],
                         "baseline_receipt_sha256": legacy["baseline"]["receipt_sha256"],
                         "toolchain_sha256": RUNNER.canonical_sha256(legacy["toolchain"]),
                         "configs_sha256": RUNNER.canonical_sha256(legacy["configs"]),
                         "run_root": fields["run_root"], "disposable_target_paths": disposable,
                         "scope": "validated-successful-and-cap-stopped-task-intermediates",
                         "flow": ["verify", "recycle", "match", "purge-exact", "verify-release", "receipt"],
                         "checker": {"path": str(checker), "sha256": RUNNER.sha256_file(checker), "bytes": checker.stat().st_size},
                         "verification_tools": tools, "protected_roots": [source_receipt["live_source"], *source_roots],
                         "allowed_source_roots": source_roots, "allow_recent_new_outputs": True,
                         "local_integrity_key": {"path": str(key), "sha256": RUNNER.sha256_file(key), "bytes": 32},
                         "protected": ["source", "git-common-directory", "daily-cache", "existing-last-72-hours", "stores", "seeds", "quality-source-failures", "unrelated-recycle-items", "pagefile"]})
    fields["retention_policy"] = {"mode": "sealed-verified-recycle", "approval": {"path": str(approval), "sha256": RUNNER.sha256_file(approval)}}
    return fields

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

    def test_diagnostic_selector_resolves_only_a_cargo_emitted_target_artifact(self):
        protocol = self.protocol()
        target = self.root / "targets"
        executable = target / "debug" / "agent-session-grep.exe"
        executable.parent.mkdir(parents=True)
        executable.write_bytes(b"synthetic Cargo binary")
        receipt = RUNNER._artifact_identity(executable)
        arm = {
            "config": protocol["configs"]["candidate"],
            "target_root": str(target),
            "jobs": [{"cargo": {"artifacts": [{"files": [receipt]}]}}],
            "feedback_warm_jobs": [],
            "edit_jobs": [],
        }
        selector = {"artifact_relative_path": "debug/agent-session-grep.exe",
                    "args": ["--help"]}
        helper = SimpleNamespace(validate_policy=lambda approved: (
            {}, {"diagnostic_commands": {RUNNER.canonical_sha256(arm["config"]): selector}}))
        with patch.object(RUNNER, "_retention_module", return_value=helper):
            self.assertEqual(RUNNER.approved_diagnostic_argv(protocol, arm),
                             [str(executable), "--help"])
        selector["artifact_relative_path"] = "debug/missing.exe"
        with patch.object(RUNNER, "_retention_module", return_value=helper), \
             self.assertRaisesRegex(RUNNER.ProtocolError, "not_cargo_emitted"):
            RUNNER.approved_diagnostic_argv(protocol, arm)

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

    def test_v2_ignored_source_output_must_use_the_exact_declared_suite(self):
        protocol = self.protocol()
        protocol["schema_version"] = 2
        source = Path(protocol["source"]["root"])
        suite = source / "evidence-output" / "build-feedback" / "declared-suite"
        protocol["qualification_allocations"] = [{"source_root": str(source), "release_suite_root": str(suite)}]
        declared = suite / "report" / "official.json"
        declared.parent.mkdir(parents=True)
        declared.write_bytes(b"synthetic ignored report")
        unrelated = source / "target" / "old-cache.bin"
        unrelated.parent.mkdir(parents=True)
        unrelated.write_bytes(b"undeclared ignored output")
        ignored = {"paths": [declared.relative_to(source).as_posix()]}

        def git(root, *args):
            if args == ("ls-files", "--others", "--ignored", "--exclude-standard", "-z"):
                return "\0".join(ignored["paths"]) + "\0"
            return git_identity(root, *args)

        with patch.object(RUNNER, "_git", side_effect=git):
            RUNNER._verify_source(source, protocol)
            ignored["paths"].append(unrelated.relative_to(source).as_posix())
            with self.assertRaisesRegex(RUNNER.ProtocolError, "ignored_output_not_declared"):
                RUNNER._verify_source(source, protocol)
            ignored["paths"] = [unrelated.relative_to(source).as_posix()]
            legacy = {**protocol, "schema_version": 1}
            RUNNER._verify_source(source, legacy)

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



class BoundedProtocolTests(unittest.TestCase):

    def revised_fields(self, phase="capacity"):
        return bounded_fields(self.fields_legacy, phase=phase)

    def setUp(self):
        ProtocolTests.setUp(self)
        self.fields_legacy = self.fields
        def synthetic_peak(reference, expected, scope):
            _, raw = RUNNER._proof_reference(reference, "reservation_observation")
            RUNNER._require(raw.get("status") == "ok" and raw.get("allocation_complete") is True,
                            "reservation_observation_failed")
            return raw["owned_allocated_bytes"]
        self.reservation_patch = patch.object(RUNNER, "_reservation_observation_peak", side_effect=synthetic_peak)
        self.reservation_patch.start()
        self.addCleanup(self.reservation_patch.stop)

    def test_capacity_single_leaf_and_exact_56gib_admission(self):
        protocol = RUNNER.make_protocol(self.revised_fields())
        RUNNER._validate_protocol(protocol)
        self.assertEqual(protocol["pairs"], 0)
        self.assertEqual(protocol["order"], [])
        self.assertEqual(protocol["allocations"], [])
        self.assertEqual(protocol["memory_limit_bytes"], 24 * RUNNER.GIB)
        command = RUNNER.capacity_command(protocol, self.root / "declared-target")
        self.assertEqual(command[1:4], ["check", "-p", "agent-session-grep-domain"])
        self.assertNotIn("--workspace", command)
        self.assertIn("--all-targets", command)
        self.assertIn("--locked", command)
        self.assertIn("--offline", command)
        self.assertEqual(command[command.index("--target-dir") + 1], str(self.root / "declared-target"))
        sample = {"schema_version": 1, "complete": True, "errors": [], "allocated_bytes": 7 * RUNNER.GIB,
                  "disk_free_bytes": 56 * RUNNER.GIB}
        preview = RUNNER.preview_protocol(protocol, sample)
        self.assertTrue(preview["allowed"], preview["reasons"])
        self.assertEqual(preview["budget"]["minimum_start_free_bytes"], 56 * RUNNER.GIB)
        sample["disk_free_bytes"] -= 1
        self.assertFalse(RUNNER.preview_protocol(protocol, sample)["allowed"])
        self.assertFalse(Path(protocol["run_root"]).exists())

    def test_v1_rejects_revision_fields_and_retains_fixed_schedule(self):
        legacy = copy.deepcopy(self.fields_legacy)
        legacy["budget_policy"] = {"policy_version": 2, "minimum_free_bytes": 40 * RUNNER.GIB,
                                   "external_growth_allowance_bytes": 8 * RUNNER.GIB}
        with self.assertRaisesRegex(RUNNER.ProtocolError, "require_protocol_v2"):
            RUNNER.make_protocol(legacy)
        legacy.pop("budget_policy")
        legacy.update(phase="formal", pairs=12)
        with self.assertRaisesRegex(RUNNER.ProtocolError, "fixed_pairs_required"):
            RUNNER.make_protocol(legacy)
        with self.assertRaisesRegex(RUNNER.ProtocolError, "fixed_pairs_required"):
            RUNNER.balanced_order(12, 7)
        self.assertEqual(len(RUNNER.balanced_order(12, 7, protocol_version=2)), 12)

    def test_revision_strict_floor_allowance_cap_and_no_paired_capacity(self):
        fields = self.revised_fields()
        changes = [({"protocol_version": True}, "unsupported_protocol"),
                   ({"budget_policy": {"policy_version": 2, "minimum_free_bytes": 39 * RUNNER.GIB, "external_growth_allowance_bytes": 8 * RUNNER.GIB}}, "budget_policy"),
                   ({"budget_policy": {"policy_version": 2, "minimum_free_bytes": 40 * RUNNER.GIB, "external_growth_allowance_bytes": 0}}, "budget_policy"),
                   ({"pairs": 5}, "fixed_pairs_required"),
                   ({"allocations": self.fields_legacy["allocations"]}, "capacity_has_no_pair_allocations"),
                   ({"reservation_provenance": {"kind": "unmeasured-small-forecast"}}, "initial_capacity_reservation")]
        for update, reason in changes:
            with self.subTest(update=update), self.assertRaisesRegex(RUNNER.ProtocolError, reason):
                RUNNER.make_protocol({**fields, **update})
        fields["capacity_probe"]["owned_output_cap_bytes"] = 4 * RUNNER.GIB
        fields["predicted_growth_bytes"] = 4 * RUNNER.GIB
        with self.assertRaisesRegex(RUNNER.ProtocolError, "initial_capacity_cap_must_be_8gib"):
            RUNNER.make_protocol(fields)

    def test_changed_retention_approval_scope_and_checker_refuse(self):
        fields = self.revised_fields()
        approval_ref = fields["retention_policy"]["approval"]
        approval_path = Path(approval_ref["path"])
        approval = RUNNER._read_json(approval_path)
        approved = copy.deepcopy(approval)
        approval["disposable_target_paths"].append(str(self.root / "daily-target"))
        write_json(approval_path, approval)
        with self.assertRaisesRegex(RUNNER.ProtocolError, "retention_approval_changed"):
            RUNNER.make_protocol(fields)
        approval_ref["sha256"] = RUNNER.sha256_file(approval_path)
        with self.assertRaisesRegex(RUNNER.ProtocolError, "scope_mismatch"):
            RUNNER.make_protocol(fields)
        write_json(approval_path, approved)
        approval_ref["sha256"] = RUNNER.sha256_file(approval_path)
        approved["checker"]["sha256"] = "0" * 64
        write_json(approval_path, approved)
        approval_ref["sha256"] = RUNNER.sha256_file(approval_path)
        with self.assertRaisesRegex(RUNNER.ProtocolError, "retention_checker_changed"):
            RUNNER.make_protocol(fields)

    def test_v2_pilot_preserves_six_jobs_five_pairs_and_measured_reservation(self):
        fields = self.revised_fields("pilot")
        protocol = RUNNER.make_protocol(fields)
        RUNNER._validate_protocol(protocol)
        self.assertEqual(protocol["pairs"], 5)
        self.assertEqual([len(RUNNER.phase_queues(config)) for config in protocol["configs"].values()], [6, 3])
        self.assertEqual(sum(map(len, RUNNER.phase_queues(protocol["configs"]["candidate"]))), 6)
        self.assertIsNone(protocol["formal_registration"])
        bad = copy.deepcopy(fields)
        bad["predicted_growth_bytes"] = 2 * RUNNER.GIB - 1
        with self.assertRaisesRegex(RUNNER.ProtocolError, "below_observed_peak_and_margin"):
            RUNNER.make_protocol(bad)
        observation = Path(RUNNER._read_json(Path(fields["reservation_provenance"]["receipt"]["path"]))["observations"][0]["path"])
        payload = RUNNER._read_json(observation)
        payload["status"] = "failed"
        write_json(observation, payload)
        reservation_ref = fields["reservation_provenance"]["receipt"]
        reservation_path = Path(reservation_ref["path"])
        reservation = RUNNER._read_json(reservation_path)
        reservation["observations"][0]["sha256"] = RUNNER.sha256_file(observation)
        write_json(reservation_path, reservation)
        reservation_ref["sha256"] = RUNNER.sha256_file(reservation_path)
        with self.assertRaisesRegex(RUNNER.ProtocolError, "observation_failed"):
            RUNNER.make_protocol(fields)

    def test_formal_v2_requires_preregistered_fixed_n_and_exact_pilot(self):
        pilot_fields = self.revised_fields("pilot")
        pilot_protocol = RUNNER.make_protocol(pilot_fields)
        pilot_protocol_path = self.root / "pilot-protocol.json"
        write_json(pilot_protocol_path, pilot_protocol)
        pilot_path = self.root / "pilot-campaign.json"
        write_json(pilot_path, {"phase": "pilot", "complete": True, "status": "complete",
                               "source_tree": pilot_protocol["source"]["tree"], "source_commit": pilot_protocol["source"]["commit"],
                               "planned_pairs": 5, "completed_valid_pairs": 5,
                               "toolchain_sha256": RUNNER.canonical_sha256(pilot_protocol["toolchain"]),
                               "protocol_path": str(pilot_protocol_path),
                               "peak_pair_growth_bytes": 2 * RUNNER.GIB, "peak_active_arm_growth_bytes": RUNNER.GIB})
        fields = copy.deepcopy(pilot_fields)
        fields.update(phase="formal", pairs=12, seed=123, predicted_growth_bytes=3 * RUNNER.GIB,
                      pilot_receipt_path=str(pilot_path), pilot_receipt_sha256=RUNNER.sha256_file(pilot_path))
        registration_path = self.root / "formal-registration.json"
        plan = {"schema_version": 2, "kind": "build-feedback-formal-registration", "approved": True,
                "formal_data_collected": False, "approved_at": "2026-10-18T00:00:00+00:00",
                "pairs": 12, "seed": 123, "order": RUNNER.balanced_order(12, 123, protocol_version=2),
                "pilot_receipt_sha256": RUNNER.sha256_file(pilot_path),
                "source_receipt_sha256": pilot_protocol["source"]["receipt_sha256"],
                "baseline_receipt_sha256": pilot_protocol["baseline"]["receipt_sha256"],
                "toolchain_sha256": RUNNER.canonical_sha256(pilot_protocol["toolchain"]),
                "configs_sha256": RUNNER.canonical_sha256(pilot_protocol["configs"]),
                "run_root": fields["run_root"], "stopping_rule": "fixed-n-no-adaptive-additions",
                "rationale": "Synthetic variance plan: validates binding only.",
                "maximum_compute_seconds": 3600, "maximum_storage_bytes": 4 * RUNNER.GIB}
        write_json(registration_path, plan)
        fields["formal_registration"] = {"path": str(registration_path), "sha256": RUNNER.sha256_file(registration_path)}
        # Completed five-pair integrity is exercised by existing campaign tests;
        # this fixture isolates preregistration and measured active-arm margin.
        with patch.object(RUNNER, "_validate_resume", return_value=None), patch.object(RUNNER, "_verify_completed_v2_pilot"):
            protocol = RUNNER.make_protocol(fields)
            RUNNER._validate_protocol(protocol)
            self.assertEqual(protocol["pairs"], 12)
            for key, value in (("pairs", 13), ("formal_data_collected", True), ("approved_at", "2099-01-01T00:00:00+00:00"),
                               ("stopping_rule", "keep-adding-until-significant")):
                changed = {**plan, key: value}
                write_json(registration_path, changed)
                fields["formal_registration"]["sha256"] = RUNNER.sha256_file(registration_path)
                with self.subTest(key=key), self.assertRaises(RUNNER.ProtocolError):
                    RUNNER.make_protocol(fields)
            write_json(registration_path, plan)
            fields["formal_registration"]["sha256"] = RUNNER.sha256_file(registration_path)
            fields["formal_registration"] = None
            with self.assertRaisesRegex(RUNNER.ProtocolError, "formal_registration"):
                RUNNER.make_protocol(fields)

    def test_finalizer_inline_verified_flag_cannot_qualify_or_start_next_arm(self):
        protocol = RUNNER.make_protocol(self.revised_fields("pilot"))
        path = self.root / "pilot-finalizer-protocol.json"
        write_json(path, protocol)
        sample = {"schema_version": 1, "complete": True, "errors": [], "allocated_bytes": 7 * RUNNER.GIB,
                  "disk_free_bytes": 56 * RUNNER.GIB, "budget": {"allowed": True, "reasons": []}}
        helper = SimpleNamespace(validate_policy=RUNNER._retention_module().validate_policy,
                                 verify_retained_arm=lambda *args: (_ for _ in ()).throw(ValueError("missing pinned seal/disposition")))
        finalizer = lambda **kwargs: {"verified": True}
        with patch.object(RUNNER, "build_process_snapshot", return_value=[]), patch.object(RUNNER, "_budget_sample", return_value=sample), patch.object(RUNNER, "_free_bytes", return_value=56 * RUNNER.GIB), patch.object(RUNNER, "_verify_segment_toolchain", side_effect=CampaignExecutionTests.fake_segment), patch.object(RUNNER, "_run_arm", side_effect=CampaignExecutionTests.fake_arm) as arms, patch.object(RUNNER, "_retention_module", return_value=helper):
            state = RUNNER.run_protocol(path, pair_limit=1, arm_finalizer=finalizer)
        self.assertEqual(arms.call_count, 1)
        self.assertEqual(state["status"], "failed")
        self.assertEqual(state["completed_valid_pairs"], 0)
        self.assertEqual(state["pairs"][0]["status"], "capture_failed")
        self.assertIn("missing pinned seal", state["pairs"][0]["finalization_reasons"][0])
        self.assertNotIn("retained_evidence", state["pairs"][0])

    def test_real_v2_arm_receipt_carries_precreation_output_lease(self):
        protocol = RUNNER.make_protocol(self.revised_fields("pilot"))
        source = Path(self.fields_legacy["allocations"][0]["candidate_source_root"])
        arm_root = Path(protocol["run_root"]) / "pairs" / "pair-0000-attempt-00" / "candidate"
        sample = {"schema_version": 1, "complete": True, "errors": [],
                  "allocated_bytes": 7 * RUNNER.GIB, "disk_free_bytes": 56 * RUNNER.GIB,
                  "budget": {"allowed": True, "reasons": []}}
        class FakeJob:
            def account(self):
                return {"peak_commit_bytes": 100}
            def close(self):
                pass
        def parallel(job, protocol, config, source, targets, logs, phase, environment, monitor, **kwargs):
            entries = []
            for fixed in RUNNER.JOBS:
                target = targets / "primary" / fixed["lane"]
                argv = RUNNER.job_command(protocol, fixed, target)
                out, err = logs / (phase + fixed["name"] + ".stdout"), logs / (phase + fixed["name"] + ".stderr")
                out.write_bytes(b'{"reason":"build-finished","success":true}\n')
                err.write_bytes(b"")
                entries.append({"job": fixed["name"], "status": "ok", "reasons": [], "returncode": 0, "duration_ms": 10,
                                "peak_commit_bytes": 100, "process_tree_capture_complete": True, "external_processes": [],
                                "resource_samples": [], "argv": argv, "stdout_path": str(out), "stderr_path": str(err),
                                "cargo": {"build_success": True, "errors": [], "artifacts": []}})
            return entries, 20
        real = RUNNER._inventory_module()
        module = SimpleNamespace(budget_status=real.budget_status, inventory=lambda *args, **kwargs: {"schema_version": 1, "complete": True, "errors": [], "allocated_bytes": 123})
        with patch.object(RUNNER, "build_process_snapshot", return_value=[]), patch.object(RUNNER, "WindowsJob", FakeJob), patch.object(RUNNER, "_run_parallel_phase", side_effect=parallel), patch.object(RUNNER, "_LiveBudget", return_value=lambda: sample), patch.object(RUNNER, "_memory_status", return_value={"physical_available": 4 * RUNNER.GIB, "commit_available": 30 * RUNNER.GIB}), patch.object(RUNNER, "_budget_sample", return_value=sample), patch.object(RUNNER, "_inventory_module", return_value=module), patch.dict(os.environ, {"CARGO_HOME": str(self.root)}):
            arm = RUNNER._run_arm(protocol, "candidate", source, arm_root, None)
        self.assertEqual(arm["status"], "ok", arm["reasons"])
        self.assertEqual([len(arm[field]) for field in ("jobs", "feedback_warm_jobs", "edit_jobs")], [6, 6, 6])
        lease = arm["output_lease"]
        self.assertEqual(Path(lease["path"]).parent, Path(protocol["run_root"]) / "leases")
        self.assertFalse(Path(lease["path"]).is_relative_to(arm_root))
        self.assertEqual(RUNNER._read_json(Path(lease["path"]))["target"], str(arm_root / "targets"))
        RUNNER._retention_module()._safe_target(protocol, arm_root / "targets", lease)
        self.assertEqual(RUNNER._read_json(arm_root / "arm.json")["output_lease"], lease)

    def test_rebuilt_revision_detects_inline_projection_tampering(self):
        protocol = RUNNER.make_protocol(self.revised_fields())
        for key, value in (("pairs", 5), ("memory_limit_bytes", 8 * RUNNER.GIB), ("formal_registration", {"verified": True}),
                           ("predicted_growth_bytes", 7 * RUNNER.GIB)):
            changed = copy.deepcopy(protocol)
            changed[key] = value
            with self.subTest(key=key), self.assertRaises(RUNNER.ProtocolError):
                RUNNER._validate_protocol(changed)

    def test_real_entry_refuses_unrelated_checker_before_creating_run_root(self):
        fields = self.revised_fields()
        approval_ref = fields["retention_policy"]["approval"]
        approval_path = Path(approval_ref["path"])
        approval = RUNNER._read_json(approval_path)
        unrelated = self.root / "unrelated-checker.py"
        marker = self.root / "must-never-be-written.txt"
        unrelated.write_text("from pathlib import Path\nPath(" + repr(str(marker)) + ").write_text('executed')\n", encoding="utf-8")
        approval["checker"] = {"path": str(unrelated), "sha256": RUNNER.sha256_file(unrelated), "bytes": unrelated.stat().st_size}
        write_json(approval_path, approval)
        approval_ref["sha256"] = RUNNER.sha256_file(approval_path)
        # Protocol planning may bind the receipt, but execution selects only
        # the already known sibling and rejects receipt-selected code.
        protocol = RUNNER.make_protocol(fields)
        protocol_path = self.root / "unapproved-checker-protocol.json"
        write_json(protocol_path, protocol)
        with patch.object(RUNNER, "_budget_sample", side_effect=AssertionError("no inventory before checker gate")), self.assertRaisesRegex(ValueError, "unapproved retention checker"):
            RUNNER.run_protocol(protocol_path)
        self.assertFalse(Path(protocol["run_root"]).exists())
        self.assertFalse(marker.exists())

    def test_v2_pairs_refuse_missing_finalizer_before_any_allocation(self):
        protocol = RUNNER.make_protocol(self.revised_fields("pilot"))
        path = self.root / "missing-finalizer-protocol.json"
        write_json(path, protocol)
        with patch.object(RUNNER, "_budget_sample", side_effect=AssertionError("no inventory before missing callback refusal")), patch.object(RUNNER, "_run_arm", side_effect=AssertionError("cannot run arms")), self.assertRaisesRegex(RUNNER.ProtocolError, "v2_paired_run_requires_arm_finalizer_before_allocation"):
            RUNNER.run_protocol(path)
        self.assertFalse(Path(protocol["run_root"]).exists())
        with self.assertRaisesRegex(RUNNER.ProtocolError, "invalid_arm_finalizer"):
            RUNNER.run_protocol(path, arm_finalizer={"verified": True})
        self.assertFalse(Path(protocol["run_root"]).exists())

    def test_literal_output_lease_is_published_before_target_and_outside_arm(self):
        protocol = RUNNER.make_protocol(self.revised_fields())
        target = Path(protocol["run_root"]) / "capacity" / "targets"
        reference = RUNNER._declare_trial_output(protocol, target)
        self.assertFalse(target.exists())
        self.assertFalse(target.parent.exists())
        path = Path(reference["path"])
        self.assertEqual(path.parent, Path(protocol["run_root"]) / "leases")
        self.assertFalse(path.is_relative_to(target.parent))
        lease = RUNNER._read_json(path)
        self.assertIs(lease["observed_absent"], True)
        self.assertEqual(lease["target"], str(target))
        self.assertEqual(lease["protocol_sha256"], RUNNER.canonical_sha256(protocol))
        self.assertEqual(reference["sha256"], RUNNER.sha256_file(path))
        RUNNER._retention_module()._safe_target(protocol, target, reference)
        with self.assertRaisesRegex(ValueError, "immutable receipt already exists"):
            RUNNER._declare_trial_output(protocol, target)
        target.mkdir(parents=True)
        with self.assertRaisesRegex(RUNNER.ProtocolError, "new_target_lease_scope_mismatch"):
            RUNNER._declare_trial_output(protocol, target)
        self.assertEqual(reference["sha256"], RUNNER.sha256_file(path))

    def test_v2_budget_scan_counts_raw_retained_run_evidence_explicitly(self):
        protocol = RUNNER.make_protocol(self.revised_fields())
        calls = []
        real = RUNNER._inventory_module()
        def inventory(roots, store_roots, **kwargs):
            calls.append((roots, store_roots, kwargs))
            return {"schema_version": 1, "complete": True, "errors": [], "allocated_bytes": 7 * RUNNER.GIB}
        module = SimpleNamespace(inventory=inventory, budget_status=real.budget_status)
        with patch.object(RUNNER, "_inventory_module", return_value=module), patch.object(RUNNER, "_free_bytes", return_value=56 * RUNNER.GIB):
            report = RUNNER._budget_sample(protocol)
        self.assertIn(protocol["run_root"], calls[0][1])
        self.assertEqual(calls[0][2]["minimum_free_bytes"], 40 * RUNNER.GIB)
        self.assertEqual(report["budget"]["external_growth_allowance_bytes"], 8 * RUNNER.GIB)

    def test_quick_v2_disk_refusal_has_no_target_side_effect(self):
        protocol = RUNNER.make_protocol(self.revised_fields())
        path = self.root / "capacity-protocol.json"
        write_json(path, protocol)
        with patch.object(RUNNER, "build_process_snapshot", return_value=[]), patch.object(RUNNER, "_free_bytes", return_value=56 * RUNNER.GIB - 1), patch.object(RUNNER, "_budget_sample", side_effect=AssertionError("must refuse before scan")), self.assertRaisesRegex(RUNNER.ProtocolError, "projected_disk_free_below_minimum"):
            RUNNER.run_protocol(path)
        self.assertFalse(Path(protocol["run_root"]).exists())
        refusal = RUNNER._read_json(next(self.root.glob("admission-disk-refused-*.json")))
        self.assertEqual(refusal["minimum_free_bytes"], 40 * RUNNER.GIB)
        self.assertEqual(refusal["external_growth_allowance_bytes"], 8 * RUNNER.GIB)


class CapacityLifecycleTests(unittest.TestCase):
    def setUp(self):
        ProtocolTests.setUp(self)
        self.fields = bounded_fields(self.fields)
        self.protocol = RUNNER.make_protocol(self.fields)
        self.source = Path(self.protocol["capacity_probe"]["source_root"])
        self.sample = {"schema_version": 1, "complete": True, "errors": [],
                       "allocated_bytes": 7 * RUNNER.GIB, "disk_free_bytes": 56 * RUNNER.GIB,
                       "budget": {"allowed": True, "reasons": []}}

    def probe(self, status="ok", *, owned=123, altered_source=False):
        class FakeJob:
            def __init__(self, limit):
                self.limit = limit
            def account(self):
                return {"active_processes": 0}
            def start(self, argv, cwd, env, logs, label):
                out, err = logs / (label + ".stdout"), logs / (label + ".stderr")
                out.write_bytes(b'{"reason":"build-finished","success":true}\n')
                err.write_bytes(b"")
                return {"argv": argv, "cwd": str(cwd), "stdout_path": str(out), "stderr_path": str(err)}
            def close(self):
                pass
        class FakeMonitor:
            def __init__(self, protocol, before, job, **kwargs):
                self.io_origin = 0
                self.floor_free = before["disk_free_bytes"]
                self.last = {"job_write_bound_bytes": 100}
            def _write_bound(self):
                return 100
        def capture(job, process, protocol, **kwargs):
            if altered_source:
                (self.source / RUNNER.DEFAULT_EDIT_PATH).write_bytes(b"foreign modification")
            return {**process, "status": status, "reasons": ["owned_output_cap_reached"] if status == "capacity_limited" else [] if status == "ok" else ["command_nonzero"], "process_tree_capture_complete": True,
                    "peak_commit_bytes": 100, "returncode": 0 if status == "ok" else 7,
                    "stdout_sha256": RUNNER.sha256_file(Path(process["stdout_path"])),
                    "stderr_sha256": RUNNER.sha256_file(Path(process["stderr_path"]))}
        inventory = RUNNER._inventory_module()
        def allocation(*args, **kwargs):
            return {"schema_version": 1, "complete": True, "errors": [], "allocated_bytes": owned}
        module = SimpleNamespace(budget_status=inventory.budget_status, inventory=allocation)
        with patch.object(RUNNER, "WindowsJob", FakeJob), patch.object(RUNNER, "_LiveBudget", FakeMonitor), patch.object(RUNNER, "_wait_capture", side_effect=capture), patch.object(RUNNER, "build_process_snapshot", return_value=[]), patch.object(RUNNER, "_budget_sample", return_value=self.sample), patch.object(RUNNER, "_memory_status", return_value={"physical_available": 4 * RUNNER.GIB, "commit_available": 30 * RUNNER.GIB}), patch.object(RUNNER, "_inventory_module", return_value=module), patch.dict(os.environ, {"CARGO_HOME": str(self.root)}):
            return RUNNER._run_capacity_probe(self.protocol, self.root / "run" / "capacity", None)

    def test_single_capacity_capture_preserves_proofs_and_no_performance_count(self):
        result = self.probe()
        self.assertEqual(result["status"], "ok", result["reasons"])
        self.assertTrue(result["capture_complete"])
        self.assertTrue(result["artifacts_verified"])
        self.assertEqual(len(result["jobs"]), 1)
        self.assertEqual(result["completed_valid_pairs"], 0)
        self.assertFalse(result["performance_evidence"])
        self.assertEqual(result["memory_limit_bytes"], 24 * RUNNER.GIB)
        lease = result["output_lease"]
        self.assertEqual(Path(lease["path"]).parent, self.root / "run" / "leases")
        self.assertEqual(lease["sha256"], RUNNER.sha256_file(Path(lease["path"])))
        RUNNER._retention_module()._safe_target(self.protocol, Path(result["target_root"]), lease)
        for field in ("capture_receipt", "target_allocation_receipt", "owned_allocation_receipt"):
            reference = result[field]
            self.assertEqual(RUNNER.sha256_file(Path(reference["path"])), reference["sha256"])
        self.assertTrue((self.root / "run/capacity/capacity.json").is_file())

    def test_deliberate_cap_stop_eligible_but_quality_failure_retained(self):
        result = self.probe("capacity_limited")
        self.assertEqual(result["status"], "capacity_limited")
        self.assertTrue(result["capture_complete"])
        self.assertTrue(result["disposal_eligible"])
        self.assertFalse(result["artifacts_verified"])
        self.assertFalse(result["performance_evidence"])

    def test_quality_failure_is_not_disposable_capacity_completion(self):
        result = self.probe("failed")
        self.assertEqual(result["status"], "failed")
        self.assertFalse(result["capture_complete"])
        self.assertFalse(result["disposal_eligible"])
        self.assertTrue((self.root / "run/capacity/capacity.json").exists())

    def test_final_allocation_overrun_cannot_be_relabelled_successful_cap_stop(self):
        result = self.probe("capacity_limited", owned=8 * RUNNER.GIB + 1)
        self.assertEqual(result["status"], "budget_limited")
        self.assertIn("owned_output_cap_overrun", result["reasons"])
        self.assertFalse(result["disposal_eligible"])

    def test_foreign_source_failure_preserved_after_drain(self):
        result = self.probe("capacity_limited", altered_source=True)
        self.assertEqual(result["status"], "capture_failed")
        self.assertFalse(result["disposal_eligible"])
        self.assertTrue(result["process_tree_drained"])
        self.assertEqual((self.source / RUNNER.DEFAULT_EDIT_PATH).read_bytes(), b"foreign modification")

    def test_real_campaign_dispatch_stops_at_capacity_observation(self):
        path = self.root / "protocol.json"
        write_json(path, self.protocol)
        def observed(protocol, directory, review):
            directory.mkdir(parents=True)
            value = {"status": "capacity_limited", "reasons": ["owned_output_cap_reached"],
                     "capture_complete": True, "performance_evidence": False, "completed_valid_pairs": 0}
            write_json(directory / "capacity.json", value)
            return value
        with patch.object(RUNNER, "build_process_snapshot", return_value=[]), patch.object(RUNNER, "_free_bytes", return_value=56 * RUNNER.GIB), patch.object(RUNNER, "_budget_sample", return_value=self.sample), patch.object(RUNNER, "_verify_segment_toolchain", side_effect=CampaignExecutionTests.fake_segment), patch.object(RUNNER, "_run_capacity_probe", side_effect=observed) as probe, patch.object(RUNNER, "_run_arm", side_effect=AssertionError("capacity cannot dispatch pair arms")):
            state = RUNNER.run_protocol(path)
            self.assertEqual(probe.call_count, 1)
            self.assertEqual(state["status"], "complete")
            self.assertTrue(state["complete"])
            self.assertEqual(state["planned_pairs"], 0)
            self.assertEqual(state["completed_valid_pairs"], 0)
            self.assertFalse(state["performance_evidence"])
            self.assertEqual(state["pairs"], [])
            RUNNER.run_protocol(path)
            self.assertEqual(probe.call_count, 1)

    def test_capacity_campaign_complete_still_has_zero_pairs_and_rechecks_raw(self):
        path = self.root / "protocol.json"
        write_json(path, self.protocol)
        result = self.probe("capacity_limited")
        observation_path = self.root / "run/capacity/capacity.json"
        # Exercise campaign dispatch with a prevalidated observation created by
        # the real capacity function above; each mutable campaign is sealed.
        result_root = self.root / "run"
        state = {"schema_version": 2, "protocol_sha256": RUNNER.canonical_sha256(self.protocol),
                 "phase": "capacity", "planned_pairs": 0, "completed_valid_pairs": 0,
                 "performance_evidence": False, "pairs": [], "complete": True, "status": "complete",
                 "capacity_observation": result,
                 "capacity_receipt": {"path": str(observation_path), "sha256": RUNNER.sha256_file(observation_path)}}
        RUNNER._write_state(result_root / "campaign.json", state)
        value = RUNNER._read_json(result_root / "campaign.json")
        RUNNER._validate_resume(value, self.protocol, RUNNER.canonical_sha256(self.protocol))
        changed = copy.deepcopy(value)
        changed["completed_valid_pairs"] = 1
        RUNNER._write_state(result_root / "changed-state.json", changed)
        with self.assertRaisesRegex(RUNNER.ProtocolError, "completed_count_changed"):
            RUNNER._validate_resume(RUNNER._read_json(result_root / "changed-state.json"), self.protocol, RUNNER.canonical_sha256(self.protocol))
        observation_path.write_text("{}", encoding="utf-8")
        with self.assertRaisesRegex(RUNNER.ProtocolError, "capacity_receipt_changed"):
            RUNNER._validate_resume(value, self.protocol, RUNNER.canonical_sha256(self.protocol))


class FullCapacityProtocolTests(unittest.TestCase):
    """Full capacity remains unpaired and its declared cap is not a measured forecast."""
    def setUp(self):
        ProtocolTests.setUp(self)
        self.leaf_fields = bounded_fields(self.fields)
        self.leaf = RUNNER.make_protocol(self.leaf_fields)
        self.leaf_path = self.root / "leaf-protocol.json"
        write_json(self.leaf_path, self.leaf)
        self.prerequisite = self.root / "leaf-observation.json"
        write_json(self.prerequisite, {"protocol": {"path": str(self.leaf_path), "sha256": RUNNER.sha256_file(self.leaf_path)},
                                     "observed_peak_bytes": 4096, "config_sha256": RUNNER.canonical_sha256(self.leaf["configs"]["candidate"])})
        self.peak_patch = patch.object(RUNNER, "_reservation_observation_peak", return_value=4096)
        self.peak_patch.start()
        self.addCleanup(self.peak_patch.stop)

    def full_fields(self, *, cap=8 * RUNNER.GIB, arm="candidate"):
        fields = copy.deepcopy(self.leaf_fields)
        source = self.root / "full-capacity-source"
        source.mkdir(exist_ok=True)
        for entry in RUNNER._manifest(RUNNER._read_json(Path(fields["source_receipt_path"]))):
            destination = source / entry["path"]
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_bytes((Path(fields["source_root"]) / entry["path"]).read_bytes())
        fields.update(run_root=str(self.root / "full-run"), predicted_growth_bytes=cap,
                      capacity_probe={"kind": "full-arm-and-qualification", "stage": "expanded", "source_root": str(source),
                                      "arm": arm, "owned_output_cap_bytes": cap},
                      reservation_provenance={"kind": "declared-full-arm-cap", "prerequisite": {"path": str(self.prerequisite),
                                              "sha256": RUNNER.sha256_file(self.prerequisite)},
                                              "rationale": "Controlled cap may stop; complete workload remains unmeasured."})
        approval = RUNNER._read_json(Path(fields["retention_policy"]["approval"]["path"]))
        approval.update(run_root=fields["run_root"], disposable_target_paths=[str(Path(fields["run_root"]) / "capacity" / "targets")])
        approval["allowed_source_roots"].append(str(source))
        approval["protected_roots"].append(str(source))
        path = self.root / "full-approval.json"
        write_json(path, approval)
        fields["retention_policy"]["approval"] = {"path": str(path), "sha256": RUNNER.sha256_file(path)}
        return fields

    def test_full_controlled_8gib_cap_is_unpaired_and_exact_56gib_start(self):
        fields = self.full_fields()
        protocol = RUNNER.make_protocol(fields)
        RUNNER._validate_protocol(protocol)
        self.assertEqual((protocol["phase"], protocol["pairs"], protocol["allocations"], protocol["order"]), ("capacity", 0, [], []))
        suite = Path(fields["capacity_probe"]["source_root"]) / "evidence-output" / "build-feedback" / "asg-build-feedback-capacity-capacity-0-candidate"
        self.assertEqual(protocol["qualification_allocations"], [{"pair_index": None, "attempt": None, "arm": "candidate",
                          "source_root": fields["capacity_probe"]["source_root"], "release_suite_root": str(suite)}])
        sample = {"schema_version": 1, "complete": True, "errors": [], "allocated_bytes": 7 * RUNNER.GIB,
                  "disk_free_bytes": 56 * RUNNER.GIB}
        self.assertTrue(RUNNER.preview_protocol(protocol, sample)["allowed"])
        self.assertFalse(RUNNER.preview_protocol(protocol, {**sample, "disk_free_bytes": 56 * RUNNER.GIB - 1})["allowed"])
        changed = copy.deepcopy(protocol)
        changed["qualification_allocations"][0]["release_suite_root"] = str(self.root / "unrelated")
        with self.assertRaisesRegex(RUNNER.ProtocolError, "contract_changed"):
            RUNNER._validate_protocol(changed)
        changed = copy.deepcopy(protocol)
        changed["pairs"] = False
        with self.assertRaisesRegex(RUNNER.ProtocolError, "fixed_pairs_required"):
            RUNNER._validate_protocol(changed)

    def test_full_cap_requires_prior_leaf_and_explicit_unmeasured_rationale(self):
        fields = self.full_fields()
        with patch.object(RUNNER, "_reservation_observation_peak", return_value=7 * RUNNER.GIB), self.assertRaisesRegex(RUNNER.ProtocolError, "leaf_margin"):
            RUNNER.make_protocol(fields)
        with self.assertRaisesRegex(RUNNER.ProtocolError, "at_least_8gib"):
            RUNNER.make_protocol({**fields, "predicted_growth_bytes": 4 * RUNNER.GIB,
                                 "capacity_probe": {**fields["capacity_probe"], "owned_output_cap_bytes": 4 * RUNNER.GIB}})
        with self.assertRaisesRegex(RUNNER.ProtocolError, "provenance"):
            RUNNER.make_protocol({**fields, "reservation_provenance": {"kind": "measured-capacity", "receipt": {"verified": True}}})
        changed = copy.deepcopy(self.leaf)
        changed["capacity_probe"]["stage"] = "expanded"
        write_json(self.leaf_path, changed)
        prerequisite = RUNNER._read_json(self.prerequisite)
        prerequisite["protocol"]["sha256"] = RUNNER.sha256_file(self.leaf_path)
        write_json(self.prerequisite, prerequisite)
        fields["reservation_provenance"]["prerequisite"]["sha256"] = RUNNER.sha256_file(self.prerequisite)
        with self.assertRaisesRegex(RUNNER.ProtocolError, "successful_initial_leaf"):
            RUNNER.make_protocol(fields)

    def test_full_capacity_requires_lifecycle_before_output_allocation(self):
        protocol = RUNNER.make_protocol(self.full_fields())
        path = self.root / "full-protocol.json"
        write_json(path, protocol)
        helper = SimpleNamespace(validate_policy=lambda protocol: None)
        with patch.object(RUNNER, "_retention_module", return_value=helper), self.assertRaisesRegex(RUNNER.ProtocolError, "full_capacity_requires_arm_finalizer_before_allocation"):
            RUNNER.run_protocol(path)
        self.assertFalse(Path(protocol["run_root"]).exists())

    def test_external_release_suites_are_explicitly_billed(self):
        protocol = RUNNER.make_protocol(self.full_fields())
        seen = []
        def scan(roots, **keywords):
            seen.append(keywords["store_roots"])
            return {"complete": True, "errors": [], "allocated_bytes": 4096}
        real = RUNNER._inventory_module()
        with patch.object(RUNNER, "_pending_recycle_roots", return_value=[]), patch.object(RUNNER, "_free_bytes", return_value=56 * RUNNER.GIB), patch.object(RUNNER, "_inventory_module", return_value=SimpleNamespace(inventory=scan, budget_status=real.budget_status)):
            RUNNER._budget_sample(protocol)
        self.assertIn(protocol["qualification_allocations"][0]["release_suite_root"], seen[0])
        self.assertIn(protocol["run_root"], seen[0])

    def test_missing_other_configuration_observation_refuses_pilot_reservation(self):
        fields = bounded_fields(self.fields, phase="pilot")
        path = Path(fields["reservation_provenance"]["receipt"]["path"])
        value = RUNNER._read_json(path)
        value["observations"] = value["observations"][:1]
        write_json(path, value)
        fields["reservation_provenance"]["receipt"]["sha256"] = RUNNER.sha256_file(path)
        with patch.object(RUNNER, "_reservation_observation_peak", return_value=RUNNER.GIB), self.assertRaisesRegex(RUNNER.ProtocolError, "both_configurations"):
            RUNNER.make_protocol(fields)


class FullCapacityExecutionTests(unittest.TestCase):
    """Exercise the real dispatch/seal boundary with no Cargo or actual disposal."""
    setUp = FullCapacityProtocolTests.setUp
    full_fields = FullCapacityProtocolTests.full_fields
    def make_arm(self, protocol, arm, source, arm_root, review, *, status="ok"):
        arm_root.mkdir(parents=True)
        target = arm_root / "targets"
        target.mkdir()
        record = {"schema_version": 2, "kind": "build-feedback-arm", "phase": "capacity", "arm": arm,
                  "source_root": str(source), "target_root": str(target), "config": protocol["configs"][arm],
                  "protocol_sha256": RUNNER.canonical_sha256(protocol), "status": status,
                  "reasons": [] if status == "ok" else ["owned_output_cap_reached"], "capture_complete": status in ("ok", "capacity_limited"),
                  "started_at": RUNNER._stamp(), "source_verified": True, "process_tree_drained": True,
                  "output_lease": {"path": "synthetic-lease"}}
        write_json(arm_root / "arm.json", record)
        return record

    def helper(self, record, *, status="ok"):
        seal = {"kind": "build-feedback-full-capacity-seal", "phase": "capacity", "status": status,
                "performance_evidence": False, "arm": record.get("arm", "candidate"), "pair_index": None, "attempt": None,
                "workload_complete": status == "ok", "reservation_eligible": status == "ok", "observed_peak_bytes": 8192}
        return SimpleNamespace(verify_full_capacity_link=lambda reference, protocol: {"arm_receipt": record, "seal": seal})

    def test_full_runner_lifecycle_binds_immutable_capacity_arm_and_null_ordinals(self):
        protocol = RUNNER.make_protocol(self.full_fields())
        arm_root = Path(protocol["run_root"]) / "capacity"
        record = {}
        calls = []
        def arm(*args):
            value = self.make_arm(*args)
            record.update(value)
            return value
        def finalize(**arguments):
            calls.append(arguments)
            return {"path": "synthetic-independent-link", "sha256": "a" * 64}
        with patch.object(RUNNER, "_run_arm", side_effect=arm), patch.object(RUNNER, "_retention_module", side_effect=lambda: self.helper(record)):
            result = RUNNER._run_full_capacity_probe(protocol, arm_root, None, finalize)
        self.assertEqual(result["status"], "ok")
        self.assertTrue(result["reservation_eligible"])
        self.assertFalse(result["performance_evidence"])
        self.assertEqual(result["completed_valid_pairs"], 0)
        self.assertEqual((calls[0]["phase"], calls[0]["pair_index"], calls[0]["attempt"]), ("capacity", None, None))
        self.assertEqual(RUNNER._read_json(arm_root / "arm.json"), record)

    def test_qualifier_cap_stop_remains_capacity_even_if_eighteen_jobs_completed(self):
        protocol = RUNNER.make_protocol(self.full_fields())
        arm_root = Path(protocol["run_root"]) / "capacity"
        record = {}
        def arm(*args):
            value = self.make_arm(*args)
            record.update(value)
            return value
        with patch.object(RUNNER, "_run_arm", side_effect=arm), patch.object(RUNNER, "_retention_module", side_effect=lambda: self.helper(record, status="capacity_limited")):
            result = RUNNER._run_full_capacity_probe(protocol, arm_root, None, lambda **arguments: {"path": "verified-link"})
        self.assertEqual(result["status"], "capacity_limited")
        self.assertTrue(result["capture_complete"])
        self.assertFalse(result["workload_complete"])
        self.assertFalse(result["reservation_eligible"])

    def test_inline_finalizer_flags_and_failed_workload_cannot_supply_reservation(self):
        protocol = RUNNER.make_protocol(self.full_fields())
        arm_root = Path(protocol["run_root"]) / "capacity"
        with patch.object(RUNNER, "_run_arm", side_effect=self.make_arm), patch.object(RUNNER, "_retention_module", return_value=SimpleNamespace(verify_full_capacity_link=lambda *args: (_ for _ in ()).throw(ValueError("missing verified seal")))):
            result = RUNNER._run_full_capacity_probe(protocol, arm_root, None, lambda **arguments: {"verified": True})
        self.assertEqual(result["status"], "capture_failed")
        self.assertFalse(result["reservation_eligible"])
        self.assertTrue((arm_root / "arm.json").exists())
        failed_protocol = copy.deepcopy(protocol)
        failed_protocol["run_root"] = str(self.root / "failed-run")
        with patch.object(RUNNER, "_run_arm", side_effect=lambda *args: self.make_arm(*args, status="failed")):
            result = RUNNER._run_full_capacity_probe(failed_protocol, Path(failed_protocol["run_root"]) / "capacity", None,
                           lambda **arguments: (_ for _ in ()).throw(AssertionError("failed source must remain protected")))
        self.assertEqual(result["status"], "failed")
        self.assertFalse(result["disposal_eligible"])

    def test_full_capacity_campaign_dispatch_and_resume_keep_zero_pairs(self):
        protocol = RUNNER.make_protocol(self.full_fields())
        path = self.root / "full-protocol.json"
        write_json(path, protocol)
        record = {}
        def execute(*args):
            value = self.make_arm(*args)
            record.update(value)
            return value
        helper = self.helper(record)
        helper.validate_policy = lambda approved: None
        sample = {"schema_version": 1, "complete": True, "errors": [], "allocated_bytes": 7 * RUNNER.GIB,
                  "disk_free_bytes": 56 * RUNNER.GIB, "budget": {"allowed": True, "reasons": []}}
        with patch.object(RUNNER, "_retention_module", return_value=helper), patch.object(RUNNER, "_run_arm", side_effect=execute) as run, \
             patch.object(RUNNER, "build_process_snapshot", return_value=[]), patch.object(RUNNER, "_free_bytes", return_value=56 * RUNNER.GIB), \
             patch.object(RUNNER, "_budget_sample", return_value=sample), patch.object(RUNNER, "_verify_segment_toolchain", side_effect=CampaignExecutionTests.fake_segment), \
             patch.object(RUNNER, "_run_capacity_probe", side_effect=AssertionError("full capacity cannot enter leaf command")):
            state = RUNNER.run_protocol(path, arm_finalizer=lambda **arguments: {"path": "verified-capacity-link"})
            self.assertTrue(state["complete"], state["reasons"])
            self.assertEqual(state["completed_valid_pairs"], 0)
            self.assertEqual(state["pairs"], [])
            self.assertFalse(state["performance_evidence"])
            self.assertEqual(state["capacity_observation"]["kind"], "build-feedback-full-capacity-observation")
            RUNNER.run_protocol(path, arm_finalizer=lambda **arguments: {"path": "verified-capacity-link"})
            self.assertEqual(run.call_count, 1)
            Path(state["capacity_receipt"]["path"]).write_text("{}", encoding="utf-8")
            with self.assertRaisesRegex(RUNNER.ProtocolError, "capacity_receipt_changed"):
                RUNNER.run_protocol(path, arm_finalizer=lambda **arguments: {"path": "verified-capacity-link"})

    def test_qualifier_child_environment_rejects_ambient_release_overrides(self):
        fields = self.full_fields()
        msvc = self.root / "msvc-environment.json"
        script = self.root / "vcvarsall.bat"
        script.write_bytes(b"synthetic vcvars environment fixture")
        write_json(msvc, {"schema_version": 1, "identity": fields["toolchain"]["msvc"],
                          "environment": {"INCLUDE": "sdk-include", "LIB": "sdk-lib", "VCToolsInstallDir": str(self.root), "WindowsSdkDir": str(self.root)},
                          "script": {"path": str(script), "sha256": RUNNER.sha256_file(script)}})
        fields["msvc_environment_receipt_path"] = str(msvc)
        protocol = RUNNER.make_protocol(fields)
        record = self.make_arm(protocol, "candidate", Path(protocol["capacity_probe"]["source_root"]), Path(protocol["run_root"]) / "capacity", None)
        before = dict(os.environ)
        with patch.dict(os.environ, {"CARGO_HOME": str(self.root), "CARGO_PROFILE_RELEASE_OPT_LEVEL": "1", "RUSTC": "foreign", "RUSTFLAGS": "foreign", "CC": "foreign"}):
            active = dict(os.environ)
            environment = RUNNER.release_environment(protocol, record)
            self.assertEqual(dict(os.environ), active)
        self.assertEqual(dict(os.environ), before)
        for key in ("RUSTC", "RUSTFLAGS", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER", "CC", "CXX", "CARGO_PROFILE_RELEASE_OPT_LEVEL", "CARGO_PROFILE_DEV_DEBUG"):
            self.assertNotIn(key, environment)
        self.assertEqual(environment["INCLUDE"], "sdk-include")
        self.assertEqual(environment["LIB"], "sdk-lib")
        self.assertEqual(environment["PYTHONDONTWRITEBYTECODE"], "1")
        self.assertEqual(Path(environment["TEMP"]), Path(protocol["qualification_allocations"][0]["release_suite_root"]) / "scratch")
        self.assertTrue(environment["PATH"].startswith(str(Path(protocol["toolchain"]["cargo"]["path"]).parent)))


    def test_release_child_environment_satisfies_unchanged_official_configuration(self):
        # Exercise the real frozen validator contract without compiling Cargo.
        validator = Path(__file__).resolve().parents[2] / ".trellis/.runtime/build-feedback-infrastructure-20261018/pilot/pair-0001/baseline-source/scripts/evidence/core_beta_benchmark.py"
        if not validator.is_file():
            self.skipTest("local frozen official release producer unavailable")
        stats_spec = importlib.util.spec_from_file_location("build_feedback_frozen_loader_test", Path(__file__).with_name("build_feedback_statistics.py"))
        self.assertIsNotNone(stats_spec)
        stats = importlib.util.module_from_spec(stats_spec)
        stats_spec.loader.exec_module(stats)
        spec = importlib.util.spec_from_file_location("build_feedback_frozen_release_env_test", validator,
            loader=stats._ApprovedSourceLoader(validator, validator.read_bytes()))
        self.assertIsNotNone(spec)
        official = importlib.util.module_from_spec(spec)
        # Read-only test of frozen producer; do not emit bytecode into its source.
        spec.loader.exec_module(official)
        protocol, arm = FullQualificationCaptureTests.qualified_arm(self)
        environment_before = dict(os.environ)
        foreign = {prefix + "_" + name: "foreign" for prefix in ("HOST", "TARGET")
                   for name in ("CC", "CXX", "AR", "CFLAGS", "CXXFLAGS", "LDFLAGS")}
        foreign.update(CRATE_CC_NO_DEFAULTS="1", CC_ENABLE_DEBUG_OUTPUT="1", SQLCIPHER_LIB_DIR="foreign",
                       PKG_CONFIG_PATH="foreign", CARGO_PROFILE_RELEASE_DEBUG="2", RUSTFLAGS="foreign")
        with patch.dict(os.environ, {**foreign, "CARGO_HOME": str(self.root)}):
            ambient = dict(os.environ)
            child = RUNNER.release_environment(protocol, arm)
            self.assertEqual(dict(os.environ), ambient)
        self.assertEqual(dict(os.environ), environment_before)
        self.assertFalse(set(foreign) & set(child))
        cargo, rustc = protocol["toolchain"]["cargo"]["path"], protocol["toolchain"]["rustc"]["path"]
        args = SimpleNamespace(cargo=cargo, rustc=rustc)
        versions = "rustc 1.97.1\nhost: x86_64-pc-windows-msvc"
        def which(name):
            return cargo if Path(name).stem == "cargo" else rustc
        with patch.dict(os.environ, child, clear=True), patch.object(official.shutil, "which", side_effect=which), \
             patch.object(official, "checked_command", return_value=versions), \
             patch.object(official.subprocess, "run", side_effect=AssertionError("configuration must not build")), \
             patch.object(Path, "exists", return_value=False):
            config, target, cargo_home = official.build_configuration(Path(arm["source_root"]), args)
        self.assertEqual(config["compiler_link_overrides"], "none")
        self.assertEqual(config["environment"]["CARGO_BUILD_JOBS"], "2")
        self.assertEqual(target, Path(arm["target_root"]) / "qualification/release")
        self.assertEqual(cargo_home, self.root.resolve())


class FullCapacityArmClosureTests(unittest.TestCase):
    """Actual arm orchestration preserves edit reuse and gives quality failure precedence."""
    setUp = FullCapacityProtocolTests.setUp
    full_fields = FullCapacityProtocolTests.full_fields

    def run_arm(self, *, stop=False, quality_error=False):
        protocol = RUNNER.make_protocol(self.full_fields())
        source = Path(protocol["capacity_probe"]["source_root"])
        arm_root = Path(protocol["run_root"]) / "capacity"
        sample = {"schema_version": 1, "complete": True, "errors": [], "allocated_bytes": 7 * RUNNER.GIB,
                  "disk_free_bytes": 56 * RUNNER.GIB, "budget": {"allowed": True, "reasons": []}}
        owner = self
        class FakeJob:
            def __init__(self):
                self.active = 1
            def account(self):
                return {"peak_commit_bytes": 123, "active_processes": self.active}
            def terminate(self):
                self.active = 0
            def close(self):
                owner.assertEqual(self.active, 0)
        monitor = SimpleNamespace(io_origin=0, floor_free=55 * RUNNER.GIB, _write_bound=lambda: 8 * RUNNER.GIB if stop else 6000)
        def phase(job, approved, config, current_source, target, logs, name, environment, live, **keywords):
            captures = []
            for index, fixed in enumerate(RUNNER.JOBS):
                status = "capacity_limited" if stop else "ok"
                if quality_error and index == 2:
                    status = "failed"
                captures.append({"job": fixed["name"], "status": status,
                                 "reasons": ["compiler_error_before_capacity_stop"] if status == "failed" else ["owned_output_cap_reached"] if stop else [],
                                 "peak_commit_bytes": 123, "process_tree_capture_complete": True, "external_processes": [], "resource_samples": [],
                                 "cargo": {"build_success": not stop, "errors": []}})
            return captures, 20
        real = RUNNER._inventory_module()
        inventory = SimpleNamespace(budget_status=real.budget_status,
                                    inventory=lambda *args, **kwargs: {"schema_version": 1, "complete": True, "errors": [], "allocated_bytes": 5000})
        with patch.object(RUNNER, "_declare_trial_output", return_value={"path": "fixture-lease"}), \
             patch.object(RUNNER, "WindowsJob", FakeJob), patch.object(RUNNER, "_LiveBudget", return_value=monitor), \
             patch.object(RUNNER, "_run_parallel_phase", side_effect=phase), patch.object(RUNNER, "build_process_snapshot", return_value=[]), \
             patch.object(RUNNER, "_budget_sample", return_value=sample), patch.object(RUNNER, "_inventory_module", return_value=inventory), \
             patch.object(RUNNER, "_memory_status", return_value={"physical_available": 4 * RUNNER.GIB, "commit_available": 30 * RUNNER.GIB}), \
             patch.dict(os.environ, {"CARGO_HOME": str(self.root)}):
            result = RUNNER._run_arm(protocol, "candidate", source, arm_root, None)
        return result, source, arm_root

    def test_complete_capacity_arm_retains_eighteen_commands_and_restores_edit(self):
        arm, source, arm_root = self.run_arm()
        self.assertEqual(arm["status"], "ok", arm["reasons"])
        self.assertEqual([len(arm[field]) for field in ("jobs", "feedback_warm_jobs", "edit_jobs")], [6, 6, 6])
        self.assertTrue(arm["own_edit_restored"])
        self.assertFalse((source / RUNNER.DEFAULT_EDIT_PATH).read_bytes().endswith(RUNNER.EDIT_SUFFIX))
        self.assertEqual(arm["final_job_write_bound_bytes"], 6000)
        self.assertEqual(arm["minimum_disk_free_bytes"], 55 * RUNNER.GIB)
        self.assertFalse(arm["performance_evidence"])
        self.assertFalse(arm["reservation_eligible"])
        self.assertEqual(RUNNER._read_json(arm_root / "arm.json"), arm)

    def test_partial_cap_stop_does_not_execute_warm_or_edit_phases(self):
        arm, _source, _arm_root = self.run_arm(stop=True)
        self.assertEqual(arm["status"], "capacity_limited")
        self.assertEqual([len(arm[field]) for field in ("jobs", "feedback_warm_jobs", "edit_jobs")], [6, 0, 0])
        self.assertTrue(arm["capture_complete"])
        self.assertFalse(arm["workload_complete"])
        self.assertFalse(arm["reservation_eligible"])
        self.assertTrue(arm["disposal_eligible"])
        self.assertEqual(arm["final_job_write_bound_bytes"], 8 * RUNNER.GIB)

    def test_late_quality_failure_cannot_be_masked_by_earlier_capacity_stop(self):
        arm, _source, arm_root = self.run_arm(stop=True, quality_error=True)
        self.assertEqual(arm["status"], "failed")
        self.assertIn("compiler_error_before_capacity_stop", arm["reasons"])
        self.assertFalse(arm["capture_complete"])
        self.assertFalse(arm["disposal_eligible"])
        self.assertEqual(len(arm["jobs"]), 6)
        self.assertTrue((arm_root / "arm.json").is_file())


class FullQualificationCaptureTests(unittest.TestCase):
    """Real cumulative monitor and capture producer; no Cargo compilation or deletion."""
    setUp = FullCapacityProtocolTests.setUp
    full_fields = FullCapacityProtocolTests.full_fields
    make_arm = FullCapacityExecutionTests.make_arm

    def qualified_arm(self):
        fields = self.full_fields()
        script = self.root / "vcvarsall.bat"
        script.write_bytes(b"fixture msvc script")
        receipt = self.root / "msvc.json"
        write_json(receipt, {"schema_version": 1, "identity": fields["toolchain"]["msvc"],
                            "environment": {"LIB": "sdk-lib", "INCLUDE": "sdk-include", "VCToolsInstallDir": str(self.root), "WindowsSdkDir": str(self.root)},
                            "script": {"path": str(script), "sha256": RUNNER.sha256_file(script)}})
        fields["msvc_environment_receipt_path"] = str(receipt)
        protocol = RUNNER.make_protocol(fields)
        source = Path(protocol["capacity_probe"]["source_root"])
        arm_root = Path(protocol["run_root"]) / "capacity"
        arm = self.make_arm(protocol, "candidate", source, arm_root, None)
        sample = {"allocation_kind": "conservative-disk-and-job-write-bound", "allocated_bytes": 7 * RUNNER.GIB + 4000,
                  "disk_free_bytes": 55 * RUNNER.GIB, "job_write_bound_bytes": 4000,
                  "budget_allowed": True, "budget_reasons": []}
        arm.update(allocated_before_bytes=7 * RUNNER.GIB, disk_free_before_bytes=56 * RUNNER.GIB,
                   final_job_write_bound_bytes=4000, minimum_disk_free_bytes=55 * RUNNER.GIB,
                   jobs=[{"resource_samples": [sample]}], feedback_warm_jobs=[], edit_jobs=[])
        write_json(arm_root / "arm.json", arm)
        return protocol, arm

    def capture_context(self, protocol, arm, *, status="ok", add_write=100, foreign_source=False):
        state = {"write": 0, "captured": False}
        owner = self
        class FakeJob:
            def __init__(self, limit):
                owner.assertEqual(limit, 24 * RUNNER.GIB)
            def __enter__(self):
                return self
            def __exit__(self, *args):
                pass
            def account(self):
                return {"active_processes": 0}
            def start(self, argv, cwd, env, logs, label):
                logs.mkdir(parents=True)
                out, err = logs / "stdout.txt", logs / "stderr.txt"
                out.write_bytes(b'{"reason":"build-finished","success":true}\n')
                err.write_bytes(b"")
                return {"argv": argv, "cwd": str(cwd), "stdout_path": str(out), "stderr_path": str(err)}
        def wait(job, process, approved, *, monitor, review_cap):
            state["write"] += add_write
            state["captured"] = True
            sample = monitor()
            return {"schema_version": 1, "kind": "build-feedback-process", "status": status,
                    "reasons": [] if status == "ok" else ["owned_output_cap_reached"], "argv": process["argv"], "cwd": process["cwd"],
                    "stdout_path": process["stdout_path"], "stderr_path": process["stderr_path"],
                    "stdout_sha256": RUNNER.sha256_file(Path(process["stdout_path"])), "stderr_sha256": RUNNER.sha256_file(Path(process["stderr_path"])),
                    "returncode": 0 if status == "ok" else 57346, "duration_ms": 1, "peak_commit_bytes": 100,
                    "memory_limit_bytes": 24 * RUNNER.GIB, "process_tree_capture_complete": True, "external_processes": [],
                    "started_at": RUNNER._stamp(), "finished_at": RUNNER._stamp(), "resource_samples": [{**sample, "budget_allowed": sample["budget"]["allowed"], "budget_reasons": sample["budget"]["reasons"]}]}
        real = RUNNER._inventory_module()
        module = SimpleNamespace(budget_status=real.budget_status,
                                 inventory=lambda *args, **kwargs: {"schema_version": 1, "complete": True, "errors": [], "allocated_bytes": 5000})
        sample = {"schema_version": 1, "complete": True, "errors": [], "allocated_bytes": 7 * RUNNER.GIB + 5000,
                  "disk_free_bytes": 54 * RUNNER.GIB, "budget": {"allowed": True, "reasons": []}}
        helper = SimpleNamespace(validate_policy=lambda protocol: ({}, {}))
        source_checks = {"count": 0}
        def source(root, approved, **kwargs):
            source_checks["count"] += 1
            if foreign_source and state["captured"]:
                raise RUNNER.ProtocolError("foreign_source_change")
        return [patch.object(RUNNER, "WindowsJob", FakeJob), patch.object(RUNNER, "_wait_capture", side_effect=wait),
                patch.object(RUNNER._LiveBudget, "_write_bound", side_effect=lambda monitor: state["write"], autospec=True),
                patch.object(RUNNER, "_inventory_module", return_value=module), patch.object(RUNNER, "_budget_sample", return_value=sample),
                patch.object(RUNNER, "_free_bytes", return_value=54 * RUNNER.GIB), patch.object(RUNNER, "_verify_source", side_effect=source),
                patch.object(RUNNER, "_retention_module", return_value=helper), patch.object(RUNNER, "_memory_status", return_value={"physical_available": 4 * RUNNER.GIB, "commit_available": 30 * RUNNER.GIB}),
                patch.object(RUNNER, "build_process_snapshot", return_value=[]), patch.dict(os.environ, {"CARGO_HOME": str(self.root)})]

    def run_with_context(self, protocol, arm, **options):
        stubs = self.capture_context(protocol, arm, **options)
        for stub in stubs:
            stub.start()
        try:
            return RUNNER.capture_qualification(protocol, arm, "clippy")
        finally:
            for stub in reversed(stubs):
                stub.stop()

    def test_real_producer_keeps_original_origin_and_counts_preceding_outputs(self):
        protocol, arm = self.qualified_arm()
        result = self.run_with_context(protocol, arm)
        self.assertEqual(result["status"], "ok", result["reasons"])
        self.assertEqual(result["resource_guard"]["origin_allocated_bytes"], 7 * RUNNER.GIB)
        self.assertEqual(result["resource_guard"]["origin_disk_free_bytes"], 56 * RUNNER.GIB)
        self.assertEqual(result["resource_guard"]["prior_write_bound_bytes"], 5000)
        self.assertEqual(result["final_job_write_bound_bytes"], 5100)
        self.assertEqual(result["minimum_disk_free_bytes"], 54 * RUNNER.GIB)
        self.assertEqual(result["resource_samples"][0]["job_write_bound_bytes"], 5100)
        self.assertEqual(result["qualification_budget"]["final_job_write_bound_bytes"], 5100)
        self.assertEqual(RUNNER._read_json(Path(arm["target_root"]).parent / "qualification-captures/clippy/capture.json"), result)
        self.assertEqual(result["argv"][1:4], ["clippy", "--workspace", "--all-targets"])

    def test_guarded_qualifier_stop_does_not_report_complete_quality(self):
        protocol, arm = self.qualified_arm()
        result = self.run_with_context(protocol, arm, status="capacity_limited", add_write=8 * RUNNER.GIB)
        self.assertEqual(result["status"], "capacity_limited")
        self.assertNotIn("cargo", result)
        self.assertFalse(result["resource_samples"][0]["budget_allowed"])
        self.assertGreaterEqual(result["final_job_write_bound_bytes"], 8 * RUNNER.GIB)

    def test_post_capture_foreign_source_failure_keeps_raw_logs_and_receipt(self):
        protocol, arm = self.qualified_arm()
        result = self.run_with_context(protocol, arm, foreign_source=True)
        self.assertEqual(result["status"], "capture_failed")
        self.assertIn("foreign_source_change", result["reasons"])
        self.assertTrue(Path(result["stdout_path"]).is_file())
        self.assertEqual(RUNNER._read_json(Path(arm["target_root"]).parent / "qualification-captures/clippy/capture.json"), result)

    def test_omitted_prior_capture_and_synthetic_sample_refuse_before_launch(self):
        protocol, arm = self.qualified_arm()
        prior = Path(arm["target_root"]).parent / "qualification-captures/clippy"
        prior.mkdir(parents=True)
        write_json(prior / "capture.json", {"qualification": "clippy", "status": "ok"})
        with patch.object(RUNNER, "_retention_module", return_value=SimpleNamespace(validate_policy=lambda protocol: ({}, {}))), \
             patch.object(RUNNER, "WindowsJob", side_effect=AssertionError("must refuse prior omission before launch")), \
             self.assertRaisesRegex(RUNNER.ProtocolError, "all_previous"):
            RUNNER.capture_qualification(protocol, arm, "optional_features")

    def test_second_qualifier_must_carry_first_final_write_bound(self):
        protocol, arm = self.qualified_arm()
        first = self.run_with_context(protocol, arm)
        stubs = self.capture_context(protocol, arm)
        for stub in stubs:
            stub.start()
        try:
            with patch.object(RUNNER, "qualification_command", return_value=["approved-diagnostic.exe"]):
                second = RUNNER.capture_qualification(protocol, arm, "diagnostics", prior_captures=[first])
        finally:
            for stub in reversed(stubs):
                stub.stop()
        self.assertEqual(second["status"], "ok")
        self.assertEqual(second["resource_guard"]["origin_allocated_bytes"], arm["allocated_before_bytes"])
        self.assertEqual(second["resource_guard"]["prior_write_bound_bytes"], first["final_job_write_bound_bytes"])
        self.assertEqual(second["final_job_write_bound_bytes"], first["final_job_write_bound_bytes"] + 100)
        self.assertEqual(second["minimum_disk_free_bytes"], first["minimum_disk_free_bytes"])

    def test_rehashed_prior_capture_cannot_reset_original_budget_origin(self):
        protocol, arm = self.qualified_arm()
        first = self.run_with_context(protocol, arm)
        first["resource_guard"]["origin_allocated_bytes"] += 1
        write_json(Path(arm["target_root"]).parent / "qualification-captures/clippy/capture.json", first)
        with patch.object(RUNNER, "_retention_module", return_value=SimpleNamespace(validate_policy=lambda approved: ({}, {}))), \
             patch.object(RUNNER, "WindowsJob", side_effect=AssertionError("changed origin cannot launch")), \
             self.assertRaisesRegex(RUNNER.ProtocolError, "prior_capture_not_verified"):
            RUNNER.capture_qualification(protocol, arm, "diagnostics", prior_captures=[first])

    def test_synthetic_arm_resource_samples_cannot_admit_real_qualification(self):
        protocol, arm = self.qualified_arm()
        arm["jobs"][0]["resource_samples"][0]["allocation_kind"] = "synthetic"
        write_json(Path(arm["target_root"]).parent / "arm.json", arm)
        stubs = self.capture_context(protocol, arm)
        for stub in stubs:
            stub.start()
        try:
            with patch.object(RUNNER, "WindowsJob", side_effect=AssertionError("synthetic evidence cannot launch qualification")), \
                 self.assertRaisesRegex(RUNNER.ProtocolError, "real_qualification_resource_samples_required"):
                RUNNER.capture_qualification(protocol, arm, "clippy")
        finally:
            for stub in reversed(stubs):
                stub.stop()

    def test_live_monitor_carries_prior_writes_and_historical_free_minimum(self):
        protocol = {"schema_version": 2, "phase": "capacity", "baseline": {"allocated_bytes": 0},
                    "predicted_growth_bytes": 8 * RUNNER.GIB,
                    "budget_policy": {"policy_version": 2, "minimum_free_bytes": 40 * RUNNER.GIB, "external_growth_allowance_bytes": 8 * RUNNER.GIB}}
        before = {"allocated_bytes": 0, "disk_free_bytes": 56 * RUNNER.GIB}
        with patch.object(RUNNER._LiveBudget, "_write_bound", side_effect=[0, RUNNER.GIB]), patch.object(RUNNER, "_free_bytes", return_value=55 * RUNNER.GIB):
            monitor = RUNNER._LiveBudget(protocol, before, None, prior_write_bound_bytes=7 * RUNNER.GIB, minimum_free_bytes=53 * RUNNER.GIB)
            sample = monitor()
        self.assertEqual(sample["job_write_bound_bytes"], 8 * RUNNER.GIB)
        self.assertEqual(sample["physical_growth_bytes"], 3 * RUNNER.GIB)
        self.assertEqual(sample["budget"]["reasons"], ["owned_output_cap_reached"])


class StreamingCargoTests(unittest.TestCase):
    def test_file_and_bytes_share_test_tail_and_rejection_semantics(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for content in (b'{"reason":"build-finished","success":true}\n{not json: legal test tail}\n',
                            b'{not json before build}\n{"reason":"build-finished","success":true}\n',
                            b'{"reason":"build-finished","success":true}\n{"reason":"build-finished","success":true}\n',
                            b'{"reason":"build-finished","success":false}\n'):
                output = root / "cargo.stdout"
                output.write_bytes(content)
                expected = RUNNER.parse_cargo_json(content, root, root)
                with patch.object(Path, "read_bytes", side_effect=AssertionError("file parser must stream")):
                    actual = RUNNER.parse_cargo_json_file(output, root, root)
                self.assertEqual(actual, expected)
            self.assertFalse(RUNNER.parse_cargo_json(b'{"reason":"build-finished","success":true}\n{legal tail}\n', root, root)["errors"])
            output.write_bytes(b'{"reason":"build-finished","success":true}\n\xff')
            with self.assertRaises(UnicodeError):
                RUNNER.parse_cargo_json_file(output, root, root)

    def test_cap_stop_uses_shared_decoder_and_retains_recorded_quality_failures(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            output = root / "cargo.stdout"
            unfinished = {"reason": "compiler-artifact", "filenames": [str(root / "unfinished.rlib")],
                          "fresh": False, "profile": {"test": False}, "executable": None}
            output.write_text(json.dumps(unfinished) + "\n", encoding="utf-8")
            self.assertEqual(RUNNER.capacity_cargo_failure(output, root, root), [])
            with self.assertRaises(FileNotFoundError):
                RUNNER.parse_cargo_json_file(output, root, root)
            diagnostic = {"reason": "compiler-message", "message": {"level": "error", "message": "compile failed", "spans": []}}
            output.write_text(json.dumps(unfinished) + "\n" + json.dumps(diagnostic) + "\n" + '{"reason":"build-finished","success":false}\n', encoding="utf-8")
            failures = RUNNER.capacity_cargo_failure(output, root, root)
            self.assertIn("cargo_build_failed_before_capacity_stop", failures)
            self.assertIn("compiler_error_before_capacity_stop", failures)
            output.write_bytes(b'{malformed\n')
            self.assertIn("malformed_cargo_json", RUNNER.capacity_cargo_failure(output, root, root))

    def test_stream_memory_marker_crosses_chunk_boundary(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "cargo.stderr"
            output.write_bytes(b"x" * (64 * 1024 - 4) + b"Out Of Memory" + b"x" * 100)
            with patch.object(Path, "read_bytes", side_effect=AssertionError("must stream")):
                self.assertTrue(RUNNER._file_contains_markers(output, RUNNER.MEMORY_ERROR_MARKERS))
            output.write_bytes(b"x" * 160000)
            self.assertFalse(RUNNER._file_contains_markers(output, RUNNER.MEMORY_ERROR_MARKERS))

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



class BoundedLiveBudgetTests(unittest.TestCase):
    def setUp(self):
        self.protocol = {"schema_version": 2, "phase": "capacity", "baseline": {"allocated_bytes": 7 * RUNNER.GIB},
                         "predicted_growth_bytes": 8 * RUNNER.GIB,
                         "budget_policy": {"policy_version": 2, "minimum_free_bytes": 40 * RUNNER.GIB,
                                           "external_growth_allowance_bytes": 8 * RUNNER.GIB}}
        self.before = {"allocated_bytes": 7 * RUNNER.GIB, "disk_free_bytes": 56 * RUNNER.GIB}

    def test_own_write_cap_retains_external_allowance_and_no_rebase(self):
        with patch.object(RUNNER, "_free_bytes", return_value=56 * RUNNER.GIB), patch.object(RUNNER._LiveBudget, "_write_bound", side_effect=[0, 8 * RUNNER.GIB]):
            monitor = RUNNER._LiveBudget(self.protocol, self.before, None)
            sample = monitor()
        self.assertFalse(sample["budget"]["allowed"])
        self.assertEqual(sample["budget"]["reasons"], ["owned_output_cap_reached"])
        self.assertEqual(sample["job_write_bound_bytes"], 8 * RUNNER.GIB)

    def test_external_growth_is_never_mislabeled_owned_capacity_stop(self):
        with patch.object(RUNNER, "_free_bytes", return_value=48 * RUNNER.GIB), patch.object(RUNNER._LiveBudget, "_write_bound", return_value=0):
            monitor = RUNNER._LiveBudget(self.protocol, self.before, None)
            sample = monitor()
        self.assertTrue(sample["budget"]["allowed"])
        self.assertEqual(sample["physical_growth_bytes"], 8 * RUNNER.GIB)
        self.assertEqual(sample["job_write_bound_bytes"], 0)
        self.assertNotIn("owned_output_cap_reached", sample["budget"]["reasons"])
        with patch.object(RUNNER, "_free_bytes", return_value=48 * RUNNER.GIB - 1), patch.object(RUNNER._LiveBudget, "_write_bound", return_value=0):
            sample = RUNNER._LiveBudget(self.protocol, self.before, None)()
        self.assertFalse(sample["budget"]["allowed"])
        self.assertIn("projected_disk_free_below_minimum", sample["budget"]["reasons"])
        self.assertNotIn("owned_output_cap_reached", sample["budget"]["reasons"])

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

    def test_capacity_guard_stops_owned_tree_without_claiming_performance(self):
        monitor = lambda: {"allocated_bytes": 8 * RUNNER.GIB, "disk_free_bytes": 48 * RUNNER.GIB,
                           "budget": {"allowed": False, "reasons": ["owned_output_cap_reached"]}}
        with RUNNER.WindowsJob(128 * 1024**2) as job:
            process = job.start([sys.executable, "-c", "import subprocess,sys,time;subprocess.Popen([sys.executable,'-c','import time;time.sleep(30)']);time.sleep(30)"], self.root, dict(os.environ), self.root, "capacity-stop")
            result = RUNNER._wait_capture(job, process, {"phase": "capacity", "timeout_seconds": 5, "poll_seconds": 0.05}, monitor=monitor)
            self.assertEqual(job.account()["active_processes"], 0)
        self.assertEqual(result["status"], "capacity_limited")
        self.assertTrue(result["process_tree_capture_complete"])
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
            with patch.object(RUNNER, "job_command", side_effect=command), patch.object(RUNNER, "build_process_snapshot", return_value=[]), patch.object(RUNNER, "parse_cargo_json_file", return_value={"build_success": True, "errors": [], "artifacts": []}):
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
            with patch.object(RUNNER, "job_command", return_value=[sys.executable, "-c", "print('done')"]), patch.object(RUNNER, "build_process_snapshot", return_value=[]), patch.object(RUNNER, "parse_cargo_json_file", side_effect=parse), RUNNER.WindowsJob(128 * 1024**2) as job:
                rows, _ = RUNNER._run_parallel_phase(job, {"timeout_seconds": 10, "poll_seconds": 0.05}, {"layout": "isolated", "tool": {"kind": "native"}}, root, root / "targets", root, "primary", dict(os.environ), monitor)
            self.assertEqual([row["job"] for row in rows], [fixed["name"] for fixed in RUNNER.JOBS])
            self.assertEqual(rows[0]["status"], "capture_failed")
            self.assertEqual(rows[0]["reasons"], ["artifact_outside_invocation_target"])
            self.assertTrue(all((root / ("primary-" + fixed["name"] + ".json")).is_file() for fixed in RUNNER.JOBS))


@unittest.skipUnless(os.name == "nt", "Windows owned parallel cap-stop process test")
class FullCapacityParallelStopTests(unittest.TestCase):
    def test_real_concurrent_job_cap_stop_preserves_all_launched_captures(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            protocol = {"schema_version": 2, "phase": "capacity", "capacity_probe": {"kind": "full-arm-and-qualification"},
                        "timeout_seconds": 10, "poll_seconds": 0.05}
            sample = {"allocated_bytes": 8 * RUNNER.GIB, "disk_free_bytes": 54 * RUNNER.GIB,
                      "allocation_kind": "conservative-disk-and-job-write-bound", "job_write_bound_bytes": 8 * RUNNER.GIB,
                      "budget": {"allowed": False, "reasons": ["owned_output_cap_reached"]}}
            with patch.object(RUNNER, "job_command", return_value=[sys.executable, "-c", "import time;time.sleep(10)"]), \
                 patch.object(RUNNER, "build_process_snapshot", return_value=[]), RUNNER.WindowsJob(128 * 1024 ** 2) as job:
                captures, _ = RUNNER._run_parallel_phase(job, protocol, {"tool": {"kind": "native"}, "layout": "isolated", "incremental": "on"},
                                 root, root / "targets", root, "primary", dict(os.environ), lambda: sample)
                self.assertEqual(job.account()["active_processes"], 0)
            self.assertEqual(len(captures), 6)
            self.assertTrue(all(capture["status"] == "capacity_limited" for capture in captures))
            self.assertTrue(all(capture["reasons"] == ["owned_output_cap_reached"] for capture in captures))
            self.assertEqual(len(list(root.glob("primary-*.json"))), 6)
            self.assertEqual([capture["job"] for capture in captures], [fixed["name"] for fixed in RUNNER.JOBS])


class BoundedEvidenceIntegrationTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)

    def pinned(self, name, value):
        path = self.root / name
        write_json(path, value)
        return {"path": str(path), "sha256": RUNNER.sha256_file(path)}

    def test_inline_status_cannot_supply_capacity_reservation(self):
        reference = self.pinned("observation.json", {"kind": "build-feedback-arm", "status": "ok",
                    "allocation_complete": True, "source_verified": True,
                    "process_tree_drained": True, "owned_allocated_bytes": RUNNER.GIB})
        with self.assertRaisesRegex(RUNNER.ProtocolError, "reservation_observation_protocol"):
            RUNNER._reservation_observation_peak(reference, {}, "full-arm-and-qualification")

    def test_signed_capacity_origin_rechecks_raw_allocation_and_peak(self):
        source = {"receipt_sha256": "a" * 64}
        baseline = {"receipt_sha256": "b" * 64}
        origin = {"schema_version": 2, "phase": "capacity", "source": source,
                  "baseline": baseline, "toolchain": {}}
        allocation = self.pinned("owned-allocation.json", {"complete": True, "errors": [], "allocated_bytes": 4096})
        raw = {"status": "ok", "capture_complete": True, "allocation_complete": True,
               "protocol_sha256": RUNNER.canonical_sha256(origin), "source_receipt_sha256": "a" * 64,
               "toolchain_sha256": RUNNER.canonical_sha256({}), "task_output_allocation_receipt": allocation,
               "task_output_allocated_bytes": 4096, "peak_owned_output_bytes": 8192}
        raw_reference = self.pinned("capacity.json", raw)
        seal = {"kind": "build-feedback-capacity-seal", "probe_receipt": raw, "probe_reference": raw_reference}
        observation = {"protocol": self.pinned("protocol.json", origin), "seal": {"path": "signed-local-seal", "sha256": "d" * 64},
                       "observed_peak_bytes": 8192}
        reference = self.pinned("observation.json", observation)
        helper = SimpleNamespace(_verify_seal=lambda *args: seal)
        # Signature/log integrity has dedicated helper tests; mock only the seal
        # verifier here. Protocol/raw/allocation/hash/value bindings stay real.
        with patch.object(RUNNER, "_validate_protocol"), patch.object(RUNNER, "_retention_module", return_value=helper):
            self.assertEqual(RUNNER._reservation_observation_peak(reference, origin, "expanded-capacity"), 8192)
            with self.assertRaisesRegex(RUNNER.ProtocolError, "full_arm_seal"):
                RUNNER._reservation_observation_peak(reference, origin, "full-arm-and-qualification")
            reference = self.pinned("observation.json", {**observation, "observed_peak_bytes": 4096})
            with self.assertRaisesRegex(RUNNER.ProtocolError, "observation_closure"):
                RUNNER._reservation_observation_peak(reference, origin, "expanded-capacity")
            Path(allocation["path"]).write_bytes(b"tampered allocation")
            with self.assertRaisesRegex(RUNNER.ProtocolError, "allocation_changed"):
                RUNNER._reservation_observation_peak(reference, origin, "expanded-capacity")

    def test_formal_pilot_summary_without_both_retained_arms_refuses(self):
        protocol = {"schema_version": 2, "phase": "pilot", "pairs": 5}
        state = {"phase": "pilot", "complete": True, "status": "complete", "completed_valid_pairs": 5,
                 "pairs": [{"pair_index": index, "attempt": 0, "status": "ok", "arms": {"baseline": {}, "candidate": {}}}
                           for index in range(5)]}
        with patch.object(RUNNER, "_validate_resume"), self.assertRaisesRegex(RUNNER.ProtocolError, "retained_proofs"):
            RUNNER._verify_completed_v2_pilot(state, protocol)
        for pair in state["pairs"]:
            pair["retained_evidence"] = {"baseline": {"verified": True}, "candidate": {"verified": True}}
        helper = SimpleNamespace(verify_retained_arm=lambda *args: (_ for _ in ()).throw(ValueError("pinned seal missing")))
        with patch.object(RUNNER, "_validate_resume"), patch.object(RUNNER, "_retention_module", return_value=helper), \
             self.assertRaisesRegex(ValueError, "pinned seal missing"):
            RUNNER._verify_completed_v2_pilot(state, protocol)

    def test_registered_compute_and_storage_limits_include_previous_segments(self):
        registration = self.pinned("registration.json", {"maximum_compute_seconds": 10, "maximum_storage_bytes": 8192})
        protocol = {"formal_registration": registration, "baseline": {"allocated_bytes": 4096}}
        with patch.object(RUNNER.time, "monotonic", return_value=100):
            guard = RUNNER._FormalLimits(protocol, {"registered_compute_seconds": 7})
        with patch.object(RUNNER.time, "monotonic", return_value=102):
            self.assertEqual(guard.reasons(4096 + 8191), [])
        with patch.object(RUNNER.time, "monotonic", return_value=103):
            self.assertEqual(guard.reasons(4096 + 8192), ["registered_compute_limit_reached", "registered_storage_limit_reached"])
            state = {}
            guard.checkpoint(state)
            self.assertEqual(state["registered_compute_seconds"], 10)
            self.assertEqual(state["formal_registration_sha256"], registration["sha256"])

    def test_registered_live_limit_drains_only_owned_process_tree(self):
        job = SimpleNamespace(terminated=False)
        job.terminate = lambda: setattr(job, "terminated", True)
        process = {"argv": ["synthetic"], "cwd": str(self.root), "pid": 1,
                   "started_monotonic": 1, "started_at": "synthetic-start"}
        stdout, stderr = self.root / "out.log", self.root / "err.log"
        stdout.write_bytes(b"")
        stderr.write_bytes(b"")
        process.update(stdout_path=str(stdout), stderr_path=str(stderr))
        job.account_value = {"active_processes": 1, "peak_commit_bytes": 0, "memory_limit_bytes": RUNNER.MEMORY_LIMIT_BYTES,
                             "memory_limited": False}
        # A meaningful native drain is covered in NativeWindowsTests; this
        # isolates the new registration budget reason/status boundary.
        job.account = lambda: job.account_value
        job.members = lambda: {1}
        job.poll = lambda process: 0 if job.terminated else None
        job.process_wall_ms = lambda process: 10
        sample = {"allocated_bytes": 0, "disk_free_bytes": 64 * RUNNER.GIB,
                  "budget": {"allowed": False, "reasons": ["registered_compute_limit_reached"]}}
        with patch.object(RUNNER, "_external_build_processes", return_value=[]):
            capture = RUNNER._wait_capture(job, process, {"timeout_seconds": 30, "poll_seconds": 0.1}, monitor=lambda: sample)
        self.assertTrue(job.terminated)
        self.assertEqual(capture["status"], "budget_limited")
        self.assertEqual(capture["reasons"], ["registered_compute_limit_reached"])

    def test_pending_payload_is_counted_and_unknown_move_never_claims_release(self):
        run_root = self.root / "run"
        target = run_root / "capacity" / "targets"
        disposition = target.parent / "disposition"
        disposition.mkdir(parents=True)
        payload = self.root / "pending-recycle-payload"
        payload.mkdir()
        identity = [1, 2]
        tree = {"root_identity": identity, "allocated_bytes": 4096, "entries": []}
        seal = {"target": str(target), "tree": tree}
        self.pinned("unused.json", {})
        write_json(disposition / "moved.json", {"seal": {"path": "seal"}, "item": {"identity": identity, "path": str(payload)}})
        helper = SimpleNamespace(_verify_signed=lambda *args: None, _verify_seal=lambda *args: seal,
                                 _path=lambda path: Path(path).resolve(strict=True), tree_manifest=lambda path: tree)
        protocol = {"schema_version": 2, "phase": "capacity", "run_root": str(run_root), "qualification_allocations": [], "store_roots": [],
                    "inventory_roots": [str(self.root)], "baseline": {"allocated_bytes": 0},
                    "budget_policy": {"policy_version": 2, "minimum_free_bytes": 40 * RUNNER.GIB,
                                      "external_growth_allowance_bytes": 8 * RUNNER.GIB}}
        real_inventory = RUNNER._inventory_module()
        calls = []
        def inventory(roots, **kwargs):
            calls.append(kwargs["store_roots"])
            return {"complete": True, "errors": [], "allocated_bytes": 4096}
        module = SimpleNamespace(inventory=inventory, budget_status=real_inventory.budget_status)
        with patch.object(RUNNER, "_retention_module", return_value=helper), patch.object(RUNNER, "_inventory_module", return_value=module), \
             patch.object(RUNNER, "_free_bytes", return_value=64 * RUNNER.GIB):
            report = RUNNER._budget_sample(protocol)
            self.assertEqual(calls, [[str(run_root), str(payload)]])
            self.assertEqual(report["allocated_bytes"], 4096)
            (disposition / "move-intent.json").write_text("{}", encoding="utf-8")
            # Unpublished moved receipt is simulated by a missing path binding;
            # the production branch cannot silently subtract this directory.
            with patch.object(Path, "exists", autospec=True, side_effect=lambda path: False if path == disposition / "moved.json" else path.is_dir() or path == disposition / "move-intent.json"), \
                 self.assertRaisesRegex(RUNNER.ProtocolError, "requires_reconciliation"):
                RUNNER._budget_sample(protocol)



if __name__ == "__main__":
    unittest.main()
