-- Small key/value store: the machine id that names this machine's sync ref, and future settings.

CREATE TABLE meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
