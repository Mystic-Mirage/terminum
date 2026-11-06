-- SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
-- The tasks' store (terminum-kanban.md AC-119).
CREATE TABLE users (
  id TEXT PRIMARY KEY,
  kind TEXT NOT NULL CHECK (kind IN ('human', 'agent', 'system')),
  name TEXT NOT NULL,
  role TEXT NOT NULL
);
CREATE TABLE projects (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  color INTEGER NOT NULL,
  host TEXT,
  dir TEXT NOT NULL,
  place INTEGER NOT NULL,
  is_default INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE project_members (
  project TEXT NOT NULL REFERENCES projects ON DELETE CASCADE,
  pane_key TEXT NOT NULL UNIQUE,
  place INTEGER NOT NULL
);
CREATE TABLE list_order (place INTEGER PRIMARY KEY, part TEXT NOT NULL);
CREATE TABLE servers (start TEXT PRIMARY KEY, terminum_id TEXT NOT NULL);
CREATE TABLE tasks (
  id TEXT PRIMARY KEY,
  number INTEGER NOT NULL UNIQUE,
  project TEXT NOT NULL REFERENCES projects,
  parent TEXT REFERENCES tasks,
  title TEXT NOT NULL,
  body TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('TODO', 'IN PROGRESS', 'WAIT FOR HUMAN', 'FAILED', 'DONE')),
  severity TEXT NOT NULL CHECK (severity IN ('P0', 'P1', 'P2', 'P3')),
  rank REAL NOT NULL,
  owner TEXT NOT NULL REFERENCES users,
  by_agent INTEGER NOT NULL DEFAULT 0,
  session_key TEXT,
  session_server TEXT,
  agent_session TEXT,
  budget_seconds INTEGER,
  archived_at INTEGER,
  updated_at INTEGER NOT NULL
);
CREATE INDEX tasks_board ON tasks (archived_at, status, severity, rank);
CREATE INDEX tasks_project ON tasks (project);
CREATE INDEX tasks_session ON tasks (session_key);
CREATE TABLE events (
  id TEXT PRIMARY KEY,
  task TEXT NOT NULL REFERENCES tasks,
  at INTEGER NOT NULL,
  author TEXT NOT NULL REFERENCES users,
  kind TEXT NOT NULL,
  text TEXT,
  data TEXT
);
CREATE INDEX events_task ON events (task, at);
CREATE TABLE files (
  id TEXT PRIMARY KEY,
  task TEXT NOT NULL REFERENCES tasks,
  event TEXT REFERENCES events,
  name TEXT NOT NULL,
  bytes INTEGER NOT NULL,
  mime TEXT NOT NULL
);
CREATE INDEX files_task ON files (task);
CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
INSERT INTO users (id, kind, name, role) VALUES
  ('human', 'human', '', 'admin'),
  ('dispatcher', 'system', 'Dispatcher', 'system');
INSERT INTO projects (id, name, color, dir, place, is_default) VALUES ('default', 'Default', -1, '', -1, 1);
