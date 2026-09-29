"""Frozen synthetic recipes and independent, hand-written expected observations."""
from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
import os
import sqlite3
from typing import Callable

from fixtures import (SID, STAMP, Fixtures, LiveWriter, Raw, encoded, files_of,
                      fingerprint, preservation, synthetic_area)
from probe import (Limits, ProbeError, open_snapshot, parse_cursor, probe_cursor,
                   probe_hermes, readonly_uri)


@dataclass(frozen=True)
class Case:
    id: str
    recipe: str
    run: Callable
    expected: dict


CASES = []


def case(id, recipe, expected):
    def register(function):
        CASES.append(Case(id, recipe, function, expected))
        return function
    return register


def error(code, stage):
    return {"status": "error", "code": code, "stage": stage}


def attempt(function):
    try:
        return function()
    except ProbeError as exc:
        return exc.observation()


def execute(spec: Case):
    with synthetic_area() as fixtures:
        return attempt(lambda: spec.run(fixtures))


def heads(*ids):
    return [{"bubbleId": mid, "type": 1 if index == 0 else 2} for index, mid in enumerate(ids)]


ZID = "z:💡/#%_e\u0301"
AID = "a:𐀀/雪"


@case("cursor.header_order_exact_ids", "KV insertion/key order opposes header order; non-BMP and separator IDs", {
    "status": "ok", "session_id": SID, "message_ids": [ZID, AID],
    "ordinals": [0, 1], "roles": ["user", "assistant"],
    "texts": ["  synthetic 雪 e\u0301  ", "synthetic second"],
})
def cursor_order(f):
    path, _ = f.cursor(heads(ZID, AID), {
        AID: {"type": 1, "text": "synthetic second"},
        ZID: {"type": 2, "text": "  synthetic 雪 e\u0301  "},
    })
    value = probe_cursor(path, SID)
    return {"status": value["status"], "session_id": value["native_session_id"],
            "message_ids": [m["native_message_id"] for m in value["messages"]],
            "ordinals": [m["ordinal"] for m in value["messages"]],
            "roles": [m["role"] for m in value["messages"]],
            "texts": [m["text"] for m in value["messages"]]}


@case("cursor.missing_null_malformed", "Keep a slot for malformed, missing, SQL NULL, non-object and invalid UTF-8 bubbles", {
    "status": "partial", "ids": ["valid", "bad", "missing", "null", "array", "utf8"],
    "states": ["ok", "malformed_json", "missing_row", "null_value", "invalid_bubble_shape", "invalid_utf8"],
})
def cursor_bubbles(f):
    path, _ = f.cursor(heads("valid", "bad", "missing", "null", "array", "utf8"), {
        "valid": {"text": "synthetic"}, "bad": Raw("{"), "null": None,
        "array": [], "utf8": Raw(b"\xff"),
    })
    value = probe_cursor(path, SID)
    return {"status": value["status"], "ids": [m["native_message_id"] for m in value["messages"]],
            "states": [m["state"] for m in value["messages"]]}


@case("cursor.arguments_encodings", "rawArgs, params, plain text, blank field and JSON null; never execute inputs", {
    "selected": ["rawArgs", "params", "rawArgs", "params", "rawArgs"],
    "raw_encoding": ["json_text", "missing", "unparsed_text", "empty_text", "json_text"],
    "params_encoding": ["json_value", "json_value", "missing", "json_text", "json_value"],
    "values": [{"path": "synthetic/雪"}, {"path": "synthetic/params"}, "not a command: synthetic", {"n": 2}, None],
    "result_encoding": ["json_text", "json_value", "missing", "missing", "missing"],
    "native_call_ids": ["call:雪/#%", None, None, None, None],
})
def cursor_arguments(f):
    tools = [
        {"name": "synthetic_tool", "toolCallId": "call:雪/#%", "rawArgs": '{"path":"synthetic/雪"}',
         "params": {"ignored": True}, "result": '{"output":"synthetic result"}'},
        {"name": "synthetic_tool", "params": {"path": "synthetic/params"}, "result": {"output": "synthetic result"}},
        {"name": "synthetic_tool", "rawArgs": "not a command: synthetic"},
        {"name": "synthetic_tool", "rawArgs": "  ", "params": '{"n":2}'},
        {"name": "synthetic_tool", "rawArgs": "null", "params": {"must_not_replace_null": True}},
    ]
    ids = [str(i) for i in range(len(tools))]
    path, _ = f.cursor(heads(*ids), {mid: {"toolFormerData": tool} for mid, tool in zip(ids, tools)})
    observed = [m["tool"] for m in probe_cursor(path, SID)["messages"]]
    return {"selected": [t["selected_input"] for t in observed],
            "raw_encoding": [t["rawArgs"]["encoding"] for t in observed],
            "params_encoding": [t["params"]["encoding"] for t in observed],
            "values": [t[t["selected_input"]]["value"] for t in observed],
            "result_encoding": [t["result"]["encoding"] for t in observed],
            "native_call_ids": [t["native_call_id"] for t in observed]}


