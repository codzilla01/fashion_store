-- NASEEJ POS schema v1. Applied only after opening SQLCipher with a valid key.
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS schema_migrations (
  version INTEGER PRIMARY KEY,
  checksum TEXT NOT NULL,
  applied_at TEXT NOT NULL
);

CREATE TABLE branches (
  id TEXT PRIMARY KEY,
  code TEXT NOT NULL UNIQUE,
  name_ar TEXT NOT NULL,
  name_en TEXT NOT NULL,
  currency_code TEXT NOT NULL DEFAULT 'EGP' CHECK(length(currency_code)=3),
  tax_rate_bps INTEGER NOT NULL DEFAULT 1400 CHECK(tax_rate_bps BETWEEN 0 AND 10000),
  prices_include_tax INTEGER CHECK(prices_include_tax IN (0,1)), -- NULL until owner confirms
  timezone TEXT NOT NULL DEFAULT 'Africa/Cairo',
  created_at TEXT NOT NULL
);

CREATE TABLE users (
  id TEXT PRIMARY KEY,
  username TEXT NOT NULL COLLATE NOCASE UNIQUE,
  display_name TEXT NOT NULL,
  password_phc TEXT NOT NULL,
  is_active INTEGER NOT NULL DEFAULT 1 CHECK(is_active IN (0,1)),
  failed_logins INTEGER NOT NULL DEFAULT 0 CHECK(failed_logins >= 0),
  locked_until TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE roles (
  code TEXT PRIMARY KEY,
  name_ar TEXT NOT NULL,
  name_en TEXT NOT NULL
);

CREATE TABLE permissions (
  code TEXT PRIMARY KEY,
  description TEXT NOT NULL
);

CREATE TABLE role_permissions (
  role_code TEXT NOT NULL REFERENCES roles(code),
  permission_code TEXT NOT NULL REFERENCES permissions(code),
  PRIMARY KEY(role_code, permission_code)
);

CREATE TABLE user_branch_roles (
  user_id TEXT NOT NULL REFERENCES users(id),
  branch_id TEXT NOT NULL REFERENCES branches(id),
  role_code TEXT NOT NULL REFERENCES roles(code),
  granted_by TEXT REFERENCES users(id),
  granted_at TEXT NOT NULL,
  PRIMARY KEY(user_id, branch_id, role_code)
);

CREATE TABLE employees (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL UNIQUE REFERENCES users(id),
  branch_id TEXT NOT NULL REFERENCES branches(id),
  employee_code TEXT NOT NULL,
  hire_date TEXT,
  commission_basis_points INTEGER NOT NULL DEFAULT 0 CHECK(commission_basis_points BETWEEN 0 AND 10000),
  is_active INTEGER NOT NULL DEFAULT 1 CHECK(is_active IN (0,1)),
  UNIQUE(branch_id, employee_code),
  UNIQUE(branch_id, id)
);

CREATE TABLE employee_shifts (
  id TEXT PRIMARY KEY,
  branch_id TEXT NOT NULL REFERENCES branches(id),
  employee_id TEXT NOT NULL,
  starts_at TEXT NOT NULL,
  ends_at TEXT,
  note TEXT,
  created_by TEXT NOT NULL REFERENCES users(id),
  FOREIGN KEY(branch_id, employee_id) REFERENCES employees(branch_id, id),
  UNIQUE(branch_id, id)
);

CREATE TABLE categories (
  id TEXT PRIMARY KEY,
  branch_id TEXT NOT NULL REFERENCES branches(id),
  name_ar TEXT NOT NULL,
  name_en TEXT NOT NULL,
  parent_id TEXT,
  is_active INTEGER NOT NULL DEFAULT 1 CHECK(is_active IN (0,1)),
  sort_order INTEGER NOT NULL DEFAULT 0,
  FOREIGN KEY(branch_id, parent_id) REFERENCES categories(branch_id, id),
  UNIQUE(branch_id, id),
  UNIQUE(branch_id, name_ar),
  UNIQUE(branch_id, name_en)
);

CREATE TABLE brands (
  id TEXT PRIMARY KEY,
  branch_id TEXT NOT NULL REFERENCES branches(id),
  name_ar TEXT NOT NULL,
  name_en TEXT NOT NULL,
  is_active INTEGER NOT NULL DEFAULT 1 CHECK(is_active IN (0,1)),
  UNIQUE(branch_id, id),
  UNIQUE(branch_id, name_ar),
  UNIQUE(branch_id, name_en)
);

CREATE TABLE seasons (
  id TEXT PRIMARY KEY,
  branch_id TEXT NOT NULL REFERENCES branches(id),
  name_ar TEXT NOT NULL,
  name_en TEXT NOT NULL,
  starts_on TEXT,
  ends_on TEXT,
  UNIQUE(branch_id, id),
  UNIQUE(branch_id, name_ar)
);

CREATE TABLE sizes (
  id TEXT PRIMARY KEY,
  branch_id TEXT NOT NULL REFERENCES branches(id),
  profile_code TEXT NOT NULL DEFAULT 'CLOTHING',
  size_code TEXT NOT NULL,
  name_ar TEXT NOT NULL,
  name_en TEXT NOT NULL,
  sort_order INTEGER NOT NULL DEFAULT 0,
  is_active INTEGER NOT NULL DEFAULT 1 CHECK(is_active IN (0,1)),
  UNIQUE(branch_id, id),
  UNIQUE(branch_id, profile_code, size_code)
);

CREATE TABLE colors (
  id TEXT PRIMARY KEY,
  branch_id TEXT NOT NULL REFERENCES branches(id),
  color_code TEXT NOT NULL,
  name_ar TEXT NOT NULL,
  name_en TEXT NOT NULL,
  hex_color TEXT CHECK(hex_color IS NULL OR (length(hex_color)=7 AND substr(hex_color,1,1)='#')),
  is_active INTEGER NOT NULL DEFAULT 1 CHECK(is_active IN (0,1)),
  UNIQUE(branch_id, id),
  UNIQUE(branch_id, color_code),
  UNIQUE(branch_id, name_ar)
);

CREATE TABLE products (
  id TEXT PRIMARY KEY,
  branch_id TEXT NOT NULL REFERENCES branches(id),
  category_id TEXT NOT NULL,
  brand_id TEXT,
  season_id TEXT,
  name_ar TEXT NOT NULL,
  name_en TEXT NOT NULL,
  description_ar TEXT,
  description_en TEXT,
  base_cost_minor INTEGER NOT NULL DEFAULT 0 CHECK(base_cost_minor >= 0),
  base_price_minor INTEGER NOT NULL CHECK(base_price_minor >= 0),
  create_idempotency_key TEXT,
  create_request_hash TEXT, 
  is_active INTEGER NOT NULL DEFAULT 1 CHECK(is_active IN (0,1)),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(branch_id, category_id) REFERENCES categories(branch_id, id),
  FOREIGN KEY(branch_id, brand_id) REFERENCES brands(branch_id, id),
  FOREIGN KEY(branch_id, season_id) REFERENCES seasons(branch_id, id),
  UNIQUE(branch_id, id),
  UNIQUE(branch_id, create_idempotency_key)
);

CREATE TABLE product_images (
  id TEXT PRIMARY KEY,
  branch_id TEXT NOT NULL REFERENCES branches(id),
  product_id TEXT NOT NULL,
  relative_path TEXT NOT NULL,
  mime_type TEXT NOT NULL CHECK(mime_type IN ('image/jpeg','image/png','image/webp')),
  sha256 TEXT NOT NULL,
  created_at TEXT NOT NULL,
  FOREIGN KEY(branch_id, product_id) REFERENCES products(branch_id, id),
  UNIQUE(branch_id, id),
  UNIQUE(branch_id, relative_path)
);

CREATE TABLE product_variants (
  id TEXT PRIMARY KEY,
  branch_id TEXT NOT NULL REFERENCES branches(id),
  product_id TEXT NOT NULL,
  size_id TEXT NOT NULL,
  color_id TEXT NOT NULL,
  sku TEXT NOT NULL,
  barcode TEXT,
  cost_override_minor INTEGER CHECK(cost_override_minor IS NULL OR cost_override_minor >= 0),
  price_override_minor INTEGER CHECK(price_override_minor IS NULL OR price_override_minor >= 0),
  reorder_level INTEGER NOT NULL DEFAULT 0 CHECK(reorder_level >= 0),
  is_active INTEGER NOT NULL DEFAULT 1 CHECK(is_active IN (0,1)),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(branch_id, product_id) REFERENCES products(branch_id, id),
  FOREIGN KEY(branch_id, size_id) REFERENCES sizes(branch_id, id),
  FOREIGN KEY(branch_id, color_id) REFERENCES colors(branch_id, id),
  UNIQUE(branch_id, id),
  UNIQUE(branch_id, sku),
  UNIQUE(branch_id, barcode),
  UNIQUE(branch_id, product_id, size_id, color_id)
);

CREATE INDEX idx_products_name_ar ON products(branch_id, name_ar COLLATE NOCASE);
CREATE INDEX idx_products_name_en ON products(branch_id, name_en COLLATE NOCASE);
CREATE INDEX idx_variants_product ON product_variants(branch_id, product_id, is_active);
CREATE INDEX idx_variants_barcode ON product_variants(branch_id, barcode);

CREATE TABLE inventory_balances (
  branch_id TEXT NOT NULL REFERENCES branches(id),
  variant_id TEXT NOT NULL,
  bucket TEXT NOT NULL CHECK(bucket IN ('SELLABLE','DAMAGED','QUARANTINE')),
  quantity INTEGER NOT NULL DEFAULT 0,
  updated_at TEXT NOT NULL,
  PRIMARY KEY(branch_id, variant_id, bucket),
  FOREIGN KEY(branch_id, variant_id) REFERENCES product_variants(branch_id, id)
);

CREATE TABLE inventory_movements (
  id TEXT PRIMARY KEY,
  branch_id TEXT NOT NULL REFERENCES branches(id),
  variant_id TEXT NOT NULL,
  bucket TEXT NOT NULL CHECK(bucket IN ('SELLABLE','DAMAGED','QUARANTINE')),
  movement_type TEXT NOT NULL CHECK(movement_type IN (
    'OPENING_BALANCE','SALE','RETURN_RESTOCK','RETURN_DAMAGED','RETURN_QUARANTINE','GOODS_RECEIPT',
    'COUNT_ADJUSTMENT','DAMAGE_WRITE_OFF','TRANSFER_IN','TRANSFER_OUT','REVERSAL'
  )),
  quantity_delta INTEGER NOT NULL CHECK(quantity_delta <> 0),
  unit_cost_minor INTEGER CHECK(unit_cost_minor IS NULL OR unit_cost_minor >= 0),
  source_type TEXT NOT NULL,
  source_id TEXT NOT NULL,
  movement_group_id TEXT NOT NULL,
  idempotency_key TEXT NOT NULL,
  actor_user_id TEXT NOT NULL REFERENCES users(id),
  reason TEXT,
  created_at TEXT NOT NULL,
  FOREIGN KEY(branch_id, variant_id) REFERENCES product_variants(branch_id, id),
  UNIQUE(branch_id, id),
  UNIQUE(branch_id, idempotency_key)
);
CREATE INDEX idx_movements_variant_time ON inventory_movements(branch_id, variant_id, created_at);
CREATE INDEX idx_movements_source ON inventory_movements(branch_id, source_type, source_id);

CREATE TRIGGER inventory_movements_apply_balance
AFTER INSERT ON inventory_movements
BEGIN
  INSERT INTO inventory_balances(branch_id, variant_id, bucket, quantity, updated_at)
  VALUES(NEW.branch_id, NEW.variant_id, NEW.bucket, NEW.quantity_delta, NEW.created_at)
  ON CONFLICT(branch_id, variant_id, bucket)
  DO UPDATE SET quantity = inventory_balances.quantity + NEW.quantity_delta,
                updated_at = NEW.created_at;
END;
CREATE TRIGGER inventory_movements_no_update
BEFORE UPDATE ON inventory_movements BEGIN SELECT RAISE(ABORT, 'inventory movements are append-only'); END;
CREATE TRIGGER inventory_movements_no_delete
BEFORE DELETE ON inventory_movements BEGIN SELECT RAISE(ABORT, 'inventory movements are append-only'); END;

CREATE TABLE stock_counts (
  id TEXT PRIMARY KEY,
  branch_id TEXT NOT NULL REFERENCES branches(id),
  count_no TEXT NOT NULL,
  status TEXT NOT NULL CHECK(status IN ('DRAFT','REVIEW','POSTED','CANCELLED')),
  started_at TEXT NOT NULL,
  posted_at TEXT,
  created_by TEXT NOT NULL REFERENCES users(id),
  approved_by TEXT REFERENCES users(id),
  reason TEXT NOT NULL,
  UNIQUE(branch_id, id),
  UNIQUE(branch_id, count_no)
);

CREATE TABLE stock_count_lines (
  id TEXT PRIMARY KEY,
  branch_id TEXT NOT NULL REFERENCES branches(id),
  stock_count_id TEXT NOT NULL,
  variant_id TEXT NOT NULL,
  bucket TEXT NOT NULL CHECK(bucket IN ('SELLABLE','DAMAGED','QUARANTINE')),
  expected_quantity INTEGER NOT NULL,
  counted_quantity INTEGER NOT NULL CHECK(counted_quantity >= 0),
  difference_quantity INTEGER GENERATED ALWAYS AS (counted_quantity - expected_quantity) VIRTUAL,
  note TEXT,
  FOREIGN KEY(branch_id, stock_count_id) REFERENCES stock_counts(branch_id, id),
  FOREIGN KEY(branch_id, variant_id) REFERENCES product_variants(branch_id, id),
  UNIQUE(branch_id, stock_count_id, variant_id, bucket)
);

CREATE TABLE suppliers (
  id TEXT PRIMARY KEY,
  branch_id TEXT NOT NULL REFERENCES branches(id),
  supplier_code TEXT NOT NULL,
  name TEXT NOT NULL,
  phone_ciphertext BLOB,
  email_ciphertext BLOB,
  address_ciphertext BLOB,
  tax_registration_ciphertext BLOB,
  notes_ciphertext BLOB,
  is_active INTEGER NOT NULL DEFAULT 1 CHECK(is_active IN (0,1)),
  created_at TEXT NOT NULL,
  UNIQUE(branch_id, id),
  UNIQUE(branch_id, supplier_code)
);

CREATE TABLE purchase_orders (
  id TEXT PRIMARY KEY,
  branch_id TEXT NOT NULL REFERENCES branches(id),
  supplier_id TEXT NOT NULL,
  order_no TEXT NOT NULL,
  status TEXT NOT NULL CHECK(status IN ('DRAFT','ORDERED','PARTIAL','RECEIVED','CANCELLED')),
  ordered_at TEXT NOT NULL,
  expected_at TEXT,
  currency_code TEXT NOT NULL DEFAULT 'EGP',
  notes TEXT,
  created_by TEXT NOT NULL REFERENCES users(id),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(branch_id, supplier_id) REFERENCES suppliers(branch_id, id),
  UNIQUE(branch_id, id),
  UNIQUE(branch_id, order_no)
);

CREATE TABLE purchase_order_lines (
  id TEXT PRIMARY KEY,
  branch_id TEXT NOT NULL REFERENCES branches(id),
  purchase_order_id TEXT NOT NULL,
  variant_id TEXT NOT NULL,
  quantity_ordered INTEGER NOT NULL CHECK(quantity_ordered > 0),
  quantity_received INTEGER NOT NULL DEFAULT 0 CHECK(quantity_received >= 0 AND quantity_received <= quantity_ordered),
  unit_cost_minor INTEGER NOT NULL CHECK(unit_cost_minor >= 0),
  tax_rate_bps INTEGER NOT NULL DEFAULT 0 CHECK(tax_rate_bps BETWEEN 0 AND 10000),
  FOREIGN KEY(branch_id, purchase_order_id) REFERENCES purchase_orders(branch_id, id),
  FOREIGN KEY(branch_id, variant_id) REFERENCES product_variants(branch_id, id),
  UNIQUE(branch_id, id),
  UNIQUE(purchase_order_id, variant_id)
);

CREATE TABLE goods_receipts (
  id TEXT PRIMARY KEY,
  branch_id TEXT NOT NULL REFERENCES branches(id),
  purchase_order_id TEXT,
  supplier_id TEXT NOT NULL,
  receipt_no TEXT NOT NULL,
  supplier_invoice_ref TEXT,
  status TEXT NOT NULL CHECK(status IN ('POSTED','VOIDED')),
  received_at TEXT NOT NULL,
  idempotency_key TEXT NOT NULL,
  request_hash TEXT NOT NULL,
  received_by TEXT NOT NULL REFERENCES users(id),
  reason TEXT,
  FOREIGN KEY(branch_id, purchase_order_id) REFERENCES purchase_orders(branch_id, id),
  FOREIGN KEY(branch_id, supplier_id) REFERENCES suppliers(branch_id, id),
  UNIQUE(branch_id, id),
  UNIQUE(branch_id, receipt_no),
  UNIQUE(branch_id, idempotency_key)
);

CREATE TABLE goods_receipt_lines (
  id TEXT PRIMARY KEY,
  branch_id TEXT NOT NULL REFERENCES branches(id),
  goods_receipt_id TEXT NOT NULL,
  purchase_order_line_id TEXT,
  variant_id TEXT NOT NULL,
  quantity_received INTEGER NOT NULL CHECK(quantity_received > 0),
  unit_cost_minor INTEGER NOT NULL CHECK(unit_cost_minor >= 0),
  FOREIGN KEY(branch_id, goods_receipt_id) REFERENCES goods_receipts(branch_id, id),
  FOREIGN KEY(branch_id, purchase_order_line_id) REFERENCES purchase_order_lines(branch_id, id),
  FOREIGN KEY(branch_id, variant_id) REFERENCES product_variants(branch_id, id),
  UNIQUE(branch_id, id)
);

CREATE TABLE customers (
  id TEXT PRIMARY KEY,
  branch_id TEXT NOT NULL REFERENCES branches(id),
  name_ciphertext BLOB,
  phone_ciphertext BLOB,
  phone_lookup_hmac TEXT,
  email_ciphertext BLOB,
  notes_ciphertext BLOB,
  size_preferences_ciphertext BLOB,
  consent_status TEXT NOT NULL DEFAULT 'NOT_GIVEN' CHECK(consent_status IN ('NOT_GIVEN','GIVEN','WITHDRAWN')),
  consent_at TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  is_active INTEGER NOT NULL DEFAULT 1 CHECK(is_active IN (0,1)),
  UNIQUE(branch_id, id)
);
CREATE INDEX idx_customers_phone_hmac ON customers(branch_id, phone_lookup_hmac);

CREATE TABLE loyalty_ledger (
  id TEXT PRIMARY KEY,
  branch_id TEXT NOT NULL REFERENCES branches(id),
  customer_id TEXT NOT NULL,
  invoice_id TEXT,
  points_delta INTEGER NOT NULL CHECK(points_delta <> 0),
  event_type TEXT NOT NULL CHECK(event_type IN ('EARN','REDEEM','ADJUST','EXPIRE','REVERSAL')),
  reason TEXT NOT NULL,
  actor_user_id TEXT NOT NULL REFERENCES users(id),
  idempotency_key TEXT NOT NULL,
  created_at TEXT NOT NULL,
  FOREIGN KEY(branch_id, customer_id) REFERENCES customers(branch_id, id),
  UNIQUE(branch_id, idempotency_key)
);
CREATE TRIGGER loyalty_ledger_no_update BEFORE UPDATE ON loyalty_ledger BEGIN SELECT RAISE(ABORT, 'loyalty ledger is append-only'); END;
CREATE TRIGGER loyalty_ledger_no_delete BEFORE DELETE ON loyalty_ledger BEGIN SELECT RAISE(ABORT, 'loyalty ledger is append-only'); END;

CREATE TABLE invoices (
  id TEXT PRIMARY KEY,
  branch_id TEXT NOT NULL REFERENCES branches(id),
  invoice_no TEXT NOT NULL,
  idempotency_key TEXT NOT NULL,
  request_hash TEXT NOT NULL,
  customer_id TEXT,
  cashier_user_id TEXT NOT NULL REFERENCES users(id),
  status TEXT NOT NULL CHECK(status IN ('COMPLETED','VOIDED','PARTIALLY_RETURNED','RETURNED')),
  currency_code TEXT NOT NULL DEFAULT 'EGP',
  tax_rate_bps INTEGER NOT NULL CHECK(tax_rate_bps BETWEEN 0 AND 10000),
  prices_include_tax INTEGER NOT NULL CHECK(prices_include_tax IN (0,1)),
  subtotal_minor INTEGER NOT NULL CHECK(subtotal_minor >= 0),
  item_discount_minor INTEGER NOT NULL DEFAULT 0 CHECK(item_discount_minor >= 0),
  invoice_discount_minor INTEGER NOT NULL DEFAULT 0 CHECK(invoice_discount_minor >= 0),
  tax_minor INTEGER NOT NULL CHECK(tax_minor >= 0),
  total_minor INTEGER NOT NULL CHECK(total_minor >= 0),
  discount_reason TEXT,
  created_at TEXT NOT NULL,
  FOREIGN KEY(branch_id, customer_id) REFERENCES customers(branch_id, id),
  UNIQUE(branch_id, id),
  UNIQUE(branch_id, invoice_no),
  UNIQUE(branch_id, idempotency_key)
);
CREATE INDEX idx_invoices_date ON invoices(branch_id, created_at);
CREATE INDEX idx_invoices_cashier ON invoices(branch_id, cashier_user_id, created_at);
CREATE INDEX idx_invoices_customer ON invoices(branch_id, customer_id, created_at);

CREATE TABLE invoice_lines (
  id TEXT PRIMARY KEY,
  branch_id TEXT NOT NULL REFERENCES branches(id),
  invoice_id TEXT NOT NULL,
  variant_id TEXT NOT NULL,
  sku_snapshot TEXT NOT NULL,
  barcode_snapshot TEXT,
  product_name_ar_snapshot TEXT NOT NULL,
  product_name_en_snapshot TEXT NOT NULL,
  size_snapshot TEXT NOT NULL,
  color_snapshot TEXT NOT NULL,
  quantity INTEGER NOT NULL CHECK(quantity > 0),
  unit_price_minor INTEGER NOT NULL CHECK(unit_price_minor >= 0),
  unit_cost_snapshot_minor INTEGER NOT NULL CHECK(unit_cost_snapshot_minor >= 0),
  line_discount_minor INTEGER NOT NULL DEFAULT 0 CHECK(line_discount_minor >= 0),
  tax_rate_bps INTEGER NOT NULL CHECK(tax_rate_bps BETWEEN 0 AND 10000),
  tax_minor INTEGER NOT NULL CHECK(tax_minor >= 0),
  line_total_minor INTEGER NOT NULL CHECK(line_total_minor >= 0),
  FOREIGN KEY(branch_id, invoice_id) REFERENCES invoices(branch_id, id),
  FOREIGN KEY(branch_id, variant_id) REFERENCES product_variants(branch_id, id),
  UNIQUE(branch_id, id)
);
CREATE INDEX idx_invoice_lines_variant ON invoice_lines(branch_id, variant_id);
CREATE TRIGGER invoice_lines_no_update BEFORE UPDATE ON invoice_lines BEGIN SELECT RAISE(ABORT, 'invoice lines are immutable'); END;
CREATE TRIGGER invoice_lines_no_delete BEFORE DELETE ON invoice_lines BEGIN SELECT RAISE(ABORT, 'invoice lines are immutable'); END;

CREATE TABLE payments (
  id TEXT PRIMARY KEY,
  branch_id TEXT NOT NULL REFERENCES branches(id),
  invoice_id TEXT NOT NULL,
  method TEXT NOT NULL CHECK(method IN ('CASH','CARD','WALLET','STORE_CREDIT')),
  amount_minor INTEGER NOT NULL CHECK(amount_minor > 0),
  currency_code TEXT NOT NULL DEFAULT 'EGP',
  reference_ciphertext BLOB,
  idempotency_key TEXT NOT NULL,
  created_at TEXT NOT NULL,
  FOREIGN KEY(branch_id, invoice_id) REFERENCES invoices(branch_id, id),
  UNIQUE(branch_id, idempotency_key)
);
CREATE INDEX idx_payments_invoice ON payments(branch_id, invoice_id);
CREATE TRIGGER payments_no_update BEFORE UPDATE ON payments BEGIN SELECT RAISE(ABORT, 'payments are immutable'); END;
CREATE TRIGGER payments_no_delete BEFORE DELETE ON payments BEGIN SELECT RAISE(ABORT, 'payments are immutable'); END;

CREATE TABLE return_documents (
  id TEXT PRIMARY KEY,
  branch_id TEXT NOT NULL REFERENCES branches(id),
  return_no TEXT NOT NULL,
  original_invoice_id TEXT NOT NULL,
  customer_id TEXT,
  kind TEXT NOT NULL CHECK(kind IN ('RETURN','EXCHANGE')),
  status TEXT NOT NULL CHECK(status IN ('COMPLETED','VOIDED')),
  reason TEXT NOT NULL,
  refund_due_minor INTEGER NOT NULL DEFAULT 0 CHECK(refund_due_minor >= 0),
  created_by TEXT NOT NULL REFERENCES users(id),
  created_at TEXT NOT NULL,
  idempotency_key TEXT NOT NULL,
  request_hash TEXT NOT NULL,
  FOREIGN KEY(branch_id, original_invoice_id) REFERENCES invoices(branch_id, id),
  FOREIGN KEY(branch_id, customer_id) REFERENCES customers(branch_id, id),
  UNIQUE(branch_id, id),
  UNIQUE(branch_id, return_no),
  UNIQUE(branch_id, idempotency_key)
);

CREATE TABLE return_lines (
  id TEXT PRIMARY KEY,
  branch_id TEXT NOT NULL REFERENCES branches(id),
  return_document_id TEXT NOT NULL,
  original_invoice_line_id TEXT NOT NULL,
  variant_id TEXT NOT NULL,
  quantity INTEGER NOT NULL CHECK(quantity > 0),
  reason TEXT NOT NULL,
  item_condition TEXT NOT NULL CHECK(item_condition IN ('UNOPENED','NEW_TAGS','WORN','DAMAGED')),
  disposition TEXT NOT NULL CHECK(disposition IN ('RESTOCK','DAMAGED','QUARANTINE','DISCARD')),
  refund_line_minor INTEGER NOT NULL CHECK(refund_line_minor >= 0),
  FOREIGN KEY(branch_id, return_document_id) REFERENCES return_documents(branch_id, id),
  FOREIGN KEY(branch_id, original_invoice_line_id) REFERENCES invoice_lines(branch_id, id),
  FOREIGN KEY(branch_id, variant_id) REFERENCES product_variants(branch_id, id),
  UNIQUE(branch_id, id)
);

CREATE TABLE return_refunds (
  id TEXT PRIMARY KEY,
  branch_id TEXT NOT NULL REFERENCES branches(id),
  return_document_id TEXT NOT NULL,
  method TEXT NOT NULL CHECK(method IN ('CASH','CARD','WALLET','STORE_CREDIT')),
  amount_minor INTEGER NOT NULL CHECK(amount_minor > 0),
  reference_ciphertext BLOB,
  idempotency_key TEXT NOT NULL,
  processed_by TEXT NOT NULL REFERENCES users(id),
  created_at TEXT NOT NULL,
  FOREIGN KEY(branch_id, return_document_id) REFERENCES return_documents(branch_id, id),
  UNIQUE(branch_id, idempotency_key)
);

CREATE TABLE exchange_groups (
  id TEXT PRIMARY KEY,
  branch_id TEXT NOT NULL REFERENCES branches(id),
  return_document_id TEXT NOT NULL,
  replacement_invoice_id TEXT NOT NULL,
  price_difference_minor INTEGER NOT NULL,
  settlement_method TEXT CHECK(settlement_method IN ('CASH','CARD','WALLET','STORE_CREDIT','REFUND')),
  created_at TEXT NOT NULL,
  created_by TEXT NOT NULL REFERENCES users(id),
  FOREIGN KEY(branch_id, return_document_id) REFERENCES return_documents(branch_id, id),
  FOREIGN KEY(branch_id, replacement_invoice_id) REFERENCES invoices(branch_id, id),
  UNIQUE(branch_id, return_document_id),
  UNIQUE(branch_id, replacement_invoice_id)
);

CREATE TABLE commission_rules (
  id TEXT PRIMARY KEY,
  branch_id TEXT NOT NULL REFERENCES branches(id),
  name TEXT NOT NULL,
  rate_basis_points INTEGER NOT NULL CHECK(rate_basis_points BETWEEN 0 AND 10000),
  applies_to TEXT NOT NULL CHECK(applies_to IN ('NET_SALES','GROSS_PROFIT')),
  starts_at TEXT NOT NULL,
  ends_at TEXT,
  is_active INTEGER NOT NULL DEFAULT 1 CHECK(is_active IN (0,1)),
  UNIQUE(branch_id, id)
);

CREATE TABLE commission_entries (
  id TEXT PRIMARY KEY,
  branch_id TEXT NOT NULL REFERENCES branches(id),
  employee_id TEXT NOT NULL,
  invoice_id TEXT NOT NULL,
  rule_id TEXT,
  base_minor INTEGER NOT NULL CHECK(base_minor >= 0),
  commission_minor INTEGER NOT NULL CHECK(commission_minor >= 0),
  status TEXT NOT NULL CHECK(status IN ('PENDING','APPROVED','PAID','REVERSED')),
  calculated_at TEXT NOT NULL,
  approved_by TEXT REFERENCES users(id),
  FOREIGN KEY(branch_id, employee_id) REFERENCES employees(branch_id, id),
  FOREIGN KEY(branch_id, invoice_id) REFERENCES invoices(branch_id, id),
  FOREIGN KEY(branch_id, rule_id) REFERENCES commission_rules(branch_id, id),
  UNIQUE(branch_id, id),
  UNIQUE(branch_id, employee_id, invoice_id)
);

CREATE TABLE cash_shifts (
  id TEXT PRIMARY KEY,
  branch_id TEXT NOT NULL REFERENCES branches(id),
  cashier_user_id TEXT NOT NULL REFERENCES users(id),
  opened_at TEXT NOT NULL,
  closed_at TEXT,
  opening_cash_minor INTEGER NOT NULL DEFAULT 0 CHECK(opening_cash_minor >= 0),
  expected_cash_minor INTEGER,
  counted_cash_minor INTEGER,
  status TEXT NOT NULL CHECK(status IN ('OPEN','CLOSED','RECONCILIATION_REQUIRED')),
  close_reason TEXT,
  UNIQUE(branch_id, id)
);

CREATE TABLE cash_drawer_movements (
  id TEXT PRIMARY KEY,
  branch_id TEXT NOT NULL REFERENCES branches(id),
  cash_shift_id TEXT NOT NULL,
  direction TEXT NOT NULL CHECK(direction IN ('IN','OUT')),
  amount_minor INTEGER NOT NULL CHECK(amount_minor > 0),
  reason TEXT NOT NULL,
  actor_user_id TEXT NOT NULL REFERENCES users(id),
  created_at TEXT NOT NULL,
  FOREIGN KEY(branch_id, cash_shift_id) REFERENCES cash_shifts(branch_id, id),
  UNIQUE(branch_id, id)
);

CREATE TABLE print_jobs (
  id TEXT PRIMARY KEY,
  branch_id TEXT NOT NULL REFERENCES branches(id),
  invoice_id TEXT,
  job_type TEXT NOT NULL CHECK(job_type IN ('RECEIPT','BARCODE_LABEL','CUSTOMER_DISPLAY')),
  paper_width_mm INTEGER CHECK(paper_width_mm IN (58,80)),
  status TEXT NOT NULL CHECK(status IN ('PENDING','PRINTED','FAILED','CANCELLED')),
  attempts INTEGER NOT NULL DEFAULT 0 CHECK(attempts >= 0),
  last_error_code TEXT,
  created_by TEXT NOT NULL REFERENCES users(id),
  created_at TEXT NOT NULL,
  completed_at TEXT,
  FOREIGN KEY(branch_id, invoice_id) REFERENCES invoices(branch_id, id),
  UNIQUE(branch_id, id)
);

CREATE TABLE audit_log (
  id TEXT PRIMARY KEY,
  branch_id TEXT REFERENCES branches(id),
  actor_user_id TEXT REFERENCES users(id),
  action_code TEXT NOT NULL,
  entity_type TEXT NOT NULL,
  entity_id TEXT,
  reason TEXT,
  details_json TEXT NOT NULL DEFAULT '{}',
  occurred_at TEXT NOT NULL,
  previous_hash TEXT,
  event_hash TEXT NOT NULL
);
CREATE INDEX idx_audit_time ON audit_log(branch_id, occurred_at);
CREATE TRIGGER audit_log_no_update BEFORE UPDATE ON audit_log BEGIN SELECT RAISE(ABORT, 'audit log is append-only'); END;
CREATE TRIGGER audit_log_no_delete BEFORE DELETE ON audit_log BEGIN SELECT RAISE(ABORT, 'audit log is append-only'); END;

CREATE TABLE system_settings (
  branch_id TEXT NOT NULL REFERENCES branches(id),
  setting_key TEXT NOT NULL,
  setting_value_json TEXT NOT NULL,
  version INTEGER NOT NULL DEFAULT 1,
  changed_by TEXT REFERENCES users(id),
  changed_at TEXT NOT NULL,
  PRIMARY KEY(branch_id, setting_key)
);

CREATE TABLE backup_jobs (
  id TEXT PRIMARY KEY,
  branch_id TEXT NOT NULL REFERENCES branches(id),
  status TEXT NOT NULL CHECK(status IN ('STARTED','SUCCESS','FAILED','RESTORED')),
  backup_kind TEXT NOT NULL CHECK(backup_kind IN ('SCHEDULED','MANUAL','PRE_RESTORE','PRE_UPGRADE')),
  filename TEXT,
  schema_version INTEGER NOT NULL,
  byte_size INTEGER,
  sha256 TEXT,
  destination_kind TEXT CHECK(destination_kind IN ('LOCAL','USB','EXTERNAL','CLOUD_OPT_IN')),
  error_code TEXT,
  started_at TEXT NOT NULL,
  finished_at TEXT,
  created_by TEXT REFERENCES users(id),
  UNIQUE(branch_id, id)
);

CREATE TABLE import_batches (
  id TEXT PRIMARY KEY,
  branch_id TEXT NOT NULL REFERENCES branches(id),
  import_type TEXT NOT NULL CHECK(import_type IN ('PRODUCTS','VARIANTS','OPENING_STOCK','CUSTOMERS','SUPPLIERS','PURCHASE_ORDERS')),
  source_name TEXT NOT NULL,
  source_sha256 TEXT NOT NULL,
  status TEXT NOT NULL CHECK(status IN ('PREVIEW','VALIDATED','COMMITTED','CANCELLED')),
  total_rows INTEGER NOT NULL DEFAULT 0 CHECK(total_rows >= 0),
  accepted_rows INTEGER NOT NULL DEFAULT 0 CHECK(accepted_rows >= 0),
  rejected_rows INTEGER NOT NULL DEFAULT 0 CHECK(rejected_rows >= 0),
  created_by TEXT NOT NULL REFERENCES users(id),
  created_at TEXT NOT NULL,
  committed_at TEXT,
  UNIQUE(branch_id, id)
);

CREATE TABLE import_rows (
  id TEXT PRIMARY KEY,
  branch_id TEXT NOT NULL REFERENCES branches(id),
  import_batch_id TEXT NOT NULL,
  row_number INTEGER NOT NULL CHECK(row_number > 0),
  status TEXT NOT NULL CHECK(status IN ('ACCEPTED','REJECTED','IMPORTED','SKIPPED')),
  sanitized_preview_json TEXT NOT NULL DEFAULT '{}',
  errors_json TEXT NOT NULL DEFAULT '[]',
  imported_entity_id TEXT,
  FOREIGN KEY(branch_id, import_batch_id) REFERENCES import_batches(branch_id, id),
  UNIQUE(import_batch_id, row_number)
);

CREATE TABLE schema_migrations_lock (
  singleton INTEGER PRIMARY KEY CHECK(singleton=1),
  locked_at TEXT NOT NULL
);

CREATE TABLE local_license_state (
  singleton INTEGER PRIMARY KEY CHECK(singleton=1),
  license_id TEXT,
  signed_payload_ciphertext BLOB,
  last_seen_utc TEXT,
  last_invoice_counter INTEGER NOT NULL DEFAULT 0,
  state_updated_at TEXT NOT NULL
);

-- Default role/permission vocabulary. First-run wizard must create owner; no default password is seeded.
INSERT INTO roles(code,name_ar,name_en) VALUES
  ('cashier','كاشير','Cashier'),
  ('manager','مدير','Manager'),
  ('owner','مالك','Owner');

INSERT INTO permissions(code,description) VALUES
  ('sales.read','Read sales and catalog'),
  ('sales.create','Create sale'),
  ('sales.discount','Apply manual discount'),
  ('catalog.manage','Manage catalog'),
  ('inventory.read','Read SKU inventory'),
  ('inventory.adjust','Adjust inventory'),
  ('inventory.sell_negative','Sell with negative stock'),
  ('customers.manage','Manage customer profiles'),
  ('returns.process','Process returns and exchanges'),
  ('purchases.manage','Manage suppliers and purchases'),
  ('reports.sales','Read non-profit sales reports'),
  ('cost.read','Read purchase cost'),
  ('reports.profit','Read profit and margin reports'),
  ('employees.manage','Manage employees and shifts'),
  ('commissions.read','Read commissions'),
  ('users.manage','Manage users and permissions'),
  ('settings.manage','Manage store settings'),
  ('backup.manage','Create and restore backups'),
  ('imports.manage','Import data'),
  ('audit.read','Read audit log');

INSERT INTO role_permissions(role_code,permission_code)
SELECT 'cashier', code FROM permissions
WHERE code IN ('sales.read','sales.create','inventory.read','customers.manage');

INSERT INTO role_permissions(role_code,permission_code)
SELECT 'manager', code FROM permissions
WHERE code <> 'users.manage';

INSERT INTO role_permissions(role_code,permission_code)
SELECT 'owner', code FROM permissions;

-- The application inserts one branch and all its first-run configuration transactionally.
-- Do not seed a default password or demo inventory into the live database.
