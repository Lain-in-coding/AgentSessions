import hashlib
import importlib.util
import json
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

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
