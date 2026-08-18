import importlib.util
import tempfile
import unittest
from pathlib import Path

MODULE_PATH = Path(__file__).with_name("semantic_benchmark.py")
SPEC = importlib.util.spec_from_file_location("semantic_benchmark", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
BENCHMARK = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BENCHMARK)


class BenchmarkCorpusTests(unittest.TestCase):
    def test_corpus_is_byte_deterministic(self) -> None:
        with tempfile.TemporaryDirectory() as first_name, tempfile.TemporaryDirectory() as second_name:
            first = Path(first_name)
            second = Path(second_name)
            first_files, first_markers = BENCHMARK.write_corpus(first, 4, 4)
            second_files, second_markers = BENCHMARK.write_corpus(second, 4, 4)
            self.assertEqual(first_markers, second_markers)
            self.assertEqual(
                BENCHMARK.tree_hash(first_files, first),
                BENCHMARK.tree_hash(second_files, second),
            )

    def test_corpus_has_no_real_paths(self) -> None:
        with tempfile.TemporaryDirectory() as name:
            files, _ = BENCHMARK.write_corpus(Path(name), 4, 4)
            content = files[0].read_text(encoding="utf-8")
            self.assertNotIn(str(Path.home()), content)
            self.assertNotIn("C:\\", content)
            self.assertNotIn("/home/", content)

    def test_every_topic_query_maps_to_a_unique_marker(self) -> None:
        with tempfile.TemporaryDirectory() as name:
            _, markers = BENCHMARK.write_corpus(Path(name), 6, 4)
        self.assertEqual(len(markers), 6)
        self.assertEqual(len(set(markers.values())), 6)
        for query, marker in markers.items():
            self.assertEqual(marker, f"[topic-{list(markers.values()).index(marker):02d}]")

    def test_topic_messages_carry_marker_and_distractors_do_not(self) -> None:
        with tempfile.TemporaryDirectory() as name:
            files, markers = BENCHMARK.write_corpus(Path(name), 4, 4)
            lines = files[0].read_text(encoding="utf-8").splitlines()
        marker_set = set(markers.values())
        marker_texts = [
            json_line["message"]["content"]
            for line in lines
            for json_line in [__import__("json").loads(line)]
            if json_line["message"]["content"].rstrip().endswith(tuple(marker_set))
        ]
        # 4 topics x 4 messages each
        self.assertEqual(len(marker_texts), 16)
        distractor_texts = [
            json_line["message"]["content"]
            for line in lines
            for json_line in [__import__("json").loads(line)]
            if not json_line["message"]["content"].rstrip().endswith(tuple(marker_set))
        ]
        self.assertEqual(len(distractor_texts), 4)
        for text in distractor_texts:
            self.assertNotIn("[topic-", text)

    def test_paraphrases_share_no_tokens_with_query(self) -> None:
        # The gate's signal: paraphrase texts must not contain the query's
        # distinctive term, otherwise lexical mode would trivially find them.
        for topic in BENCHMARK.TOPICS:
            terms = topic["query"].split()
            for text in topic["paraphrases"]:
                lowered = text.lower()
                for term in terms:
                    self.assertNotIn(
                        term.lower(),
                        lowered,
                        f"paraphrase {text!r} leaks query term {term!r}",
                    )
            for text in topic["anchors"]:
                self.assertTrue(
                    any(term.lower() in text.lower() for term in terms),
                    f"anchor {text!r} must contain a query term",
                )

    def test_profiles_cover_available_topics(self) -> None:
        for profile in BENCHMARK.PROFILES.values():
            self.assertLessEqual(profile["topics"], len(BENCHMARK.TOPICS))
            self.assertLessEqual(profile["distractors"], len(BENCHMARK.DISTRACTORS))


class BenchmarkStatisticsTests(unittest.TestCase):
    def test_nearest_rank_percentiles(self) -> None:
        samples = [float(value) for value in range(1, 101)]
        summary = BENCHMARK.rounded_summary(samples)
        self.assertEqual(summary["p50"], 50.0)
        self.assertEqual(summary["p95"], 95.0)
        self.assertEqual(summary["count"], 100)

    def test_single_sample_stddev_is_zero(self) -> None:
        self.assertEqual(BENCHMARK.rounded_summary([12.5])["sample_stddev"], 0.0)

    def test_summary_validation_rejects_tampering(self) -> None:
        samples = [1.0, 2.0, 3.0]
        summary = BENCHMARK.rounded_summary(samples)
        summary["p95"] = 999.0
        with self.assertRaises(ValueError):
            BENCHMARK.assert_summary(samples, summary, "latency_ms")


