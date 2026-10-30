-- AIPS emergency service schema. Separate database from analytics (EMG-01).
-- The local disk journal is authoritative during outages; this table is a durable copy.
CREATE TABLE IF NOT EXISTS incidents (
  id       text PRIMARY KEY,
  doc      jsonb NOT NULL,
  updated  timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS incidents_updated_idx ON incidents (updated DESC);
