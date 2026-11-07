-- SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
-- A project's memory limits, and a run told of them once (terminum-dispatcher.md AC-128).
CREATE TABLE project_limits (
  project TEXT PRIMARY KEY REFERENCES projects ON DELETE CASCADE,
  warn_gib REAL NOT NULL,
  end_gib REAL NOT NULL
);
ALTER TABLE runs ADD COLUMN memory_warned INTEGER NOT NULL DEFAULT 0;
