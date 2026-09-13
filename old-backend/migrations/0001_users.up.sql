-- People who have asked for an API key. Keyed by the email that
-- /auth/request-key/{email} already hands out tokens for, so there is no
-- separate id to reconcile.
--
-- DSQL fixes the primary key at CREATE TABLE time -- it cannot be added or
-- changed later -- so every table declares one here.
CREATE TABLE users (
    email      text        PRIMARY KEY,
    created_at timestamptz NOT NULL DEFAULT now()
);
