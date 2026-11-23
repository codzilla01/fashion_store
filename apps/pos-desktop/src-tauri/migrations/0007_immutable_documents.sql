-- Close the immutability gap on the documents that are already posted.
--
-- `invoice_lines` and `payments` are protected against UPDATE and DELETE, but
-- the invoice header and the return documents were not. An operator with a
-- SQLite console and the DPAPI key could therefore rewrite an invoice total,
-- change a return's refund, or delete a completed invoice and leave its
-- payments and stock movements pointing at nothing. The ledgers stayed
-- append-only, so the history could no longer be reconciled.
--
-- A completed document is now immutable. Status changes are still allowed
-- through a narrow set of transitions, because a return and a void have to
-- move an invoice out of COMPLETED:
--
--   COMPLETED          -> PARTIALLY_RETURNED | RETURNED | VOIDED
--   PARTIALLY_RETURNED -> RETURNED              (further partial returns)
--
-- `BEFORE UPDATE OF ...` restricts the guard to the financial columns, so a
-- later migration that adds an unrelated column (a printed flag, a note) is not
-- blocked by this trigger.
--
-- Nothing is dropped: DELETE is refused outright on both tables.

CREATE TRIGGER IF NOT EXISTS invoices_financials_are_immutable
BEFORE UPDATE OF
  branch_id,
  invoice_no,
  idempotency_key,
  request_hash,
  customer_id,
  cashier_user_id,
  currency_code,
  tax_rate_bps,
  prices_include_tax,
  subtotal_minor,
  item_discount_minor,
  invoice_discount_minor,
  tax_minor,
  total_minor,
  discount_reason,
  created_at
ON invoices
BEGIN
  SELECT RAISE(ABORT, 'invoice totals are immutable');
END;

CREATE TRIGGER IF NOT EXISTS invoices_status_transitions_are_legal
BEFORE UPDATE OF status ON invoices
WHEN NEW.status <> OLD.status
 AND NOT (
   OLD.status = 'COMPLETED'
   AND NEW.status IN ('PARTIALLY_RETURNED', 'RETURNED', 'VOIDED')
 )
 AND NOT (
   OLD.status = 'PARTIALLY_RETURNED'
   AND NEW.status = 'RETURNED'
 )
BEGIN
  SELECT RAISE(ABORT, 'invoice status transition is not allowed');
END;

CREATE TRIGGER IF NOT EXISTS invoices_no_delete
BEFORE DELETE ON invoices
BEGIN
  SELECT RAISE(ABORT, 'invoices are never deleted');
END;

CREATE TRIGGER IF NOT EXISTS return_documents_are_immutable
BEFORE UPDATE OF
  branch_id,
  return_no,
  original_invoice_id,
  customer_id,
  kind,
  reason,
  refund_due_minor,
  created_by,
  created_at,
  idempotency_key,
  request_hash
ON return_documents
BEGIN
  SELECT RAISE(ABORT, 'return documents are immutable');
END;

-- The only legal change to a return is voiding it, and never the other way
-- round: a refund that has been paid out cannot be un-voided.
CREATE TRIGGER IF NOT EXISTS return_documents_status_transitions_are_legal
BEFORE UPDATE OF status ON return_documents
WHEN NEW.status <> OLD.status AND NOT (OLD.status = 'COMPLETED' AND NEW.status = 'VOIDED')
BEGIN
  SELECT RAISE(ABORT, 'return status transition is not allowed');
END;

CREATE TRIGGER IF NOT EXISTS return_documents_no_delete
BEFORE DELETE ON return_documents
BEGIN
  SELECT RAISE(ABORT, 'return documents are never deleted');
END;
