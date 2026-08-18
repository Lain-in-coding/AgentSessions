import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

MODULE_PATH = Path(__file__).with_name("performance_gate_benchmark.py")
SPEC = importlib.util.spec_from_file_location("performance_gate_benchmark", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
PERF = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PERF)

GATE_PATH = Path(__file__).with_name("open_source_gate_benchmark.py")
GATE_SPEC = importlib.util.spec_from_file_location("open_source_gate_benchmark", GATE_PATH)
assert GATE_SPEC is not None and GATE_SPEC.loader is not None
GATE = importlib.util.module_from_spec(GATE_SPEC)
GATE_SPEC.loader.exec_module(GATE)


def corpus_block(at_threshold_scale: bool = True, messages: int | None = None) -> dict:
    count = messages if messages is not None else PERF.DEFAULT_MESSAGES
    return {
        "id": f"synthetic-corpus-{count}",
        "contains_real_transcripts": False,
        "message_count": count,
        "session_count": 5000,
        "source_bytes": 50_920_199,
        "at_threshold_scale": at_threshold_scale,
    }


def reference_block(at_threshold_scale: bool = True, messages: int | None = None) -> dict:
    block = corpus_block(at_threshold_scale, messages)
    return {
        "id": block["id"],
        "message_count": block["message_count"],
        "session_count": block["session_count"],
        "source_bytes": block["source_bytes"],
        "at_threshold_scale": block["at_threshold_scale"],
    }


def manifest_with(metrics: list[dict], *, at_threshold_scale: bool = True, profile: str = "full") -> dict:
    thresholded = [m for m in metrics if m["name"] in PERF.PERFORMANCE_THRESHOLDS]
    failures = sorted(m["name"] for m in thresholded if m["pass"] is False)
    deferred = sorted(m["name"] for m in thresholded if m["state"] == PERF.STATE_BELOW_SCALE)
    return {
        "schema_version": PERF.PERFORMANCE_SCHEMA_VERSION,
        "profile": profile,
        "commit": "a" * 40,
        "corpus": corpus_block(at_threshold_scale, None if at_threshold_scale else 6_000),
        "metrics": metrics,
        "runtime": {"total_wall_clock_s": 123.4},
        "ci_viability": {"verdict": "recorded"},
        "gate": {
            "pass": None if deferred else not failures,
            "failures": failures,
            "deferred": deferred,
        },
    }


def passing_metrics(at_threshold_scale: bool = True) -> list[dict]:
    """Every schema metric, all thresholded ones passing."""
    state = PERF.STATE_MEASURED if at_threshold_scale else PERF.STATE_BELOW_SCALE
    reason = None if at_threshold_scale else "below the scale the thresholds are stated at"
    messages = None if at_threshold_scale else 6_000
    reference = reference_block(at_threshold_scale, messages)
    mcp_detail = {
        "encoder_resident": False,
        "binary_vector_backend": PERF.FALLBACK_BACKEND,
        "binary_vector_model_id": "bigram-hash-v1",
    }
    values = {
        "initial_index_throughput_mib_s": 9.0,
        "noop_sync_latency_ms": 500.0,
        "search_latency_p95_ms": 20.0,
        "mcp_single_call_latency_p95_ms": 20.0,
    }
    metrics = [
        PERF.performance_metric(
            name,
            "unit",
            value,
            action=f"action for {name}",
            corpus=reference,
            sample_count=10,
            sample_unit="sample",
            state=state,
            reason=reason,
            detail=dict(mcp_detail) if name.startswith("mcp_") else None,
        )
        for name, value in values.items()
    ]
    metrics.extend(
        PERF.performance_metric(
            name,
            "unit",
            1.0,
            action=f"action for {name}",
            corpus=reference,
            sample_count=10,
            sample_unit="sample",
            detail=dict(mcp_detail) if name.startswith("mcp_") else None,
        )
        for name in PERF.PERFORMANCE_INFORMATIONAL_METRICS
    )
    return metrics


