# A simple, key-value cache.
# - Concurrency safe
# - Handles eviction

import os
import sqlite3
from typing import Optional

_handle: Optional[sqlite3.Connection] = None
CACHE_SIZE = 1000
CURRENT_VERSION = 1


def _db_conn() -> sqlite3.Connection:
    global _handle
    fn = os.path.expanduser('~/.ghstackcache')
    if not _handle:
        handle = sqlite3.connect(fn)
        user_version = handle.execute("PRAGMA user_version").fetchone()
        if user_version is None or user_version[0] != CURRENT_VERSION:
            handle.close()
            os.remove(fn)
            handle = sqlite3.connect(fn)
            handle.execute("""
            CREATE TABLE ghstack_cache (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                domain TEXT,
                key TEXT,
                value TEXT
            )
            """)
            handle.execute("""
            CREATE UNIQUE INDEX domain_key ON ghstack_cache (domain, key)
            """)
            handle.execute("PRAGMA user_version = {}".format(CURRENT_VERSION))
            handle.commit()
        _handle = handle
    return _handle


def get(domain: str, key: str) -> Optional[str]:
    conn = _db_conn()
    c = conn.execute(
        "SELECT value FROM ghstack_cache WHERE domain = ? AND key = ?",
        (domain, key))
    r = c.fetchone()
    if r is None:
        return None
    r = r[0]
    assert isinstance(r, str)
    return r


def put(domain: str, key: str, value: str) -> None:
    conn = _db_conn()
    conn.execute(
        "UPDATE ghstack_cache SET value = ? WHERE domain = ? AND key = ?",
        (value, domain, key))
    c = conn.execute(
        """
        INSERT INTO ghstack_cache (domain, key, value)
        SELECT ?, ?, ? WHERE (SELECT Changes() = 0)
        """,
        (domain, key, value))
    if c.lastrowid is not None:
        conn.execute(
            "DELETE FROM ghstack_cache WHERE id < ?", (c.lastrowid - CACHE_SIZE, ))
    conn.commit()
