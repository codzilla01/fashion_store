"""Schema-only smoke tests using stock SQLite; these do not replace SQLCipher tests."""
import pathlib
import sqlite3
import unittest
import uuid

ROOT = pathlib.Path(__file__).resolve().parents[1]
MIGRATION = ROOT / "apps/pos-desktop/src-tauri/migrations/0001_initial.sql"
MIGRATION_2 = ROOT / "apps/pos-desktop/src-tauri/migrations/0002_branch_management.sql"
MIGRATION_3 = ROOT / "apps/pos-desktop/src-tauri/migrations/0003_local_trial_state.sql"
MIGRATION_4 = ROOT / "apps/pos-desktop/src-tauri/migrations/0004_owner_panel.sql"
MIGRATION_5 = ROOT / "apps/pos-desktop/src-tauri/migrations/0005_clock_high_water.sql"
MIGRATION_6 = ROOT / "apps/pos-desktop/src-tauri/migrations/0006_loyalty_and_imports.sql"
MIGRATION_7 = ROOT / "apps/pos-desktop/src-tauri/migrations/0007_immutable_documents.sql"


class SchemaSmokeTests(unittest.TestCase):
    def setUp(self):
        self.db = sqlite3.connect(":memory:")
        self.db.execute("PRAGMA foreign_keys=ON")
        self.db.executescript(MIGRATION.read_text(encoding="utf-8"))
        self.db.executescript(MIGRATION_2.read_text(encoding="utf-8"))
        self.db.executescript(MIGRATION_3.read_text(encoding="utf-8"))
        self.db.executescript(MIGRATION_4.read_text(encoding="utf-8"))
        self.db.executescript(MIGRATION_5.read_text(encoding="utf-8"))
        self.db.executescript(MIGRATION_6.read_text(encoding="utf-8"))
        self.db.executescript(MIGRATION_7.read_text(encoding="utf-8"))
        self.branch = "branch-test"
        self.user = "user-test"
        self.category = "category-test"
        self.size = "size-test"
        self.color = "color-test"
        self.product = "product-test"
        self.variant = "variant-test"
        now = "2026-10-10T10:00:00Z"
        self.db.execute("INSERT INTO branches(id,code,name_ar,name_en,currency_code,tax_rate_bps,prices_include_tax,timezone,created_at) VALUES(?,?,?,?,?,?,?,?,?)",
                        (self.branch, "TEST", "فرع", "Branch", "EGP", 1400, 1, "Africa/Cairo", now))
        self.db.execute("INSERT INTO users(id,username,display_name,password_phc,created_at,updated_at) VALUES(?,?,?,?,?,?)",
                        (self.user, "owner", "Owner", "test-phc", now, now))
        self.db.execute("INSERT INTO user_branch_roles(user_id,branch_id,role_code,granted_by,granted_at) VALUES(?,?,?,?,?)",
                        (self.user, self.branch, "owner", self.user, now))
        self.db.execute("INSERT INTO categories(id,branch_id,name_ar,name_en) VALUES(?,?,?,?)",
                        (self.category, self.branch, "قمصان", "Shirts"))
        self.db.execute("INSERT INTO sizes(id,branch_id,profile_code,size_code,name_ar,name_en) VALUES(?,?,?,?,?,?)",
                        (self.size, self.branch, "CLOTHING", "M", "متوسط", "Medium"))
        self.db.execute("INSERT INTO colors(id,branch_id,color_code,name_ar,name_en,hex_color) VALUES(?,?,?,?,?,?)",
                        (self.color, self.branch, "BLUE", "أزرق", "Blue", "#88aacc"))
        self.db.execute("INSERT INTO products(id,branch_id,category_id,name_ar,name_en,base_cost_minor,base_price_minor,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?)",
                        (self.product, self.branch, self.category, "قميص", "Shirt", 40000, 80000, now, now))
        self.db.execute("INSERT INTO product_variants(id,branch_id,product_id,size_id,color_id,sku,barcode,reorder_level,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?,?)",
                        (self.variant, self.branch, self.product, self.size, self.color, "SH-M-BL", "629000000001", 2, now, now))
        self.now = now
        self.db.commit()

    def tearDown(self):
        self.db.close()

    def test_variant_stock_is_per_sku_and_triggered_from_ledger(self):
        self.db.execute("INSERT INTO inventory_movements(id,branch_id,variant_id,bucket,movement_type,quantity_delta,source_type,source_id,movement_group_id,idempotency_key,actor_user_id,created_at) VALUES(?,?,?,?,?,?,?,?,?,?,?,?)",
                        (str(uuid.uuid4()), self.branch, self.variant, "SELLABLE", "OPENING_BALANCE", 4, "IMPORT", "batch-1", "group-1", "opening-1", self.user, self.now))
        qty = self.db.execute("SELECT quantity FROM inventory_balances WHERE branch_id=? AND variant_id=? AND bucket='SELLABLE'", (self.branch, self.variant)).fetchone()[0]
        self.assertEqual(qty, 4)

    def test_sku_and_barcode_are_unique_within_branch(self):
        with self.assertRaises(sqlite3.IntegrityError):
            self.db.execute("INSERT INTO product_variants(id,branch_id,product_id,size_id,color_id,sku,barcode,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?)",
                            ("variant-2", self.branch, self.product, self.size, self.color, "SH-M-BL", "629000000002", self.now, self.now))
        with self.assertRaises(sqlite3.IntegrityError):
            self.db.execute("INSERT INTO product_variants(id,branch_id,product_id,size_id,color_id,sku,barcode,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?)",
                            ("variant-3", self.branch, self.product, self.size, self.color, "SH-M-BL-2", "629000000001", self.now, self.now))

    def test_inventory_ledger_cannot_be_edited_or_deleted(self):
        movement = ("movement-1", self.branch, self.variant, "SELLABLE", "OPENING_BALANCE", 1, "IMPORT", "batch", "group", "move-once", self.user, self.now)
        self.db.execute("INSERT INTO inventory_movements(id,branch_id,variant_id,bucket,movement_type,quantity_delta,source_type,source_id,movement_group_id,idempotency_key,actor_user_id,created_at) VALUES(?,?,?,?,?,?,?,?,?,?,?,?)", movement)
        with self.assertRaises(sqlite3.IntegrityError):
            self.db.execute("UPDATE inventory_movements SET quantity_delta=10 WHERE id='movement-1'")
        with self.assertRaises(sqlite3.IntegrityError):
            self.db.execute("DELETE FROM inventory_movements WHERE id='movement-1'")

    def test_rollback_does_not_leave_partial_invoice(self):
        now = self.now
        self.db.execute("BEGIN")
        try:
            self.db.execute("INSERT INTO invoices(id,branch_id,invoice_no,idempotency_key,cashier_user_id,status,currency_code,tax_rate_bps,prices_include_tax,subtotal_minor,item_discount_minor,invoice_discount_minor,tax_minor,total_minor,created_at) VALUES(?,?,?,?,?,'COMPLETED','EGP',1400,1,80000,0,0,9825,80000,?)",
                            ("invoice-fail", self.branch, "NS-FAIL", "idempotency-fail", self.user, now))
            # A zero movement is rejected by the DB. The whole caller transaction must roll back.
            self.db.execute("INSERT INTO inventory_movements(id,branch_id,variant_id,bucket,movement_type,quantity_delta,source_type,source_id,movement_group_id,idempotency_key,actor_user_id,created_at) VALUES(?,?,?,?,?,?,?,?,?,?,?,?)",
                            ("bad-move", self.branch, self.variant, "SELLABLE", "SALE", 0, "INVOICE", "invoice-fail", "invoice-fail", "bad-idem", self.user, now))
            self.db.commit()
        except sqlite3.IntegrityError:
            self.db.rollback()
        self.assertEqual(self.db.execute("SELECT COUNT(*) FROM invoices WHERE id='invoice-fail'").fetchone()[0], 0)
        self.assertEqual(self.db.execute("SELECT COUNT(*) FROM inventory_movements WHERE id='bad-move'").fetchone()[0], 0)

    def test_manager_and_owner_have_cost_permission_but_cashier_does_not(self):
        rows = {row[0]: row[1] for row in self.db.execute("SELECT code, EXISTS(SELECT 1 FROM role_permissions rp WHERE rp.role_code=roles.code AND rp.permission_code='cost.read') FROM roles")}
        self.assertFalse(bool(rows["cashier"]))
        self.assertTrue(bool(rows["manager"]))
        self.assertTrue(bool(rows["owner"]))

    def test_branch_administration_is_owner_only_and_migration_is_repeatable(self):
        self.db.executescript(MIGRATION_2.read_text(encoding="utf-8"))
        self.db.executescript(MIGRATION_3.read_text(encoding="utf-8"))
        self.db.executescript(MIGRATION_4.read_text(encoding="utf-8"))
        self.db.executescript(MIGRATION_5.read_text(encoding="utf-8"))
        self.db.executescript(MIGRATION_6.read_text(encoding="utf-8"))
        self.db.executescript(MIGRATION_7.read_text(encoding="utf-8"))
        rows = {row[0]: bool(row[1]) for row in self.db.execute(
            "SELECT code,EXISTS(SELECT 1 FROM role_permissions rp WHERE rp.role_code=roles.code AND rp.permission_code='branches.manage') FROM roles")}
        self.assertFalse(rows["cashier"])
        self.assertFalse(rows["manager"])
        self.assertTrue(rows["owner"])

    def test_idempotency_request_hash_columns_are_present(self):
        columns = {
            table: {row[1] for row in self.db.execute(f"PRAGMA table_info({table})")}
            for table in ("products", "invoices", "return_documents", "goods_receipts")
        }
        self.assertIn("create_request_hash", columns["products"])
        self.assertIn("request_hash", columns["invoices"])
        self.assertIn("request_hash", columns["return_documents"])
        self.assertIn("request_hash", columns["goods_receipts"])

    def test_exchange_links_return_and_replacement_with_net_stock_change(self):
        self.db.execute("INSERT INTO inventory_movements(id,branch_id,variant_id,bucket,movement_type,quantity_delta,source_type,source_id,movement_group_id,idempotency_key,actor_user_id,created_at) VALUES(?,?,?,?,?,?,?,?,?,?,?,?)",
                        ("open-stock", self.branch, self.variant, "SELLABLE", "OPENING_BALANCE", 3, "IMPORT", "opening", "opening", "opening-stock", self.user, self.now))
        self.db.execute("INSERT INTO invoices(id,branch_id,invoice_no,idempotency_key,request_hash,cashier_user_id,status,currency_code,tax_rate_bps,prices_include_tax,subtotal_minor,item_discount_minor,invoice_discount_minor,tax_minor,total_minor,created_at) VALUES(?,?,?,?,?,?,'RETURNED','EGP',0,1,80000,0,0,0,80000,?)",
                        ("invoice-old", self.branch, "NS-OLD", "sale-old", "hash-old", self.user, self.now))
        self.db.execute("INSERT INTO invoice_lines(id,branch_id,invoice_id,variant_id,sku_snapshot,product_name_ar_snapshot,product_name_en_snapshot,size_snapshot,color_snapshot,quantity,unit_price_minor,unit_cost_snapshot_minor,line_discount_minor,tax_rate_bps,tax_minor,line_total_minor) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
                        ("line-old", self.branch, "invoice-old", self.variant, "SH-M-BL", "قميص", "Shirt", "M", "أزرق", 1, 80000, 40000, 0, 0, 0, 80000))
        self.db.execute("INSERT INTO inventory_movements(id,branch_id,variant_id,bucket,movement_type,quantity_delta,unit_cost_minor,source_type,source_id,movement_group_id,idempotency_key,actor_user_id,created_at) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?)",
                        ("sale-move", self.branch, self.variant, "SELLABLE", "SALE", -1, 40000, "INVOICE", "invoice-old", "invoice-old", "sale-move-key", self.user, self.now))
        self.db.execute("INSERT INTO return_documents(id,branch_id,return_no,original_invoice_id,kind,status,reason,refund_due_minor,created_by,created_at,idempotency_key,request_hash) VALUES(?,?,?,?, 'EXCHANGE','COMPLETED',?, ?,?,?,?,?)",
                        ("exchange-return", self.branch, "EX-001", "invoice-old", "Size change", 80000, self.user, self.now, "exchange-once", "hash-exchange"))
        self.db.execute("INSERT INTO return_lines(id,branch_id,return_document_id,original_invoice_line_id,variant_id,quantity,reason,item_condition,disposition,refund_line_minor) VALUES(?,?,?,?,?,?,?,?,?,?)",
                        ("return-line", self.branch, "exchange-return", "line-old", self.variant, 1, "Size change", "NEW_TAGS", "RESTOCK", 80000))
        self.db.execute("INSERT INTO inventory_movements(id,branch_id,variant_id,bucket,movement_type,quantity_delta,source_type,source_id,movement_group_id,idempotency_key,actor_user_id,created_at) VALUES(?,?,?,?,?,?,?,?,?,?,?,?)",
                        ("return-move", self.branch, self.variant, "SELLABLE", "RETURN_RESTOCK", 1, "EXCHANGE", "exchange-return", "exchange-return", "return-move-key", self.user, self.now))
        self.db.execute("INSERT INTO invoices(id,branch_id,invoice_no,idempotency_key,request_hash,cashier_user_id,status,currency_code,tax_rate_bps,prices_include_tax,subtotal_minor,item_discount_minor,invoice_discount_minor,tax_minor,total_minor,created_at) VALUES(?,?,?,?,?,?,'COMPLETED','EGP',0,1,90000,0,0,0,90000,?)",
                        ("invoice-new", self.branch, "NS-NEW", "exchange-sale", "hash-exchange", self.user, self.now))
        self.db.execute("INSERT INTO invoice_lines(id,branch_id,invoice_id,variant_id,sku_snapshot,product_name_ar_snapshot,product_name_en_snapshot,size_snapshot,color_snapshot,quantity,unit_price_minor,unit_cost_snapshot_minor,line_discount_minor,tax_rate_bps,tax_minor,line_total_minor) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
                        ("line-new", self.branch, "invoice-new", self.variant, "SH-M-BL", "قميص", "Shirt", "M", "أزرق", 1, 90000, 40000, 0, 0, 0, 90000))
        self.db.execute("INSERT INTO payments(id,branch_id,invoice_id,method,amount_minor,idempotency_key,created_at) VALUES(?,?,?,?,?,?,?)",
                        ("exchange-credit", self.branch, "invoice-new", "STORE_CREDIT", 80000, "exchange-credit-key", self.now))
        self.db.execute("INSERT INTO payments(id,branch_id,invoice_id,method,amount_minor,idempotency_key,created_at) VALUES(?,?,?,?,?,?,?)",
                        ("exchange-cash", self.branch, "invoice-new", "CASH", 10000, "exchange-cash-key", self.now))
        self.db.execute("INSERT INTO inventory_movements(id,branch_id,variant_id,bucket,movement_type,quantity_delta,unit_cost_minor,source_type,source_id,movement_group_id,idempotency_key,actor_user_id,created_at) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?)",
                        ("replacement-move", self.branch, self.variant, "SELLABLE", "SALE", -1, 40000, "EXCHANGE_INVOICE", "invoice-new", "invoice-new", "replacement-move-key", self.user, self.now))
        self.db.execute("INSERT INTO exchange_groups(id,branch_id,return_document_id,replacement_invoice_id,price_difference_minor,settlement_method,created_at,created_by) VALUES(?,?,?,?,?,?,?,?)",
                        ("exchange-group", self.branch, "exchange-return", "invoice-new", 10000, "CASH", self.now, self.user))
        quantity = self.db.execute("SELECT quantity FROM inventory_balances WHERE branch_id=? AND variant_id=? AND bucket='SELLABLE'", (self.branch, self.variant)).fetchone()[0]
        self.assertEqual(quantity, 2)
        self.assertEqual(self.db.execute("SELECT COUNT(*) FROM exchange_groups WHERE return_document_id='exchange-return'").fetchone()[0], 1)
        self.assertEqual(self.db.execute("SELECT SUM(amount_minor) FROM payments WHERE invoice_id='invoice-new'").fetchone()[0], 90000)




    def test_local_trial_state_is_singleton_and_append_only(self):
        self.db.execute("INSERT INTO local_trial_state(singleton,device_fingerprint,trial_started_at_utc,trial_expires_at_utc,grace_expires_at_utc,created_at) VALUES(1,?,?,?,?,?)",
                        ("fingerprint-1", "2026-10-10T10:00:00Z", "2026-10-17T10:00:00Z", "2026-10-31T10:00:00Z", "2026-10-10T10:00:00Z"))
        with self.assertRaises(sqlite3.IntegrityError):
            self.db.execute("INSERT INTO local_trial_state(singleton,device_fingerprint,trial_started_at_utc,trial_expires_at_utc,grace_expires_at_utc,created_at) VALUES(1,?,?,?,?,?)",
                            ("fingerprint-2", "2026-11-10T10:00:00Z", "2026-11-17T10:00:00Z", "2026-11-30T10:00:00Z", "2026-11-10T10:00:00Z"))
        with self.assertRaises(sqlite3.IntegrityError):
            self.db.execute("UPDATE local_trial_state SET trial_expires_at_utc='2099-01-01T00:00:00Z' WHERE singleton=1")
        with self.assertRaises(sqlite3.IntegrityError):
            self.db.execute("DELETE FROM local_trial_state WHERE singleton=1")

    def test_local_trial_state_records_the_device_fingerprint(self):
        self.db.execute("INSERT INTO local_trial_state(singleton,device_fingerprint,trial_started_at_utc,trial_expires_at_utc,grace_expires_at_utc,created_at) VALUES(1,?,?,?,?,?)",
                        ("abc123", "2026-10-10T10:00:00Z", "2026-10-17T10:00:00Z", "2026-10-31T10:00:00Z", "2026-10-10T10:00:00Z"))
        row = self.db.execute("SELECT device_fingerprint, trial_expires_at_utc FROM local_trial_state WHERE singleton=1").fetchone()
        self.assertEqual(row[0], "abc123")
        self.assertEqual(row[1], "2026-10-17T10:00:00Z")


