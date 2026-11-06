-- SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
-- A project's agents (terminum-dispatcher.md AC-137).
CREATE TABLE project_agents (
  id TEXT PRIMARY KEY,
  project TEXT NOT NULL REFERENCES projects ON DELETE CASCADE,
  name TEXT NOT NULL,
  kind TEXT NOT NULL CHECK (kind IN ('claude', 'codex')),
  host TEXT,
  dir TEXT NOT NULL,
  command TEXT NOT NULL,
  UNIQUE (project, name)
);
