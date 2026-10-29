-- Tool events keep both output streams, each stored by hash.
--
-- `output_hash` is stdout; this migration adds the stderr counterpart.

ALTER TABLE tool_events ADD COLUMN stderr_hash TEXT;