@case("cursor.no_unicode_normalization", "Separate NFC/NFD session IDs and wildcard-bearing neighboring keys", {
    "ids": ["é", "e\u0301", "id%_"], "texts": ["composed", "decomposed", "literal wildcard"],
})
def cursor_unicode(f):
    path, conn = f.cursor(heads("bubble"), {"bubble": {"text": "composed"}}, sid="é")
    f.put_cursor(conn, "e\u0301", heads("bubble"), {"bubble": {"text": "decomposed"}})
    f.put_cursor(conn, "id%_", heads("bubble"), {"bubble": {"text": "literal wildcard"}})
    f.put_cursor(conn, "idXX", heads("bubble"), {"bubble": {"text": "not selected"}})
    values = [probe_cursor(path, sid) for sid in ("é", "e\u0301", "id%_")]
    return {"ids": [v["native_session_id"] for v in values], "texts": [v["messages"][0]["text"] for v in values]}


@case("cursor.duplicate_occurrences", "Repeated bubble IDs preserve distinct header ordinals; not canonical message proof", {
    "ids": ["same", "same"], "ordinals": [0, 1], "roles": ["user", "assistant"],
})
def cursor_occurrences(f):
    path, _ = f.cursor(heads("same", "same"), {"same": {"text": "synthetic"}})
    rows = probe_cursor(path, SID)["messages"]
    return {"ids": [r["native_message_id"] for r in rows], "ordinals": [r["ordinal"] for r in rows],
            "roles": [r["role"] for r in rows]}


@case("cursor.compound_key_ambiguity", "Two native tuples can spell the same colon-delimited storage key", {
    "native_tuples": [["a:b", "c"], ["a", "b:c"]], "texts": ["shared storage cell", "shared storage cell"],
})
def cursor_compound(f):
    path, conn = f.cursor(heads("c"), {"c": {"text": "shared storage cell"}}, sid="a:b")
    f.put_cursor(conn, "a", heads("b:c"), {})
    values = [probe_cursor(path, sid) for sid in ("a:b", "a")]
    return {"native_tuples": [[v["native_session_id"], v["messages"][0]["native_message_id"]] for v in values],
            "texts": [v["messages"][0]["text"] for v in values]}


@case("cursor.invalid_headers_explicit", "Missing ID and unknown header role are not silently assigned an assistant role", {
    "status": "partial", "states": ["invalid_header", "unknown_role"], "role": "unknown",
})
def cursor_invalid_headers(f):
    path, _ = f.cursor([{"type": 1}, {"bubbleId": "x", "type": 77}], {"x": {"text": "synthetic", "type": 2}})
    value = probe_cursor(path, SID)
    return {"status": value["status"], "states": [m["state"] for m in value["messages"]],
            "role": value["messages"][1]["role"]}


def cursor_negative(f, mode):
    path, conn = f.cursor(heads("x"), {"x": {"text": "synthetic"}})
    limits = Limits()
    composer_key = "composerData:" + SID
    if mode == "missing_composer":
        conn.execute("DELETE FROM cursorDiskKV WHERE key = ?", (composer_key,))
    elif mode in ("bad_json", "null_composer", "missing_headers", "bad_headers", "duplicate_json_key"):
        value = {"bad_json": "{", "null_composer": None, "missing_headers": "{}",
                 "bad_headers": '{"fullConversationHeadersOnly":{}}',
                 "duplicate_json_key": '{"fullConversationHeadersOnly":[],"fullConversationHeadersOnly":[]}'}[mode]
        conn.execute("UPDATE cursorDiskKV SET value = ? WHERE key = ?", (value, composer_key))
    elif mode == "duplicate_row":
        conn.execute("ALTER TABLE cursorDiskKV RENAME TO oldkv")
        conn.execute("CREATE TABLE cursorDiskKV (key TEXT, value BLOB)")
        conn.execute("INSERT INTO cursorDiskKV SELECT * FROM oldkv")
        conn.execute("INSERT INTO cursorDiskKV SELECT * FROM oldkv")
    elif mode == "huge_cell":
        conn.execute("UPDATE cursorDiskKV SET value = zeroblob(2097152) WHERE key = ?", (composer_key,))
    elif mode == "headers_limit":
        conn.execute("UPDATE cursorDiskKV SET value = ? WHERE key = ?",
                     (encoded({"fullConversationHeadersOnly": heads("x", "y", "z")}), composer_key))
        limits = Limits(rows=2)
    elif mode == "total_bytes":
        conn.execute("UPDATE cursorDiskKV SET value = ? WHERE key != ?", (encoded({"text": "界" * 1000}), composer_key))
        limits = Limits(total_bytes=2000)
    elif mode == "logical_size":
        limits = Limits(logical_bytes=4096)
    elif mode == "file_size":
        limits = Limits(file_bytes=4096)
    else:
        raise AssertionError("unregistered synthetic recipe")
    return probe_cursor(path, SID, limits)