class ThresholdSchemaTests(unittest.TestCase):
    def test_every_threshold_has_a_direction_and_an_origin(self) -> None:
        self.assertEqual(set(PERF.PERFORMANCE_THRESHOLDS), set(PERF.THRESHOLD_DIRECTION))
        self.assertEqual(set(PERF.PERFORMANCE_THRESHOLDS), set(PERF.THRESHOLD_ORIGIN))
        for name, origin in PERF.THRESHOLD_ORIGIN.items():
            self.assertTrue(origin.strip(), f"{name} has an empty origin")

    def test_thresholds_match_the_owner_stated_numbers(self) -> None:
        # Pinned so a future edit that loosens a threshold to make a run pass
        # has to change this test too, in the open.
        self.assertEqual(
            PERF.PERFORMANCE_THRESHOLDS,
            {
                "initial_index_throughput_mib_s": 3.5,
                "noop_sync_latency_ms": 1000.0,
                "search_latency_p95_ms": 50.0,
                "mcp_single_call_latency_p95_ms": 50.0,
            },
        )

    def test_thresholded_and_informational_sets_are_disjoint(self) -> None:
        overlap = set(PERF.PERFORMANCE_THRESHOLDS) & set(PERF.PERFORMANCE_INFORMATIONAL_METRICS)
        self.assertEqual(overlap, set())

    def test_every_informational_metric_carries_a_reason(self) -> None:
        for name, reason in PERF.PERFORMANCE_INFORMATIONAL_METRICS.items():
            self.assertTrue(reason.strip(), f"{name} has an empty reason")

    def test_correctness_thresholds_are_untouched(self) -> None:
        # This gate must not weaken the correctness gate or move any of its four
        # metrics into an informational set.
        self.assertEqual(
            GATE.GATE_THRESHOLDS,
            {
                "lexical_recall_at_10": 0.95,
                "parse_loss_ratio": 0.05,
                "discovery_coverage": 0.95,
                "resume_handoff_success": 1.0,
            },
        )
        self.assertEqual(
            GATE.INFORMATIONAL_METRICS, {"semantic_recall_at_10", "hybrid_recall_at_10"}
        )


class EvaluateTests(unittest.TestCase):
    def test_min_direction_passes_at_and_above_threshold(self) -> None:
        self.assertTrue(PERF.evaluate("initial_index_throughput_mib_s", 3.5))
        self.assertTrue(PERF.evaluate("initial_index_throughput_mib_s", 12.0))
        self.assertFalse(PERF.evaluate("initial_index_throughput_mib_s", 3.49))

    def test_max_direction_passes_at_and_below_threshold(self) -> None:
        self.assertTrue(PERF.evaluate("noop_sync_latency_ms", 1000.0))
        self.assertTrue(PERF.evaluate("search_latency_p95_ms", 12.0))
        self.assertFalse(PERF.evaluate("noop_sync_latency_ms", 1000.1))
        self.assertFalse(PERF.evaluate("mcp_single_call_latency_p95_ms", 51.0))

    def test_unknown_metric_raises(self) -> None:
        with self.assertRaises(KeyError):
            PERF.evaluate("not_a_metric", 1.0)


class ThroughputTests(unittest.TestCase):
    def test_one_mib_in_one_second(self) -> None:
        self.assertAlmostEqual(PERF.throughput_mib_s(1024 * 1024, 1000.0), 1.0, places=6)

    def test_the_stated_ceiling_is_reproduced(self) -> None:
        # 1 GiB in 5 minutes is exactly the 3.5 MiB/s ceiling, to a rounding.
        value = PERF.throughput_mib_s(1024**3, 5 * 60 * 1000.0)
        self.assertAlmostEqual(value, 3.413333, places=5)

    def test_zero_elapsed_is_rejected(self) -> None:
        with self.assertRaises(ValueError):
            PERF.throughput_mib_s(1024, 0.0)


