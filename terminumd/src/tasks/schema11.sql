-- SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
-- A task's ID by its project (terminum-kanban.md AC-148): TERM-7 from the project's prefix and its own count,
-- every ID it had still found (task_slugs, "T-12" too); when a task, a comment was made and changed
-- (AC-149); the project's template for agents' tasks (AC-145). slugs.rs fills prefixes and slugs.
ALTER TABLE projects ADD COLUMN prefix TEXT;
ALTER TABLE projects ADD COLUMN next_seq INTEGER NOT NULL DEFAULT 1;
ALTER TABLE projects ADD COLUMN task_template TEXT;
CREATE UNIQUE INDEX projects_prefix ON projects (prefix);
UPDATE projects SET prefix = 'TASK' WHERE id = 'default';
ALTER TABLE tasks ADD COLUMN slug TEXT;
CREATE UNIQUE INDEX tasks_slug ON tasks (slug);
ALTER TABLE tasks ADD COLUMN created_at INTEGER NOT NULL DEFAULT 0;
UPDATE tasks SET created_at = coalesce((SELECT min(e.at) FROM events e WHERE e.task = tasks.id), updated_at);
ALTER TABLE events ADD COLUMN edited_at INTEGER;
CREATE INDEX tasks_made_in ON tasks (made_in);   -- the tasks a session made (AC-150)
CREATE TABLE task_slugs (slug TEXT PRIMARY KEY, task TEXT NOT NULL REFERENCES tasks) WITHOUT ROWID;
INSERT INTO task_slugs (slug, task) SELECT 'T-' || number, id FROM tasks;
