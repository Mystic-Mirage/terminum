-- SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
-- A run knows its project, and may have no task: + Session (terminum-dispatcher.md AC-142, AC-144). Columns
-- change in place: the rows, their order and run_tasks' foreign key stand.
ALTER TABLE runs ADD COLUMN lead TEXT REFERENCES tasks;
ALTER TABLE runs ADD COLUMN project TEXT NOT NULL DEFAULT 'default';
UPDATE runs SET lead = task, project = COALESCE((SELECT t.project FROM tasks t WHERE t.id = runs.task), 'default');
DROP INDEX runs_task;
ALTER TABLE runs DROP COLUMN task;
ALTER TABLE runs RENAME COLUMN lead TO task;
CREATE INDEX runs_task ON runs (task);