class MetricEntryTests(unittest.TestCase):
    def test_thresholded_metric_carries_verdict_corpus_and_n(self) -> None:
        entry = PERF.performance_metric(
            "search_latency_p95_ms",
            "ms",
            42.0,
            action="one-shot CLI lexical search",
            corpus=reference_block(),
            sample_count=100,
            sample_unit="search invocation",
        )
        self.assertEqual(entry["threshold"], 50.0)
        self.assertTrue(entry["pass"])
        self.assertEqual(entry["threshold_direction"], "max")
        self.assertEqual(entry["sample_count"], 100)
        self.assertEqual(entry["corpus"]["message_count"], PERF.DEFAULT_MESSAGES)
        self.assertTrue(entry["action"])

    def test_failing_value_records_a_failure_not_an_adjusted_threshold(self) -> None:
        entry = PERF.performance_metric(
            "noop_sync_latency_ms",
            "ms",
            8_000.0,
            action="no-op re-sync",
            corpus=reference_block(),
            sample_count=3,
            sample_unit="full-corpus no-op pass",
        )
        self.assertEqual(entry["threshold"], 1000.0)
        self.assertIs(entry["pass"], False)
        self.assertEqual(entry["value"], 8_000.0)

    def test_below_scale_metric_has_no_verdict(self) -> None:
        entry = PERF.performance_metric(
            "search_latency_p95_ms",
            "ms",
            5.0,
            action="one-shot CLI lexical search",
            corpus=reference_block(False, 6_000),
            sample_count=10,
            sample_unit="search invocation",
            state=PERF.STATE_BELOW_SCALE,
            reason="6,000 messages is below the 100,000 the threshold is stated at",
        )
        self.assertIsNone(entry["pass"])
        self.assertEqual(entry["threshold"], 50.0)
        self.assertTrue(entry["reason"])

    def test_informational_metric_never_carries_a_verdict(self) -> None:
        entry = PERF.performance_metric(
            "mcp_semantic_call_latency_p95_ms",
            "ms",
            9_999.0,
            action="semantic tools/call",
            corpus=reference_block(),
            sample_count=100,
            sample_unit="tools/call",
        )
        self.assertIsNone(entry["threshold"])
        self.assertIsNone(entry["pass"])
        self.assertIn("bigram", entry["reason"])

    def test_unknown_metric_name_is_rejected(self) -> None:
        with self.assertRaises(ValueError):
            PERF.performance_metric(
                "invented_metric",
                "ms",
                1.0,
                action="a",
                corpus=reference_block(),
                sample_count=1,
                sample_unit="sample",
            )


