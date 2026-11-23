//! Invoice totals, shared by the sale and the exchange path.
//!
//! # Why the tax is computed on the net total, not per line
//!
//! The approved policy rounds the tax to the nearest whole EGP (one EGP is
//! 100 minor units). Rounding each line and then adding the results does not
//! give the same answer as rounding the whole invoice, so the till could show
//! one total and charge another:
//!
//! ```text
//! two lines of 88.60 EGP at 14 %, prices exclusive
//!   per line  : round(12.40) + round(12.40) = 12 + 12 = 24.00 EGP
//!   whole bill: round(24.80)                = 25.00 EGP
//! ```
//!
//! The invoice stores the rounded whole-bill tax, and the difference is then
//! spread across the lines with the largest-remainder method. Every line still
//! carries a whole number of piastres, the `invoice_lines` tax column still
//! sums exactly to `invoices.tax_minor`, and returns that read a line's own
//! snapshot stay consistent with what was charged.

/// Round a non-negative minor-unit amount to the nearest whole currency unit.
///
/// Half a unit rounds up: 12.40 EGP becomes 12, 12.50 EGP becomes 13.
pub fn round_to_whole_unit(minor: i64) -> Result<i64, String> {
    if minor < 0 {
        return Err("AMOUNT_INVALID".to_string());
    }
    let rounded = ((minor as i128 + 50) / 100) * 100;
    i64::try_from(rounded).map_err(|_| "AMOUNT_OVERFLOW".to_string())
}

/// Round `numerator / denominator` to the nearest integer, halves up.
pub fn rounded_ratio(numerator: i128, denominator: i128) -> Result<i64, String> {
    if denominator <= 0 || numerator < 0 {
        return Err("AMOUNT_INVALID".to_string());
    }
    let rounded = (numerator + denominator / 2) / denominator;
    i64::try_from(rounded).map_err(|_| "AMOUNT_OVERFLOW".to_string())
}

fn checked_i64(value: i128) -> Result<i64, String> {
    i64::try_from(value).map_err(|_| "AMOUNT_OVERFLOW".to_string())
}

/// Tax carried by the whole invoice, in minor units, rounded to policy.
///
/// `net_minor` is the invoice value after every discount. When prices include
/// tax the tax is carved out of that value; otherwise it is added on top.
pub fn invoice_tax(
    net_minor: i64,
    tax_rate_bps: i64,
    prices_include_tax: bool,
) -> Result<i64, String> {
    if net_minor < 0 {
        return Err("AMOUNT_INVALID".to_string());
    }
    let rate = tax_rate_bps.clamp(0, 10_000) as i128;
    if rate == 0 {
        return Ok(0);
    }
    let net = net_minor as i128;
    let exact = if prices_include_tax {
        rounded_ratio(net * rate, 10_000 + rate)?
    } else {
        rounded_ratio(net * rate, 10_000)?
    };
    round_to_whole_unit(exact)
}

/// The amount the customer pays.
pub fn invoice_total(
    net_minor: i64,
    tax_minor: i64,
    prices_include_tax: bool,
) -> Result<i64, String> {
    if net_minor < 0 || tax_minor < 0 {
        return Err("AMOUNT_INVALID".to_string());
    }
    if prices_include_tax {
        Ok(net_minor)
    } else {
        net_minor
            .checked_add(tax_minor)
            .ok_or_else(|| "AMOUNT_OVERFLOW".to_string())
    }
}