class BenchmarkRecallTests(unittest.TestCase):
    def hit(self, text):
        return {"id": f"msg_v1_{text}", "score": 0.5, "text": text}

    def test_recall_full_relevant(self) -> None:
        hits = [self.hit(f"content {BENCHMARK.marker_for(0)}") for _ in range(4)]
        recall, retrieved, relevant = BENCHMARK.recall_at_k(hits, BENCHMARK.marker_for(0))
        self.assertEqual(recall, 1.0)
        self.assertEqual(retrieved, 4)
        self.assertEqual(relevant, 4)

    def test_recall_half_relevant(self) -> None:
        hits = [self.hit(f"anchor text {BENCHMARK.marker_for(1)}") for _ in range(2)]
        hits += [self.hit("unrelated distractor text") for _ in range(8)]
        recall, retrieved, _ = BENCHMARK.recall_at_k(hits, BENCHMARK.marker_for(1))
        self.assertEqual(recall, 0.5)
        self.assertEqual(retrieved, 2)

    def test_recall_zero(self) -> None:
        hits = [self.hit("completely unrelated") for _ in range(10)]
        recall, retrieved, _ = BENCHMARK.recall_at_k(hits, BENCHMARK.marker_for(2))
        self.assertEqual(recall, 0.0)
        self.assertEqual(retrieved, 0)

    def test_other_topic_markers_do_not_count(self) -> None:
        hits = [self.hit(f"wrong topic {BENCHMARK.marker_for(3)}") for _ in range(4)]
        recall, retrieved, _ = BENCHMARK.recall_at_k(hits, BENCHMARK.marker_for(0))
        self.assertEqual(recall, 0.0)
        self.assertEqual(retrieved, 0)