class ValidatorTests(unittest.TestCase):
    def validate(self, manifest: dict) -> dict:
        with tempfile.TemporaryDirectory() as name:
            path = Path(name) / "manifest.json"
            path.write_text(json.dumps(manifest), encoding="utf-8")
            return PERF.validate_performance_manifest(path)

    def test_accepts_a_full_passing_manifest(self) -> None:
        self.validate(manifest_with(passing_metrics()))

    def test_accepts_a_manifest_with_a_real_failure(self) -> None:
        metrics = passing_metrics()
        for entry in metrics:
            if entry["name"] == "noop_sync_latency_ms":
                entry["value"] = 8_000.0
                entry["pass"] = False
        manifest = manifest_with(metrics)
        result = self.validate(manifest)
        self.assertEqual(result["gate"]["failures"], ["noop_sync_latency_ms"])
        self.assertIs(result["gate"]["pass"], False)

    def test_accepts_a_below_scale_smoke_manifest_with_no_verdict(self) -> None:
        manifest = manifest_with(
            passing_metrics(False), at_threshold_scale=False, profile="smoke"
        )
        result = self.validate(manifest)
        self.assertIsNone(result["gate"]["pass"])
        self.assertEqual(len(result["gate"]["deferred"]), len(PERF.PERFORMANCE_THRESHOLDS))

    def test_rejects_a_tuned_threshold(self) -> None:
        metrics = passing_metrics()
        for entry in metrics:
            if entry["name"] == "search_latency_p95_ms":
                entry["threshold"] = 5_000.0
        with self.assertRaises(ValueError):
            self.validate(manifest_with(metrics))

    def test_rejects_a_pass_flag_that_contradicts_the_value(self) -> None:
        metrics = passing_metrics()
        for entry in metrics:
            if entry["name"] == "noop_sync_latency_ms":
                entry["value"] = 8_000.0  # fails, but pass stays True
        with self.assertRaises(ValueError):
            self.validate(manifest_with(metrics))

    def test_rejects_a_dropped_metric(self) -> None:
        metrics = [m for m in passing_metrics() if m["name"] != "noop_sync_latency_ms"]
        with self.assertRaises(ValueError):
            self.validate(manifest_with(metrics))

    def test_rejects_a_metric_without_its_corpus_named_inline(self) -> None:
        metrics = passing_metrics()
        for entry in metrics:
            if entry["name"] == "search_latency_p95_ms":
                del entry["corpus"]
        with self.assertRaises(ValueError):
            self.validate(manifest_with(metrics))

    def test_rejects_a_metric_without_a_sample_count(self) -> None:
        metrics = passing_metrics()
        for entry in metrics:
            if entry["name"] == "search_latency_p95_ms":
                entry["sample_count"] = 0
        with self.assertRaises(ValueError):
            self.validate(manifest_with(metrics))

    def test_rejects_a_corpus_reference_that_disagrees_with_the_header(self) -> None:
        metrics = passing_metrics()
        for entry in metrics:
            if entry["name"] == "search_latency_p95_ms":
                entry["corpus"] = dict(entry["corpus"], message_count=4_000)
        with self.assertRaises(ValueError):
            self.validate(manifest_with(metrics))

    def test_rejects_a_verdict_below_threshold_scale(self) -> None:
        # Measured state at 6,000 messages would be a manufactured pass.
        metrics = passing_metrics()
        for entry in metrics:
            entry["corpus"] = reference_block(False, 6_000)
        manifest = manifest_with(metrics, at_threshold_scale=False, profile="smoke")
        manifest["corpus"] = corpus_block(False, 6_000)
        with self.assertRaises(ValueError):
            self.validate(manifest)

    def test_rejects_a_threshold_scale_claim_at_the_wrong_message_count(self) -> None:
        manifest = manifest_with(passing_metrics())
        manifest["corpus"] = dict(manifest["corpus"], message_count=4_000)
        for entry in manifest["metrics"]:
            entry["corpus"] = dict(entry["corpus"], message_count=4_000)
        with self.assertRaises(ValueError):
            self.validate(manifest)

    def test_rejects_an_informational_metric_promoted_to_a_gate(self) -> None:
        metrics = passing_metrics()
        for entry in metrics:
            if entry["name"] == "mcp_semantic_call_latency_p95_ms":
                entry["threshold"] = 50.0
                entry["pass"] = True
        with self.assertRaises(ValueError):
            self.validate(manifest_with(metrics))

    def test_rejects_real_transcript_provenance(self) -> None:
        manifest = manifest_with(passing_metrics())
        manifest["corpus"] = dict(manifest["corpus"], contains_real_transcripts=True)
        with self.assertRaises(ValueError):
            self.validate(manifest)

    def test_rejects_a_missing_runtime_wall_clock(self) -> None:
        manifest = manifest_with(passing_metrics())
        manifest["runtime"] = {}
        with self.assertRaises(ValueError):
            self.validate(manifest)

    def test_rejects_a_gate_verdict_that_hides_a_failure(self) -> None:
        metrics = passing_metrics()
        for entry in metrics:
            if entry["name"] == "noop_sync_latency_ms":
                entry["value"] = 8_000.0
                entry["pass"] = False
        manifest = manifest_with(metrics)
        manifest["gate"] = {"pass": True, "failures": [], "deferred": []}
        with self.assertRaises(ValueError):
            self.validate(manifest)

    def test_rejects_an_mcp_metric_that_hides_which_backend_was_resident(self) -> None:
        # The 50 ms MCP threshold is worded "encoder resident". A reader checking
        # that one entry must not have to cross-reference limitations to learn
        # the number describes a narrower condition.
        for field in ("encoder_resident", "binary_vector_backend"):
            metrics = passing_metrics()
            for entry in metrics:
                if entry["name"] == "mcp_single_call_latency_p95_ms":
                    del entry["detail"][field]
            with self.assertRaises(ValueError, msg=f"missing {field} was accepted"):
                self.validate(manifest_with(metrics))

    def test_rejects_an_mcp_metric_with_no_detail_block(self) -> None:
        metrics = passing_metrics()
        for entry in metrics:
            if entry["name"] == "mcp_semantic_call_latency_p95_ms":
                entry.pop("detail", None)
        with self.assertRaises(ValueError):
            self.validate(manifest_with(metrics))

    def test_accepts_an_unmeasured_informational_metric_with_a_reason(self) -> None:
        metrics = passing_metrics()
        for entry in metrics:
            if entry["name"] == "embeddings_index_build_ms":
                entry["state"] = PERF.STATE_NOT_MEASURED
                entry["value"] = None
                entry["detail"] = {"skipped_reason": "--skip-embeddings"}
        self.validate(manifest_with(metrics))

    def test_rejects_an_unmeasured_metric_that_still_carries_a_value(self) -> None:
        metrics = passing_metrics()
        for entry in metrics:
            if entry["name"] == "embeddings_index_build_ms":
                entry["state"] = PERF.STATE_NOT_MEASURED
                entry["value"] = 285_000.0  # an estimate masquerading as a skip
                entry["detail"] = {"skipped_reason": "--skip-embeddings"}
        with self.assertRaises(ValueError):
            self.validate(manifest_with(metrics))

    def test_rejects_an_unmeasured_metric_with_no_stated_reason(self) -> None:
        metrics = passing_metrics()
        for entry in metrics:
            if entry["name"] == "embeddings_index_build_ms":
                entry["state"] = PERF.STATE_NOT_MEASURED
                entry["value"] = None
                entry["detail"] = {}
        with self.assertRaises(ValueError):
            self.validate(manifest_with(metrics))

    def test_a_thresholded_metric_can_never_be_left_unmeasured(self) -> None:
        # --skip-embeddings must not become a way to drop a gated metric.
        with self.assertRaises(ValueError):
            PERF.performance_metric(
                "search_latency_p95_ms",
                "ms",
                None,
                action="skipped",
                corpus=reference_block(),
                sample_count=1,
                sample_unit="search invocation",
                state=PERF.STATE_NOT_MEASURED,
                reason="skipped",
            )