/// Spread `total` over `weights` so the parts sum to exactly `total`.
///
/// Uses the largest-remainder method, which is deterministic: the largest
/// fractional parts get the residual units, ties go to the earlier line. This
/// is what keeps `SUM(invoice_lines.tax_minor) == invoices.tax_minor`.
pub fn allocate_exactly(weights: &[i64], total: i64) -> Result<Vec<i64>, String> {
    if total < 0 || weights.iter().any(|weight| *weight < 0) {
        return Err("AMOUNT_INVALID".to_string());
    }
    if weights.is_empty() {
        return if total == 0 {
            Ok(Vec::new())
        } else {
            Err("ALLOCATION_IMPOSSIBLE".to_string())
        };
    }
    let total_i128 = total as i128;
    let weight_sum: i128 = weights.iter().map(|weight| *weight as i128).sum();
    if weight_sum == 0 {
        return if total == 0 {
            Ok(vec![0; weights.len()])
        } else {
            Err("ALLOCATION_IMPOSSIBLE".to_string())
        };
    }
    let mut parts = Vec::with_capacity(weights.len());
    let mut remainders: Vec<(usize, i128)> = Vec::with_capacity(weights.len());
    let mut allocated = 0i128;
    for (index, weight) in weights.iter().enumerate() {
        // Expressing the share over `weight_sum` keeps the residual smaller
        // than the number of lines, so it can always be handed out.
        let numerator = *weight as i128 * total_i128;
        let part = numerator / weight_sum;
        parts.push(checked_i64(part)?);
        allocated += part;
        remainders.push((index, numerator % weight_sum));
    }
    let residual = total_i128 - allocated;
    if residual < 0 || residual > weights.len() as i128 {
        return Err("ALLOCATION_FAILED".to_string());
    }
    remainders.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    for (index, _) in remainders.iter().take(residual as usize) {
        parts[*index] = parts[*index]
            .checked_add(1)
            .ok_or_else(|| "AMOUNT_OVERFLOW".to_string())?;
    }
    debug_assert_eq!(
        parts.iter().map(|part| *part as i128).sum::<i128>(),
        total_i128
    );
    Ok(parts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tax_rounds_to_the_nearest_whole_egp() {
        // Minor units are piastres: 100 minor = 1 EGP.
        assert_eq!(round_to_whole_unit(1_240), Ok(1_200));
        assert_eq!(round_to_whole_unit(1_250), Ok(1_300));
        assert_eq!(round_to_whole_unit(49), Ok(0));
        assert_eq!(round_to_whole_unit(50), Ok(100));
        assert_eq!(round_to_whole_unit(0), Ok(0));
        assert!(round_to_whole_unit(-1).is_err());
    }

    #[test]
    fn a_whole_bill_rounds_once_and_not_per_line() {
        // Two lines of 88.60 EGP at 14 %, prices exclusive. Rounding each
        // line would give 24.00 EGP, which is the defect this guards against.
        let net = 2 * 8_860;
        let tax = invoice_tax(net, 1_400, false).unwrap();
        assert_eq!(tax, 2_500);
        assert_eq!(invoice_total(net, tax, false), Ok(net + 2_500));
    }

    #[test]
    fn inclusive_prices_carve_the_tax_out_of_the_total() {
        // 100.00 EGP inclusive of 14 % carries 12.28 EGP of tax, which the
        // policy rounds to 12.00 EGP.
        assert_eq!(invoice_tax(10_000, 1_400, true), Ok(1_200));
        // The customer pays the shelf price, never shelf price plus tax.
        assert_eq!(invoice_total(10_000, 1_200, true), Ok(10_000));
    }

    #[test]
    fn a_zero_rate_costs_nothing_and_is_not_an_error() {
        assert_eq!(invoice_tax(123_456, 0, false), Ok(0));
        assert_eq!(invoice_tax(123_456, 0, true), Ok(0));
        assert_eq!(invoice_total(123_456, 0, false), Ok(123_456));
    }

    #[test]
    fn allocation_sums_back_to_the_whole_bill() {
        let weights = [8_860, 8_860];
        let tax = invoice_tax(17_720, 1_400, false).unwrap();
        let parts = allocate_exactly(&weights, tax).unwrap();
        assert_eq!(parts.iter().sum::<i64>(), tax);
        assert_eq!(parts.len(), 2);
        // The two lines are identical, so the split must be even.
        assert_eq!(parts, vec![1_250, 1_250]);
    }

    #[test]
    fn allocation_hands_the_residual_to_the_largest_remainder() {
        // 100 minor split three ways is 34/33/33 with ties going to the
        // earlier line, and the parts always add back up.
        let parts = allocate_exactly(&[1, 1, 1], 100).unwrap();
        assert_eq!(parts.iter().sum::<i64>(), 100);
        assert_eq!(parts, vec![34, 33, 33]);
    }

    #[test]
    fn allocation_refuses_impossible_requests() {
        assert!(allocate_exactly(&[], 100).is_err());
        assert!(allocate_exactly(&[0, 0], 100).is_err());
        assert!(allocate_exactly(&[1], -1).is_err());
        // A negative weight is meaningless, but a single non-zero weight takes
        // the whole amount.
        assert!(allocate_exactly(&[-1], 10).is_err());
        assert_eq!(allocate_exactly(&[1], 10), Ok(vec![10]));
        assert_eq!(allocate_exactly(&[], 0), Ok(Vec::new()));
    }
}