for mode, code, stage in (
    ("missing_composer", "missing_composer", "cursor.composer"),
    ("bad_json", "malformed_json", "cursor.composer"),
    ("null_composer", "null_composer", "cursor.composer"),
    ("missing_headers", "missing_headers", "cursor.composer"),
    ("bad_headers", "invalid_headers", "cursor.composer"),
    ("duplicate_json_key", "malformed_json", "cursor.composer"),
    ("duplicate_row", "duplicate_row", "cursor.composer"),
    ("huge_cell", "cell_limit", "cursor.composer"),
    ("headers_limit", "header_limit", "cursor.composer"),
    ("total_bytes", "total_byte_limit", "cursor.bubble"),
    ("logical_size", "logical_size_limit", "source.size"),
    ("file_size", "file_size_limit", "source.files"),
):
    CASES.append(Case("cursor." + mode, "Isolated negative mutation: " + mode,
                      lambda f, mode=mode: cursor_negative(f, mode), error(code, stage)))


@case("hermes.order_and_native_text", "Message timestamp seconds first, integer ID second; no text trimming", {
    "status": "ok", "session_id": SID, "ids": [30, 10, 20],
    "seconds": [STAMP - 1, STAMP, STAMP], "texts": ["  synthetic 雪  ", "tie first", "tie second"],
})
def hermes_order(f):
    path, _ = f.hermes([
        {"id": 20, "content": "tie second"}, {"id": 30, "timestamp": STAMP - 1, "content": "  synthetic 雪  "},
        {"id": 10, "content": "tie first"},
    ])
    value = probe_hermes(path, SID)
    return {"status": value["status"], "session_id": value["native_session_id"],
            "ids": [m["native_message_id"] for m in value["messages"]],
            "seconds": [m["timestamp_seconds"] for m in value["messages"]],
            "texts": [m["content"] for m in value["messages"]]}


@case("hermes.profile_namespaces", "Main vs named default vs Unicode profile; identical native session IDs", {
    "namespaces": [{"kind": "main", "profile": None}, {"kind": "profile", "profile": "default"},
                   {"kind": "profile", "profile": "档案 # % e\u0301"}],
    "ids": [SID, SID, SID], "texts": ["main", "named default", "unicode profile"],
})
def hermes_profiles(f):
    values = []
    for profile, text, relative in (
        (None, "main", "state.db"), ("default", "named default", "profiles/default/state.db"),
        ("档案 # % e\u0301", "unicode profile", "profiles/档案 # % e\u0301/state.db"),
    ):
        path, _ = f.hermes([{"id": 1, "content": text}], relative=relative)
        values.append(probe_hermes(path, SID, profile))
    return {"namespaces": [v["namespace"] for v in values], "ids": [v["native_session_id"] for v in values],
            "texts": [v["messages"][0]["content"] for v in values]}


@case("hermes.tool_shapes", "Compact + function tool calls; IDs stay native, missing IDs stay null", {
    "shapes": ["compact", "function"], "ids": [None, "call:雪/#%"],
    "encodings": ["json_text", "json_value"], "values": [{"n": 1}, {"n": 2}],
    "association": {"basis": "observed_id", "authoritative": False, "candidates": [{"message_id": 1, "call_index": 1}]},
})
def hermes_tools(f):
    path, _ = f.hermes([
        {"id": 1, "role": "assistant", "tool_calls": [
            {"name": "synthetic_tool", "arguments": '{"n":1}'},
            {"id": "call:雪/#%", "type": "function", "function": {"name": "synthetic_tool", "arguments": {"n": 2}}},
        ]},
        {"id": 2, "role": "tool", "tool_call_id": "call:雪/#%", "tool_name": "synthetic_tool", "content": "synthetic result"},
    ])
    rows = probe_hermes(path, SID)["messages"]
    calls = rows[0]["tool_calls"]
    return {"shapes": [c["shape"] for c in calls], "ids": [c["native_call_id"] for c in calls],
            "encodings": [c["arguments"]["encoding"] for c in calls],
            "values": [c["arguments"]["value"] for c in calls], "association": rows[1]["association"]}