class QuerySetTests(unittest.TestCase):
    def test_query_set_is_non_empty_and_unique(self) -> None:
        self.assertTrue(PERF.SEARCH_QUERIES)
        self.assertEqual(len(set(PERF.SEARCH_QUERIES)), len(PERF.SEARCH_QUERIES))

    def test_query_set_covers_cjk_and_latin(self) -> None:
        has_cjk = any(any("一" <= ch <= "鿿" for ch in q) for q in PERF.SEARCH_QUERIES)
        has_latin = any(q.isascii() for q in PERF.SEARCH_QUERIES)
        self.assertTrue(has_cjk)
        self.assertTrue(has_latin)

    def test_dropped_queries_are_recorded_with_a_reason(self) -> None:
        # A query set with silent removals reads as cherry-picked; each drop
        # carries why it was dropped.
        self.assertTrue(PERF.DROPPED_QUERIES)
        for entry in PERF.DROPPED_QUERIES:
            self.assertTrue(entry["query"])
            self.assertTrue(entry["reason"])
            self.assertNotIn(entry["query"], PERF.SEARCH_QUERIES)


class ProfileTests(unittest.TestCase):
    def test_full_profile_matches_the_frozen_corpus_scale(self) -> None:
        self.assertEqual(PERF.PROFILES["full"]["messages"], PERF.DEFAULT_MESSAGES)
        self.assertEqual(PERF.PROFILES["full"]["sessions"], PERF.DEFAULT_SESSIONS)

    def test_smoke_profile_is_strictly_smaller(self) -> None:
        self.assertLess(PERF.PROFILES["smoke"]["messages"], PERF.DEFAULT_MESSAGES)


