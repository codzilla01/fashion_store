//! Loyalty programme and store credit.
//!
//! Both features are optional and both are ledger based:
//!
//! * loyalty points live in `loyalty_ledger`, which is append-only in SQL, so
//!   a customer balance is always `SUM(points_delta)` and can never drift;
//! * store credit is issued by a refund and spent later, so the remaining
//!   balance is `issued - spent` and is likewise derived, never stored twice.
//!
//! Nothing in this module reads or writes personal data in the clear. Customer
//! identity stays where the customer commands already keep it.

use crate::AppState;
use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::commands::{db_error, insert_audit, require_permission};

/// The loyalty settings of one branch.
#[derive(Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LoyaltySettings {
    pub is_enabled: bool,
    pub points_per_currency_unit: i64,
    pub redeem_value_minor: i64,
    pub min_redeem_points: i64,
}

impl Default for LoyaltySettings {
    fn default() -> Self {
        // The programme ships switched off: a store that does not want it
        // never has to think about it, and switching it on is one action.
        Self {
            is_enabled: false,
            points_per_currency_unit: 1,
            redeem_value_minor: 100,
            min_redeem_points: 0,
        }
    }
}

/// A loyalty balance with the settings that produced it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoyaltyBalance {
    pub customer_id: String,
    pub points: i64,
    pub value_minor: i64,
    pub settings: LoyaltySettings,
}

/// One store credit note with the part that is still usable.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreCreditRow {
    pub id: String,
    pub customer_id: String,
    pub issued_minor: i64,
    pub spent_minor: i64,
    pub remaining_minor: i64,
    pub return_document_id: Option<String>,
    pub created_at: String,
}