if __name__ == "__main__":
    unittest.main(verbosity=2)


class OptionalTaxAndLoyaltyTests(unittest.TestCase):
    """The tax switch, loyalty ledger, and store credit behave as documented."""

    def setUp(self):
        self.db = sqlite3.connect(":memory:")
        self.db.execute("PRAGMA foreign_keys=ON")
        for migration in (MIGRATION, MIGRATION_2, MIGRATION_3, MIGRATION_4, MIGRATION_5, MIGRATION_6, MIGRATION_7):
            self.db.executescript(migration.read_text(encoding="utf-8"))
        self.now = "2026-11-21T09:00:00Z"
        self.branch = uuid.uuid4().hex
        self.user = uuid.uuid4().hex
        self.customer = uuid.uuid4().hex
        self.db.execute(
            "INSERT INTO branches(id,code,name_ar,name_en,currency_code,tax_rate_bps,prices_include_tax,timezone,created_at)"
            " VALUES(?,?,?,?,?,?,?,?,?)",
            (self.branch, "MAIN", "فرع", "Branch", "EGP", 0, 0, "Africa/Cairo", self.now),
        )
        self.db.execute(
            "INSERT INTO users(id,username,display_name,password_phc,created_at,updated_at) VALUES(?,?,?,?,?,?)",
            (self.user, "owner", "Owner", "phc", self.now, self.now),
        )
        self.db.execute(
            "INSERT INTO customers(id,branch_id,consent_status,created_at,updated_at) VALUES(?,?,?,?,?)",
            (self.customer, self.branch, "GIVEN", self.now, self.now),
        )

    def test_new_branch_defaults_to_tax_off(self):
        rate, mode = self.db.execute(
            "SELECT tax_rate_bps, prices_include_tax FROM branches WHERE id=?", (self.branch,)
        ).fetchone()
        self.assertEqual(rate, 0)
        self.assertEqual(mode, 0)

    def test_migration_0004_does_not_force_fourteen_percent(self):
        # 0004 stamps prices_include_tax but must leave the rate untouched.
        self.db.execute("UPDATE branches SET prices_include_tax=NULL, tax_rate_bps=0 WHERE id=?", (self.branch,))
        self.db.executescript(MIGRATION_4.read_text(encoding="utf-8"))
        rate, mode = self.db.execute(
            "SELECT tax_rate_bps, prices_include_tax FROM branches WHERE id=?", (self.branch,)
        ).fetchone()
        self.assertEqual(rate, 0)
        self.assertEqual(mode, 0)

    def test_store_credit_balance_is_derived_from_note_and_spends(self):
        credit = uuid.uuid4().hex
        self.db.execute(
            "INSERT INTO store_credits(id,branch_id,customer_id,amount_minor,created_at) VALUES(?,?,?,?,?)",
            (credit, self.branch, self.customer, 25000, self.now),
        )
        self.assertEqual(
            self.db.execute(
                "SELECT SUM(amount_minor) FROM store_credits WHERE branch_id=? AND customer_id=?",
                (self.branch, self.customer),
            ).fetchone()[0],
            25000,
        )

    def test_loyalty_ledger_is_append_only(self):
        row = uuid.uuid4().hex
        self.db.execute(
            "INSERT INTO loyalty_ledger(id,branch_id,customer_id,points_delta,event_type,reason,"
            "actor_user_id,idempotency_key,created_at) VALUES(?,?,?,?,?,?,?,?,?)",
            (row, self.branch, self.customer, 120, "EARN", "sale", self.user, "k1", self.now),
        )
        with self.assertRaises(sqlite3.IntegrityError):
            self.db.execute("UPDATE loyalty_ledger SET points_delta=0 WHERE id=?", (row,))
        with self.assertRaises(sqlite3.IntegrityError):
            self.db.execute("DELETE FROM loyalty_ledger WHERE id=?", (row,))

    def test_import_batch_meta_requires_a_batch(self):
        with self.assertRaises(sqlite3.IntegrityError):
            self.db.execute(
                "INSERT INTO import_batch_meta(branch_id,import_batch_id,source_kind) VALUES(?,?,?)",
                (self.branch, uuid.uuid4().hex, "CSV"),
            )

    def test_clock_high_water_is_single_row(self):
        self.db.execute(
            "INSERT INTO clock_high_water(singleton,high_water_utc,updated_at) VALUES(1,?,?)",
            (self.now, self.now),
        )
        with self.assertRaises(sqlite3.IntegrityError):
            self.db.execute(
                "INSERT INTO clock_high_water(singleton,high_water_utc,updated_at) VALUES(2,?,?)",
                (self.now, self.now),
            )


