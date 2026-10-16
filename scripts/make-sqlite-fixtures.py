#!/usr/bin/env python3
"""Generates the SQLite fixtures read by the tests in src/sqlite.rs.

Run from anywhere:  python -I scripts/make-sqlite-fixtures.py
Writes into tests/fixtures/:
  small.db                 the two Cursor keys (token as TEXT, plan as BLOB) plus a few other rows
  big-1024.db, big-4096.db ~2000 rows (every 13th deleted), interior pages in both sqlite_master and
                           ItemTable, a 20000-byte BLOB and a 9000-char TEXT (overflow chains) and a
                           3005-char key (a key that itself overflows)
  wal.db, wal.db-wal       WAL-mode db whose newest values exist only in the -wal file, followed by
                           uncommitted (spilled) frames that must be ignored
All tokens are fake.
"""
import base64
import json
import os
import shutil
import sqlite3
import tempfile
from pathlib import Path

OUT = Path(__file__).resolve().parent.parent / "tests" / "fixtures"
SCHEMA = "CREATE TABLE ItemTable (key TEXT UNIQUE ON CONFLICT REPLACE, value BLOB)"
TOKEN_KEY = "cursorAuth/accessToken"
PLAN_KEY = "cursorAuth/stripeMembershipType"


def b64url(obj):
    return base64.urlsafe_b64encode(json.dumps(obj, separators=(",", ":")).encode()).rstrip(b"=").decode()


# A fake, unsigned JWT: {"sub": "auth0|user_fixture", "exp": 4102444800}.
FAKE_TOKEN = ".".join([b64url({"alg": "none", "typ": "JWT"}), b64url({"sub": "auth0|user_fixture", "exp": 4102444800}), "fixture"])


def value(i):
    return f"value-{i}-" * (i % 7 + 1)


def fresh(path):
    for suffix in ("", "-wal", "-shm", "-journal"):
        p = Path(str(path) + suffix)
        if p.exists():
            p.unlink()


def make_small():
    path = OUT / "small.db"
    fresh(path)
    con = sqlite3.connect(path)
    con.execute(SCHEMA)
    con.executemany("INSERT INTO ItemTable VALUES (?, ?)", [
        ("workbench.panel.width", "300"),
        (TOKEN_KEY, FAKE_TOKEN),
        (PLAN_KEY, b"pro"),
        ("number", 42),
        ("nothing", None),
    ])
    con.commit()
    con.close()


def make_big(page_size):
    path = OUT / f"big-{page_size}.db"
    fresh(path)
    con = sqlite3.connect(path)
    con.execute(f"PRAGMA page_size={page_size}")
    con.execute("PRAGMA journal_mode=DELETE")
    # Enough schema rows that sqlite_master (page 1) needs interior pages too.
    for t in range(150):
        con.execute(f"CREATE TABLE extra_table_{t:03d} (id INTEGER PRIMARY KEY, a_rather_long_column_name_{t} TEXT, b TEXT, c BLOB)")
    con.execute(SCHEMA)
    long_key = "long/" + "k" * 3000
    for i in range(2000):
        con.execute("INSERT INTO ItemTable VALUES (?, ?)", (f"key/{i:04d}", value(i)))
        if i == 1000:
            con.execute("INSERT INTO ItemTable VALUES (?, ?)", ("huge/blob", bytes((j * 7 + 3) % 251 for j in range(20000))))
            con.execute("INSERT INTO ItemTable VALUES (?, ?)", ("huge/text", "abcdefghij" * 900))
            con.execute("INSERT INTO ItemTable VALUES (?, ?)", (long_key, "long-key-value"))
    con.execute("DELETE FROM ItemTable WHERE key IN (%s)" % ",".join("?" * len(range(0, 2000, 13))),
                [f"key/{i:04d}" for i in range(0, 2000, 13)])
    con.commit()
    con.close()


def read_back(db, wal=None):
    """Reads the keys through SQLite itself from a scratch copy (opening a WAL db may checkpoint it)."""
    with tempfile.TemporaryDirectory() as d:
        copy = Path(d) / "x.db"
        shutil.copyfile(db, copy)
        if wal:
            shutil.copyfile(wal, str(copy) + "-wal")
        con = sqlite3.connect(copy)
        rows = dict(con.execute("SELECT key, value FROM ItemTable WHERE key IN (?, ?, ?)", (TOKEN_KEY, PLAN_KEY, "wal/only")))
        con.close()
        return rows


def make_wal():
    path = OUT / "wal.db"
    fresh(path)
    with tempfile.TemporaryDirectory() as d:
        live = Path(d) / "wal.db"
        con = sqlite3.connect(live, isolation_level=None)
        con.execute("PRAGMA page_size=4096")
        assert con.execute("PRAGMA journal_mode=WAL").fetchone()[0] == "wal"
        con.execute("PRAGMA wal_autocheckpoint=0")
        con.execute(SCHEMA)
        con.execute("INSERT INTO ItemTable VALUES (?, ?)", (TOKEN_KEY, "old-token"))
        con.execute("INSERT INTO ItemTable VALUES (?, ?)", (PLAN_KEY, "free"))
        for i in range(300):
            con.execute("INSERT INTO ItemTable VALUES (?, ?)", (f"filler/{i:04d}", value(i)))
        con.execute("PRAGMA wal_checkpoint(TRUNCATE)")
        # Committed after the checkpoint: these exist only in the WAL.
        con.execute("UPDATE ItemTable SET value = ? WHERE key = ?", ("new-1", TOKEN_KEY))
        con.execute("UPDATE ItemTable SET value = ? WHERE key = ?", ("new-2", TOKEN_KEY))
        con.execute("BEGIN")
        con.execute("UPDATE ItemTable SET value = ? WHERE key = ?", ("pro-wal", PLAN_KEY))
        con.execute("INSERT INTO ItemTable VALUES (?, ?)", ("wal/only", "only in wal"))
        con.execute("COMMIT")
        committed_size = os.path.getsize(str(live) + "-wal")
        # An open transaction big enough to spill uncommitted frames into the WAL.
        con.execute("PRAGMA cache_size=2")
        con.execute("BEGIN")
        con.execute("UPDATE ItemTable SET value = ? WHERE key = ?", ("uncommitted", TOKEN_KEY))
        for i in range(3000):
            con.execute("INSERT INTO ItemTable VALUES (?, ?)", (f"spill/{i:05d}", "x" * 200))
        spilled_size = os.path.getsize(str(live) + "-wal")
        # Copy while the connection (and transaction) is still open, so the WAL is not checkpointed away.
        shutil.copyfile(live, path)
        shutil.copyfile(str(live) + "-wal", str(path) + "-wal")
        con.execute("ROLLBACK")
        con.close()
    assert spilled_size > committed_size, "expected uncommitted frames to spill into the WAL"
    main_only = read_back(path)
    assert main_only == {TOKEN_KEY: "old-token", PLAN_KEY: "free"}, main_only
    with_wal = read_back(path, str(path) + "-wal")
    assert with_wal == {TOKEN_KEY: "new-2", PLAN_KEY: "pro-wal", "wal/only": "only in wal"}, with_wal


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    make_small()
    make_big(1024)
    make_big(4096)
    make_wal()
    for p in sorted(OUT.iterdir()):
        print(f"{p.name:16} {p.stat().st_size:>9} bytes")


if __name__ == "__main__":
    main()