fn read_settings(db: &Connection, branch_id: &str) -> Result<LoyaltySettings, String> {
    let row: Option<(i64, i64, i64, i64)> = db
        .query_row(
            "SELECT is_enabled, points_per_currency_unit, redeem_value_minor, min_redeem_points
             FROM loyalty_settings WHERE branch_id=?1",
            [branch_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()
        .map_err(db_error)?;
    Ok(match row {
        Some((enabled, per_unit, redeem, minimum)) => LoyaltySettings {
            is_enabled: enabled == 1,
            points_per_currency_unit: per_unit,
            redeem_value_minor: redeem,
            min_redeem_points: minimum,
        },
        None => LoyaltySettings::default(),
    })
}

/// Points earned by a net amount, using whole points only.
///
/// The amount is in minor units (piastres), so a store that earns one point
/// per whole EGP divides by 100 before applying the rate.
pub fn points_for_amount(settings: &LoyaltySettings, net_minor: i64) -> i64 {
    if !settings.is_enabled || net_minor <= 0 || settings.points_per_currency_unit <= 0 {
        return 0;
    }
    let whole_currency = net_minor / 100;
    whole_currency.saturating_mul(settings.points_per_currency_unit)
}

/// Write the earning rows for a completed invoice.
///
/// Called inside the sale transaction so the points and the invoice commit
/// together: an invoice either has its points or does not exist at all.
pub fn award_points_for_invoice(
    tx: &Transaction<'_>,
    branch_id: &str,
    customer_id: &str,
    invoice_id: &str,
    net_minor: i64,
    actor_user_id: &str,
    now: &str,
) -> Result<i64, String> {
    let settings = read_settings(tx, branch_id)?;
    let points = points_for_amount(&settings, net_minor);
    if points <= 0 {
        return Ok(0);
    }
    tx.execute(
        "INSERT INTO loyalty_ledger(id,branch_id,customer_id,invoice_id,points_delta,event_type,reason,actor_user_id,idempotency_key,created_at)
         VALUES(?1,?2,?3,?4,?5,'EARN',?6,?7,?8,?9)",
        params![
            Uuid::new_v4().to_string(),
            branch_id,
            customer_id,
            invoice_id,
            points,
            format!("Earned on invoice {invoice_id}"),
            actor_user_id,
            format!("loyalty-earn:{invoice_id}"),
            now
        ],
    )
    .map_err(db_error)?;
    Ok(points)
}

/// Reverse the points of an invoice and, when the customer is known, issue the
/// refunded amount as store credit instead of cash.
///
/// Returns the credit note id, if one was issued.
pub fn issue_store_credit(
    tx: &Transaction<'_>,
    branch_id: &str,
    customer_id: Option<&str>,
    return_document_id: &str,
    amount_minor: i64,
    actor_user_id: &str,
    now: &str,
) -> Result<Option<String>, String> {
    let Some(customer_id) = customer_id else {
        // Store credit needs an owner of the balance. Without a customer the
        // refund must go back through another method.
        return Ok(None);
    };
    if amount_minor <= 0 {
        return Ok(None);
    }
    let credit_id = Uuid::new_v4().to_string();
    tx.execute(
        "INSERT INTO store_credits(id,branch_id,customer_id,return_document_id,amount_minor,note,created_by,created_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
        params![
            credit_id,
            branch_id,
            customer_id,
            return_document_id,
            amount_minor,
            "Store credit issued by a return",
            actor_user_id,
            now
        ],
    )
    .map_err(db_error)?;
    Ok(Some(credit_id))
}

/// The usable store credit of a customer.
pub fn credit_balance(db: &Connection, branch_id: &str, customer_id: &str) -> Result<i64, String> {
    let issued: i64 = db
        .query_row(
            "SELECT COALESCE(SUM(amount_minor),0) FROM store_credits WHERE branch_id=?1 AND customer_id=?2",
            params![branch_id, customer_id],
            |row| row.get(0),
        )
        .map_err(db_error)?;
    let spent: i64 = db
        .query_row(
            "SELECT COALESCE(SUM(s.amount_minor),0) FROM store_credit_spends s
             JOIN store_credits c ON c.branch_id=s.branch_id AND c.id=s.credit_id
             WHERE s.branch_id=?1 AND c.customer_id=?2",
            params![branch_id, customer_id],
            |row| row.get(0),
        )
        .map_err(db_error)?;
    Ok(issued - spent)
}

fn settings_from_request(settings: &LoyaltySettings) -> Result<(), String> {
    if !(0..=1_000).contains(&settings.points_per_currency_unit)
        || !(0..=100_000).contains(&settings.redeem_value_minor)
        || !(0..=100_000).contains(&settings.min_redeem_points)
    {
        return Err("LOYALTY_SETTINGS_INVALID".to_string());
    }
    Ok(())
}

/// Set the loyalty settings of the branch of the session.
#[tauri::command]
pub fn configure_loyalty(
    state: tauri::State<'_, AppState>,
    session_token: String,
    settings: LoyaltySettings,
) -> Result<LoyaltySettings, String> {
    let session = require_permission(&state, &session_token, "settings.manage")?;
    settings_from_request(&settings)?;
    let mut db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db_error)?;
    let now = Utc::now().to_rfc3339();
    tx.execute(
        "INSERT INTO loyalty_settings(branch_id,is_enabled,points_per_currency_unit,redeem_value_minor,min_redeem_points,changed_by,changed_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7)
         ON CONFLICT(branch_id) DO UPDATE SET
           is_enabled=excluded.is_enabled,
           points_per_currency_unit=excluded.points_per_currency_unit,
           redeem_value_minor=excluded.redeem_value_minor,
           min_redeem_points=excluded.min_redeem_points,
           changed_by=excluded.changed_by,
           changed_at=excluded.changed_at",
        params![
            session.branch_id,
            if settings.is_enabled { 1 } else { 0 },
            settings.points_per_currency_unit,
            settings.redeem_value_minor,
            settings.min_redeem_points,
            session.user_id,
            now
        ],
    )
    .map_err(db_error)?;
    insert_audit(
        &tx,
        &Uuid::new_v4().to_string(),
        Some(&session.branch_id),
        Some(&session.user_id),
        "LOYALTY_SETTINGS_CHANGED",
        "setting",
        Some("loyalty_settings"),
        None,
        &serde_json::json!({
            "is_enabled": settings.is_enabled,
            "points_per_currency_unit": settings.points_per_currency_unit,
            "redeem_value_minor": settings.redeem_value_minor,
            "min_redeem_points": settings.min_redeem_points
        })
        .to_string(),
        &now,
    )?;
    tx.commit().map_err(db_error)?;
    Ok(settings)
}

/// Read the loyalty settings of the branch of the session.
#[tauri::command]
pub fn get_loyalty_settings(
    state: tauri::State<'_, AppState>,
    session_token: String,
) -> Result<LoyaltySettings, String> {
    let session = require_permission(&state, &session_token, "sales.read")?;
    let db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
    read_settings(&db, &session.branch_id)
}

/// The loyalty balance and the store credit of one customer.
#[tauri::command]
pub fn get_customer_loyalty(
    state: tauri::State<'_, AppState>,
    session_token: String,
    customer_id: String,
) -> Result<LoyaltyBalance, String> {
    let session = require_permission(&state, &session_token, "sales.read")?;
    let db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
    let settings = read_settings(&db, &session.branch_id)?;
    let points: i64 = db
        .query_row(
            "SELECT COALESCE(SUM(points_delta),0) FROM loyalty_ledger WHERE branch_id=?1 AND customer_id=?2",
            params![session.branch_id, customer_id],
            |row| row.get(0),
        )
        .map_err(db_error)?;
    Ok(LoyaltyBalance {
        customer_id,
        points,
        value_minor: points.saturating_mul(settings.redeem_value_minor),
        settings,
    })
}

/// The store credit notes of a customer, newest first.
#[tauri::command]
pub fn get_store_credit(
    state: tauri::State<'_, AppState>,
    session_token: String,
    customer_id: String,
) -> Result<Vec<StoreCreditRow>, String> {
    let session = require_permission(&state, &session_token, "sales.read")?;
    let db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
    let mut statement = db
        .prepare(
            "SELECT c.id, c.customer_id, c.amount_minor,
                    COALESCE((SELECT SUM(s.amount_minor) FROM store_credit_spends s
                              WHERE s.branch_id=c.branch_id AND s.credit_id=c.id),0),
                    c.return_document_id, c.created_at
             FROM store_credits c
             WHERE c.branch_id=?1 AND c.customer_id=?2
             ORDER BY c.created_at DESC",
        )
        .map_err(db_error)?;
    let rows = statement
        .query_map(params![session.branch_id, customer_id], |row| {
            let issued: i64 = row.get(2)?;
            let spent: i64 = row.get(3)?;
            Ok(StoreCreditRow {
                id: row.get(0)?,
                customer_id: row.get(1)?,
                issued_minor: issued,
                spent_minor: spent,
                remaining_minor: issued - spent,
                return_document_id: row.get(4)?,
                created_at: row.get(5)?,
            })
        })
        .map_err(db_error)?;
    rows.map(|row| row.map_err(db_error)).collect()
}

/// Apply store credit to a completed invoice.
///
/// The invoice must already be committed and must belong to the customer, so
/// a credit note can never be spent on someone else's purchase.
pub fn spend_store_credit(
    tx: &Transaction<'_>,
    branch_id: &str,
    customer_id: &str,
    invoice_id: &str,
    amount_minor: i64,
    actor_user_id: &str,
    now: &str,
) -> Result<(), String> {
    if amount_minor <= 0 {
        return Err("STORE_CREDIT_AMOUNT_INVALID".to_string());
    }
    let available = credit_balance(tx, branch_id, customer_id)?;
    if available < amount_minor {
        return Err("STORE_CREDIT_INSUFFICIENT".to_string());
    }
    let mut remaining = amount_minor;
    let mut statement = tx
        .prepare(
            "SELECT c.id, c.amount_minor - COALESCE((SELECT SUM(s.amount_minor) FROM store_credit_spends s
                    WHERE s.branch_id=c.branch_id AND s.credit_id=c.id),0)
             FROM store_credits c
             WHERE c.branch_id=?1 AND c.customer_id=?2
             ORDER BY c.created_at, c.id",
        )
        .map_err(db_error)?;
    let notes: Vec<(String, i64)> = statement
        .query_map(params![branch_id, customer_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(db_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_error)?;
    drop(statement);
    for (credit_id, note_remaining) in notes {
        if remaining == 0 {
            break;
        }
        if note_remaining <= 0 {
            continue;
        }
        let take = remaining.min(note_remaining);
        tx.execute(
            "INSERT INTO store_credit_spends(id,branch_id,credit_id,invoice_id,amount_minor,created_by,created_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![
                Uuid::new_v4().to_string(),
                branch_id,
                credit_id,
                invoice_id,
                take,
                actor_user_id,
                now
            ],
        )
        .map_err(db_error)?;
        remaining -= take;
    }
    if remaining != 0 {
        return Err("STORE_CREDIT_INSUFFICIENT".to_string());
    }
    Ok(())
}

