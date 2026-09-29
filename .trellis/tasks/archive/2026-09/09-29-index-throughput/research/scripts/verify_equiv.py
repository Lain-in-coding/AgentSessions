#!/usr/bin/env python3
"""Equivalence check: ingest one corpus with two binaries into fresh catalogs and
compare every authoritative/projection table byte-for-byte."""
from __future__ import annotations

import argparse
import hashlib
import pathlib
import shutil
import sqlite3
import subprocess

TABLES = [
    ("catalog", "SELECT id, payload FROM catalog ORDER BY id"),
    ("message_placements", "SELECT placement_id, session_id, document_id, message_id, source_ordinal, is_sidechain, byte_start, byte_end FROM message_placements ORDER BY placement_id"),
    ("message_edges", "SELECT child_placement_id, parent_message_id, parent_native_id, relation FROM message_edges ORDER BY child_placement_id"),
    ("tool_activities", "SELECT activity_id, message_id, kind, actor, name, target, status FROM tool_activities ORDER BY activity_id"),
    ("usage_events", "SELECT usage_id, session_id, message_id, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, reasoning_tokens, token_source FROM usage_events ORDER BY usage_id"),
    ("fts", "SELECT rowid, id, text FROM fts ORDER BY rowid"),
    ("fts_ids", "SELECT wire_id, id_json, fts_rowid FROM fts_ids ORDER BY wire_id"),
    ("session_fts_ids", "SELECT session_wire, fts_rowid FROM session_fts_ids ORDER BY session_wire"),
    ("session_fts", "SELECT rowid, text FROM session_fts ORDER BY rowid"),
    ("source_membership", "SELECT source_path, message_id, document_id FROM source_membership ORDER BY source_path, message_id"),
    ("source_placement_membership", "SELECT source_path, placement_id FROM source_placement_membership ORDER BY source_path, placement_id"),
    ("source_scans", "SELECT source_path, len_bytes, fingerprint, parser_version FROM source_scans ORDER BY source_path"),
    ("source_relation_scans", "SELECT source_path, relation_schema_version FROM source_relation_scans ORDER BY source_path"),
    ("source_session_resume_claims", "SELECT * FROM source_session_resume_claims ORDER BY source_path, session_id"),
    # operation_digest embeds the installation-assignment wall clock (created_at_ms),
    # so it legitimately differs between two runs; the durable lifecycle fields must not.
    ("index_batches", "SELECT base_generation, target_generation, state, durable_point FROM index_batches ORDER BY base_generation, target_generation"),
]


def ingest(binary: str, dataset: pathlib.Path, scratch: pathlib.Path, batch_size: int) -> pathlib.Path:
    scratch.mkdir(parents=True, exist_ok=True)
    db = scratch / 'catalog.db'
    files = sorted(dataset.glob('session-*.jsonl'))
    for offset in range(0, len(files), batch_size):
        batch = files[offset:offset + batch_size]
        command = [binary, '--db', str(db), '--output', 'json', 'sync', *(str(p) for p in batch)]
        result = subprocess.run(command, capture_output=True, text=True)
        if result.returncode != 0:
            raise SystemExit(f'ingest failed ({result.returncode}): {result.stdout} {result.stderr}')
    return db


def dump(db: pathlib.Path) -> dict:
    connection = sqlite3.connect(f'file:{db.as_posix()}?mode=ro', uri=True)
    digest = {}
    for name, sql in TABLES:
        try:
            rows = connection.execute(sql).fetchall()
        except sqlite3.OperationalError as error:
            digest[name] = f'missing: {error}'
            continue
        hasher = hashlib.sha256()
        for row in rows:
            hasher.update(repr(row).encode('utf-8'))
            hasher.update(b'\n')
        digest[name] = {'rows': len(rows), 'sha256': hasher.hexdigest()}
    generation = connection.execute('SELECT active_generation FROM store_metadata WHERE singleton = 1').fetchone()
    digest['generation'] = generation[0]
    connection.close()
    return digest


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary-a', required=True)
    parser.add_argument('--binary-b', required=True)
    parser.add_argument('--dataset', required=True)
    parser.add_argument('--scratch', required=True)
    parser.add_argument('--batch-size', type=int, default=40)
    args = parser.parse_args()
    root = pathlib.Path(args.scratch)
    shutil.rmtree(root, ignore_errors=True)
    db_a = ingest(args.binary_a, pathlib.Path(args.dataset), root / 'a', args.batch_size)
    db_b = ingest(args.binary_b, pathlib.Path(args.dataset), root / 'b', args.batch_size)
    dump_a, dump_b = dump(db_a), dump(db_b)
    mismatch = []
    for key in sorted(set(dump_a) | set(dump_b)):
        if dump_a.get(key) != dump_b.get(key):
            mismatch.append((key, dump_a.get(key), dump_b.get(key)))
    print(f'tables compared: {len(TABLES) + 1}; mismatches: {len(mismatch)}')
    for key, a, b in mismatch:
        print(f'  {key}: A={a} B={b}')
    return 1 if mismatch else 0


if __name__ == '__main__':
    raise SystemExit(main())
