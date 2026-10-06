"""Independent synthetic fixture builders. Never load external DBs/transcripts."""
from __future__ import annotations

from contextlib import contextmanager
from dataclasses import dataclass
import hashlib
import json
from pathlib import Path
import sqlite3
import stat
import tempfile

HERE = Path(__file__).resolve().parent
FIXTURE_REVISION = 1
SID = "session:雪/#%_e\u0301"
STAMP = 1700000000.125


@dataclass(frozen=True)
class Raw:
    value: str | bytes | None


def encoded(value):
    if isinstance(value, Raw):
        return value.value
    if value is None:
        return None
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"), allow_nan=False)


class Fixtures:
    def __init__(self, root: Path):
        self.root = root
        self.connections = []
        self.permissions = []

    def path(self, relative="source.sqlite") -> Path:
        path = (self.root / relative).resolve()
        path.relative_to(self.root)  # all created files are in this owned scratch area
        return path

    def database(self, schema: str, relative="source.sqlite"):
        path = self.path(relative)
        path.parent.mkdir(parents=True, exist_ok=True)
        with path.open("xb"):
            pass  # no-clobber, even for a mistaken repeated fixture name
        conn = sqlite3.connect(path, isolation_level=None)
        self.connections.append(conn)
        conn.execute("PRAGMA user_version = 1")
        conn.executescript(schema)
        return path, conn

    def cursor(self, heads=None, bodies=None, *, sid=SID, relative="source.sqlite", wal=False):
        path, conn = self.database("CREATE TABLE cursorDiskKV (key TEXT PRIMARY KEY, value BLOB)", relative)
        if wal:
            conn.execute("PRAGMA journal_mode = WAL")
            conn.execute("PRAGMA wal_autocheckpoint = 0")
        self.put_cursor(conn, sid, heads or [], bodies or {})
        return path, conn

    @staticmethod
    def put_cursor(conn, sid, heads, bodies):
        # The IDs and header order are deliberate, not borrowed upstream values.
        rows = [("composerData:" + sid, encoded({"fullConversationHeadersOnly": heads}))]
        rows += [("bubbleId:" + sid + ":" + mid, encoded(body)) for mid, body in bodies.items()]
        conn.execute("BEGIN")
        conn.executemany("INSERT INTO cursorDiskKV VALUES (?, ?)", rows)
        conn.execute("COMMIT")

    def hermes(self, rows=(), *, sid=SID, relative="state.db", wal=False):
        path, conn = self.database(
            "CREATE TABLE sessions (id TEXT PRIMARY KEY, started_at REAL);"
            "CREATE TABLE messages (id INTEGER PRIMARY KEY, session_id TEXT, role TEXT, "
            "content TEXT, tool_calls TEXT, tool_call_id TEXT, tool_name TEXT, timestamp REAL);"
            "CREATE INDEX message_order ON messages(session_id, timestamp, id);", relative,
        )
        if wal:
            conn.execute("PRAGMA journal_mode = WAL")
            conn.execute("PRAGMA wal_autocheckpoint = 0")
        conn.execute("INSERT INTO sessions VALUES (?, ?)", (sid, STAMP))
        self.put_messages(conn, sid, rows)
        return path, conn

    @staticmethod
    def put_messages(conn, sid, rows):
        for row in rows:
            conn.execute("INSERT INTO messages VALUES (?, ?, ?, ?, ?, ?, ?, ?)", (
                row["id"], sid, row.get("role", "user"), row.get("content", "synthetic text"),
                encoded(row.get("tool_calls")), row.get("tool_call_id"), row.get("tool_name"),
                row.get("timestamp", STAMP),
            ))

    def read_only_files(self, path: Path):
        for item in files_of(path):
            if item.exists():
                self.permissions.append((item, item.stat().st_mode))
                item.chmod(stat.S_IRUSR | stat.S_IRGRP | stat.S_IROTH)

    def close(self):
        # Restore only our own generated file attributes before closing writers.
        for path, mode in reversed(self.permissions):
            if path.exists():
                path.chmod(mode)
        for conn in reversed(self.connections):
            conn.close()


@contextmanager
def synthetic_area():
    # No default HOME/temp discovery; scratch writes stay inside this experiment.
    temp = tempfile.TemporaryDirectory(prefix=".scratch-", dir=HERE)
    root = Path(temp.name).resolve()
    root.relative_to(HERE)
    assert root.name.startswith(".scratch-")
    fixtures = Fixtures(root)
    try:
        yield fixtures
    finally:
        try:
            fixtures.close()
        finally:
            # Re-check the resolved absolute cleanup target before recursive cleanup.
            root.resolve().relative_to(HERE)
            temp.cleanup()


