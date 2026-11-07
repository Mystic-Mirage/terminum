-- SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
-- What sessions spent (terminum-dispatcher.md AC-129, AC-143): a row per report that grew, and what each run has
-- heard so far, so a repeated or resumed report counts once.
ALTER TABLE runs ADD COLUMN seen_input INTEGER NOT NULL DEFAULT 0;
ALTER TABLE runs ADD COLUMN seen_output INTEGER NOT NULL DEFAULT 0;
ALTER TABLE runs ADD COLUMN seen_write INTEGER NOT NULL DEFAULT 0;
ALTER TABLE runs ADD COLUMN seen_read INTEGER NOT NULL DEFAULT 0;
CREATE TABLE usage (
  run TEXT NOT NULL REFERENCES runs,
  agent_id TEXT NOT NULL,
  at INTEGER NOT NULL,
  input INTEGER NOT NULL,
  output INTEGER NOT NULL,
  cache_write INTEGER NOT NULL,
  cache_read INTEGER NOT NULL,
  model TEXT
);
CREATE INDEX usage_run ON usage (run);
CREATE INDEX usage_at ON usage (at);
