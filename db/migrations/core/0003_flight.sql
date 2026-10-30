-- 0003: AIPS Flight trajectory summaries (additive; rollback: DROP TABLE flight_summaries).
CREATE TABLE IF NOT EXISTS flight_summaries (
  flight_id   text PRIMARY KEY REFERENCES flights(id),
  tail        text NOT NULL REFERENCES aircraft(tail),
  doc         jsonb NOT NULL,
  received_at timestamptz NOT NULL
);
CREATE INDEX IF NOT EXISTS flight_summaries_tail_idx ON flight_summaries (tail, received_at);
