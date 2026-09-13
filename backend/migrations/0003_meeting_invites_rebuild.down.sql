-- Hand-written, not generated -- see the .up.sql pair.
DROP TABLE meeting_invites;
CREATE TABLE meeting_invites (
    id text PRIMARY KEY,
    meeting_id text NOT NULL REFERENCES meetings (id),
    email text NOT NULL,
    invited_by text NOT NULL REFERENCES users (id),
    status text NOT NULL DEFAULT 'pending',
    created_at timestamptz NOT NULL DEFAULT now(),
    last_status_change timestamptz NOT NULL DEFAULT now()
);
