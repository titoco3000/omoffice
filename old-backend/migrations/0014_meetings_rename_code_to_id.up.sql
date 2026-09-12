-- hand-written: schema.rs's Column::references() target-column changes
-- (users.last_meeting, meeting_attendees.code, active_users.meeting,
-- meeting_invites.code, all now -> meetings(id)) need no DDL of their own --
-- RENAME COLUMN relinks a table's own existing foreign keys by column
-- position, not name, so this one statement covers all of them too.
ALTER TABLE meetings RENAME COLUMN code TO id;