class BenchmarkReportValidationTests(unittest.TestCase):
    def base_report(self, status: str) -> dict:
        return {
            "schema_version": BENCHMARK.SCHEMA_VERSION,
            "status": status,
            "generated_at_utc": "2026-02-01T00:00:00Z",
            "commit": "a" * 40,
            "profile": "smoke",
            "environment": {"os": "test"},
            "binary": {
                "hash_algorithm": "sha256",
                "binary_hash": "b" * 64,
                "artifact_size_bytes": 1,
                "provenance": "built_by_harness_from_workspace",
            },
            "model": {
                "feature": "semantic-candle",
                "present": status == "locally_verified",
                "verified": status == "locally_verified",
                "detail": {},
            },
            "limitations": ["synthetic corpus"],
        }

    def mode_entry(self, topics: int, repeat: int, recall: float, mode: str) -> dict:
        rows = [
            {
                "query": BENCHMARK.TOPICS[index]["query"],
                "relevant": 4,
                "retrieved_relevant": round(recall * 4),
                "recall_at_k": recall,
                "effective_mode": mode,
            }
            for index in range(topics)
        ]
        samples = [5.0] * (topics * repeat)
        return {
            "requested_mode": mode,
            "effective_modes_observed": [mode],
            "degraded_query_count": sum(
                1 for row in rows if row["effective_mode"] == "lexical_fallback"
            ),
            "mean_recall_at_k": BENCHMARK.rounded(recall),
            "queries": rows,
            "latency_ms": {
                "raw_samples": samples,
                "summary": BENCHMARK.rounded_summary(samples),
            },
        }

    def write_report(self, report: dict) -> Path:
        path = Path(tempfile.mkstemp(suffix=".json")[1])
        path.write_text(
            __import__("json").dumps(report, ensure_ascii=False, indent=2) + "\n",
            encoding="utf-8",
        )
        return path

    def test_not_imported_report_validates(self) -> None:
        report = self.base_report("model_not_imported")
        report.update({"dataset": None, "index": None, "results": None})
        path = self.write_report(report)
        validated = BENCHMARK.validate_report(path)
        self.assertEqual(validated["status"], "model_not_imported")

    def test_not_imported_report_must_not_carry_results(self) -> None:
        report = self.base_report("model_not_imported")
        report.update({"dataset": None, "index": None, "results": {"lexical": {}}})
        path = self.write_report(report)
        with self.assertRaises(ValueError):
            BENCHMARK.validate_report(path)

    def test_not_imported_report_must_not_claim_verified_model(self) -> None:
        report = self.base_report("model_not_imported")
        report["model"]["verified"] = True
        report.update({"dataset": None, "index": None, "results": None})
        path = self.write_report(report)
        with self.assertRaises(ValueError):
            BENCHMARK.validate_report(path)

    def test_verified_report_validates(self) -> None:
        profile = BENCHMARK.PROFILES["smoke"]
        report = self.base_report("locally_verified")
        report.update(
            {
                "dataset": {
                    "kind": "deterministic_synthetic_semantic_paraphrase_corpus",
                    "contains_real_transcripts": False,
                    "hash_algorithm": "sha256",
                    "dataset_hash": "c" * 64,
                    "file_count": 1,
                    "message_count": profile["topics"] * 4 + profile["distractors"],
                    "topic_count": profile["topics"],
                    "relevant_messages_per_topic": 4,
                    "distractor_count": profile["distractors"],
                    "marker_convention": "suffix marker",
                    "k": 10,
                    "sync": {},
                },
                "methodology": {},
                "index": {
                    "backend": "semantic-candle",
                    "model_id": "intfloat-multilingual-e5-small@test",
                    "indexed": 20,
                    "skipped": 0,
                    "cleared": 0,
                    "warnings": [],
                },
                "results": {
                    "lexical": self.mode_entry(
                        profile["topics"], profile["repeat"], 0.5, "lexical"
                    ),
                    "semantic": self.mode_entry(
                        profile["topics"], profile["repeat"], 1.0, "semantic"
                    ),
                    "hybrid": self.mode_entry(
                        profile["topics"], profile["repeat"], 1.0, "hybrid"
                    ),
                },
            }
        )
        path = self.write_report(report)
        validated = BENCHMARK.validate_report(path)
        self.assertEqual(validated["status"], "locally_verified")

    def test_verified_report_requires_candle_backend(self) -> None:
        profile = BENCHMARK.PROFILES["smoke"]
        report = self.base_report("locally_verified")
        report.update(
            {
                "dataset": {
                    "kind": "deterministic_synthetic_semantic_paraphrase_corpus",
                    "contains_real_transcripts": False,
                    "hash_algorithm": "sha256",
                    "dataset_hash": "c" * 64,
                    "k": 10,
                    "sync": {},
                },
                "methodology": {},
                "index": {"backend": "bigram-hash", "model_id": "bigram-hash-v1"},
                "results": {
                    "lexical": self.mode_entry(
                        profile["topics"], profile["repeat"], 0.5, "lexical"
                    ),
                    "semantic": self.mode_entry(
                        profile["topics"], profile["repeat"], 1.0, "semantic"
                    ),
                    "hybrid": self.mode_entry(
                        profile["topics"], profile["repeat"], 1.0, "hybrid"
                    ),
                },
            }
        )
        path = self.write_report(report)
        with self.assertRaises(ValueError):
            BENCHMARK.validate_report(path)

    def test_verified_report_rejects_tampered_latency_summary(self) -> None:
        profile = BENCHMARK.PROFILES["smoke"]
        report = self.base_report("locally_verified")
        semantic = self.mode_entry(
            profile["topics"], profile["repeat"], 1.0, "semantic"
        )
        semantic["latency_ms"]["summary"]["p95"] = 999.0
        report.update(
            {
                "dataset": {
                    "kind": "deterministic_synthetic_semantic_paraphrase_corpus",
                    "contains_real_transcripts": False,
                    "hash_algorithm": "sha256",
                    "dataset_hash": "c" * 64,
                    "k": 10,
                    "sync": {},
                },
                "methodology": {},
                "index": {"backend": "semantic-candle", "model_id": "test"},
                "results": {
                    "lexical": self.mode_entry(
                        profile["topics"], profile["repeat"], 0.5, "lexical"
                    ),
                    "semantic": semantic,
                    "hybrid": self.mode_entry(
                        profile["topics"], profile["repeat"], 1.0, "hybrid"
                    ),
                },
            }
        )
        path = self.write_report(report)
        with self.assertRaises(ValueError):
            BENCHMARK.validate_report(path)

    def test_verified_report_rejects_out_of_range_recall(self) -> None:
        profile = BENCHMARK.PROFILES["smoke"]
        report = self.base_report("locally_verified")
        semantic = self.mode_entry(
            profile["topics"], profile["repeat"], 1.0, "semantic"
        )
        semantic["queries"][0]["recall_at_k"] = 1.5
        semantic["queries"][0]["retrieved_relevant"] = 6
        report.update(
            {
                "dataset": {
                    "kind": "deterministic_synthetic_semantic_paraphrase_corpus",
                    "contains_real_transcripts": False,
                    "hash_algorithm": "sha256",
                    "dataset_hash": "c" * 64,
                    "k": 10,
                    "sync": {},
                },
                "methodology": {},
                "index": {"backend": "semantic-candle", "model_id": "test"},
                "results": {
                    "lexical": self.mode_entry(
                        profile["topics"], profile["repeat"], 0.5, "lexical"
                    ),
                    "semantic": semantic,
                    "hybrid": self.mode_entry(
                        profile["topics"], profile["repeat"], 1.0, "hybrid"
                    ),
                },
            }
        )
        path = self.write_report(report)
        with self.assertRaises(ValueError):
            BENCHMARK.validate_report(path)


class BenchmarkHashTests(unittest.TestCase):
    def test_sha256_file_known_vector(self) -> None:
        with tempfile.TemporaryDirectory() as name:
            path = Path(name) / "abc.txt"
            path.write_bytes(b"abc")
            self.assertEqual(
                BENCHMARK.sha256_file(path),
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            )


if __name__ == "__main__":
    unittest.main()
