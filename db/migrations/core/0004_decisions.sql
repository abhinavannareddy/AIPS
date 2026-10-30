-- 0004: additional human decisions (alert acknowledgements, record corrections, pre-departure
-- escalations, erasure requests, active model version). Additive; rollback: DROP TABLE decision_docs.
CREATE TABLE IF NOT EXISTS decision_docs (
  kind  text NOT NULL,
  key   text NOT NULL,
  doc   jsonb NOT NULL,
  at    timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (kind, key)
);
-- Record corrections are append-only: a correction is never edited, a newer version supersedes it.
CREATE OR REPLACE FUNCTION decision_docs_corrections_immutable() RETURNS trigger AS $$
BEGIN
  IF OLD.kind = 'correction' THEN RAISE EXCEPTION 'record corrections are append-only'; END IF;
  RETURN COALESCE(NEW, OLD);
END $$ LANGUAGE plpgsql;
DROP TRIGGER IF EXISTS decision_docs_corrections_guard ON decision_docs;
CREATE TRIGGER decision_docs_corrections_guard BEFORE UPDATE OR DELETE ON decision_docs
  FOR EACH ROW EXECUTE FUNCTION decision_docs_corrections_immutable();
