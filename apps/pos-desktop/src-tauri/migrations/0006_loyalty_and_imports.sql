-- Loyalty, store credit, and import bookkeeping.
--
-- All three are additive: nothing here changes an existing ledger rule, and
-- every balance is still derived from an append-only ledger so a value can be
-- recomputed from history at any time.

-- Store credit issued by a refund, and spent later as a payment method.
-- A credit note is never edited: spending it writes a negative amount.
CREATE TABLE IF NOT EXISTS store_credits (
  id TEXT PRIMARY KEY,
  branch_id TEXT NOT NULL REFERENCES branches(id),
  customer_id TEXT NOT NULL,
  return_document_id TEXT,
  amount_minor INTEGER NOT NULL CHECK(amount_minor <> 0),
  note TEXT,
  created_by TEXT REFERENCES users(id),
  created_at TEXT NOT NULL,
  FOREIGN KEY(branch_id, customer_id) REFERENCES customers(branch_id, id),
  FOREIGN KEY(branch_id, return_document_id) REFERENCES return_documents(branch_id, id),
  UNIQUE(branch_id, id)
);
CREATE INDEX IF NOT EXISTS idx_store_credits_customer ON store_credits(branch_id, customer_id);

CREATE TABLE IF NOT EXISTS store_credit_spends (
  id TEXT PRIMARY KEY,
  branch_id TEXT NOT NULL REFERENCES branches(id),
  credit_id TEXT NOT NULL,
  invoice_id TEXT NOT NULL,
  amount_minor INTEGER NOT NULL CHECK(amount_minor > 0),
  created_by TEXT REFERENCES users(id),
  created_at TEXT NOT NULL,
  FOREIGN KEY(branch_id, credit_id) REFERENCES store_credits(branch_id, id),
  FOREIGN KEY(branch_id, invoice_id) REFERENCES invoices(branch_id, id),
  UNIQUE(branch_id, id)
);
CREATE INDEX IF NOT EXISTS idx_store_credit_spends_invoice ON store_credit_spends(branch_id, invoice_id);

-- The loyalty programme is optional and configured per branch.
-- `points_per_currency_unit` is points earned per 1 EGP of net spend, and
-- `redeem_value_minor` is the value of one point when redeemed.
CREATE TABLE IF NOT EXISTS loyalty_settings (
  branch_id TEXT PRIMARY KEY REFERENCES branches(id),
  is_enabled INTEGER NOT NULL DEFAULT 0 CHECK(is_enabled IN (0,1)),
  points_per_currency_unit INTEGER NOT NULL DEFAULT 1 CHECK(points_per_currency_unit >= 0),
  redeem_value_minor INTEGER NOT NULL DEFAULT 100 CHECK(redeem_value_minor >= 0),
  min_redeem_points INTEGER NOT NULL DEFAULT 0 CHECK(min_redeem_points >= 0),
  changed_by TEXT REFERENCES users(id),
  changed_at TEXT NOT NULL
);

-- Metadata the import wizard needs on top of the existing batch tables:
-- which file was used, who approved the write, and the checksum of the rows
-- that were actually committed.
CREATE TABLE IF NOT EXISTS import_batch_meta (
  branch_id TEXT NOT NULL REFERENCES branches(id),
  import_batch_id TEXT NOT NULL,
  source_kind TEXT NOT NULL CHECK(source_kind IN ('CSV','XLSX')),
  approved_by TEXT REFERENCES users(id),
  approved_at TEXT,
  committed_checksum TEXT,
  notes TEXT,
  PRIMARY KEY(branch_id, import_batch_id),
  FOREIGN KEY(branch_id, import_batch_id) REFERENCES import_batches(branch_id, id)
);