def files_of(path: Path):
    return (path, Path(str(path) + "-wal"), Path(str(path) + "-shm"))


def fingerprint(path: Path, cap=8 * 1024 * 1024) -> list[dict]:
    result = []
    for label, item in zip(("database", "wal", "shm"), files_of(path)):
        if not item.exists():
            result.append({"file": label, "exists": False, "bytes": 0, "sha256": None})
            continue
        if item.stat().st_size > cap:
            raise ValueError("synthetic fingerprint byte cap")
        digest, size = hashlib.sha256(), 0
        with item.open("rb") as stream:
            while True:
                chunk = stream.read(min(65536, cap - size + 1))
                if not chunk:
                    break
                size += len(chunk)
                if size > cap:
                    raise ValueError("synthetic fingerprint byte cap")
                digest.update(chunk)
        result.append({"file": label, "exists": True, "bytes": size, "sha256": digest.hexdigest()})
    return result


def preservation(before, after):
    # Do not persist randomized WAL salts/hashes: compare real hashes in-process.
    # The validator replays these assertions; this is not a signed audit log.
    return [{"file": a["file"], "before_exists": a["exists"], "after_exists": b["exists"],
             "unchanged": a == b} for a, b in zip(before, after)]

# A live synthetic writer must be in another process: SQLite shares an already
# writable SHM mapping among same-process connections, even after chmod.
def _wal_writer(path_string, pipe):
    path = Path(path_string).resolve()
    path.relative_to(HERE)
    conn = None
    try:
        conn = sqlite3.connect(path, isolation_level=None, timeout=0.2)
        conn.execute("PRAGMA journal_mode = WAL")
        conn.execute("PRAGMA wal_autocheckpoint = 0")
        Fixtures.put_cursor(conn, SID, [{"bubbleId": "old", "type": 1}], {"old": {"text": "before"}})
        pipe.send("ready")
        while pipe.poll(15):
            command = pipe.recv()
            if command == "stop":
                break
            if command != "commit":
                pipe.send("invalid_command")
                break
            conn.execute("BEGIN")
            conn.execute("UPDATE cursorDiskKV SET value = ? WHERE key = ?", (
                encoded({"fullConversationHeadersOnly": [{"bubbleId": "new", "type": 2}]}),
                "composerData:" + SID,
            ))
            conn.execute("INSERT INTO cursorDiskKV VALUES (?, ?)", ("bubbleId:" + SID + ":new", encoded({"text": "after"})))
            conn.execute("COMMIT")
            pipe.send("committed")
    except (OSError, sqlite3.Error):
        pipe.send("writer_failed")
    finally:
        if conn is not None:
            conn.close()
        pipe.close()


class LiveWriter:
    def __init__(self, fixtures: Fixtures):
        import multiprocessing
        self.fixtures = fixtures
        self.path, base = fixtures.database("CREATE TABLE cursorDiskKV (key TEXT PRIMARY KEY, value BLOB)")
        base.close()
        fixtures.connections.remove(base)
        context = multiprocessing.get_context("spawn")
        self.pipe, child = context.Pipe()
        self.process = context.Process(target=_wal_writer, args=(str(self.path), child), daemon=True)
        self.process.start()
        child.close()
        if self.receive() != "ready":
            self.close()
            raise RuntimeError("synthetic WAL writer initialization failed")

    def receive(self):
        if not self.pipe.poll(10):
            raise RuntimeError("synthetic WAL writer timed out")
        return self.pipe.recv()

    def commit(self):
        self.pipe.send("commit")
        if self.receive() != "committed":
            raise RuntimeError("synthetic WAL writer commit failed")

    def close(self):
        if self.process.is_alive():
            self.pipe.send("stop")
            self.process.join(5)
        if self.process.is_alive():
            self.process.terminate()  # only this owned, bounded test helper
            self.process.join(5)
        self.pipe.close()
        if self.process.exitcode != 0:
            raise RuntimeError("synthetic WAL writer did not shut down cleanly")

    def __enter__(self):
        return self

    def __exit__(self, *_):
        # The fixtures guard must restore attributes before writer shutdown.
        for path, mode in reversed(self.fixtures.permissions):
            if path.exists():
                path.chmod(mode)
        self.fixtures.permissions.clear()
        self.close()
