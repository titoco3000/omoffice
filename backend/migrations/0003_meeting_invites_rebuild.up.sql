-- Hand-written, not generated: DSQL's ALTER TABLE ADD COLUMN refuses any
-- constraint on the new column (NOT NULL, a foreign key, ...), so turning
-- `email` into `invited_user_id text NOT NULL REFERENCES users (id)` can't
-- be an ALTER at all -- see core::data::orm::diff's own doc. The table has
-- carried no real invites in any environment yet, so a drop and recreate
-- costs nothing.
DROP TABLE meeting_invites;
CREATE TABLE meeting_invites (
    id text PRIMARY KEY,
    meeting_id text NOT NULL REFERENCES meetings (id),
    invited_user_id text NOT NULL REFERENCES users (id),
    invited_by text NOT NULL REFERENCES users (id),
    created_at timestamptz NOT NULL DEFAULT now()
);
