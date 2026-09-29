#!/usr/bin/env python3
"""Extended equivalence check for the batch-scoped commit-state optimization.

Scenario (run once per binary; both binaries ingest the SAME absolute source
paths so that path-derived columns stay comparable):

    initial both -> no-op both -> shrink replacement (f0) -> post-replacement
    no-op (both sources; f0 was replaced earlier, f1 never changed) ->
    empty-source tombstone (f1) -> second no-op (both sources)

Compares every non-shadow table plus the active generation. Emits a JSON
artifact (``--out``) and exits non-zero on any mismatch.

The artifact records all exclusions and their reasons:

* FTS5 shadow tables (``fts_*`` / ``session_fts_*``) are physical storage of
  the ``fts`` / ``session_fts`` virtual tables; the logical projections
  (``fts``, ``fts_ids``, ``session_fts``, ``session_fts_ids``) are compared.
* Fresh catalogs allocate a random installation namespace (``namespace_id``,
  ``namespace_input``, ``created_at_ms``) per catalog, so
  ``installation_namespaces`` / ``installation_locations`` /
  ``source_installations`` are compared on their stable columns only
  (provider/origin, provider/root/state, source_path/source_key).
* ``index_batches.source_replacements_json`` embeds the installation namespace
  allocation (wall clock + namespace id) and is therefore not comparable
  across two fresh catalogs; the durable lifecycle columns and the
  upsert/delete/relation JSON columns are compared instead.
* ``index_batches.{operation_id,operation_digest,created_at_ms,committed_at_ms}``
  and ``source_scans.scanned_at_ms`` are per-run identifiers/wall clocks.
"""
from __future__ import annotations

import argparse
import datetime
import hashlib
import json
import pathlib
import shutil
import sqlite3
import subprocess
import sys
import time

COUNT_FIELDS = ("sources", "emitted", "committed", "unchanged", "retained",
                "deferred", "skipped", "diagnostics")
SHADOW_TABLES = (
    "fts_config", "fts_content", "fts_data", "fts_docsize", "fts_idx",
    "session_fts_config", "session_fts_content", "session_fts_data",
    "session_fts_docsize", "session_fts_idx",
)
# Stable-column projections for tables whose identity/creation columns are
# allocated per fresh catalog (see module docstring).
PROJECTIONS = {
    "index_batches": ["base_generation", "target_generation", "state", "durable_point",
                      "upsert_ids_json", "delete_ids_json",
                      "relation_upserts_json", "relation_deletes_json", "relocation_json"],
    "installation_namespaces": ["provider_id", "origin"],
    "installation_locations": ["provider_id", "root_key", "root_locator", "state"],
    "source_installations": ["source_path", "source_key"],
    "source_scans": ["source_path", "len_bytes", "fingerprint", "provider_id", "parser_version"],
}


