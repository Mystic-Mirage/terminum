-- SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
-- The message history (terminum.md AC-147, terminum-terminal.md AC-90): a row per message sent in a pane,
-- a shell's command at Enter or a coding agent's prompt, by whom, into what; searched by its words (FTS5, BM25).
CREATE TABLE messages (
  id INTEGER PRIMARY KEY,
  at INTEGER NOT NULL,
  sender TEXT NOT NULL REFERENCES users,
  via TEXT NOT NULL,
  agent TEXT,
  pane TEXT NOT NULL,
  line INTEGER,
  host TEXT NOT NULL,
  cwd TEXT NOT NULL,
  title TEXT,
  project TEXT,
  project_name TEXT,
  text TEXT NOT NULL
);
CREATE INDEX messages_at ON messages (at);
CREATE INDEX messages_folder ON messages (host, cwd, at);
CREATE INDEX messages_pane ON messages (pane, at);
CREATE VIRTUAL TABLE messages_fts USING fts5(text, content='messages', content_rowid='id', tokenize='unicode61 remove_diacritics 2');
CREATE TRIGGER messages_ai AFTER INSERT ON messages BEGIN
  INSERT INTO messages_fts (rowid, text) VALUES (new.id, new.text);
END;
CREATE TRIGGER messages_ad AFTER DELETE ON messages BEGIN
  INSERT INTO messages_fts (messages_fts, rowid, text) VALUES ('delete', old.id, old.text);
END;
CREATE TRIGGER messages_au AFTER UPDATE OF text ON messages BEGIN
  INSERT INTO messages_fts (messages_fts, rowid, text) VALUES ('delete', old.id, old.text);
  INSERT INTO messages_fts (rowid, text) VALUES (new.id, new.text);
END;
