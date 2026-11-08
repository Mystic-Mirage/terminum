-- SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
-- The input log (terminum.md AC-147): a row per write into a pane, by whom, from where, into what.
CREATE TABLE inputs (
  id INTEGER PRIMARY KEY,
  at INTEGER NOT NULL,
  ended INTEGER NOT NULL,
  sender TEXT NOT NULL REFERENCES users,
  component TEXT NOT NULL,
  kind TEXT NOT NULL,
  text TEXT NOT NULL,
  bytes INTEGER NOT NULL,
  pane TEXT NOT NULL,
  machine TEXT,
  session TEXT,
  window TEXT,
  program TEXT,
  cwd TEXT,
  project TEXT,
  project_name TEXT,
  run TEXT,
  tasks TEXT,
  agent TEXT,
  page INTEGER,
  device TEXT,
  ip TEXT,
  browser TEXT,
  tailnet_user TEXT,
  version TEXT NOT NULL,
  error TEXT
);
CREATE INDEX inputs_at ON inputs (at);
CREATE INDEX inputs_pane ON inputs (pane, at);
