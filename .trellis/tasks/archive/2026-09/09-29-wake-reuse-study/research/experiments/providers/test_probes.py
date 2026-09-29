"""Synthetic-only probe contract and evidence tests."""
import unittest

from fixtures import SID, LiveWriter, fingerprint, preservation, synthetic_area
from probe import probe_cursor, open_snapshot, parse_cursor


class SnapshotTests(unittest.TestCase):
    def test_read_only_wal_source_is_unchanged(self):
        with synthetic_area() as f, LiveWriter(f) as writer:
            f.read_only_files(writer.path)
            before = fingerprint(writer.path)
            result = probe_cursor(writer.path, SID)
            after = fingerprint(writer.path)
            self.assertEqual(result['messages'][0]['text'], 'before')
            self.assertEqual(before, after, preservation(before, after))

    def test_read_transaction_remains_pinned_during_commit(self):
        with synthetic_area() as f, LiveWriter(f) as writer:
            f.read_only_files(writer.path)
            with open_snapshot(writer.path) as db:
                before = parse_cursor(db, SID)
                writer.commit()
                during = parse_cursor(db, SID)
            after = probe_cursor(writer.path, SID)
            self.assertEqual(before, during)
            self.assertEqual(after['messages'][0]['text'], 'after')


if __name__ == '__main__':
    unittest.main()
