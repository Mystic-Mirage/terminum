#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
"""AS-112's tasks in a test's store: 5,000 open and 100,000 archived (e2e_liveboardedge.mjs's budgets)."""
import secrets
import sqlite3
import sys

COLUMNS = ["TODO", "IN PROGRESS", "WAIT FOR HUMAN", "FAILED", "DONE"]
SEVERITIES = ["P0", "P1", "P2", "P3"]
NOW = 1700000000


def rows(start):
    for i in range(5000):
        yield (secrets.token_hex(16), start + i + 1, "default", COLUMNS[i % 5], SEVERITIES[i % 4], float(i), "human", NOW, None, f"load task {i}", "b" * 2000)
    for i in range(100000):
        yield (secrets.token_hex(16), start + 5001 + i, "default", "DONE", "P2", float(i), "human", NOW, NOW, f"archived {i}", "b" * 500)


def main():
    store = sqlite3.connect(sys.argv[1])
    store.execute("PRAGMA busy_timeout=8000")
    start = store.execute("SELECT coalesce(max(number), 0) FROM tasks").fetchone()[0]
    store.executemany("INSERT INTO tasks (id, number, project, status, severity, rank, owner, updated_at, archived_at, title, body) VALUES (?,?,?,?,?,?,?,?,?,?,?)", rows(start))
    store.commit()


if __name__ == "__main__":
    main()
