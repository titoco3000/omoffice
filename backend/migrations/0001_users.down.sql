-- Drops every recorded user. meetings.created_by references this table, so
-- 0002 has to be reverted first.
DROP TABLE users;