def sha256_file(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def tree_hash(directory: pathlib.Path) -> dict:
    """Harness-declared method: sha256(sorted(relative_posix_path + NUL + file_bytes + NUL))."""
    digest = hashlib.sha256()
    files = sorted(directory.glob("session-*.jsonl"))
    total = 0
    for path in files:
        raw = path.read_bytes()
        digest.update(path.name.encode("utf-8") + b"\0")
        digest.update(raw)
        digest.update(b"\0")
        total += len(raw)
    return {"method": "sha256(sorted(relative_posix_path + NUL + file_bytes + NUL))",
            "sha256": digest.hexdigest(), "file_count": len(files), "total_bytes": total}


def sync(binary: pathlib.Path, db: pathlib.Path, files: list[pathlib.Path], label: str) -> dict:
    command = [str(binary), "--db", str(db), "--output", "json", "sync", *(str(p) for p in files)]
    started = time.perf_counter()
    result = subprocess.run(command, capture_output=True, text=True)
    duration_ms = round((time.perf_counter() - started) * 1000.0, 1)
    if result.returncode != 0:
        raise SystemExit(f"{label} failed rc={result.returncode}\n"
                         f"stdout={result.stdout[:2000]}\nstderr={result.stderr[:2000]}")
    frame = json.loads(result.stdout.strip().splitlines()[-1])
    data = frame.get("data", {})
    return {"label": label, "files": [p.name for p in files], "duration_ms": duration_ms,
            "counts": {key: data.get(key) for key in COUNT_FIELDS}}


def copy_pristine(dataset: pathlib.Path, shared: pathlib.Path) -> list[pathlib.Path]:
    sources = sorted(dataset.glob("session-*.jsonl"))[:2]
    if len(sources) < 2:
        raise SystemExit(f"dataset {dataset} needs at least two session-*.jsonl files")
    shutil.rmtree(shared, ignore_errors=True)
    shared.mkdir(parents=True)
    copies = []
    for source in sources:
        target = shared / source.name
        shutil.copyfile(source, target)
        copies.append(target)
    return copies


def run_scenario(binary: pathlib.Path, side_root: pathlib.Path, dataset: pathlib.Path,
                 shared: pathlib.Path) -> tuple[pathlib.Path, list[dict]]:
    side_root.mkdir(parents=True, exist_ok=True)
    db = side_root / "catalog.db"
    if db.exists():
        raise SystemExit(f"refusing: {db} already exists")
    f0, f1 = copy_pristine(dataset, shared)
    steps = []
    steps.append(sync(binary, db, [f0, f1], "initial-both"))
    steps.append(sync(binary, db, [f0, f1], "noop-both"))
    lines = f0.read_text(encoding="utf-8").splitlines()
    f0.write_text("\n".join(lines[: len(lines) // 2]) + "\n", encoding="utf-8", newline="\n")
    steps.append(sync(binary, db, [f0], "shrink-replacement"))
    steps.append(sync(binary, db, [f0, f1], "post-replacement-noop"))
    f1.write_text("", encoding="utf-8", newline="\n")
    steps.append(sync(binary, db, [f1], "empty-source-tombstone"))
    steps.append(sync(binary, db, [f0, f1], "second-noop"))
    return db, steps


def dump(db: pathlib.Path) -> dict:
    connection = sqlite3.connect(str(db))
    tables = [row[0] for row in connection.execute(
        "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")]
    digest = {}
    for name in tables:
        if name in SHADOW_TABLES:
            continue
        columns = [row[1] for row in connection.execute(f'PRAGMA table_info("{name}")')]
        keep = PROJECTIONS.get(name, columns)
        order = ", ".join(f'"{column}"' for column in keep)
        rows = connection.execute(f'SELECT {order} FROM "{name}" ORDER BY {order}').fetchall()
        hasher = hashlib.sha256()
        for row in rows:
            hasher.update(repr(row).encode("utf-8"))
            hasher.update(b"\n")
        digest[name] = {"rows": len(rows), "sha256": hasher.hexdigest(),
                        "columns_compared": keep,
                        "columns_excluded": [c for c in columns if c not in keep]}
    connection.close()
    return digest


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary-a", required=True)
    parser.add_argument("--binary-b", required=True)
    parser.add_argument("--dataset", required=True)
    parser.add_argument("--scratch", required=True)
    parser.add_argument("--out", required=True)
    args = parser.parse_args()

    binary_a = pathlib.Path(args.binary_a).resolve()
    binary_b = pathlib.Path(args.binary_b).resolve()
    dataset = pathlib.Path(args.dataset).resolve()
    scratch = pathlib.Path(args.scratch).resolve()
    out = pathlib.Path(args.out)
    if scratch.exists():
        shutil.rmtree(scratch)
    scratch.mkdir(parents=True)
    shared = scratch / "shared-dataset"
    dataset_info = tree_hash(dataset)

    db_a, steps_a = run_scenario(binary_a, scratch / "a", dataset, shared)
    db_b, steps_b = run_scenario(binary_b, scratch / "b", dataset, shared)

    dump_a, dump_b = dump(db_a), dump(db_b)
    mismatches = []
    for key in sorted(set(dump_a) | set(dump_b)):
        if dump_a.get(key) != dump_b.get(key):
            mismatches.append({"table": key, "a": dump_a.get(key), "b": dump_b.get(key)})
    generation = {}
    for side, db in (("a", db_a), ("b", db_b)):
        connection = sqlite3.connect(str(db))
        generation[side] = connection.execute(
            "SELECT active_generation FROM store_metadata WHERE singleton = 1").fetchone()[0]
        connection.close()
    if generation["a"] != generation["b"]:
        mismatches.append({"table": "active_generation", "a": generation["a"], "b": generation["b"]})

    report = {
        "schema": "asg.equiv-extended/v1",
        "generated_at_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "scenario": [step["label"] for step in steps_a],
        "dataset": {"path": str(dataset), **dataset_info},
        "binaries": {
            "a": {"path": str(binary_a), "sha256": sha256_file(binary_a)},
            "b": {"path": str(binary_b), "sha256": sha256_file(binary_b)},
        },
        "steps": {"a": steps_a, "b": steps_b},
        "compared_tables": len(dump_a),
        "generation": generation,
        "tables_a": dump_a,
        "tables_b": dump_b,
        "mismatches": mismatches,
        "excluded_tables": ["FTS5 shadow tables: " + ", ".join(SHADOW_TABLES)],
        "exclusion_notes": [
            "installation namespace id/input/created_at_ms are randomly allocated per fresh catalog",
            "index_batches.source_replacements_json embeds the installation allocation and wall clock",
            "index_batches operation id/digest and timestamps, source_scans.scanned_at_ms are per-run values",
        ],
        "conclusion": ("0 mismatches across %d tables + active_generation" % len(dump_a))
        if not mismatches else ("%d mismatches" % len(mismatches)),
    }
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"tables": len(dump_a), "mismatches": len(mismatches),
                      "generation": generation, "out": str(out),
                      "a": report["binaries"]["a"]["sha256"][:16],
                      "b": report["binaries"]["b"]["sha256"][:16]}))
    for mismatch in mismatches:
        print("MISMATCH:", json.dumps(mismatch, ensure_ascii=False)[:500])
    return 1 if mismatches else 0


if __name__ == "__main__":
    raise SystemExit(main())