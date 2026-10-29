-- A merge is only allowed after a review, and only of the commit that was reviewed.
--
-- The review stores the branch head it approved, so a later commit invalidates the approval
-- instead of slipping through under an old one.

CREATE TABLE reviews (
    id          TEXT PRIMARY KEY,
    attempt_id  TEXT NOT NULL REFERENCES attempts(id) ON DELETE CASCADE,
    verdict     TEXT NOT NULL CHECK (verdict IN ('approved', 'rejected')),
    reviewer    TEXT NOT NULL,
    head_commit TEXT NOT NULL,
    note        TEXT,
    created_at  TEXT NOT NULL
);

CREATE INDEX reviews_attempt ON reviews(attempt_id, created_at);