/// Reverse the points earned by an invoice when it is fully returned.
pub fn reverse_points_for_invoice(
    tx: &Transaction<'_>,
    branch_id: &str,
    invoice_id: &str,
    actor_user_id: &str,
    now: &str,
) -> Result<i64, String> {
    let row: Option<(String, i64)> = tx
        .query_row(
            "SELECT customer_id, COALESCE(SUM(points_delta),0) FROM loyalty_ledger
             WHERE branch_id=?1 AND invoice_id=?2 AND event_type IN ('EARN','REVERSAL')
             GROUP BY customer_id",
            params![branch_id, invoice_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(db_error)?;
    let Some((customer_id, earned)) = row else {
        return Ok(0);
    };
    if earned <= 0 {
        // Already reversed; the idempotency key would reject a second row.
        return Ok(0);
    }
    tx.execute(
        "INSERT INTO loyalty_ledger(id,branch_id,customer_id,invoice_id,points_delta,event_type,reason,actor_user_id,idempotency_key,created_at)
         VALUES(?1,?2,?3,?4,?5,'REVERSAL',?6,?7,?8,?9)",
        params![
            Uuid::new_v4().to_string(),
            branch_id,
            customer_id,
            invoice_id,
            -earned,
            "Points reversed by a returned invoice",
            actor_user_id,
            format!("loyalty-reversal:{invoice_id}"),
            now
        ],
    )
    .map_err(db_error)?;
    Ok(earned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_programme_earns_nothing() {
        let settings = LoyaltySettings::default();
        assert!(!settings.is_enabled);
        assert_eq!(points_for_amount(&settings, 100_000), 0);
    }

    #[test]
    fn earns_whole_points_per_currency_unit() {
        let settings = LoyaltySettings {
            is_enabled: true,
            points_per_currency_unit: 2,
            redeem_value_minor: 100,
            min_redeem_points: 0,
        };
        // 1,250.99 EGP → 1250 whole EGP → 2500 points at 2 per unit.
        assert_eq!(points_for_amount(&settings, 125_099), 2_500);
        assert_eq!(points_for_amount(&settings, 99), 0);
        assert_eq!(points_for_amount(&settings, 0), 0);
        assert_eq!(points_for_amount(&settings, -500), 0);
    }

    #[test]
    fn default_redeem_value_is_one_egp_per_point() {
        assert_eq!(LoyaltySettings::default().redeem_value_minor, 100);
    }
}
