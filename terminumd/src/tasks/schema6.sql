-- SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
-- A pool of agents, no project's (terminum-dispatcher.md AC-137): what starts is the agent's, where is the
-- project's (AC-140), and what a run started with stays on the run. A name two projects used stays unique.
CREATE TABLE agents (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL UNIQUE,
  kind TEXT NOT NULL CHECK (kind IN ('claude', 'codex')),
  command TEXT NOT NULL,
  effort TEXT NOT NULL DEFAULT '',
  color INTEGER NOT NULL,
  at_once INTEGER NOT NULL DEFAULT 1,
  week_limit REAL,
  month_limit REAL
);
INSERT INTO agents (id, name, kind, command, effort, color)
  SELECT a.id,
         CASE WHEN EXISTS (SELECT 1 FROM project_agents o WHERE o.name = a.name AND o.rowid < a.rowid) THEN substr(a.name, 1, 30) || ' (' || a.rowid || ')' ELSE a.name END,
         a.kind, a.command, a.effort, a.rowid
  FROM project_agents a ORDER BY a.rowid;
DROP TABLE project_agents;
ALTER TABLE projects ADD COLUMN agent_limit INTEGER;
UPDATE projects SET dir = '~' WHERE is_default = 1;
