-- SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
-- Who runs a task's session (terminum-dispatcher.md §9).
CREATE TABLE runs (
  id TEXT PRIMARY KEY,
  task TEXT NOT NULL REFERENCES tasks,
  number INTEGER NOT NULL,
  agent_id TEXT NOT NULL,
  kind TEXT NOT NULL,
  command TEXT NOT NULL,
  host TEXT,
  folder TEXT NOT NULL,
  pane_key TEXT,
  agent_session TEXT,
  state TEXT NOT NULL,
  prompt TEXT,
  hooked INTEGER NOT NULL DEFAULT 0,
  turn_ended INTEGER NOT NULL DEFAULT 0,
  open_tools INTEGER NOT NULL DEFAULT 0,
  idle_for INTEGER NOT NULL DEFAULT 0,
  waited_for INTEGER NOT NULL DEFAULT 0,
  closing_for INTEGER NOT NULL DEFAULT 0,
  budget_used INTEGER NOT NULL DEFAULT 0,
  nudged INTEGER NOT NULL DEFAULT 0,
  nudge_turn INTEGER NOT NULL DEFAULT 0,
  warned INTEGER NOT NULL DEFAULT 0,
  held INTEGER NOT NULL DEFAULT 0,
  wait_moved INTEGER NOT NULL DEFAULT 0,
  no_result_wait INTEGER NOT NULL DEFAULT 0,
  result INTEGER NOT NULL DEFAULT 0,
  ended_said INTEGER NOT NULL DEFAULT 0,
  last_walk INTEGER NOT NULL DEFAULT 0,
  started_at INTEGER NOT NULL
);
CREATE INDEX runs_task ON runs (task, number);
CREATE INDEX runs_pane ON runs (pane_key, state);
