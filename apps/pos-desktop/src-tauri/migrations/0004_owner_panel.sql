-- Owner control panel.
--
-- The owner opens the panel with a long press on the application logo and
-- then proves who they are with the device authenticator (Windows Hello on a
-- computer, the platform authenticator on a phone) or with the owner
-- password. This migration stores the enrolled authenticators and the
-- one-shot challenges that bind a single ceremony to a single moment.
--
-- The panel never stores a secret that can unlock it. The credential table
-- holds public keys and credential handles only, so a stolen database file
-- gives an attacker nothing to sign with.
CREATE TABLE IF NOT EXISTS owner_panel_devices (
  id TEXT PRIMARY KEY,
  owner_user_id TEXT NOT NULL REFERENCES users(id),
  label TEXT NOT NULL,
  platform TEXT NOT NULL,
  device_fingerprint TEXT NOT NULL,
  enrolled_at TEXT NOT NULL,
  last_unlocked_at TEXT,
  revoked_at TEXT
);

CREATE TABLE IF NOT EXISTS owner_panel_credentials (
  id TEXT PRIMARY KEY,
  owner_user_id TEXT NOT NULL REFERENCES users(id),
  device_id TEXT REFERENCES owner_panel_devices(id),
  credential_id BLOB NOT NULL UNIQUE,
  public_key_cose BLOB NOT NULL,
  public_key_x BLOB NOT NULL,
  public_key_y BLOB NOT NULL,
  algorithm TEXT NOT NULL,
  rp_id TEXT NOT NULL,
  label TEXT NOT NULL,
  enrolled_at TEXT NOT NULL,
  last_used_at TEXT,
  revoked_at TEXT
);

CREATE TABLE IF NOT EXISTS owner_panel_challenges (
  challenge_id TEXT PRIMARY KEY,
  owner_user_id TEXT NOT NULL REFERENCES users(id),
  purpose TEXT NOT NULL CHECK(purpose IN ('register','assert')),
  challenge BLOB NOT NULL,
  rp_id TEXT NOT NULL,
  created_at TEXT NOT NULL,
  expires_at TEXT NOT NULL,
  consumed_at TEXT
);

CREATE INDEX IF NOT EXISTS owner_panel_challenges_owner ON owner_panel_challenges(owner_user_id, purpose);

-- Tax is optional. A branch starts with the tax switched off (rate 0) and
-- shelf prices entered as they are charged; the owner switches it on, and
-- sets the rate and the price display mode, from the control panel. The
-- chosen values are what every invoice uses, and the printed receipt shows
-- the tax line separately.
UPDATE branches SET prices_include_tax = 0 WHERE prices_include_tax IS NULL;

-- A default return policy so the returns desk works from the first day. The
-- owner replaces these values from the control panel; the change is written
-- to the same key and is audited.
INSERT OR IGNORE INTO system_settings(
  branch_id, setting_key, setting_value_json, version, changed_by, changed_at
)
SELECT
  id,
  'returns_policy',
  '{"periodDays":14,"allowDiscountedItems":true,"restockConditions":["UNOPENED","NEW_TAGS"]}',
  1,
  NULL,
  strftime('%Y-%m-%dT%H:%M:%fZ','now')
FROM branches;
