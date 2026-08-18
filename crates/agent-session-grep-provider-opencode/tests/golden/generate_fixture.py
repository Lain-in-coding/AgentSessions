#!/usr/bin/env python3
"""Generate the synthetic OpenCode golden fixture `basic.db`.

Pure-synthetic: no real transcript data. The SQL below is the exact schema and
rows that the provider adapter (`src/lib.rs`) queries. The produced bytes are
committed; `basic.expected.json` pins their BLAKE3 fingerprint.

`time_created` values are epoch **milliseconds** (OpenCode's own unit) inside a
fictional 2026-02-14T09:1x UTC window, so the golden output proves that a
`--since` / `--until` filter actually admits these messages. They are invented
timestamps, not observations of any real session.

Run from this directory:
    python generate_fixture.py
"""

import sqlite3
import sys

DB = "basic.db"

# Fictional epoch-millisecond instants (UTC), fixed so the fixture bytes stay
# byte-stable across regenerations.
SESSION_CREATED = 1771060499000  # 2026-02-14T09:14:59.000Z
SESSION_UPDATED = 1771060800000  # 2026-02-14T09:20:00.000Z
MSG_1 = 1771060500000  # 2026-02-14T09:15:00.000Z
MSG_2 = 1771060504250  # 2026-02-14T09:15:04.250Z
MSG_3 = 1771060560000  # 2026-02-14T09:16:00.000Z (system role — skipped)
MSG_4 = 1771060650750  # 2026-02-14T09:17:30.750Z


def main() -> None:
    conn = sqlite3.connect(DB)
    conn.executescript(
        f"""
        CREATE TABLE session (
            id TEXT PRIMARY KEY,
            title TEXT,
            directory TEXT,
            time_created INTEGER,
            time_updated INTEGER
        );
        CREATE TABLE message (
            id TEXT PRIMARY KEY,
            session_id TEXT,
            data TEXT,
            time_created INTEGER
        );
        CREATE TABLE part (
            id TEXT PRIMARY KEY,
            message_id TEXT,
            data TEXT,
            time_created INTEGER
        );

        INSERT INTO session VALUES ('ses_1', 'synthetic project', '/work/placeholder', {SESSION_CREATED}, {SESSION_UPDATED});

        INSERT INTO message VALUES ('msg_1', 'ses_1', '{{"role":"user"}}', {MSG_1});
        INSERT INTO message VALUES ('msg_2', 'ses_1', '{{"role":"assistant"}}', {MSG_2});
        INSERT INTO message VALUES ('msg_3', 'ses_1', '{{"role":"system"}}', {MSG_3});
        INSERT INTO message VALUES ('msg_4', 'ses_1', '{{"role":"user"}}', {MSG_4});

        INSERT INTO part VALUES ('part_1', 'msg_1', '{{"type":"text","text":"hello world"}}', {MSG_1});
        INSERT INTO part VALUES ('part_2', 'msg_1', '{{"type":"text","text":"中文 second line"}}', {MSG_1 + 500});
        INSERT INTO part VALUES ('part_3', 'msg_2', '{{"type":"text","text":"hi there"}}', {MSG_2});
        INSERT INTO part VALUES ('part_4', 'msg_4', '{{"type":"text","text":"final user question"}}', {MSG_4});
        """
    )
    conn.commit()
    conn.close()
    print(f"wrote {DB}")


if __name__ == "__main__":
    sys.exit(main())
