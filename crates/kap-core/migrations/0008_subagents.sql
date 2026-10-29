-- Claude Code runs subagents inside one session; their tool calls carry the subagent's identity.
--
-- Recording it separates "the main agent did this" from "a subagent did this" without changing
-- how a run is stored.

ALTER TABLE tool_events ADD COLUMN agent_id TEXT;
ALTER TABLE tool_events ADD COLUMN agent_type TEXT;
