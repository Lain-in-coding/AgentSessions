import hashlib
import importlib.util
import json
import os
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

if os.name != "nt":
    raise unittest.SkipTest("Windows build-feedback evidence suite")

MODULE_PATH = Path(__file__).with_name("build_feedback_coordinator.py")
SPEC = importlib.util.spec_from_file_location("build_feedback_coordinator_tests", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
COORDINATOR = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(COORDINATOR)


def write_json(path: Path, value: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value), encoding="utf-8")


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


class FixtureAuthorityTests(unittest.TestCase):
    def test_synthetic_authority_is_separate_exact_and_validated(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source_root = root / "source"
            source_root.mkdir()
            source_receipt = root / "source.json"
            baseline = root / "baseline.json"
            write_json(source_receipt, {"kind": "source"})
            write_json(baseline, {"kind": "baseline"})
            source_protocol = {
                "schema_version": 2,
                "phase": "capacity",
                "run_root": str(root / "old-run"),
                "source": {"root": str(source_root), "receipt_sha256": digest(source_receipt)},
                "baseline": {"receipt_sha256": digest(baseline)},
                "toolchain": {"cargo": {"sha256": "a" * 64}},
                "configs": {
                    "baseline": {"debug": "full", "incremental": "on",
                                 "layout": "isolated", "tool": {"kind": "native"}},
                    "candidate": {"debug": "line-tables-only", "incremental": "on",
                                  "layout": "3lane", "tool": {"kind": "native"}},
                },
                "store_roots": [],
            }
            protocol_path = root / "source-protocol.json"
            write_json(protocol_path, source_protocol)
            prepared = COORDINATOR.prepare_fixture_protocol(
                protocol_path, root / "fixture-control")
            protocol = COORDINATOR._read(prepared["protocol"]["path"])
            policy = COORDINATOR._module("retention").validate_policy(protocol)
            approval = policy[1]
            self.assertEqual(approval["purpose"], "synthetic-recycle-lifecycle")
            self.assertEqual(approval["disposable_target_paths"], [prepared["target"]])
            self.assertNotEqual(protocol["run_root"], source_protocol["run_root"])
            self.assertFalse(Path(prepared["target"]).exists())


class PreparationTests(unittest.TestCase):
    def test_diagnostic_sentinel_binds_absolute_source_line_contract(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory)
            sentinel = source / "crates/agent-session-grep-cli/src/bin/build_feedback_diagnostic.rs"
            sentinel.parent.mkdir(parents=True)
            sentinel.write_text(
                'fn main() {\n'
                '    let source = concat!(env!("CARGO_MANIFEST_DIR"), "/src/bin/build_feedback_diagnostic.rs");\n'
                '    println!("{}:{}:", source.replace(\'\\\\\', "/"), line!());\n'
                '}\n',
                encoding="utf-8", newline="\n")
            configs = {
                "baseline": {"debug": "full", "incremental": "on",
                             "layout": "isolated", "tool": {"kind": "native"}},
                "candidate": {"debug": "line-tables-only", "incremental": "on",
                              "layout": "3lane", "tool": {"kind": "native"}},
            }
            checks = COORDINATOR._diagnostic_source_checks(configs, source)
            self.assertEqual(len(checks), 2)
            for value in checks.values():
                self.assertEqual(value, [{
                    "path": "crates/agent-session-grep-cli/src/bin/build_feedback_diagnostic.rs",
                    "line": 3,
                }])
            commands = COORDINATOR._diagnostic_commands(configs)
            self.assertEqual(commands[COORDINATOR._canonical(configs["baseline"])], {
                "artifact_relative_path": "primary/cli/debug/build_feedback_diagnostic.exe",
                "args": [],
            })
            self.assertEqual(commands[COORDINATOR._canonical(configs["candidate"])], {
                "artifact_relative_path": "primary/domain-cli/debug/build_feedback_diagnostic.exe",
                "args": [],
            })
            sentinel.write_text("fn main() {}\n", encoding="utf-8", newline="\n")
            with self.assertRaisesRegex(ValueError, "source line"):
                COORDINATOR._diagnostic_source_checks(configs, source)

    def test_full_capacity_candidate_config_is_independent_of_arm_name(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "seed-source"
            source.mkdir()
            seed = {
                "source": {
                    "root": str(source), "commit": "a" * 40, "tree": "b" * 40,
                    "receipt_path": str(root / "source.json"),
                    "receipt_sha256": "c" * 64,
                },
                "baseline": {"receipt_path": str(root / "baseline.json"),
                             "receipt_sha256": "d" * 64, "allocated_bytes": 1},
                "toolchain": {"cargo": {"sha256": "e" * 64}},
                "configs": {
                    "baseline": {"debug": "full", "incremental": "on",
                                 "layout": "isolated", "tool": {"kind": "native"}},
                    "candidate": {"debug": "full", "incremental": "on",
                                  "layout": "isolated", "tool": {"kind": "native"}},
                },
                "inventory_roots": [str(root)],
                "store_roots": [],
                "timeout_seconds": 7200,
                "poll_seconds": 0.2,
            }
            seed_path = root / "seed.json"
            write_json(seed_path, seed)
            captured = {}
            fake_runner = SimpleNamespace(
                GIB=1024 ** 3,
                _read_json=lambda path: json.loads(Path(path).read_text(encoding="utf-8")),
                canonical_sha256=lambda value: hashlib.sha256(
                    json.dumps(value, sort_keys=True, separators=(",", ":"),
                               ensure_ascii=False, allow_nan=False).encode("utf-8")
                ).hexdigest(),
                make_protocol=lambda fields: captured.update(fields) or fields,
            )
            approval = {"path": str(root / "approval.json"), "sha256": "f" * 64}
            with patch.object(COORDINATOR, "_module", return_value=fake_runner), \
                 patch.object(COORDINATOR, "_reference",
                              return_value={"path": str(root / "leaf.json"), "sha256": "1" * 64}), \
                 patch.object(COORDINATOR, "_retention_approval",
                              return_value={}), \
                 patch.object(COORDINATOR, "_new", return_value=approval), \
                 patch.object(COORDINATOR, "_diagnostic_source_checks",
                              return_value={"x": [{"path": "sentinel", "line": 3}]}):
                COORDINATOR.prepare_full_capacity(
                    seed_path, root / "leaf.json", "candidate", source,
                    root / "run", root / "control", 20 * 1024 ** 3,
                    8 * 1024 ** 3)
            self.assertEqual(captured["candidate_config"],
                             {"debug": "line-tables-only", "incremental": "on",
                              "layout": "3lane", "tool": {"kind": "native"}})
            self.assertEqual(captured["baseline_config"],
                             {"debug": "full", "incremental": "on",
                              "layout": "isolated", "tool": {"kind": "native"}})
            self.assertEqual(captured["predicted_growth_bytes"], 8 * 1024 ** 3)
            self.assertEqual(captured["capacity_probe"]["owned_output_cap_bytes"],
                             20 * 1024 ** 3)
            captured.clear()
            with patch.object(COORDINATOR, "_module", return_value=fake_runner), \
                 patch.object(COORDINATOR, "_reference",
                              return_value={"path": str(root / "leaf.json"), "sha256": "1" * 64}), \
                 patch.object(COORDINATOR, "_retention_approval", return_value={}), \
                 patch.object(COORDINATOR, "_new", return_value=approval), \
                 patch.object(COORDINATOR, "_diagnostic_source_checks",
                              return_value={"x": [{"path": "sentinel", "line": 3}]}):
                COORDINATOR.prepare_full_capacity(
                    seed_path, root / "leaf.json", "candidate", source,
                    root / "run-default", root / "control-default",
                    20 * 1024 ** 3)
            self.assertEqual(captured["predicted_growth_bytes"], 20 * 1024 ** 3)


class QualificationProofTests(unittest.TestCase):
    def test_clippy_capture_becomes_command_proof_and_report(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "source"
            arm_root = root / "arm"
            source.mkdir()
            arm_root.mkdir()
            stdout = root / "stdout.log"
            stderr = root / "stderr.log"
            artifact_path = arm_root / "artifact.exe"
            stdout.write_text("", encoding="utf-8")
            stderr.write_text("", encoding="utf-8")
            artifact_path.write_bytes(b"artifact")
            artifact = {"path": str(artifact_path), "sha256": digest(artifact_path), "bytes": 8}
            capture = {
                "kind": "build-feedback-process", "schema_version": 1, "status": "ok",
                "returncode": 0, "qualification": "clippy",
                "argv": ["cargo", "clippy", "--workspace", "--all-targets", "--locked",
                         "--offline", "-j2", "--message-format=json", "--target-dir", "target"],
                "cwd": str(source), "stdout_path": str(stdout),
                "stdout_sha256": digest(stdout), "stderr_path": str(stderr),
                "stderr_sha256": digest(stderr),
                "cargo": {"build_success": True, "errors": [], "artifacts": [{"files": [artifact]}]},
            }
            config = {"debug": "full", "incremental": "on",
                      "layout": "isolated", "tool": {"kind": "native"}}
            protocol = {"schema_version": 2, "configs": {"baseline": config}}
            arm = {"arm": "baseline", "pair_index": 0, "attempt": 0,
                   "source_root": str(source), "target_root": str(arm_root / "targets"),
                   "config": config}

            def canonical(value):
                return hashlib.sha256(json.dumps(
                    value, sort_keys=True, separators=(",", ":"),
                    ensure_ascii=False, allow_nan=False).encode()).hexdigest()

            fake_runner = SimpleNamespace(
                canonical_sha256=canonical,
                sha256_file=lambda path: digest(Path(path)),
                _verify_source=lambda *args, **kwargs: None,
            )
            fake_benchmark = SimpleNamespace(
                normalize_runner_arm=lambda arm_record, approved: {
                    "identity": {"source_commit": "a" * 40},
                    "config_sha256": canonical(config),
                }
            )
            fake_retention = SimpleNamespace(validate_policy=lambda protocol: ({}, {}))
            modules = {"runner": fake_runner, "benchmark": fake_benchmark,
                       "retention": fake_retention}

            def new(path, value):
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(json.dumps(value), encoding="utf-8")
                return {"path": str(path), "sha256": digest(path)}

            with patch.object(COORDINATOR, "_module", side_effect=lambda name: modules[name]), \
                 patch.object(COORDINATOR, "_canonical", side_effect=canonical), \
                 patch.object(COORDINATOR, "_new", side_effect=new):
                reference = COORDINATOR._qualification_proof(
                    protocol, arm, arm_root, "clippy", capture)
            proof = json.loads(Path(reference["path"]).read_text(encoding="utf-8"))
            report = json.loads(Path(proof["report"]["path"]).read_text(encoding="utf-8"))
            self.assertEqual(proof["kind"], "build-feedback-qualification-command")
            self.assertEqual(proof["qualification"], "clippy")
            self.assertEqual(proof["artifacts"], [artifact])
            self.assertEqual(report["status"], "passed")
            self.assertEqual(report["artifacts"], [artifact])
            self.assertEqual(report["capture_sha256"], canonical(capture))


class LifecycleTests(unittest.TestCase):
    def test_full_capacity_physical_reservation_is_independent_of_write_cap(self):
        protocol = {"predicted_growth_bytes": 8 * 1024 ** 3,
                    "capacity_probe": {"owned_output_cap_bytes": 32 * 1024 ** 3}}
        COORDINATOR._verify_full_capacity_physical_reservation(
            protocol, 8 * 1024 ** 3, "physical reservation exceeded")
        with self.assertRaisesRegex(ValueError, "physical reservation exceeded"):
            COORDINATOR._verify_full_capacity_physical_reservation(
                protocol, 9 * 1024 ** 3, "physical reservation exceeded")

    def test_qualifications_run_in_frozen_order_and_emit_proofs(self):
        with tempfile.TemporaryDirectory() as directory:
            arm_root = Path(directory)
            order = []
            names = ("clippy", "diagnostics", "optional_features", "release")

            def capture(protocol, arm, name, **kwargs):
                order.append((name, len(kwargs.get("prior_captures", ()))))
                return {"status": "ok", "qualification": name}

            fake_runner = SimpleNamespace(
                QUALIFICATION_NAMES=names,
                capture_qualification=capture,
            )
            with patch.object(COORDINATOR, "_module", return_value=fake_runner), \
                 patch.object(COORDINATOR, "_qualification_refs", return_value=[]), \
                 patch.object(COORDINATOR, "_reference",
                              side_effect=lambda path: {
                                  "path": str(path),
                                  "sha256": str(path).rjust(64, "0")[:64],
                              }), \
                 patch.object(COORDINATOR, "_qualification_proof",
                              side_effect=lambda protocol, arm, root, name, value: {
                                  "path": str(root / "qualification-proofs" / (name + ".json")),
                                  "sha256": name.rjust(64, "0")[:64],
                              }):
                proofs = COORDINATOR._run_qualifications(
                    {"schema_version": 2}, {"arm": "baseline"}, arm_root,
                    review_receipt_path=None, release_options={"x": "y"})
            self.assertEqual(order, [(name, index) for index, name in enumerate(names)])
            self.assertEqual(set(proofs), set(names))

    def test_finalizer_publishes_one_link_and_reuses_it_on_resume(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            arm_root = root / "arm"
            arm_root.mkdir()
            target = arm_root / "targets"
            target.mkdir()
            protocol = {
                "schema_version": 2,
                "phase": "capacity",
                "run_root": str(root / "run"),
                "configs": {"baseline": {"debug": "full", "incremental": "on",
                                         "layout": "isolated", "tool": {"kind": "native"}}},
                "predicted_growth_bytes": 8 * 1024 ** 3,
                "capacity_probe": {"kind": "full-arm-and-qualification",
                                   "stage": "expanded", "arm": "baseline",
                                   "source_root": str(root / "source")},
            }
            arm_record = {
                "arm": "baseline", "phase": "capacity",
                "performance_evidence": False, "completed_valid_pairs": 0,
                "source_root": str(root / "source"), "target_root": str(target),
                "config": protocol["configs"]["baseline"],
                "protocol_sha256": "a" * 64,
                "output_lease": {"path": str(root / "lease.json"), "sha256": "b" * 64},
                "owned_allocated_bytes": 1024 ** 3,
            }
            arm_receipt = {"path": str(arm_root / "arm.json"), "sha256": "c" * 64}
            seal = {"path": str(arm_root / "full-capacity-seal.json"), "sha256": "d" * 64}
            seal_payload = {**seal, "task_output_allocated_bytes": 1024 ** 3}
            disposition = {"path": str(arm_root / "disposition/completed.json"), "sha256": "e" * 64}
            fake_retention = SimpleNamespace(
                validate_policy=lambda protocol: ({}, {}),
                create_full_capacity_seal=lambda *args: seal,
                dispose_output=lambda *args: disposition,
                _verify_seal=lambda reference, approved: seal_payload,
                verify_full_capacity_link=lambda reference, approved: {
                    "arm_receipt": arm_record, "seal": seal_payload, "disposition": disposition,
                },
            )
            fake_runner = SimpleNamespace(_validate_protocol=lambda protocol: None,
                                          sha256_file=lambda path: digest(Path(path)))
            fake_benchmark = SimpleNamespace(
                normalize_runner_arm=lambda arm, approved: {
                    "capture_complete": True, "source_verified": True,
                    "artifacts_verified": True, "allocation_complete": True,
                    "quiet": True,
                }
            )
            modules = {"runner": fake_runner, "retention": fake_retention,
                       "benchmark": fake_benchmark}

            def new(path, value):
                path.write_text(json.dumps(value), encoding="utf-8")
                return {"path": str(path), "sha256": digest(path)}

            calls = []
            with patch.object(COORDINATOR, "_module", side_effect=lambda name: modules[name]), \
                 patch.object(COORDINATOR, "_verify_arm_receipt", return_value=arm_root / "arm.json"), \
                 patch.object(COORDINATOR, "_arm_identity", return_value=(target, arm_root)), \
                 patch.object(COORDINATOR, "_run_qualifications", return_value={"clippy": {}}), \
                 patch.object(COORDINATOR, "_new", side_effect=new), \
                 patch.object(fake_retention, "dispose_output",
                              side_effect=lambda *args: calls.append("dispose") or disposition):
                reference = COORDINATOR.finalize_arm(
                    protocol=protocol, phase="capacity", arm="baseline",
                    pair_index=None, attempt=None, arm_receipt=arm_receipt,
                    arm_record=arm_record)
                second = COORDINATOR.finalize_arm(
                    protocol=protocol, phase="capacity", arm="baseline",
                    pair_index=None, attempt=None, arm_receipt=arm_receipt,
                    arm_record=arm_record)
            self.assertEqual(reference, second)
            self.assertEqual(calls, ["dispose"])
            link = json.loads(Path(reference["path"]).read_text(encoding="utf-8"))
            self.assertEqual(link, {"seal": seal, "disposition": disposition})


class ReservationEvidenceTests(unittest.TestCase):
    def template(self) -> dict:
        return {
            "source": {"receipt_sha256": "a" * 64},
            "baseline": {"receipt_sha256": "b" * 64},
            "toolchain": {"cargo": {"sha256": "c" * 64}},
            "configs": {
                "baseline": {"debug": "full", "incremental": "on",
                             "layout": "isolated", "tool": {"kind": "native"}},
                "candidate": {"debug": "line-tables-only", "incremental": "on",
                              "layout": "3lane", "tool": {"kind": "native"}},
            },
        }

    @staticmethod
    def canonical(value):
        return hashlib.sha256(json.dumps(
            value, sort_keys=True, separators=(",", ":"), ensure_ascii=False,
            allow_nan=False).encode("utf-8")).hexdigest()

    @staticmethod
    def read_json(path):
        return json.loads(Path(path).read_text(encoding="utf-8"))

    @staticmethod
    def write_new(path, value):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(value), encoding="utf-8")

    @staticmethod
    def sha256_file(path):
        return digest(Path(path))

    def test_reservation_observation_binds_scope_config_and_peak(self):
        template = self.template()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            observation_path = root / "observation.json"
            config_hash = self.canonical(template["configs"]["candidate"])
            write_json(observation_path, {
                "kind": "build-feedback-reservation-observation",
                "workload_scope": "full-arm-and-qualification",
                "performance_evidence": False,
                "config_sha256": config_hash,
                "observed_peak_bytes": 123,
            })
            reference = {"path": str(observation_path), "sha256": digest(observation_path)}
            fake_runner = SimpleNamespace(
                canonical_sha256=self.canonical,
                _read_json=self.read_json,
                _reservation_observation_peak=lambda *args: 123,
            )
            with patch.object(COORDINATOR, "_module", return_value=fake_runner):
                self.assertEqual(COORDINATOR._verified_observation(reference, template),
                                 (123, config_hash))
            write_json(observation_path, {
                "kind": "build-feedback-reservation-observation",
                "workload_scope": "full-arm-and-qualification",
                "performance_evidence": False,
                "config_sha256": "f" * 64,
                "observed_peak_bytes": 123,
            })
            bad_reference = {"path": str(observation_path), "sha256": digest(observation_path)}
            with patch.object(COORDINATOR, "_module", return_value=fake_runner), \
                 self.assertRaisesRegex(ValueError, "configuration changed"):
                COORDINATOR._verified_observation(bad_reference, template)

    def test_reservation_uses_both_distinct_verified_peaks_and_rechecks_margin(self):
        value = self.template()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            template = root / "template.json"
            write_json(template, value)
            observations = [
                {"path": str(root / "baseline-observation.json"), "sha256": "d" * 64},
                {"path": str(root / "candidate-observation.json"), "sha256": "e" * 64},
            ]
            configs = [self.canonical(value["configs"]["baseline"]),
                       self.canonical(value["configs"]["candidate"])]
            fake_runner = SimpleNamespace(canonical_sha256=self.canonical,
                                          _read_json=self.read_json,
                                          _write_new=self.write_new,
                                          sha256_file=self.sha256_file)
            with patch.object(COORDINATOR, "_module", return_value=fake_runner), \
                 patch.object(COORDINATOR, "_verified_observation",
                              side_effect=[(100, configs[0]), (200, configs[1])]):
                reference = COORDINATOR.make_reservation_evidence(
                    template, observations[0], observations[1], 50, 250,
                    root / "reservation.json")
            evidence = COORDINATOR._read(reference["path"])
            self.assertEqual(evidence["observed_peak_bytes"], 200)
            self.assertEqual(set(evidence["configurations"]), set(configs))
            self.assertEqual(evidence["observations"], observations)

    def test_reservation_refuses_duplicate_configuration_or_insufficient_growth(self):
        value = self.template()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            template = root / "template.json"
            write_json(template, value)
            observations = [
                {"path": str(root / "one.json"), "sha256": "d" * 64},
                {"path": str(root / "two.json"), "sha256": "e" * 64},
            ]
            config = self.canonical(value["configs"]["baseline"])
            fake_runner = SimpleNamespace(canonical_sha256=self.canonical,
                                          _read_json=self.read_json,
                                          _write_new=self.write_new,
                                          sha256_file=self.sha256_file)
            with patch.object(COORDINATOR, "_module", return_value=fake_runner), \
                 patch.object(COORDINATOR, "_verified_observation",
                              side_effect=[(100, config), (200, config)]), \
                 self.assertRaisesRegex(ValueError, "exactly both"):
                COORDINATOR.make_reservation_evidence(
                    template, observations[0], observations[1], 50, 250,
                    root / "duplicate.json")
            with patch.object(COORDINATOR, "_module", return_value=fake_runner), \
                 patch.object(COORDINATOR, "_verified_observation",
                              side_effect=[(100, config),
                                           (200, self.canonical(value["configs"]["candidate"]))]), \
                 self.assertRaisesRegex(ValueError, "does not cover"):
                COORDINATOR.make_reservation_evidence(
                    template, observations[0], observations[1], 50, 249,
                    root / "insufficient.json")


if __name__ == "__main__":
    unittest.main()
