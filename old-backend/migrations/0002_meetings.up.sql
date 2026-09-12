-- Meetings created through POST /private/meetings. `code` is the Chime
-- MeetingId, which is already a UUID and unique per meeting, so it is the
-- primary key: no sequence, and no secondary index to keep it unique.
--
-- The foreign key is real -- DSQL has supported them since August 2026 -- so a
-- meeting cannot name an email that never asked for a key.
CREATE TABLE meetings (
    code       text        PRIMARY KEY,
    created_by text        NOT NULL REFERENCES users (email),
    created_at timestamptz NOT NULL DEFAULT now()
);
