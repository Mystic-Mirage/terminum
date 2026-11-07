-- SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
-- A run holds one task or several (terminum-dispatcher.md AC-138); an agent has its effort and color,
-- and a run keeps its agent's name and color: a finished card stays on them when the agent is gone.
CREATE TABLE run_tasks (
  run TEXT NOT NULL REFERENCES runs,
  task TEXT NOT NULL REFERENCES tasks,
  result INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (run, task)
);
CREATE INDEX run_tasks_task ON run_tasks (task);
INSERT INTO run_tasks (run, task, result) SELECT id, task, result FROM runs;
-- what a human's move held stays theirs: no walk takes it up again
UPDATE runs SET state = 'left' WHERE held = 1 AND state NOT IN ('closed', 'left');
ALTER TABLE runs DROP COLUMN result;
ALTER TABLE runs DROP COLUMN held;
ALTER TABLE project_agents ADD COLUMN effort TEXT NOT NULL DEFAULT '';
ALTER TABLE project_agents ADD COLUMN color INTEGER NOT NULL DEFAULT 0;
UPDATE project_agents SET color = rowid;
ALTER TABLE runs ADD COLUMN agent_name TEXT NOT NULL DEFAULT '';
ALTER TABLE runs ADD COLUMN agent_color INTEGER NOT NULL DEFAULT 0;
UPDATE runs SET agent_name = COALESCE((SELECT a.name FROM project_agents a WHERE a.id = runs.agent_id), ''),
                agent_color = COALESCE((SELECT a.color FROM project_agents a WHERE a.id = runs.agent_id), 0);
