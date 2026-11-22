-- High-water mark for the device clock.
--
-- last_seen_utc alone is not enough: an operator can move the clock forward,
-- let the app record that future timestamp, and then move it back. This table
-- keeps the greatest time the application has ever observed, so both backward
-- jumps and "reload" style clock manipulation are detected on the next call.
CREATE TABLE IF NOT EXISTS clock_high_water (
  singleton INTEGER PRIMARY KEY CHECK(singleton=1),
  high_water_utc TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
