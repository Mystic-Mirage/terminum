-- SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
-- What sessions spent, by model (terminum-dispatcher.md AC-129): a run hears each model's totals apart, so
-- a session that changed model is priced by what each answer was; a cache write kept an hour is priced apart.
ALTER TABLE usage ADD COLUMN cache_write_1h INTEGER NOT NULL DEFAULT 0;
CREATE TABLE run_seen (
  run TEXT NOT NULL REFERENCES runs,
  model TEXT NOT NULL,
  input INTEGER NOT NULL,
  output INTEGER NOT NULL,
  cache_write INTEGER NOT NULL,
  cache_write_1h INTEGER NOT NULL,
  cache_read INTEGER NOT NULL,
  PRIMARY KEY (run, model)
);
-- what a run heard so far stands as its last model's, so the next report adds only what grew
INSERT INTO run_seen
  SELECT r.id, coalesce((SELECT u.model FROM usage u WHERE u.run = r.id ORDER BY u.at DESC, u.rowid DESC LIMIT 1), ''),
         r.seen_input, r.seen_output, r.seen_write, 0, r.seen_read
  FROM runs r WHERE r.seen_input + r.seen_output + r.seen_write + r.seen_read > 0;
ALTER TABLE runs DROP COLUMN seen_input;
ALTER TABLE runs DROP COLUMN seen_output;
ALTER TABLE runs DROP COLUMN seen_write;
ALTER TABLE runs DROP COLUMN seen_read;