def hermes_association(f, mode):
    calls = [{"name": "synthetic_tool", "arguments": {}}, {"name": "synthetic_tool", "arguments": {}}]
    result = {"id": 2, "role": "tool", "tool_name": "synthetic_tool"}
    if mode == "ambiguous_id":
        calls = [{**call, "id": "same:雪/#"} for call in calls]
        result["tool_call_id"] = "same:雪/#"
    elif mode == "unmatched_id":
        result["tool_call_id"] = "absent:雪/#"
    path, _ = f.hermes([{"id": 1, "role": "assistant", "tool_calls": calls}, result])
    return probe_hermes(path, SID)["messages"][1]["association"]


for mode in ("ambiguous_name", "ambiguous_id", "unmatched_id"):
    CASES.append(Case("hermes." + mode, "Retain candidates without merging output or selecting a call",
                      lambda f, mode=mode: hermes_association(f, mode), {
                          "basis": mode, "authoritative": False,
                          "candidates": [] if mode == "unmatched_id" else [
                              {"message_id": 1, "call_index": 0}, {"message_id": 1, "call_index": 1}],
                      }))


@case("hermes.invalid_tool_json", "Malformed JSON, non-array, invalid item, plain-text arguments, absent tool calls", {
    "status": "partial", "states": ["malformed_json", "invalid_tool_calls_shape", "invalid_tool_shape", "ok", "ok"],
    "plain_arguments": {"encoding": "unparsed_text", "value": "synthetic free text"}, "null_calls": [],
})
def hermes_invalid_tools(f):
    path, _ = f.hermes([
        {"id": 1, "role": "assistant", "tool_calls": Raw("[")},
        {"id": 2, "role": "assistant", "tool_calls": {}},
        {"id": 3, "role": "assistant", "tool_calls": [99]},
        {"id": 4, "role": "assistant", "tool_calls": [{"name": "synthetic", "arguments": "synthetic free text"}]},
        {"id": 5, "role": "assistant", "tool_calls": None},
    ])
    value = probe_hermes(path, SID)
    return {"status": value["status"], "states": [m["state"] for m in value["messages"]],
            "plain_arguments": value["messages"][3]["tool_calls"][0]["arguments"],
            "null_calls": value["messages"][4]["tool_calls"]}


@case("hermes.empty_session", "An existing session with zero message rows is distinguished from a missing session", {
    "status": "ok", "messages": [],
})
def hermes_empty(f):
    path, _ = f.hermes()
    value = probe_hermes(path, SID)
    return {"status": value["status"], "messages": value["messages"]}


@case("hermes.null_timestamp_content", "NULL values remain explicit, never synthesized as an epoch or empty text", {
    "status": "partial", "timestamp": None, "content": None, "state": "missing_timestamp",
})
def hermes_nulls(f):
    path, _ = f.hermes([{"id": 1, "timestamp": None, "content": None}])
    value = probe_hermes(path, SID)
    row = value["messages"][0]
    return {"status": value["status"], "timestamp": row["timestamp_seconds"], "content": row["content"], "state": row["state"]}


def hermes_negative(f, mode):
    path, conn = f.hermes([{"id": 1}, {"id": 2}, {"id": 3}])
    limits = Limits()
    if mode == "missing_session":
        conn.execute("DELETE FROM sessions")
    elif mode == "bad_timestamp":
        conn.execute("UPDATE messages SET timestamp = 'not a number' WHERE id = 2")
    elif mode in ("huge_content", "huge_tool_json"):
        column = "content" if mode == "huge_content" else "tool_calls"
        conn.execute(f"UPDATE messages SET {column} = CAST(zeroblob(2097152) AS TEXT) WHERE id = 2")
    elif mode == "rows_limit":
        limits = Limits(rows=2)
    elif mode == "total_bytes":
        conn.execute("UPDATE messages SET content = ?", ("界" * 400,))
        limits = Limits(total_bytes=2500)
    elif mode == "tools_limit":
        conn.execute("UPDATE messages SET role='assistant', tool_calls=? WHERE id=1",
                     (encoded([{"name": "synthetic", "arguments": {}}] * 3),))
        limits = Limits(tool_calls=2)
    else:
        raise AssertionError("unregistered synthetic recipe")
    return probe_hermes(path, SID, limits=limits)


for mode, code, stage in (
    ("missing_session", "missing_session", "hermes.session"),
    ("bad_timestamp", "invalid_timestamp", "hermes.messages"),
    ("huge_content", "cell_limit", "hermes.messages"),
    ("huge_tool_json", "cell_limit", "hermes.messages"),
    ("rows_limit", "row_limit", "hermes.messages"),
    ("total_bytes", "total_byte_limit", "hermes.messages"),
    ("tools_limit", "tool_call_limit", "hermes.tool_calls"),
):
    CASES.append(Case("hermes." + mode, "Isolated negative mutation: " + mode,
                      lambda f, mode=mode: hermes_negative(f, mode), error(code, stage)))
