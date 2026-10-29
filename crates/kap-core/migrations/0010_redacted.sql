-- A record whose output was redacted says so, instead of looking like a command that simply
-- printed a placeholder.

ALTER TABLE tool_events ADD COLUMN redacted INTEGER NOT NULL DEFAULT 0;
