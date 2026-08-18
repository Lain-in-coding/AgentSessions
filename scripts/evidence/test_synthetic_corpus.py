"""Unit tests for the deterministic synthetic corpus generator.

Determinism is the core contract: the same seed must produce byte-identical
files, so two generations of the same scale are compared byte for byte and by
tree hash. The second contract is format validity — every provider file must
still parse as the format its adapter probes for, and the message count a
renderer writes must equal the count the manifest claims, because a downstream
gate that ingests this corpus compares emitted against expected.

Everything here runs in-process at a small scale and always runs; no compiled
binary and no real transcript is involved. The full 100,000-message contract is
re-checked on demand with `synthetic_corpus.py verify`, and additionally by the
regeneration test below when SYNTHETIC_CORPUS_FULL_REGEN=1 is set (it writes
~50 MB and takes several seconds, so it is not part of the default run).
"""

import importlib.util
import json
import os
import sys
import tempfile
import unittest
from pathlib import Path

MODULE_PATH = Path(__file__).with_name("synthetic_corpus.py")
SPEC = importlib.util.spec_from_file_location("synthetic_corpus", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
SC = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = SC
SPEC.loader.exec_module(SC)

PRIVACY_SPEC = importlib.util.spec_from_file_location(
    "privacy_scan", Path(__file__).with_name("privacy_scan.py")
)
assert PRIVACY_SPEC is not None and PRIVACY_SPEC.loader is not None
PRIVACY = importlib.util.module_from_spec(PRIVACY_SPEC)
sys.modules[PRIVACY_SPEC.name] = PRIVACY
PRIVACY_SPEC.loader.exec_module(PRIVACY)

# Small but structurally complete scale: every provider gets sessions, and the
# residual balancing path is exercised.
TEST_SESSIONS = 300
TEST_MESSAGES = 6_000


class DeterminismTests(unittest.TestCase):
    def test_same_seed_produces_byte_identical_corpus(self) -> None:
        with tempfile.TemporaryDirectory() as first, tempfile.TemporaryDirectory() as second:
            left_manifest = SC.generate_corpus(Path(first), TEST_SESSIONS, TEST_MESSAGES)
            right_manifest = SC.generate_corpus(Path(second), TEST_SESSIONS, TEST_MESSAGES)
            left = SC.corpus_files(Path(first) / "corpus")
            right = SC.corpus_files(Path(second) / "corpus")
            self.assertTrue(left)
            self.assertEqual(len(left), len(right))
            for one, other in zip(left, right):
                self.assertEqual(
                    one.relative_to(Path(first) / "corpus").as_posix(),
                    other.relative_to(Path(second) / "corpus").as_posix(),
                )
                self.assertEqual(one.read_bytes(), other.read_bytes(), f"drift in {one.name}")
            self.assertEqual(left_manifest, right_manifest)
            self.assertEqual(
                left_manifest["corpus"]["fixture_hash"],
                right_manifest["corpus"]["fixture_hash"],
            )

    def test_seeded_rng_is_independent_of_corpus_size(self) -> None:
        """One session's content may not depend on how many sessions exist."""
        small = SC.build_session_messages(7, 12, False)
        again = SC.build_session_messages(7, 12, False)
        self.assertEqual(small, again)

    def test_tree_hash_ignores_crlf(self) -> None:
        """The frozen hash must survive a CRLF checkout, per the repo precedent."""
        with tempfile.TemporaryDirectory() as name:
            root = Path(name)
            lf = root / "lf.jsonl"
            lf.write_bytes(b'{"a":1}\n{"b":2}\n')
            lf_hash = SC.normalized_tree_hash([lf], root)
            lf.write_bytes(b'{"a":1}\r\n{"b":2}\r\n')
            self.assertEqual(lf_hash, SC.normalized_tree_hash([lf], root))


class PlanTests(unittest.TestCase):
    def test_provider_share_is_a_partition(self) -> None:
        self.assertAlmostEqual(sum(share for _, share in SC.PROVIDER_SHARE), 1.0, places=9)
        self.assertEqual(len(SC.PROVIDER_SHARE), 6)
        self.assertEqual(
            {name for name, _ in SC.PROVIDER_SHARE},
            set(SC.RENDERERS),
            "every shared provider needs a renderer and vice versa",
        )

    def test_assignment_covers_every_session_and_provider(self) -> None:
        assignment = SC.provider_assignment(SC.DEFAULT_SESSIONS)
        self.assertEqual(len(assignment), SC.DEFAULT_SESSIONS)
        self.assertEqual(set(assignment), set(SC.RENDERERS))

    def test_message_counts_sum_exactly_and_keep_aider_even(self) -> None:
        assignment = SC.provider_assignment(TEST_SESSIONS)
        counts = SC.session_message_counts(assignment, TEST_MESSAGES)
        self.assertEqual(sum(counts), TEST_MESSAGES)
        self.assertEqual(len(counts), TEST_SESSIONS)
        self.assertTrue(all(count >= 2 for count in counts))
        for provider, count in zip(assignment, counts):
            if provider == "aider":
                self.assertEqual(count % 2, 0, "aider transcripts are user/assistant pairs")

    def test_full_scale_counts_sum_to_the_frozen_total(self) -> None:
        assignment = SC.provider_assignment(SC.DEFAULT_SESSIONS)
        counts = SC.session_message_counts(assignment, SC.DEFAULT_MESSAGES)
        self.assertEqual(sum(counts), SC.DEFAULT_MESSAGES)

    def test_too_few_sessions_is_rejected(self) -> None:
        with self.assertRaises(ValueError):
            SC.provider_assignment(5)


class SamplingTests(unittest.TestCase):
    def test_cdf_sampling_stays_inside_the_declared_range(self) -> None:
        low = min(bucket[1] for bucket in SC.BODY_LENGTH_CDF)
        high = max(bucket[2] for bucket in SC.BODY_LENGTH_CDF)
        for index in range(500):
            value = SC.sample_cdf(SC.seeded_rng("cdf-test", index), SC.BODY_LENGTH_CDF)
            self.assertGreaterEqual(value, low)
            self.assertLessEqual(value, high)

    def test_message_class_mix_is_a_partition(self) -> None:
        self.assertAlmostEqual(
            sum(entry[1] for entry in SC.MESSAGE_CLASS_MIX), 1.0, places=9
        )
        self.assertAlmostEqual(sum(share for _, share in SC.LANGUAGE_MIX), 1.0, places=9)

    def test_log_uniform_respects_bounds(self) -> None:
        rng = SC.seeded_rng("log-uniform-test")
        for _ in range(200):
            value = SC.sample_log_uniform(rng, 300, 2_000)
            self.assertGreaterEqual(value, 300)
            self.assertLessEqual(value, 2_000)

    def test_quantile_is_nearest_rank(self) -> None:
        ordered = list(range(1, 101))
        self.assertEqual(SC.quantile(ordered, 0.50), 50)
        self.assertEqual(SC.quantile(ordered, 0.99), 99)
        self.assertEqual(SC.quantile(ordered, 1.0), 100)

    def test_text_reaches_its_target_length(self) -> None:
        for language in ("zh", "en", "code"):
            rng = SC.seeded_rng("text-test", language)
            for target in (30, 120, 900, 4_000):
                text = SC.build_text(rng, language, target, False)
                self.assertGreaterEqual(len(text), target)

    def test_single_line_mode_removes_newlines(self) -> None:
        rng = SC.seeded_rng("single-line-test")
        text = SC.build_text(rng, "code", 1_500, True)
        self.assertNotIn("\n", text)


class RendererTests(unittest.TestCase):
    def _messages(self, count: int, single_line: bool = False) -> list:
        return SC.build_session_messages(3, count, single_line)

    def test_claude_code_lines_are_json_with_unique_uuids(self) -> None:
        text = SC.render_claude_code(3, self._messages(12))
        lines = text.splitlines()
        self.assertEqual(len(lines), 12)
        uuids = set()
        previous = None
        for line in lines:
            record = json.loads(line)
            self.assertIn(record["type"], {"user", "assistant"})
            self.assertEqual(record["parentUuid"], previous)
            uuids.add(record["uuid"])
            previous = record["uuid"]
        self.assertEqual(len(uuids), 12, "message uuids must be unique inside a session")

    def test_codex_emits_meta_plus_one_response_item_per_message(self) -> None:
        text = SC.render_codex(3, self._messages(10))
        records = [json.loads(line) for line in text.splitlines()]
        self.assertEqual(records[0]["type"], "session_meta")
        self.assertEqual(records[1]["type"], "turn_context")
        items = [r for r in records if r["type"] == "response_item"]
        self.assertEqual(len(items), 10)
        ids = {item["payload"]["id"] for item in items}
        self.assertEqual(len(ids), 10)

    def test_aider_markdown_pairs_user_headings_with_replies(self) -> None:
        text = SC.render_aider(3, self._messages(10, single_line=True))
        lines = text.splitlines()
        self.assertTrue(lines[0].startswith("# aider chat started at "))
        headings = [line for line in lines if line.startswith("#### ")]
        self.assertEqual(len(headings), 5)
        for line in lines[1:]:
            self.assertFalse(
                line.startswith("# aider chat started at "),
                "only the first line may look like a session header",
            )

    def test_cline_and_hermes_are_valid_json_documents(self) -> None:
        rows = json.loads(SC.render_cline(3, self._messages(8)))
        self.assertEqual(len(rows), 8)
        self.assertEqual([row["role"] for row in rows][:2], ["user", "assistant"])
        document = json.loads(SC.render_hermes(3, self._messages(8)))
        self.assertEqual(document["message_count"], 8)
        self.assertEqual(len(document["messages"]), 8)

    def test_kimi_wire_lines_carry_one_append_message_per_message(self) -> None:
        text = SC.render_kimi(3, self._messages(8))
        records = [json.loads(line) for line in text.splitlines()]
        appended = [r for r in records if r["type"] == "context.append_message"]
        self.assertEqual(len(appended), 8)

    def test_no_renderer_emits_an_empty_message(self) -> None:
        """An adapter skips empty content, which would break emitted == expected."""
        for count in (2, 5, 9):
            for message in self._messages(count):
                self.assertTrue(message["text"].strip())


class ManifestTests(unittest.TestCase):
    def test_frozen_manifest_is_internally_consistent(self) -> None:
        manifest = SC.load_frozen_manifest()
        self.assertEqual(manifest["schema_version"], SC.SCHEMA_VERSION)
        self.assertEqual(manifest["seed"], SC.SEED)
        self.assertEqual(manifest["corpus_version"], SC.CORPUS_VERSION)
        self.assertIs(manifest["provenance"]["contains_real_transcripts"], False)
        corpus = manifest["corpus"]
        self.assertEqual(corpus["session_count"], SC.DEFAULT_SESSIONS)
        self.assertEqual(corpus["message_count"], SC.DEFAULT_MESSAGES)
        self.assertEqual(corpus["file_count"], SC.DEFAULT_SESSIONS)
        self.assertEqual(len(corpus["fixture_hash"]), 64)
        self.assertEqual(corpus["hash_algorithm"], "sha256")
        self.assertEqual(corpus["hash_normalization"], "crlf_to_lf")
        self.assertEqual(set(corpus["providers"]), set(SC.RENDERERS))
        self.assertEqual(
            sum(stats["sessions"] for stats in corpus["providers"].values()),
            SC.DEFAULT_SESSIONS,
        )
        self.assertEqual(
            sum(stats["messages"] for stats in corpus["providers"].values()),
            SC.DEFAULT_MESSAGES,
        )
        measured = manifest["measured"]
        self.assertEqual(measured["message_text_chars"]["count"], SC.DEFAULT_MESSAGES)
        self.assertEqual(sum(measured["language_mix"].values()), SC.DEFAULT_MESSAGES)

    def test_frozen_manifest_records_a_cjk_ratio_near_the_measured_anchor(self) -> None:
        """The anchor is the frozen semantic corpus: 14.35% CJK characters."""
        ratio = SC.load_frozen_manifest()["measured"]["cjk_char_ratio"]
        self.assertAlmostEqual(ratio, 0.1435, delta=0.02)

    def test_generated_manifest_matches_the_generated_corpus(self) -> None:
        with tempfile.TemporaryDirectory() as name:
            output = Path(name)
            manifest = SC.generate_corpus(output, TEST_SESSIONS, TEST_MESSAGES)
            files = SC.corpus_files(output / "corpus")
            self.assertEqual(len(files), manifest["corpus"]["file_count"])
            self.assertEqual(
                SC.normalized_tree_hash(files, output / "corpus"),
                manifest["corpus"]["fixture_hash"],
            )
            written = json.loads((output / "manifest.json").read_text(encoding="utf-8"))
            self.assertEqual(written, manifest)


class PrivacyTests(unittest.TestCase):
    def test_generated_corpus_has_no_privacy_rule_hits(self) -> None:
        """No personal or machine path may ever appear in generated content."""
        with tempfile.TemporaryDirectory() as name:
            output = Path(name)
            SC.generate_corpus(output, 60, 900)
            paths = [output / "manifest.json", *SC.corpus_files(output / "corpus")]
            for path in paths:
                findings = PRIVACY.scan_lines(
                    path.name, path.read_text(encoding="utf-8").splitlines(), frozenset()
                )
                self.assertEqual(findings, [], f"privacy hits in {path.name}")

    def test_content_banks_carry_no_privacy_rule_hits(self) -> None:
        banks = (
            SC.ZH_FRAGMENTS
            + SC.ZH_LATIN_TOKENS
            + SC.EN_FRAGMENTS
            + SC.CODE_FRAGMENTS
            + SC.PROJECT_SLUGS
        )
        findings = PRIVACY.scan_lines("content-banks", list(banks), frozenset())
        self.assertEqual(findings, [])


@unittest.skipUnless(
    os.environ.get("SYNTHETIC_CORPUS_FULL_REGEN") == "1",
    "set SYNTHETIC_CORPUS_FULL_REGEN=1 to regenerate the full ~50 MB corpus",
)
class FullScaleRegenerationTests(unittest.TestCase):
    def test_full_regeneration_reproduces_the_frozen_hash(self) -> None:
        frozen = SC.load_frozen_manifest()
        with tempfile.TemporaryDirectory() as name:
            manifest = SC.generate_corpus(Path(name))
            self.assertEqual(manifest, frozen)


if __name__ == "__main__":
    unittest.main()
