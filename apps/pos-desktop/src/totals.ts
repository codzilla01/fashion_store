/**
 * Invoice arithmetic shared by the till, the exchange screen and the tests.
 *
 * The rules mirror `apps/pos-desktop/src-tauri/src/totals.rs` one for one. They
 * are duplicated in TypeScript on purpose: the till computes the preview while
 * the cashier is still scanning, and the Rust side recomputes and validates the
 * same figures inside the transaction. Both must agree, so both are tested.
 */

export const MINOR_PER_UNIT = 100;

/** Round to the nearest whole currency unit; half a unit rounds up. */
export function roundToWholeUnit(minor: number): number {
  if (!Number.isFinite(minor) || minor < 0) {
    throw new Error('AMOUNT_INVALID');
  }
  return Math.round(minor / MINOR_PER_UNIT) * MINOR_PER_UNIT;
}

/** Round `numerator / denominator` to the nearest integer, halves up. */
export function roundedRatio(numerator: number, denominator: number): number {
  if (!Number.isFinite(numerator) || !Number.isFinite(denominator) || denominator <= 0 || numerator < 0) {
    throw new Error('AMOUNT_INVALID');
  }
  return Math.round(numerator / denominator);
}

/**
 * Tax carried by the whole bill, rounded to policy.
 *
 * Rounding per line and then adding the lines does not give the same answer as
 * rounding the bill, which is why this takes the net invoice value and not a
 * list of lines.
 */
export function invoiceTax(netMinor: number, taxRateBps: number, pricesIncludeTax: boolean): number {
  if (!Number.isFinite(netMinor) || netMinor < 0) {
    throw new Error('AMOUNT_INVALID');
  }
  const rate = Math.min(10_000, Math.max(0, Math.trunc(taxRateBps || 0)));
  if (rate === 0) return 0;
  const exact = pricesIncludeTax
    ? roundedRatio(netMinor * rate, 10_000 + rate)
    : roundedRatio(netMinor * rate, 10_000);
  return roundToWholeUnit(exact);
}

/** The amount the customer pays. */
export function invoiceTotal(netMinor: number, taxMinor: number, pricesIncludeTax: boolean): number {
  if (!Number.isFinite(netMinor) || !Number.isFinite(taxMinor) || netMinor < 0 || taxMinor < 0) {
    throw new Error('AMOUNT_INVALID');
  }
  return pricesIncludeTax ? netMinor : netMinor + taxMinor;
}

export type SaleLine = { priceMinor: number; quantity: number; discountBps: number };

export type InvoiceTotals = {
  subtotal: number;
  itemDiscount: number;
  invoiceDiscount: number;
  discount: number;
  net: number;
  tax: number;
  total: number;
};

/**
 * Totals of a bill. Identical to the Rust path, including the order in which
 * the rounding happens: line discounts, then the invoice discount, then the tax
 * on the remaining net value.
 */
export function computeInvoiceTotals(
  lines: SaleLine[],
  invoiceDiscountBps: number,
  taxRateBps: number,
  pricesIncludeTax: boolean,
): InvoiceTotals {
  let subtotal = 0;
  let itemDiscount = 0;
  for (const line of lines) {
    const gross = line.priceMinor * line.quantity;
    subtotal += gross;
    itemDiscount += roundedRatio(gross * line.discountBps, 10_000);
  }
  const afterItemDiscount = Math.max(0, subtotal - itemDiscount);
  const invoiceDiscount = roundedRatio(afterItemDiscount * invoiceDiscountBps, 10_000);
  const net = Math.max(0, afterItemDiscount - invoiceDiscount);
  const tax = invoiceTax(net, taxRateBps, pricesIncludeTax);
  return {
    subtotal,
    itemDiscount,
    invoiceDiscount,
    discount: itemDiscount + invoiceDiscount,
    net,
    tax,
    total: invoiceTotal(net, tax, pricesIncludeTax),
  };
}

/**
 * What the till records for a set of tendered payments.
 *
 * Non-cash methods settle exactly; cash may be tendered above the remainder and
 * the excess is the change handed back, so the invoice is never recorded as
 * paid more than its total.
 */
export function settlePayments(
  payments: { method: string; amountMinor: number }[],
  totalMinor: number,
): { applied: { method: string; amountMinor: number }[]; changeMinor: number } {
  const nonCash = payments
    .filter((payment) => payment.method !== 'CASH')
    .reduce((sum, payment) => sum + payment.amountMinor, 0);
  if (nonCash > totalMinor) throw new Error('PAYMENT_TOTAL_MISMATCH');
  let cashDue = totalMinor - nonCash;
  let changeMinor = 0;
  const applied: { method: string; amountMinor: number }[] = [];
  for (const payment of payments) {
    if (payment.method === 'CASH') {
      const used = Math.min(payment.amountMinor, cashDue);
      changeMinor += payment.amountMinor - used;
      cashDue -= used;
      if (used > 0) applied.push({ method: 'CASH', amountMinor: used });
    } else {
      applied.push({ method: payment.method, amountMinor: payment.amountMinor });
    }
  }
  if (cashDue > 0) throw new Error('PAYMENT_TOTAL_MISMATCH');
  return { applied, changeMinor };
}
