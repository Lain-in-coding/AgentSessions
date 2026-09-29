"""Bounded, synthetic-only SQLite structure probes, not product adapters.

Wake-derived format ideas: see PROVENANCE.md and LICENSE-Wake.
No discovery, resume, canonical identity, source writes or file-copy fallback.
"""
from __future__ import annotations

from contextlib import contextmanager
from dataclasses import dataclass
import json
import math
import os
from pathlib import Path
import sqlite3
from typing import Iterator


@dataclass(frozen=True)
class Limits:
    file_bytes: int = 8 * 1024 * 1024
    logical_bytes: int = 8 * 1024 * 1024
    cell_bytes: int = 4096
    total_bytes: int = 32768
    rows: int = 64
    columns: int = 32
    tool_calls: int = 32
    sql_steps: int = 200000

    def __post_init__(self):
        if any(type(value) is not int or value < 1 for value in vars(self).values()):
            raise ValueError("probe limits must be positive integers")


class ProbeError(Exception):
    """A bounded classification; never interpolate source paths or payloads."""

    def __init__(self, code: str, stage: str):
        super().__init__(f"{stage}: {code}")
        self.code, self.stage = code, stage

    def observation(self) -> dict:
        return {"status": "error", "code": self.code, "stage": self.stage}


def readonly_uri(path: Path) -> str:
    # as_uri escapes literal %, #, ?, Unicode and spaces before adding the query.
    return Path(path).absolute().as_uri() + "?mode=ro"


class Snapshot:
    """All preflight and materialization queries share ONE pinned transaction."""

    def __init__(self, conn: sqlite3.Connection, limits: Limits):
        self.conn, self.limits = conn, limits
        self.materialized_bytes = 0
        self.steps = 0
        conn.set_progress_handler(self._progress, 100)

    def _progress(self):
        self.steps += 100
        return int(self.steps > self.limits.sql_steps)

    def execute(self, sql: str, args=(), *, stage: str):
        try:
            return self.conn.execute(sql, args)
        except sqlite3.Error as error:
            code = "sql_work_limit" if self.steps > self.limits.sql_steps else "sqlite_error"
            raise ProbeError(code, stage) from error

    def table(self, name: str, required: tuple[str, ...]):
        stage = "schema." + name
        row = self.execute(
            "SELECT type FROM sqlite_schema WHERE name = ? COLLATE BINARY LIMIT 1",
            (name,), stage=stage,
        ).fetchone()
        if row is None:
            raise ProbeError("missing_table", stage)
        if row[0] != "table":
            raise ProbeError("not_a_table", stage)
        count = self.execute(
            "SELECT count(*) FROM pragma_table_info(?)", (name,), stage=stage,
        ).fetchone()[0]
        if count > self.limits.columns:
            raise ProbeError("column_limit", stage)
        # Only fetch fixed, known names. Unknown metadata is not materialized.
        marks = ",".join("?" for _ in required)
        found = self.execute(
            f"SELECT name FROM pragma_table_info(?) WHERE name IN ({marks}) "
            "LIMIT ?", (name, *required, len(required)), stage=stage,
        ).fetchall()
        if {row[0] for row in found} != set(required):
            raise ProbeError("missing_column", stage)

    def rows(self, table: str, columns: tuple[str, ...], where: str, args=(),
             *, stage: str, order: str = "", one: bool = False) -> list[tuple]:
        # SQL identifiers/clauses below are internal constants, never caller data.
        cap = 1 if one else self.limits.rows
        count = self.execute(
            f"SELECT count(*) FROM (SELECT 1 FROM {table} WHERE {where} LIMIT ?)",
            (*args, cap + 1), stage=stage,
        ).fetchone()[0]
        if count > cap:
            raise ProbeError("duplicate_row" if one else "row_limit", stage)
        if count == 0:
            return []
        lengths = [f"coalesce(length(CAST({col} AS BLOB)), 0)" for col in columns]
        stats = ", ".join(f"max({term})" for term in lengths)
        stats += ", sum(" + " + ".join(lengths) + ")"
        # BLOB byte lengths, not TEXT character counts (including embedded NUL).
        sizes = self.execute(
            f"SELECT {stats} FROM {table} WHERE {where}", args, stage=stage,
        ).fetchone()
        if max(sizes[:-1]) > self.limits.cell_bytes:
            raise ProbeError("cell_limit", stage)
        if self.materialized_bytes + sizes[-1] > self.limits.total_bytes:
            raise ProbeError("total_byte_limit", stage)
        suffix = f" ORDER BY {order}" if order else ""
        cursor = self.execute(
            f"SELECT {', '.join(columns)} FROM {table} WHERE {where}{suffix} LIMIT ?",
            (*args, cap + 1), stage=stage,
        )
        try:
            result = cursor.fetchmany(cap + 1)
        except sqlite3.Error as error:
            raise ProbeError("sqlite_error", stage) from error
        if len(result) != count:
            raise ProbeError("row_set_changed", stage)
        self.materialized_bytes += sizes[-1]
        return result


