-- KAP does not name the harness that ran a session.
--
-- The hooks are the interface; which CLI produced them is not KAP's business. The column is
-- dropped rather than left unused, so nothing can quietly start filling it again.
--
-- SQLite 3.35+ supports DROP COLUMN; the bundled SQLite is far newer than that.

ALTER TABLE runs DROP COLUMN agent;
