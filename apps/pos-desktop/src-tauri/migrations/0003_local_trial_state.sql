-- Local evaluation state.
--
-- The application starts a short, device-bound evaluation on first run so a
-- customer can try the product without an offline activation round-trip.
-- This table records when the evaluation started so the trial cannot be
-- restarted by deleting a file, and so the grace period can be measured from
-- a trusted local clock.
--
-- The row is written once, on first run, and is never rewritten: the trial
-- start instant is append-only from the application's point of view.
CREATE TABLE IF NOT EXISTS local_trial_state (
  singleton INTEGER PRIMARY KEY CHECK(singleton=1),
  device_fingerprint TEXT NOT NULL,
  trial_started_at_utc TEXT NOT NULL,
  trial_expires_at_utc TEXT NOT NULL,
  grace_expires_at_utc TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE TRIGGER IF NOT EXISTS local_trial_state_no_update
BEFORE UPDATE ON local_trial_state
BEGIN
  SELECT RAISE(ABORT, 'local trial state is append-only');
END;

CREATE TRIGGER IF NOT EXISTS local_trial_state_no_delete
BEFORE DELETE ON local_trial_state
BEGIN
  SELECT RAISE(ABORT, 'local trial state is append-only');
END;
