import { describe, expect, it } from 'vitest';
import {
  computeInvoiceTotals,
  invoiceTax,
  invoiceTotal,
  roundToWholeUnit,
  roundedRatio,
  settlePayments,
} from './totals';

describe('roundToWholeUnit', () => {
  it('rounds to the nearest EGP with halves going up', () => {
    expect(roundToWholeUnit(1240)).toBe(1200);
    expect(roundToWholeUnit(1250)).toBe(1300);
    expect(roundToWholeUnit(49)).toBe(0);
    expect(roundToWholeUnit(50)).toBe(100);
    expect(roundToWholeUnit(0)).toBe(0);
  });

  it('refuses a negative amount', () => {
    expect(() => roundToWholeUnit(-1)).toThrow('AMOUNT_INVALID');
  });
});

describe('invoiceTax', () => {
  it('rounds the whole bill once instead of rounding each line', () => {
    // Two lines of 88.60 EGP at 14 %, prices exclusive. Rounding per line gives
    // 12 + 12 = 24.00 EGP, which is not what the invoice must charge.
    const net = 2 * 8860;
    expect(invoiceTax(net, 1400, false)).toBe(2500);
    expect(invoiceTotal(net, invoiceTax(net, 1400, false), false)).toBe(net + 2500);
  });

  it('carves the tax out of a tax-inclusive price', () => {
    expect(invoiceTax(10000, 1400, true)).toBe(1200);
    expect(invoiceTotal(10000, 1200, true)).toBe(10000);
  });

  it('charges nothing when the owner has switched tax off', () => {
    expect(invoiceTax(123456, 0, false)).toBe(0);
    expect(invoiceTax(123456, 0, true)).toBe(0);
    expect(invoiceTotal(123456, 0, false)).toBe(123456);
  });
});

describe('computeInvoiceTotals', () => {
  it('matches the backend for a discounted, taxed bill', () => {
    const totals = computeInvoiceTotals(
      [
        { priceMinor: 8860, quantity: 1, discountBps: 0 },
        { priceMinor: 8860, quantity: 1, discountBps: 0 },
      ],
      1000, // 10 % off the bill
      1400,
      false,
    );
    expect(totals.subtotal).toBe(17720);
    expect(totals.invoiceDiscount).toBe(1772);
    expect(totals.net).toBe(15948);
    // 15,948 * 14 % = 2,232.72 piastres -> 22.33 EGP -> 22.00 EGP.
    expect(totals.tax).toBe(2200);
    expect(totals.total).toBe(18148);
    expect(totals.total).toBe(totals.net + totals.tax);
  });

  it('keeps the total equal to the shelf price when tax is inclusive', () => {
    const totals = computeInvoiceTotals([{ priceMinor: 10000, quantity: 1, discountBps: 0 }], 0, 1400, true);
    expect(totals.total).toBe(10000);
    expect(totals.tax).toBe(1200);
  });
});

describe('settlePayments', () => {
  it('never records more cash than the bill', () => {
    const { applied, changeMinor } = settlePayments(
      [{ method: 'CASH', amountMinor: 20000 }],
      18148,
    );
    expect(applied).toEqual([{ method: 'CASH', amountMinor: 18148 }]);
    expect(changeMinor).toBe(1852);
  });

  it('splits a payment and gives change only on the cash part', () => {
    const { applied, changeMinor } = settlePayments(
      [
        { method: 'CARD', amountMinor: 10000 },
        { method: 'CASH', amountMinor: 10000 },
      ],
      18148,
    );
    expect(applied).toEqual([
      { method: 'CARD', amountMinor: 10000 },
      { method: 'CASH', amountMinor: 8148 },
    ]);
    expect(changeMinor).toBe(1852);
  });

  it('rejects a card payment above the total, because no change can be returned', () => {
    expect(() => settlePayments([{ method: 'CARD', amountMinor: 20000 }], 18148)).toThrow('PAYMENT_TOTAL_MISMATCH');
  });

  it('rejects an underpayment', () => {
    expect(() => settlePayments([{ method: 'CASH', amountMinor: 100 }], 18148)).toThrow('PAYMENT_TOTAL_MISMATCH');
  });
});

describe('roundedRatio', () => {
  it('rounds halves up and rejects a zero denominator', () => {
    expect(roundedRatio(5, 2)).toBe(3);
    expect(roundedRatio(4, 2)).toBe(2);
    expect(() => roundedRatio(1, 0)).toThrow('AMOUNT_INVALID');
  });
});
