/**
 * The till as the cashier actually uses it.
 *
 * The arithmetic is unit tested in `totals.test.ts`; this file drives the real
 * `App` component in a DOM, the way a cashier drives it with a mouse, to prove
 * that what the screen shows is what the sale is committed with.
 */
import { describe, expect, it } from 'vitest';
import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import App from './App';

/** The total shown in the "amount due" row. */
function amountDue(container: HTMLElement): string {
  const row = container.querySelector('.due-row');
  if (!row) throw new Error('the amount due row is missing');
  const cells = within(row as HTMLElement).getAllByText(/.+/);
  return cells[cells.length - 1].textContent ?? '';
}

/** Add the first size/colour variant of the first catalog product. */
async function addFirstVariant(user: ReturnType<typeof userEvent.setup>) {
  // The app opens on the manager overview; the till is one click away.
  await user.click(screen.getByRole('button', { name: /Open store|عرض المتجر/i }));
  const card = document.querySelector('.product-card') as HTMLElement;
  await user.click(card);
  const size = await screen.findByRole('button', { name: 'M' });
  await user.click(size);
  const confirm = screen.getByRole('button', { name: /إضافة للسلة|Add to cart/i });
  await user.click(confirm);
}

describe('the till', () => {
  it('starts with tax switched off, as the owner configured it', async () => {
    const user = userEvent.setup();
    const { container } = render(<App />);
    await user.click(screen.getByRole('button', { name: 'EN' }));
    await addFirstVariant(user);
    // 895.00 EGP with no tax configured: the shelf price is the amount due.
    expect(amountDue(container)).toContain('895');
  });

  it('shows the change when the cashier takes a larger note', async () => {
    const user = userEvent.setup();
    const { container } = render(<App />);
    await user.click(screen.getByRole('button', { name: 'EN' }));
    await addFirstVariant(user);

    const amountInput = container.querySelector('.payment-amount input') as HTMLInputElement;
    await user.clear(amountInput);
    await user.type(amountInput, '1000');

    // 1000 tendered against 895 due: the remaining row turns into an
    // overpayment instead of blocking the sale.
    const remaining = container.querySelector('.remaining-row strong');
    expect(remaining?.className).toContain('overpaid');
    expect(remaining?.textContent).toContain('105');
  });

  it('applies a line discount and keeps the totals consistent', async () => {
    const user = userEvent.setup();
    const { container } = render(<App />);
    await user.click(screen.getByRole('button', { name: 'EN' }));
    await addFirstVariant(user);

    await user.click(container.querySelector('.line-discount') as HTMLElement);
    // The built-in quick discount is 5 %: 895.00 becomes 850.25.
    const discountRow = container.querySelector('.discount-total span:last-child');
    expect(discountRow?.textContent).toContain('45');
    expect(amountDue(container)).toContain('850');
  });
});