@contextmanager
def open_snapshot(path: Path, limits: Limits = Limits()) -> Iterator[Snapshot]:
    """Read-only access, no immutable URI and no copied main/WAL/SHM trio.

    WAL-mode sources require BOTH existing sidecars. Refuse instead of letting
    SQLite create a missing sidecar in an otherwise writable source directory.
    Writable SHM is explicitly unsupported: mode=ro alone can change reader
    marks. The synthetic writer is process-isolated to avoid SQLite reusing a
    pre-existing writable SHM mapping in the reader process.
    """
    path = Path(path)
    try:
        if not path.is_file():
            raise ProbeError("missing_source", "source.files")
        sidecars = [Path(str(path) + suffix) for suffix in ("-wal", "-shm")]
        for item in (path, *sidecars):
            if item.exists() and (not item.is_file() or item.stat().st_size > limits.file_bytes):
                raise ProbeError("file_size_limit", "source.files")
        with path.open("rb") as stream:
            header = stream.read(100)
    except OSError as error:
        raise ProbeError("source_io", "source.files") from error
    if len(header) != 100 or header[:16] != b"SQLite format 3\0":
        raise ProbeError("invalid_sqlite_header", "source.header")
    present = [item.exists() for item in sidecars]
    if header[18:20] == b"\x02\x02":
        if present != [True, True]:
            raise ProbeError("wal_sidecars_required", "source.files")
        if os.access(sidecars[1], os.W_OK):
            raise ProbeError("writable_shm_unsupported", "source.files")
    elif any(present):
        raise ProbeError("unexpected_sidecar", "source.files")
    conn = None
    try:
        conn = sqlite3.connect(readonly_uri(path), uri=True, timeout=0.2, isolation_level=None)
        db = Snapshot(conn, limits)
        db.execute("PRAGMA query_only = ON", stage="source.open")
        db.execute("PRAGMA trusted_schema = OFF", stage="source.open")
        db.execute("BEGIN", stage="source.pin")
        db.execute("SELECT rootpage FROM sqlite_schema LIMIT 1", stage="source.pin").fetchone()
        pages = db.execute("PRAGMA page_count", stage="source.size").fetchone()[0]
        page_size = db.execute("PRAGMA page_size", stage="source.size").fetchone()[0]
        if pages * page_size > limits.logical_bytes:
            raise ProbeError("logical_size_limit", "source.size")
        yield db
    except sqlite3.Error as error:
        raise ProbeError("sqlite_error", "source.read") from error
    finally:
        if conn is not None:
            conn.close()  # rolls back the read transaction; no source checkpoint


def _text(value, stage: str) -> str:
    if isinstance(value, bytes):
        try:
            return value.decode("utf-8")
        except UnicodeError as error:
            raise ProbeError("invalid_utf8", stage) from error
    if not isinstance(value, str):
        raise ProbeError("invalid_text_type", stage)
    return value


def _reject_constant(_):
    raise ValueError("non-finite JSON constant")


def _unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("duplicate JSON key")
        result[key] = value
    return result


def strict_json(raw: str):
    return json.loads(raw, parse_constant=_reject_constant, object_pairs_hook=_unique_object)


def _json(raw, stage: str):
    text = _text(raw, stage)
    try:
        return strict_json(text)
    except (ValueError, RecursionError) as error:
        raise ProbeError("malformed_json", stage) from error


def decoded_field(data: dict, key: str) -> dict:
    """Decode ONE JSON-in-JSON layer; keep unparsed text and original encoding."""
    if key not in data:
        return {"encoding": "missing", "value": None}
    raw = data[key]
    if raw is None:
        return {"encoding": "null", "value": None}
    if not isinstance(raw, str):
        return {"encoding": "json_value", "value": raw}
    if not raw.strip():
        return {"encoding": "empty_text", "value": raw}
    try:
        value = strict_json(raw)
    except json.JSONDecodeError:
        return {"encoding": "unparsed_text", "value": raw}
    except (ValueError, RecursionError) as error:
        raise ProbeError("malformed_nested_json", "tool.arguments") from error
    return {"encoding": "json_text", "value": value, "raw": raw}


