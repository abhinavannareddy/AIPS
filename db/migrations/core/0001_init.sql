-- AIPS core schema (PostgreSQL 16 + TimescaleDB). Idempotent.
-- Operational records are relational; parameter samples are a TimescaleDB hypertable.

DO $$ BEGIN
  CREATE EXTENSION IF NOT EXISTS timescaledb;
EXCEPTION WHEN OTHERS THEN
  RAISE NOTICE 'timescaledb extension not available; param_samples stays a plain table';
END $$;

CREATE TABLE IF NOT EXISTS replay_meta (
  id            smallint PRIMARY KEY DEFAULT 1 CHECK (id = 1),
  seed          bigint      NOT NULL,
  generated_at  timestamptz NOT NULL,
  truth         jsonb       NOT NULL,
  departures    jsonb       NOT NULL
);

CREATE TABLE IF NOT EXISTS aircraft (
  tail            text PRIMARY KEY,
  type_code       text NOT NULL,
  msn             integer NOT NULL,
  operator        text NOT NULL,
  lessor          text,
  engines         smallint NOT NULL,
  delivered       date NOT NULL,
  base            text NOT NULL,
  data_feed_last  timestamptz NOT NULL
);

CREATE TABLE IF NOT EXISTS components (
  id               text PRIMARY KEY,
  tail             text NOT NULL REFERENCES aircraft(tail),
  part_number      text NOT NULL,
  serial           text NOT NULL,
  ata              smallint NOT NULL,
  position         text NOT NULL,
  description      text NOT NULL,
  installed_at     timestamptz NOT NULL,
  cycles_since_sv  integer NOT NULL
);
CREATE INDEX IF NOT EXISTS components_serial_idx ON components (part_number, serial);

CREATE TABLE IF NOT EXISTS flights (
  id      text PRIMARY KEY,
  tail    text NOT NULL REFERENCES aircraft(tail),
  number  text NOT NULL,
  origin  text NOT NULL,
  dest    text NOT NULL,
  dep     timestamptz NOT NULL,
  arr     timestamptz NOT NULL,
  cycle   integer NOT NULL
);
CREATE INDEX IF NOT EXISTS flights_tail_dep_idx ON flights (tail, dep);

CREATE TABLE IF NOT EXISTS fault_events (
  id            text PRIMARY KEY,
  tail          text NOT NULL REFERENCES aircraft(tail),
  flight_id     text,
  ts            timestamptz NOT NULL,
  ata           smallint NOT NULL,
  system        text NOT NULL,
  code          text NOT NULL,
  component_id  text,
  severity      text NOT NULL,
  message       text NOT NULL,
  source        text NOT NULL,
  ingested_at   timestamptz NOT NULL
);
CREATE INDEX IF NOT EXISTS fault_events_tail_code_idx ON fault_events (tail, code, ts);

CREATE TABLE IF NOT EXISTS work_orders (
  id            text PRIMARY KEY,
  tail          text NOT NULL REFERENCES aircraft(tail),
  event_ids     text[] NOT NULL,
  component_id  text,
  ata           smallint NOT NULL,
  action        text NOT NULL,
  opened        timestamptz NOT NULL,
  closed        timestamptz,
  signed_off    boolean NOT NULL,
  team          text NOT NULL,
  note          text NOT NULL
);

CREATE TABLE IF NOT EXISTS mel_items (
  id         text PRIMARY KEY,
  tail       text NOT NULL REFERENCES aircraft(tail),
  reference  text NOT NULL,
  title      text NOT NULL,
  category   char(1) NOT NULL,
  ata        smallint NOT NULL,
  opened     timestamptz NOT NULL,
  due        timestamptz NOT NULL
);

CREATE TABLE IF NOT EXISTS param_samples (
  ts         timestamptz NOT NULL,
  tail       text NOT NULL,
  param      text NOT NULL,
  flight_id  text NOT NULL,
  value      double precision NOT NULL
);
CREATE INDEX IF NOT EXISTS param_samples_tail_param_ts_idx ON param_samples (tail, param, ts DESC);

DO $$ BEGIN
  IF EXISTS (SELECT 1 FROM pg_extension WHERE extname = 'timescaledb') THEN
    PERFORM create_hypertable('param_samples', 'ts', chunk_time_interval => interval '7 days', if_not_exists => true);
  END IF;
END $$;

-- Human decisions: the system of record (P1, CFM-03, INT-06).
CREATE TABLE IF NOT EXISTS hypothesis_grades (
  hypothesis_id  text PRIMARY KEY,
  grade          text NOT NULL,
  graded_by      text NOT NULL,
  graded_at      timestamptz NOT NULL,
  note           text NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS alert_feedback (
  alert_id  text PRIMARY KEY,
  useful    boolean NOT NULL,
  comment   text NOT NULL,
  by_user   text NOT NULL,
  at        timestamptz NOT NULL
);

CREATE TABLE IF NOT EXISTS case_state (
  case_id  text PRIMARY KEY,
  state    jsonb NOT NULL,
  updated  timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS preflight_acks (
  flight_id  text PRIMARY KEY,
  by_user    text NOT NULL,
  at         timestamptz NOT NULL
);

-- Append-only, hash-chained audit log (NFR-08). UPDATE/DELETE are revoked from the app role.
CREATE TABLE IF NOT EXISTS audit_log (
  seq        bigint PRIMARY KEY,
  at         timestamptz NOT NULL,
  actor      text NOT NULL,
  action     text NOT NULL,
  subject    text NOT NULL,
  detail     jsonb NOT NULL,
  prev_hash  text NOT NULL,
  hash       text NOT NULL UNIQUE
);

CREATE OR REPLACE FUNCTION audit_log_immutable() RETURNS trigger AS $$
BEGIN
  RAISE EXCEPTION 'audit_log is append-only';
END $$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS audit_log_no_update ON audit_log;
CREATE TRIGGER audit_log_no_update BEFORE UPDATE OR DELETE ON audit_log
  FOR EACH ROW EXECUTE FUNCTION audit_log_immutable();

-- Read models projected after each recompute (for BI and integration consumers).
CREATE TABLE IF NOT EXISTS projection_hypotheses (id text PRIMARY KEY, tail text NOT NULL, doc jsonb NOT NULL, updated timestamptz NOT NULL);
CREATE TABLE IF NOT EXISTS projection_alerts     (id text PRIMARY KEY, tail text NOT NULL, doc jsonb NOT NULL, updated timestamptz NOT NULL);
CREATE TABLE IF NOT EXISTS projection_cases      (id text PRIMARY KEY, tail text NOT NULL, doc jsonb NOT NULL, updated timestamptz NOT NULL);
CREATE TABLE IF NOT EXISTS projection_preflight  (flight_id text PRIMARY KEY, tail text NOT NULL, doc jsonb NOT NULL, updated timestamptz NOT NULL);