class ImmutableDocumentTests(unittest.TestCase):
    """A posted invoice is the record of a real sale; it must not be rewritable."""

    def setUp(self):
        self.db = sqlite3.connect(":memory:")
        self.db.execute("PRAGMA foreign_keys=ON")
        for migration in (MIGRATION, MIGRATION_2, MIGRATION_3, MIGRATION_4, MIGRATION_5, MIGRATION_6, MIGRATION_7):
            self.db.executescript(migration.read_text(encoding="utf-8"))
        self.now = "2026-11-21T09:00:00Z"
        self.branch = "branch-immutable"
        self.user = "user-immutable"
        self.invoice = "invoice-immutable"
        self.db.execute(
            "INSERT INTO branches(id,code,name_ar,name_en,currency_code,tax_rate_bps,prices_include_tax,timezone,created_at)"
            " VALUES(?,?,?,?,?,?,?,?,?)",
            (self.branch, "MAIN", "فرع", "Main", "EGP", 0, 0, "Africa/Cairo", self.now),
        )
        self.db.execute(
            "INSERT INTO users(id,username,display_name,password_phc,created_at,updated_at) VALUES(?,?,?,?,?,?)",
            (self.user, "owner", "Owner", "phc", self.now, self.now),
        )
        self.db.execute(
            "INSERT INTO invoices(id,branch_id,invoice_no,idempotency_key,request_hash,cashier_user_id,status,"
            "currency_code,tax_rate_bps,prices_include_tax,subtotal_minor,item_discount_minor,invoice_discount_minor,"
            "tax_minor,total_minor,created_at)"
            " VALUES(?,?,?,?,?,?,'COMPLETED','EGP',0,0,?,0,0,0,?,?)",
            (self.invoice, self.branch, "NS-20261121-0001", "idem-00000001", "hash", self.user, 10_000, 10_000, self.now),
        )
        self.db.commit()

    def tearDown(self):
        self.db.close()

    def test_the_invoice_total_cannot_be_rewritten(self):
        with self.assertRaises(sqlite3.IntegrityError):
            self.db.execute("UPDATE invoices SET total_minor=1 WHERE id=?", (self.invoice,))

    def test_the_invoice_number_and_tax_cannot_be_rewritten(self):
        with self.assertRaises(sqlite3.IntegrityError):
            self.db.execute("UPDATE invoices SET invoice_no='NS-20260101-0001' WHERE id=?", (self.invoice,))
        with self.assertRaises(sqlite3.IntegrityError):
            self.db.execute("UPDATE invoices SET tax_minor=1400, tax_rate_bps=1400 WHERE id=?", (self.invoice,))

    def test_a_completed_invoice_cannot_be_deleted(self):
        with self.assertRaises(sqlite3.IntegrityError):
            self.db.execute("DELETE FROM invoices WHERE id=?", (self.invoice,))

    def test_the_return_transitions_are_allowed_but_not_reversed(self):
        self.db.execute("UPDATE invoices SET status='PARTIALLY_RETURNED' WHERE id=?", (self.invoice,))
        self.db.execute("UPDATE invoices SET status='RETURNED' WHERE id=?", (self.invoice,))
        with self.assertRaises(sqlite3.IntegrityError):
            self.db.execute("UPDATE invoices SET status='COMPLETED' WHERE id=?", (self.invoice,))

    def test_a_void_needs_a_completed_invoice(self):
        self.db.execute("UPDATE invoices SET status='VOIDED' WHERE id=?", (self.invoice,))
        self.db.commit()
        self.assertEqual(
            self.db.execute("SELECT status FROM invoices WHERE id=?", (self.invoice,)).fetchone()[0],
            "VOIDED",
        )

    def test_a_return_document_cannot_be_rewritten_or_deleted(self):
        self.db.execute(
            "INSERT INTO return_documents(id,branch_id,return_no,original_invoice_id,customer_id,kind,status,"
            "reason,refund_due_minor,created_by,created_at,idempotency_key,request_hash)"
            " VALUES(?,?,?,?,NULL,'RETURN','COMPLETED',?,?,?,?,?,?)",
            ("return-1", self.branch, "RT-20261121-0001", self.invoice, "changed mind", 10_000, self.user, self.now, "ret-idem-01", "hash"),
        )
        with self.assertRaises(sqlite3.IntegrityError):
            self.db.execute("UPDATE return_documents SET refund_due_minor=1 WHERE id='return-1'")
        with self.assertRaises(sqlite3.IntegrityError):
            self.db.execute("DELETE FROM return_documents WHERE id='return-1'")