def _cursor_tool(data) -> dict:
    if not isinstance(data, dict):
        raise ProbeError("invalid_tool_shape", "cursor.tool")
    name, native_id = data.get("name"), data.get("toolCallId")
    if not isinstance(name, str) or (native_id is not None and not isinstance(native_id, str)):
        raise ProbeError("invalid_tool_shape", "cursor.tool")
    raw = decoded_field(data, "rawArgs")
    params = decoded_field(data, "params")
    absent = {"missing", "null", "empty_text"}
    selected = "rawArgs" if raw["encoding"] not in absent else (
        "params" if params["encoding"] not in absent else None
    )
    return {"native_call_id": native_id, "name": name, "rawArgs": raw,
            "params": params, "selected_input": selected,
            "result": decoded_field(data, "result")}


def parse_cursor(db: Snapshot, native_id: str) -> dict:
    if not isinstance(native_id, str) or not native_id:
        raise ProbeError("invalid_identifier", "cursor.request")
    if len(native_id.encode("utf-8")) > db.limits.cell_bytes:
        raise ProbeError("cell_limit", "cursor.request")
    db.table("cursorDiskKV", ("key", "value"))
    rows = db.rows("cursorDiskKV", ("value",), "key = ? COLLATE BINARY",
                   ("composerData:" + native_id,), one=True, stage="cursor.composer")
    if not rows:
        raise ProbeError("missing_composer", "cursor.composer")
    if rows[0][0] is None:
        raise ProbeError("null_composer", "cursor.composer")
    data = _json(rows[0][0], "cursor.composer")
    if not isinstance(data, dict):
        raise ProbeError("invalid_composer_shape", "cursor.composer")
    if "fullConversationHeadersOnly" not in data:
        raise ProbeError("missing_headers", "cursor.composer")
    heads = data["fullConversationHeadersOnly"]
    if not isinstance(heads, list):
        raise ProbeError("invalid_headers", "cursor.composer")
    if len(heads) > db.limits.rows:
        raise ProbeError("header_limit", "cursor.composer")
    messages = []
    for ordinal, head in enumerate(heads):
        mid = head.get("bubbleId") if isinstance(head, dict) else None
        item = {"ordinal": ordinal, "native_message_id": mid}
        messages.append(item)
        if not isinstance(mid, str) or not mid:
            item["state"] = "invalid_header"
            continue
        role = head.get("type")
        item["role"] = {1: "user", 2: "assistant"}.get(role, "unknown") if type(role) is int else "unknown"
        # No prefix slicing, LIKE, delimiter splitting or normalization of IDs.
        key = "bubbleId:" + native_id + ":" + mid
        found = db.rows("cursorDiskKV", ("value",), "key = ? COLLATE BINARY",
                        (key,), one=True, stage="cursor.bubble")
        if not found:
            item["state"] = "missing_row"
            continue
        if found[0][0] is None:
            item["state"] = "null_value"
            continue
        try:
            body = _json(found[0][0], "cursor.bubble")
            if not isinstance(body, dict):
                raise ProbeError("invalid_bubble_shape", "cursor.bubble")
            text = body.get("text")
            if text is not None:
                text = _text(text, "cursor.bubble")
            tool = _cursor_tool(body["toolFormerData"]) if "toolFormerData" in body else None
            item.update(text=text, tool=tool, state="ok" if item["role"] != "unknown" else "unknown_role")
            if text is None and tool is None:
                item["state"] = "empty_bubble"
        except ProbeError as error:
            item["state"] = error.code
    return {"status": "partial" if any(m["state"] != "ok" for m in messages) else "ok",
            "native_session_id": native_id, "order_source": "fullConversationHeadersOnly",
            "messages": messages}


def probe_cursor(path: Path, native_id: str, limits: Limits = Limits()) -> dict:
    with open_snapshot(path, limits) as db:
        return parse_cursor(db, native_id)


