-- hand-written -- `make db-generate` refuses to emit this one: dropping
-- NOT NULL from an existing column has no expressible down-migration (DSQL
-- has no ALTER COLUMN ... SET NOT NULL), and core::orm's render() panics on
-- Create(Condition::NotNull) rather than pretend otherwise. See
-- schema.rs's CONNECTIONS doc comment for why org_id is nullable now.
ALTER TABLE connections ALTER COLUMN org_id DROP NOT NULL;
