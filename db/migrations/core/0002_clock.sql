-- 0002: persist the simulation clock so a restart resumes where it stopped (additive, reversible:
-- ALTER TABLE replay_meta DROP COLUMN clock).
ALTER TABLE replay_meta ADD COLUMN IF NOT EXISTS clock jsonb;