def _hermes_calls(raw, limits: Limits) -> list[dict]:
    if raw is None:
        return []
    values = _json(raw, "hermes.tool_calls")
    if not isinstance(values, list):
        raise ProbeError("invalid_tool_calls_shape", "hermes.tool_calls")
    if len(values) > limits.tool_calls:
        raise ProbeError("tool_call_limit", "hermes.tool_calls")
    calls = []
    for index, item in enumerate(values):
        if not isinstance(item, dict):
            raise ProbeError("invalid_tool_shape", "hermes.tool_calls")
        func = item.get("function", item)
        native_id = item.get("id")
        if not isinstance(func, dict) or not isinstance(func.get("name"), str):
            raise ProbeError("invalid_tool_shape", "hermes.tool_calls")
        if native_id is not None and not isinstance(native_id, str):
            raise ProbeError("invalid_tool_shape", "hermes.tool_calls")
        calls.append({"index": index, "native_call_id": native_id,
                      "shape": "function" if "function" in item else "compact",
                      "name": func["name"], "arguments": decoded_field(func, "arguments")})
    return calls


def _associations(messages: list[dict]):
    previous = []
    for message in messages:
        if message["role"] == "tool":
            call_id, name = message["tool_call_id"], message["tool_name"]
            if call_id is not None:
                found = [c for c in previous if c["native_call_id"] == call_id]
                basis = ("ambiguous_id" if len(found) > 1 else "observed_id") if found else "unmatched_id"
            elif name is not None:
                found = [c for c in previous if c["name"] == name]
                basis = ("ambiguous_name" if len(found) > 1 else "name_only") if found else "unmatched_name"
            else:
                found, basis = [], "missing_identity"
            message["association"] = {
                "basis": basis, "authoritative": False,
                "candidates": [{"message_id": c["message_id"], "call_index": c["index"]} for c in found],
            }
        if message["role"] == "assistant":
            previous.extend({**call, "message_id": message["native_message_id"]} for call in message["tool_calls"])


def _seconds(value, stage):
    if value is not None and (type(value) not in (int, float) or not math.isfinite(value)):
        raise ProbeError("invalid_timestamp", stage)
    return value  # Preserve seconds/NULL; do not guess units or invent an epoch.


def parse_hermes(db: Snapshot, native_id: str, profile: str | None) -> dict:
    if not isinstance(native_id, str) or not native_id or (profile is not None and not isinstance(profile, str)):
        raise ProbeError("invalid_identifier", "hermes.request")
    if any(len(s.encode("utf-8")) > db.limits.cell_bytes for s in (native_id, profile or "")):
        raise ProbeError("cell_limit", "hermes.request")
    db.table("sessions", ("id", "started_at"))
    columns = ("id", "role", "content", "tool_calls", "tool_call_id", "tool_name", "timestamp")
    db.table("messages", (*columns, "session_id"))
    sessions = db.rows("sessions", ("id", "started_at"), "id = ? COLLATE BINARY",
                       (native_id,), one=True, stage="hermes.session")
    if not sessions:
        raise ProbeError("missing_session", "hermes.session")
    session_id = _text(sessions[0][0], "hermes.session")
    started = _seconds(sessions[0][1], "hermes.session")
    rows = db.rows("messages", columns, "session_id = ? COLLATE BINARY", (native_id,),
                   stage="hermes.messages", order="timestamp, id")
    messages = []
    for row in rows:
        mid, role, content, raw_calls, call_id, name, timestamp = row
        if type(mid) is not int:
            raise ProbeError("invalid_message_id", "hermes.messages")
        role = _text(role, "hermes.messages")
        content = _text(content, "hermes.messages") if content is not None else None
        call_id = _text(call_id, "hermes.messages") if call_id is not None else None
        name = _text(name, "hermes.messages") if name is not None else None
        timestamp = _seconds(timestamp, "hermes.messages")
        state = "ok"
        try:
            calls = _hermes_calls(raw_calls, db.limits)
        except ProbeError as error:
            if error.code == "tool_call_limit":
                raise
            calls, state = [], error.code
        if role not in ("user", "assistant", "tool", "system", "developer") and state == "ok":
            state = "unknown_role"
        if timestamp is None and state == "ok":
            state = "missing_timestamp"
        messages.append({"native_message_id": mid, "role": role, "content": content,
                         "timestamp_seconds": timestamp, "tool_calls": calls,
                         "tool_calls_encoding": "null" if raw_calls is None else "json_text",
                         "tool_call_id": call_id, "tool_name": name, "state": state})
    _associations(messages)
    return {"status": "partial" if any(m["state"] != "ok" for m in messages) else "ok",
            "namespace": {"kind": "main" if profile is None else "profile", "profile": profile},
            "native_session_id": session_id, "started_at_seconds": started,
            "order_source": "timestamp, id", "messages": messages}


def probe_hermes(path: Path, native_id: str, profile: str | None = None,
                 limits: Limits = Limits()) -> dict:
    with open_snapshot(path, limits) as db:
        return parse_hermes(db, native_id, profile)