class ReportingHelperTests(unittest.TestCase):
    def test_sum_durations_adds_process_wall_clock(self) -> None:
        samples = [{"duration_ms": 10.5}, {"duration_ms": 4.5}]
        self.assertEqual(PERF.sum_durations(samples), 15.0)

    def test_corpus_ref_is_derived_from_the_descriptor(self) -> None:
        descriptor = PERF.corpus_descriptor(
            {
                "corpus": {
                    "message_count": 100_000,
                    "session_count": 5_000,
                    "file_count": 5_000,
                    "total_bytes": 50_920_199,
                    "providers": {"codex": {}, "aider": {}},
                    "fixture_hash": "f" * 64,
                }
            },
            True,
        )
        self.assertIs(descriptor["contains_real_transcripts"], False)
        self.assertEqual(descriptor["providers"], ["aider", "codex"])
        reference = PERF.corpus_ref(descriptor)
        self.assertEqual(reference["message_count"], 100_000)
        self.assertIs(reference["at_threshold_scale"], True)

    def test_ci_viability_states_a_verdict_either_way(self) -> None:
        descriptor = {"message_count": 100_000, "source_bytes": 50_920_199}
        fast = PERF.ci_viability(30.0, {"initial_sync_s": 20.0}, descriptor, False)
        slow = PERF.ci_viability(900.0, {"initial_sync_s": 800.0}, descriptor, False)
        self.assertIs(fast["fits_a_5_minute_ci_budget"], True)
        self.assertIs(slow["fits_a_5_minute_ci_budget"], False)
        self.assertTrue(fast["verdict"])
        self.assertIn("900.0", slow["verdict"])

    def test_ci_viability_subtracts_the_informational_phases(self) -> None:
        # The proposed CI subset must be derived from the measured breakdown, not
        # from a guess: dropping the embeddings phases leaves all four thresholds.
        descriptor = {"message_count": 100_000, "source_bytes": 50_920_199}
        result = PERF.ci_viability(
            372.0,
            {"initial_sync_s": 70.0, "embeddings_index_s": 285.0, "mcp_semantic_s": 4.0},
            descriptor,
            False,
        )
        self.assertEqual(result["embeddings_phase_s"], 289.0)
        self.assertEqual(result["thresholded_metrics_only_wall_clock_s"], 83.0)
        self.assertIn("--skip-embeddings", result["proposed_subset_for_per_pr_ci"]["run"])

    def test_limitations_disclose_a_missing_encoder(self) -> None:
        bigram = PERF.limitations(
            {"backend": PERF.FALLBACK_BACKEND, "is_real_embedding_model": False},
            "built_by_harness_from_workspace",
            True,
        )
        self.assertTrue(any("no encoder was resident" in item for item in bigram))
        real = PERF.limitations(
            {"backend": "e5-small", "is_real_embedding_model": True},
            "built_by_harness_from_workspace",
            True,
        )
        self.assertFalse(any("no encoder was resident" in item for item in real))

    def test_limitations_disclose_a_caller_supplied_binary(self) -> None:
        items = PERF.limitations(
            {"backend": "e5-small", "is_real_embedding_model": True},
            "caller_supplied_prebuilt",
            True,
        )
        self.assertTrue(any("source-to-binary linkage" in item for item in items))


if __name__ == "__main__":
    unittest.main()
