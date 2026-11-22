use crate::{AppState, Session, db};
use argon2::{Algorithm, Argon2, Params, Version};
use chrono::{Local, Utc};
use password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    time::{Duration, Instant},
};
use uuid::Uuid;

const SESSION_IDLE: Duration = Duration::from_secs(5 * 60);
const MAIN_BRANCH_CODE: &str = "MAIN";

/// One season row: id, Arabic name, English name, start date, end date.
type SeasonRow = (String, String, String, Option<String>, Option<String>);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    pub needs_owner_setup: bool,
    pub tax_configuration_required: bool,
    /// Whether the store currently charges tax. Optional by design.
    pub tax_enabled: bool,
    pub tax_rate_bps: i64,
    pub prices_include_tax: Option<bool>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthResult {
    pub session_token: String,
    pub display_name: String,
    pub branch_id: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CatalogVariant {
    pub id: String,
    pub size: String,
    pub color: String,
    pub color_ar: String,
    pub color_en: String,
    pub color_hex: String,
    pub stock: i64,
    pub sku: String,
    pub barcode: String,
    pub price_minor: i64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CatalogProduct {
    pub id: String,
    pub name_ar: String,
    pub name_en: String,
    pub category_ar: String,
    pub category_en: String,
    pub brand: String,
    pub price_minor: i64,
    pub variants: Vec<CatalogVariant>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogOption {
    pub id: String,
    pub code: Option<String>,
    pub name_ar: String,
    pub name_en: String,
    pub color_hex: Option<String>,
    pub profile_code: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogSetupOptions {
    pub categories: Vec<CatalogOption>,
    pub brands: Vec<CatalogOption>,
    pub seasons: Vec<CatalogOption>,
    pub sizes: Vec<CatalogOption>,
    pub colors: Vec<CatalogOption>,
}

#[derive(Deserialize, Serialize)]
pub struct CreateSaleRequest {
    pub idempotency_key: String,
    pub customer_id: Option<String>,
    pub lines: Vec<SaleLineInput>,
    pub invoice_discount_bps: i64,
    pub discount_reason: Option<String>,
    pub payments: Vec<PaymentInput>,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct SaleLineInput {
    pub variant_id: String,
    pub quantity: i64,
    pub item_discount_bps: i64,
}

#[derive(Deserialize, Serialize)]
pub struct PaymentInput {
    pub method: String,
    pub amount_minor: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaleResult {
    pub invoice_id: String,
    pub invoice_no: String,
    pub total_minor: i64,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NewVariantInput {
    pub size_id: String,
    pub color_id: String,
    pub sku: String,
    pub barcode: Option<String>,
    pub cost_override_minor: Option<i64>,
    pub price_override_minor: Option<i64>,
    pub reorder_level: i64,
    pub opening_quantity: i64,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateProductRequest {
    pub idempotency_key: String,
    pub category_id: String,
    pub brand_id: Option<String>,
    pub season_id: Option<String>,
    pub name_ar: String,
    pub name_en: String,
    pub description_ar: Option<String>,
    pub description_en: Option<String>,
    pub base_cost_minor: i64,
    pub base_price_minor: i64,
    pub variants: Vec<NewVariantInput>,
    pub opening_reason: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateProductResult {
    pub product_id: String,
    pub variant_count: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InventoryRow {
    pub variant_id: String,
    pub sku: String,
    pub barcode: Option<String>,
    pub product_name_ar: String,
    pub product_name_en: String,
    pub size_code: String,
    pub color_ar: String,
    pub color_en: String,
    pub price_minor: i64,
    pub sellable: i64,
    pub damaged: i64,
    pub reorder_level: i64,
    pub needs_reorder: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StockAdjustmentRequest {
    pub variant_id: String,
    pub bucket: String,
    pub quantity_delta: i64,
    pub reason: String,
    pub idempotency_key: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdjustmentResult {
    pub movement_id: String,
    pub new_quantity: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateCustomerRequest {
    pub name: String,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub notes: Option<String>,
    pub size_preferences: Option<String>,
    pub consent_status: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomerSummary {
    pub id: String,
    pub name: String,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub consent_status: String,
    pub loyalty_points: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateUserRequest {
    pub username: String,
    pub display_name: String,
    pub password: String,
    pub role_code: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserOption {
    pub id: String,
    pub username: String,
    pub display_name: String,
    pub role_code: String,
    pub is_active: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeUserRoleRequest {
    pub user_id: String,
    pub role_code: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetUserActiveRequest {
    pub user_id: String,
    pub is_active: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BranchOption {
    pub id: String,
    pub code: String,
    pub name_ar: String,
    pub name_en: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateBranchRequest {
    pub code: String,
    pub name_ar: String,
    pub name_en: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssignBranchUserRequest {
    pub branch_id: String,
    pub user_id: String,
    pub role_code: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupOperationResult {
    pub status: String,
    pub path: String,
    pub byte_size: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupHistoryRow {
    pub id: String,
    pub status: String,
    pub backup_kind: String,
    pub filename: Option<String>,
    pub byte_size: Option<u64>,
    pub started_at: String,
    pub finished_at: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSupplierRequest {
    pub supplier_code: String,
    pub name: String,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub address: Option<String>,
    pub tax_registration: Option<String>,
    pub notes: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SupplierOption {
    pub id: String,
    pub supplier_code: String,
    pub name: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PurchaseOrderResult {
    pub purchase_order_id: String,
    pub order_no: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoodsReceiptResult {
    pub receipt_id: String,
    pub receipt_no: String,
    pub received_line_count: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PurchaseOrderLineOpen {
    pub id: String,
    pub sku: String,
    pub product_name_ar: String,
    pub product_name_en: String,
    pub size_code: String,
    pub color_ar: String,
    pub quantity_open: i64,
    pub unit_cost_minor: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PurchaseOrderOpen {
    pub id: String,
    pub order_no: String,
    pub supplier_name: String,
    pub status: String,
    pub lines: Vec<PurchaseOrderLineOpen>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatePurchaseOrderRequest {
    pub supplier_id: String,
    pub order_no: String,
    pub expected_at: Option<String>,
    pub notes: Option<String>,
    pub lines: Vec<PurchaseOrderLineInput>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PurchaseOrderLineInput {
    pub variant_id: String,
    pub quantity: i64,
    pub unit_cost_minor: i64,
    pub tax_rate_bps: i64,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReceiveGoodsRequest {
    pub purchase_order_id: String,
    pub receipt_no: String,
    pub supplier_invoice_ref: Option<String>,
    pub idempotency_key: String,
    pub lines: Vec<GoodsReceiptLineInput>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoodsReceiptLineInput {
    pub purchase_order_line_id: String,
    pub quantity_received: i64,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReturnRequest {
    pub invoice_no: String,
    pub idempotency_key: String,
    pub reason: String,
    pub lines: Vec<ReturnLineInput>,
    pub refunds: Vec<RefundInput>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefundInput {
    pub method: String,
    pub amount_minor: i64,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReturnLineInput {
    pub invoice_line_id: String,
    pub quantity: i64,
    pub reason: String,
    pub item_condition: String,
    pub disposition: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReturnResult {
    pub return_id: String,
    pub return_no: String,
    pub refund_due_minor: i64,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateExchangeRequest {
    pub invoice_no: String,
    pub idempotency_key: String,
    pub reason: String,
    pub returned_lines: Vec<ReturnLineInput>,
    pub replacement_lines: Vec<SaleLineInput>,
    pub invoice_discount_bps: i64,
    pub discount_reason: Option<String>,
    pub payments: Vec<PaymentInput>,
    pub refunds: Vec<RefundInput>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExchangeResult {
    pub return_id: String,
    pub return_no: String,
    pub replacement_invoice_id: String,
    pub replacement_invoice_no: String,
    pub returned_value_minor: i64,
    pub replacement_total_minor: i64,
    pub price_difference_minor: i64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ReturnableInvoiceLine {
    pub invoice_line_id: String,
    pub variant_id: String,
    pub sku: String,
    pub product_name_ar: String,
    pub product_name_en: String,
    pub size: String,
    pub color: String,
    pub quantity: i64,
    pub returned_quantity: i64,
    pub remaining_quantity: i64,
    pub line_total_minor: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReturnableInvoice {
    pub invoice_id: String,
    pub invoice_no: String,
    pub status: String,
    pub created_at: String,
    pub lines: Vec<ReturnableInvoiceLine>,
}

/// The return policy the owner controls from the control panel.
///
/// The type lives in the owner panel module because the panel writes it and
/// the returns desk reads it; there is only one shape for both.
pub type ReturnPolicyConfig = crate::owner_panel::ReturnPolicy;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SalesSummary {
    pub invoice_count: i64,
    pub gross_minor: i64,
    pub discounts_minor: i64,
    pub tax_minor: i64,
    pub returns_minor: i64,
    pub net_minor: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfitSummary {
    pub revenue_minor: i64,
    pub cost_minor: i64,
    pub returned_minor: i64,
    pub restocked_cost_minor: i64,
    pub gross_profit_minor: i64,
    pub margin_bps: i64,
}

/// Load the stored data of an invoice so it can be printed.
///
/// Printing reads straight from the encrypted database, so a receipt can be
/// reprinted long after the sale and without an open session.
fn load_receipt_data(
    state: &AppState,
    db: &Connection,
    branch_id: &str,
    invoice_id: &str,
) -> Result<crate::printing::ReceiptData, String> {
    //  0 invoice_no          1 currency_code      2 subtotal_minor
    //  3 item_discount      4 invoice_discount    5 tax_minor
    //  6 total_minor        7 tax_rate_bps        8 prices_include_tax
    //  9 created_at        10 branch_name_ar     11 branch_name_en
    // 12 cashier_name      13 customer_name_ciphertext
    let receipt = db
        .query_row(
            "SELECT i.invoice_no, i.currency_code, i.subtotal_minor, i.item_discount_minor,
                    i.invoice_discount_minor, i.tax_minor, i.total_minor, i.tax_rate_bps,
                    i.prices_include_tax, i.created_at, b.name_ar, b.name_en,
                    COALESCE(u.display_name,''), c.name_ciphertext
             FROM invoices i
             JOIN branches b ON b.id=i.branch_id
             JOIN users u ON u.id=i.cashier_user_id
             LEFT JOIN customers c ON c.branch_id=i.branch_id AND c.id=i.customer_id
             WHERE i.branch_id=?1 AND i.id=?2",
            params![branch_id, invoice_id],
            |row| {
                Ok(crate::printing::ReceiptData {
                    invoice_no: row.get(0)?,
                    branch_name_ar: row.get(10)?,
                    branch_name_en: row.get(11)?,
                    currency_code: row.get(1)?,
                    cashier_name: row.get(12)?,
                    created_at_utc: row.get(9)?,
                    customer_name: None,
                    subtotal_minor: row.get(2)?,
                    item_discount_minor: row.get(3)?,
                    invoice_discount_minor: row.get(4)?,
                    tax_minor: row.get(5)?,
                    total_minor: row.get(6)?,
                    tax_rate_bps: row.get(7)?,
                    prices_include_tax: row.get::<_, i64>(8)? != 0,
                    lines: Vec::new(),
                    payments: Vec::new(),
                })
            },
        )
        .map_err(|_| "INVOICE_NOT_FOUND".to_string())?;

    let customer_name: Option<String> = db
        .query_row(
            "SELECT c.name_ciphertext FROM invoices i
             JOIN customers c ON c.branch_id=i.branch_id AND c.id=i.customer_id
             WHERE i.branch_id=?1 AND i.id=?2",
            params![branch_id, invoice_id],
            |row| row.get::<_, Option<Vec<u8>>>(0),
        )
        .optional()
        .map_err(|_| "INVOICE_NOT_FOUND".to_string())?
        .flatten()
        .and_then(|blob| {
            db::decrypt_personal_field(&state.personal_data_key, &blob)
                .ok()
                .and_then(|clear| String::from_utf8(clear).ok())
        });

    let lines = {
        let mut statement = db
            .prepare(
                "SELECT product_name_ar_snapshot, product_name_en_snapshot, sku_snapshot,
                        size_snapshot, color_snapshot, quantity, unit_price_minor,
                        line_discount_minor, tax_minor, line_total_minor
                 FROM invoice_lines WHERE branch_id=?1 AND invoice_id=?2
                 ORDER BY product_name_ar_snapshot, id",
            )
            .map_err(db_error)?;
        let rows = statement
            .query_map(params![branch_id, invoice_id], |row| {
                let ar: String = row.get(0)?;
                let en: String = row.get(1)?;
                Ok(crate::printing::ReceiptLine {
                    product: if ar.is_empty() { en } else { ar },
                    quantity: row.get::<_, i64>(5)? as u32,
                    unit_price_minor: row.get(6)?,
                    discount_minor: row.get(7)?,
                    tax_minor: row.get(8)?,
                    total_minor: row.get(9)?,
                    sku: row.get(2)?,
                    size: row.get(3)?,
                    color: row.get(4)?,
                })
            })
            .map_err(db_error)?;
        let mut collected = Vec::new();
        for row in rows {
            collected.push(row.map_err(db_error)?);
        }
        collected
    };

    let payments = {
        let mut statement = db
            .prepare(
                "SELECT method, amount_minor FROM payments
                 WHERE branch_id=?1 AND invoice_id=?2 ORDER BY created_at, id",
            )
            .map_err(db_error)?;
        let rows = statement
            .query_map(params![branch_id, invoice_id], |row| {
                Ok(crate::printing::ReceiptPayment {
                    method: row.get(0)?,
                    amount_minor: row.get(1)?,
                })
            })
            .map_err(db_error)?;
        let mut collected = Vec::new();
        for row in rows {
            collected.push(row.map_err(db_error)?);
        }
        collected
    };

    let mut receipt = receipt;
    receipt.lines = lines;
    receipt.payments = payments;
    receipt.customer_name = customer_name;
    Ok(receipt)
}

/// Print the receipt of a completed invoice.
///
/// `mode` selects between sending straight to the default printer and opening
/// the system print dialog.
#[tauri::command]
pub fn print_invoice(
    state: tauri::State<'_, AppState>,
    session_token: String,
    invoice_id: String,
    mode: Option<String>,
    paper: Option<String>,
) -> Result<crate::printing::PrintOutcome, String> {
    let session = require_permission(&state, &session_token, "sales.read")?;
    let db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
    let receipt = load_receipt_data(&state, &db, &session.branch_id, &invoice_id)?;
    drop(db);

    let width = match paper.as_deref() {
        Some("80") => crate::printing::WIDTH_80,
        _ => crate::printing::WIDTH_58,
    };
    let print_mode = match mode.as_deref() {
        Some("dialog") => crate::printing::PrintMode::Dialog,
        _ => crate::printing::PrintMode::Direct,
    };
    crate::printing::print_receipt(&receipt, width, print_mode)
}

/// Render the receipt of an invoice as text, without sending it to a printer.
///
/// This is used by the on-screen receipt preview and by the tests.
#[tauri::command]
pub fn preview_invoice_receipt(
    state: tauri::State<'_, AppState>,
    session_token: String,
    invoice_id: String,
    paper: Option<String>,
) -> Result<String, String> {
    let session = require_permission(&state, &session_token, "sales.read")?;
    let db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
    let receipt = load_receipt_data(&state, &db, &session.branch_id, &invoice_id)?;
    drop(db);
    let width = match paper.as_deref() {
        Some("80") => crate::printing::WIDTH_80,
        _ => crate::printing::WIDTH_58,
    };
    Ok(crate::printing::render_text(&receipt, width))
}

#[tauri::command]
pub fn get_license_status(state: tauri::State<'_, AppState>) -> crate::license::LicenseStatus {
    let mut status = crate::license::current_status(&state);
    if status.valid
        && let Err(error) = crate::license::record_trusted_time(&state, &status)
    {
        status.valid = false;
        status.status_code = error;
    }
    status
}

#[tauri::command]
pub fn create_activation_request() -> Result<naseej_license_format::ActivationRequest, String> {
    crate::license::activation_request()
}

#[tauri::command]
pub fn import_activation_license(
    state: tauri::State<'_, AppState>,
    license_json: String,
) -> Result<crate::license::LicenseStatus, String> {
    let status = crate::license::import_signed_license(&state, &license_json)?;
    crate::license::record_trusted_time(&state, &status)?;
    Ok(status)
}

#[tauri::command]
pub fn app_status(state: tauri::State<'_, AppState>) -> Result<AppStatus, String> {
    let db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
    let users: i64 = db
        .query_row("SELECT COUNT(*) FROM users", [], |row| row.get(0))
        .map_err(db_error)?;
    let (tax_rate_bps, tax_mode): (i64, Option<i64>) = db
        .query_row(
            "SELECT tax_rate_bps, prices_include_tax FROM branches WHERE code=?1",
            [MAIN_BRANCH_CODE],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(db_error)?;
    // Tax is optional: only a customer-facing VAT obligation makes it
    // mandatory, so the app never blocks the first sale on a tax decision.
    // The owner switches it on or off from the control panel at any time.
    Ok(AppStatus {
        needs_owner_setup: users == 0,
        tax_configuration_required: false,
        tax_enabled: tax_rate_bps > 0,
        tax_rate_bps,
        prices_include_tax: tax_mode.map(|mode| mode == 1),
    })
}

#[tauri::command]
pub fn bootstrap_owner(
    state: tauri::State<'_, AppState>,
    username: String,
    display_name: String,
    password: String,
) -> Result<AuthResult, String> {
    let username = username.trim();
    let display_name = display_name.trim();
    if username.len() < 3
        || username.len() > 64
        || display_name.is_empty()
        || display_name.len() > 100
    {
        return Err("INVALID_OWNER_DETAILS".to_string());
    }
    if password.chars().count() < 12 || password.len() > 256 {
        return Err("PASSWORD_POLICY_FAILED".to_string());
    }
    let password_phc = hash_password(&password)?;
    drop(password);

    let mut db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db_error)?;
    let count: i64 = tx
        .query_row("SELECT COUNT(*) FROM users", [], |row| row.get(0))
        .map_err(db_error)?;
    if count != 0 {
        return Err("OWNER_ALREADY_CONFIGURED".to_string());
    }
    let branch_id: String = tx
        .query_row(
            "SELECT id FROM branches WHERE code=?1",
            [MAIN_BRANCH_CODE],
            |row| row.get(0),
        )
        .map_err(db_error)?;
    let user_id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    tx.execute(
        "INSERT INTO users(id, username, display_name, password_phc, is_active, created_at, updated_at) VALUES(?1, ?2, ?3, ?4, 1, ?5, ?5)",
        params![user_id, username, display_name, password_phc, now],
    ).map_err(|_| "OWNER_CREATE_FAILED".to_string())?;
    tx.execute(
        "INSERT INTO user_branch_roles(user_id, branch_id, role_code, granted_by, granted_at) VALUES(?1, ?2, 'owner', ?1, ?3)",
        params![user_id, branch_id, now],
    ).map_err(db_error)?;
    let audit_id = Uuid::new_v4().to_string();
    insert_audit(
        &tx,
        &audit_id,
        Some(&branch_id),
        Some(&user_id),
        "OWNER_BOOTSTRAPPED",
        "user",
        Some(&user_id),
        Some("Initial owner setup"),
        "{}",
        &now,
    )?;
    tx.commit().map_err(db_error)?;
    drop(db);
    Ok(issue_session(
        &state,
        user_id,
        branch_id,
        display_name.to_string(),
    ))
}

#[tauri::command]
pub fn login(
    state: tauri::State<'_, AppState>,
    username: String,
    password: String,
) -> Result<AuthResult, String> {
    let username = username.trim();
    let user_record = {
        let db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
        db.query_row(
            "SELECT u.id, u.display_name, u.password_phc, u.is_active, u.locked_until,
                    (SELECT ubr.branch_id FROM user_branch_roles ubr WHERE ubr.user_id=u.id ORDER BY ubr.branch_id LIMIT 1)
             FROM users u WHERE u.username=?1 COLLATE NOCASE",
            [username],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, i64>(3)?, row.get::<_, Option<String>>(4)?, row.get::<_, Option<String>>(5)?)),
        ).optional().map_err(db_error)?
    };

    let Some((user_id, display_name, password_phc, active, locked_until, branch)) = user_record
    else {
        return Err("LOGIN_FAILED".to_string());
    };
    if active != 1 {
        return Err("LOGIN_FAILED".to_string());
    }
    if let Some(until) = locked_until
        && until > Utc::now().to_rfc3339()
    {
        return Err("ACCOUNT_TEMPORARILY_LOCKED".to_string());
    }
    let parsed =
        PasswordHash::new(&password_phc).map_err(|_| "PASSWORD_HASH_INVALID".to_string())?;
    if argon2_instance()?
        .verify_password(password.as_bytes(), &parsed)
        .is_err()
    {
        let db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
        let now = Utc::now().to_rfc3339();
        let current: i64 = db
            .query_row(
                "SELECT failed_logins FROM users WHERE id=?1",
                [&user_id],
                |row| row.get(0),
            )
            .map_err(db_error)?;
        let next = current.saturating_add(1);
        let lock_until = if next >= 5 {
            Some((Utc::now() + chrono::Duration::minutes(10)).to_rfc3339())
        } else {
            None
        };
        db.execute(
            "UPDATE users SET failed_logins=?1, locked_until=?2, updated_at=?3 WHERE id=?4",
            params![if next >= 5 { 0 } else { next }, lock_until, now, user_id],
        )
        .map_err(db_error)?;
        return Err("LOGIN_FAILED".to_string());
    }
    let branch_id = branch.ok_or_else(|| "USER_HAS_NO_BRANCH".to_string())?;
    {
        let db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
        db.execute(
            "UPDATE users SET failed_logins=0, locked_until=NULL, updated_at=?1 WHERE id=?2",
            params![Utc::now().to_rfc3339(), user_id],
        )
        .map_err(db_error)?;
    }
    Ok(issue_session(&state, user_id, branch_id, display_name))
}

#[tauri::command]
pub fn logout(state: tauri::State<'_, AppState>, session_token: String) -> Result<(), String> {
    state
        .sessions
        .lock()
        .map_err(|_| "SESSION_LOCK_FAILED".to_string())?
        .remove(&session_token);
    Ok(())
}

#[tauri::command]
pub fn get_catalog(
    state: tauri::State<'_, AppState>,
    session_token: String,
) -> Result<Vec<CatalogProduct>, String> {
    let session = require_permission(&state, &session_token, "sales.read")?;
    let db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
    let mut statement = db.prepare(
        "SELECT p.id, p.name_ar, p.name_en, c.name_ar, c.name_en, COALESCE(b.name_ar,''), p.base_price_minor,
                v.id, s.size_code, co.name_ar, co.name_en, COALESCE(co.hex_color,'#aab4ac'), v.sku,
                COALESCE(v.barcode,''), COALESCE(v.price_override_minor,p.base_price_minor),
                COALESCE(ib.quantity,0)
         FROM products p
         JOIN categories c ON c.branch_id=p.branch_id AND c.id=p.category_id
         LEFT JOIN brands b ON b.branch_id=p.branch_id AND b.id=p.brand_id
         JOIN product_variants v ON v.branch_id=p.branch_id AND v.product_id=p.id AND v.is_active=1
         JOIN sizes s ON s.branch_id=v.branch_id AND s.id=v.size_id
         JOIN colors co ON co.branch_id=v.branch_id AND co.id=v.color_id
         LEFT JOIN inventory_balances ib ON ib.branch_id=v.branch_id AND ib.variant_id=v.id AND ib.bucket='SELLABLE'
         WHERE p.branch_id=?1 AND p.is_active=1 AND c.is_active=1
         ORDER BY c.sort_order, p.name_ar, s.sort_order, co.name_ar"
    ).map_err(db_error)?;
    let rows = statement
        .query_map([&session.branch_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, String>(8)?,
                row.get::<_, String>(9)?,
                row.get::<_, String>(10)?,
                row.get::<_, String>(11)?,
                row.get::<_, String>(12)?,
                row.get::<_, String>(13)?,
                row.get::<_, i64>(14)?,
                row.get::<_, i64>(15)?,
            ))
        })
        .map_err(db_error)?;

    let mut products: Vec<CatalogProduct> = Vec::new();
    let mut positions: HashMap<String, usize> = HashMap::new();
    for row in rows {
        let (
            id,
            name_ar,
            name_en,
            category_ar,
            category_en,
            brand,
            base_price,
            variant_id,
            size,
            color_ar,
            color_en,
            color_hex,
            sku,
            barcode,
            price,
            stock,
        ) = row.map_err(db_error)?;
        let index = *positions.entry(id.clone()).or_insert_with(|| {
            products.push(CatalogProduct {
                id: id.clone(),
                name_ar,
                name_en,
                category_ar,
                category_en,
                brand,
                price_minor: base_price,
                variants: Vec::new(),
            });
            products.len() - 1
        });
        products[index].variants.push(CatalogVariant {
            id: variant_id,
            size,
            color: color_en.clone(),
            color_ar,
            color_en,
            color_hex,
            stock,
            sku,
            barcode,
            price_minor: price,
        });
    }
    Ok(products)
}

#[tauri::command]
pub fn get_catalog_setup_options(
    state: tauri::State<'_, AppState>,
    session_token: String,
) -> Result<CatalogSetupOptions, String> {
    let session = require_permission(&state, &session_token, "catalog.manage")?;
    let db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
    let mut categories = Vec::new();
    let mut statement=db.prepare("SELECT id,name_ar,name_en,sort_order FROM categories WHERE branch_id=?1 AND is_active=1 ORDER BY sort_order,name_ar").map_err(db_error)?;
    for row in statement
        .query_map([&session.branch_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })
        .map_err(db_error)?
    {
        let (id, name_ar, name_en, sort) = row.map_err(db_error)?;
        categories.push((
            sort,
            CatalogOption {
                id,
                code: None,
                name_ar,
                name_en,
                color_hex: None,
                profile_code: None,
            },
        ));
    }
    categories.sort_by_key(|row| row.0);
    let categories = categories.into_iter().map(|row| row.1).collect();
    let mut brands = Vec::new();
    let mut statement=db.prepare("SELECT id,name_ar,name_en FROM brands WHERE branch_id=?1 AND is_active=1 ORDER BY name_ar").map_err(db_error)?;
    for row in statement
        .query_map([&session.branch_id], |row| {
            Ok(CatalogOption {
                id: row.get(0)?,
                code: None,
                name_ar: row.get(1)?,
                name_en: row.get(2)?,
                color_hex: None,
                profile_code: None,
            })
        })
        .map_err(db_error)?
    {
        brands.push(row.map_err(db_error)?);
    }
    let mut seasons = Vec::new();
    let mut statement = db
        .prepare("SELECT id,name_ar,name_en FROM seasons WHERE branch_id=?1 ORDER BY name_ar")
        .map_err(db_error)?;
    for row in statement
        .query_map([&session.branch_id], |row| {
            Ok(CatalogOption {
                id: row.get(0)?,
                code: None,
                name_ar: row.get(1)?,
                name_en: row.get(2)?,
                color_hex: None,
                profile_code: None,
            })
        })
        .map_err(db_error)?
    {
        seasons.push(row.map_err(db_error)?);
    }
    let mut sizes = Vec::new();
    let mut statement=db.prepare("SELECT id,size_code,name_ar,name_en,profile_code FROM sizes WHERE branch_id=?1 AND is_active=1 ORDER BY profile_code,sort_order,size_code").map_err(db_error)?;
    for row in statement
        .query_map([&session.branch_id], |row| {
            Ok(CatalogOption {
                id: row.get(0)?,
                code: Some(row.get(1)?),
                name_ar: row.get(2)?,
                name_en: row.get(3)?,
                color_hex: None,
                profile_code: Some(row.get(4)?),
            })
        })
        .map_err(db_error)?
    {
        sizes.push(row.map_err(db_error)?);
    }
    let mut colors = Vec::new();
    let mut statement=db.prepare("SELECT id,color_code,name_ar,name_en,hex_color FROM colors WHERE branch_id=?1 AND is_active=1 ORDER BY name_ar").map_err(db_error)?;
    for row in statement
        .query_map([&session.branch_id], |row| {
            Ok(CatalogOption {
                id: row.get(0)?,
                code: Some(row.get(1)?),
                name_ar: row.get(2)?,
                name_en: row.get(3)?,
                color_hex: row.get(4)?,
                profile_code: None,
            })
        })
        .map_err(db_error)?
    {
        colors.push(row.map_err(db_error)?);
    }
    Ok(CatalogSetupOptions {
        categories,
        brands,
        seasons,
        sizes,
        colors,
    })
}

#[tauri::command]
pub fn create_sale(
    state: tauri::State<'_, AppState>,
    session_token: String,
    request: CreateSaleRequest,
) -> Result<SaleResult, String> {
    let session = require_permission(&state, &session_token, "sales.create")?;
    if request.idempotency_key.len() < 8
        || request.idempotency_key.len() > 128
        || request.lines.is_empty()
        || request.lines.len() > 100
    {
        return Err("SALE_REQUEST_INVALID".to_string());
    }
    if !(0..=10000).contains(&request.invoice_discount_bps) {
        return Err("DISCOUNT_INVALID".to_string());
    }
    let mut variant_ids = HashSet::new();
    for line in &request.lines {
        if line.quantity <= 0
            || line.quantity > 10000
            || !(0..=10000).contains(&line.item_discount_bps)
            || !variant_ids.insert(line.variant_id.clone())
        {
            return Err("SALE_LINE_INVALID".to_string());
        }
    }
    let any_discount = request.invoice_discount_bps > 0
        || request.lines.iter().any(|line| line.item_discount_bps > 0);
    if any_discount {
        let db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
        require_permission_with_db(&db, &session, "sales.discount")?;
        if request
            .discount_reason
            .as_deref()
            .unwrap_or("")
            .trim()
            .is_empty()
        {
            return Err("DISCOUNT_REASON_REQUIRED".to_string());
        }
    }
    if request.payments.len() > 4
        || request.payments.iter().any(|p| {
            p.amount_minor <= 0
                || !matches!(
                    p.method.as_str(),
                    "CASH" | "CARD" | "WALLET" | "STORE_CREDIT"
                )
        })
    {
        return Err("PAYMENT_LINES_INVALID".to_string());
    }
    // Store credit belongs to a customer, so it can only be used when the
    // sale is attached to one.
    if request.payments.iter().any(|p| p.method == "STORE_CREDIT")
        && request.customer_id.is_none()
    {
        return Err("STORE_CREDIT_REQUIRES_CUSTOMER".to_string());
    }
    let sale_request_hash = request_hash(&request)?;

    let mut db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db_error)?;
    if let Some((invoice_id, invoice_no, total_minor, stored_hash)) = tx.query_row(
        "SELECT id, invoice_no, total_minor, request_hash FROM invoices WHERE branch_id=?1 AND idempotency_key=?2",
        params![session.branch_id, request.idempotency_key],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?, row.get::<_, String>(3)?)),
    ).optional().map_err(db_error)? {
        if stored_hash!=sale_request_hash { return Err("IDEMPOTENCY_KEY_REUSED_WITH_DIFFERENT_REQUEST".to_string()); }
        tx.commit().map_err(db_error)?;
        return Ok(SaleResult { invoice_id, invoice_no, total_minor });
    }

    // Tax is optional. When the owner has switched it off for a branch the
    // stored rate is zero; an unconfigured branch is treated as zero-rated
    // instead of blocking the sale, so a store can start selling on day one.
    let (stored_tax_rate_bps, prices_include_tax): (i64, Option<i64>) = tx
        .query_row(
            "SELECT tax_rate_bps, prices_include_tax FROM branches WHERE id=?1",
            [&session.branch_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(db_error)?;
    let tax_rate_bps = stored_tax_rate_bps.clamp(0, 10000);
    let prices_include_tax = prices_include_tax.unwrap_or(0);
    if let Some(customer_id) = request.customer_id.as_deref() {
        let active: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM customers WHERE branch_id=?1 AND id=?2 AND is_active=1)",
            params![session.branch_id,customer_id],|row|row.get(0)).map_err(db_error)?;
        if !active {
            return Err("CUSTOMER_NOT_FOUND".to_string());
        }
    }

    struct LineSnapshot {
        input: SaleLineInput,
        sku: String,
        barcode: Option<String>,
        name_ar: String,
        name_en: String,
        size: String,
        color: String,
        price: i64,
        cost: i64,
    }
    let mut snapshots = Vec::with_capacity(request.lines.len());
    let mut subtotal: i64 = 0;
    let mut item_discount: i64 = 0;
    for line in request.lines {
        let snapshot = tx.query_row(
            "SELECT v.sku, v.barcode, p.name_ar, p.name_en, s.size_code, co.name_ar,
                    COALESCE(v.price_override_minor,p.base_price_minor), COALESCE(v.cost_override_minor,p.base_cost_minor),
                    COALESCE(ib.quantity,0)
             FROM product_variants v JOIN products p ON p.branch_id=v.branch_id AND p.id=v.product_id
             JOIN sizes s ON s.branch_id=v.branch_id AND s.id=v.size_id
             JOIN colors co ON co.branch_id=v.branch_id AND co.id=v.color_id
             LEFT JOIN inventory_balances ib ON ib.branch_id=v.branch_id AND ib.variant_id=v.id AND ib.bucket='SELLABLE'
             WHERE v.branch_id=?1 AND v.id=?2 AND v.is_active=1 AND p.is_active=1",
            params![session.branch_id, line.variant_id],
            |row| Ok((row.get::<_,String>(0)?, row.get::<_,Option<String>>(1)?, row.get::<_,String>(2)?, row.get::<_,String>(3)?, row.get::<_,String>(4)?, row.get::<_,String>(5)?, row.get::<_,i64>(6)?, row.get::<_,i64>(7)?, row.get::<_,i64>(8)?)),
        ).optional().map_err(db_error)?.ok_or_else(|| "VARIANT_NOT_FOUND".to_string())?;
        if snapshot.8 < line.quantity {
            return Err("INSUFFICIENT_STOCK".to_string());
        }
        let gross = snapshot
            .6
            .checked_mul(line.quantity)
            .ok_or_else(|| "AMOUNT_OVERFLOW".to_string())?;
        let line_discount_minor =
            rounded_ratio(gross as i128 * line.item_discount_bps as i128, 10000)?;
        subtotal = subtotal
            .checked_add(gross)
            .ok_or_else(|| "AMOUNT_OVERFLOW".to_string())?;
        item_discount = item_discount
            .checked_add(line_discount_minor)
            .ok_or_else(|| "AMOUNT_OVERFLOW".to_string())?;
        snapshots.push(LineSnapshot {
            input: line,
            sku: snapshot.0,
            barcode: snapshot.1,
            name_ar: snapshot.2,
            name_en: snapshot.3,
            size: snapshot.4,
            color: snapshot.5,
            price: snapshot.6,
            cost: snapshot.7,
        });
    }

    let after_item_discount = subtotal
        .checked_sub(item_discount)
        .ok_or_else(|| "AMOUNT_OVERFLOW".to_string())?;
    let invoice_discount = rounded_ratio(
        after_item_discount as i128 * request.invoice_discount_bps as i128,
        10000,
    )?;
    let net_after_discounts = after_item_discount
        .checked_sub(invoice_discount)
        .ok_or_else(|| "AMOUNT_OVERFLOW".to_string())?;
    // Allocate invoice-level discounts to immutable invoice lines with the largest-remainder method.
    // This keeps the sum of line snapshots exactly equal to the invoice totals for later returns.
    let mut invoice_discount_allocations = vec![0i64; snapshots.len()];
    let mut remainders: Vec<(usize, i128)> = Vec::with_capacity(snapshots.len());
    let mut allocated_discount = 0i64;
    if after_item_discount > 0 {
        for (index, snapshot) in snapshots.iter().enumerate() {
            let gross = snapshot
                .price
                .checked_mul(snapshot.input.quantity)
                .ok_or_else(|| "AMOUNT_OVERFLOW".to_string())?;
            let item_disc = rounded_ratio(
                gross as i128 * snapshot.input.item_discount_bps as i128,
                10000,
            )?;
            let line_net = gross
                .checked_sub(item_disc)
                .ok_or_else(|| "AMOUNT_OVERFLOW".to_string())?;
            let numerator = line_net as i128 * invoice_discount as i128;
            let denominator = after_item_discount as i128;
            let floor = i64::try_from(numerator / denominator)
                .map_err(|_| "AMOUNT_OVERFLOW".to_string())?;
            invoice_discount_allocations[index] = floor;
            allocated_discount = allocated_discount
                .checked_add(floor)
                .ok_or_else(|| "AMOUNT_OVERFLOW".to_string())?;
            remainders.push((index, numerator % denominator));
        }
    }
    let residual_discount = usize::try_from(
        invoice_discount
            .checked_sub(allocated_discount)
            .ok_or_else(|| "AMOUNT_OVERFLOW".to_string())?,
    )
    .map_err(|_| "AMOUNT_OVERFLOW".to_string())?;
    remainders.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    if residual_discount > remainders.len() {
        return Err("DISCOUNT_ALLOCATION_FAILED".to_string());
    }
    for (index, _) in remainders.iter().take(residual_discount) {
        invoice_discount_allocations[*index] = invoice_discount_allocations[*index]
            .checked_add(1)
            .ok_or_else(|| "AMOUNT_OVERFLOW".to_string())?;
    }
    struct PostedLine {
        snapshot: LineSnapshot,
        line_discount_minor: i64,
        line_tax_minor: i64,
        line_total_minor: i64,
    }
    let mut posted_lines = Vec::with_capacity(snapshots.len());
    let mut tax_minor = 0i64;
    for (index, snapshot) in snapshots.into_iter().enumerate() {
        let gross = snapshot
            .price
            .checked_mul(snapshot.input.quantity)
            .ok_or_else(|| "AMOUNT_OVERFLOW".to_string())?;
        let item_disc = rounded_ratio(
            gross as i128 * snapshot.input.item_discount_bps as i128,
            10000,
        )?;
        let line_discount_minor = item_disc
            .checked_add(invoice_discount_allocations[index])
            .ok_or_else(|| "AMOUNT_OVERFLOW".to_string())?;
        let tax_base = gross
            .checked_sub(line_discount_minor)
            .ok_or_else(|| "AMOUNT_OVERFLOW".to_string())?;
        let line_tax_minor = round_tax_to_policy(if prices_include_tax == 1 {
            rounded_ratio(
                tax_base as i128 * tax_rate_bps as i128,
                (10000 + tax_rate_bps) as i128,
            )?
        } else {
            rounded_ratio(tax_base as i128 * tax_rate_bps as i128, 10000)?
        })?;
        let line_total_minor = if prices_include_tax == 1 {
            tax_base
        } else {
            tax_base
                .checked_add(line_tax_minor)
                .ok_or_else(|| "AMOUNT_OVERFLOW".to_string())?
        };
        tax_minor = tax_minor
            .checked_add(line_tax_minor)
            .ok_or_else(|| "AMOUNT_OVERFLOW".to_string())?;
        posted_lines.push(PostedLine {
            snapshot,
            line_discount_minor,
            line_tax_minor,
            line_total_minor,
        });
    }
    let total_minor = if prices_include_tax == 1 {
        net_after_discounts
    } else {
        net_after_discounts
            .checked_add(tax_minor)
            .ok_or_else(|| "AMOUNT_OVERFLOW".to_string())?
    };
    let payment_total = request.payments.iter().try_fold(0i64, |sum, p| {
        sum.checked_add(p.amount_minor).ok_or("AMOUNT_OVERFLOW")
    })?;
    if payment_total != total_minor {
        return Err("PAYMENT_TOTAL_MISMATCH".to_string());
    }

    let invoice_id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let local_day = Local::now().format("%Y%m%d").to_string();
    let sequence: i64 = tx
        .query_row(
            "SELECT COUNT(*) + 1 FROM invoices WHERE branch_id=?1 AND invoice_no LIKE ?2",
            params![session.branch_id, format!("NS-{local_day}-%")],
            |row| row.get(0),
        )
        .map_err(db_error)?;
    let invoice_no = format!("NS-{local_day}-{sequence:04}");
    let discount_reason = request
        .discount_reason
        .as_deref()
        .map(str::trim)
        .filter(|reason| !reason.is_empty());
    tx.execute(
        "INSERT INTO invoices(id, branch_id, invoice_no, idempotency_key, request_hash, customer_id, cashier_user_id, status,
         currency_code, tax_rate_bps, prices_include_tax, subtotal_minor, item_discount_minor, invoice_discount_minor,
         tax_minor, total_minor, discount_reason, created_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,'COMPLETED','EGP',?8,?9,?10,?11,?12,?13,?14,?15,?16)",
        params![invoice_id, session.branch_id, invoice_no, request.idempotency_key, sale_request_hash, request.customer_id, session.user_id, tax_rate_bps,
            prices_include_tax, subtotal, item_discount, invoice_discount, tax_minor, total_minor, discount_reason, now],
    ).map_err(db_error)?;

    for posting in posted_lines {
        let snapshot = posting.snapshot;
        let line_id = Uuid::new_v4().to_string();
        tx.execute(
            "INSERT INTO invoice_lines(id,branch_id,invoice_id,variant_id,sku_snapshot,barcode_snapshot,product_name_ar_snapshot,product_name_en_snapshot,size_snapshot,color_snapshot,quantity,unit_price_minor,unit_cost_snapshot_minor,line_discount_minor,tax_rate_bps,tax_minor,line_total_minor)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
            params![line_id, session.branch_id, invoice_id, snapshot.input.variant_id, snapshot.sku, snapshot.barcode,
                snapshot.name_ar, snapshot.name_en, snapshot.size, snapshot.color, snapshot.input.quantity, snapshot.price,
                snapshot.cost, posting.line_discount_minor, tax_rate_bps, posting.line_tax_minor, posting.line_total_minor],
        ).map_err(db_error)?;
        let movement_id = Uuid::new_v4().to_string();
        let movement_group = invoice_id.clone();
        tx.execute(
            "INSERT INTO inventory_movements(id,branch_id,variant_id,bucket,movement_type,quantity_delta,unit_cost_minor,source_type,source_id,movement_group_id,idempotency_key,actor_user_id,reason,created_at)
             VALUES(?1,?2,?3,'SELLABLE','SALE',?4,?5,'INVOICE',?6,?7,?8,?9,NULL,?10)",
            params![movement_id, session.branch_id, snapshot.input.variant_id, -snapshot.input.quantity, snapshot.cost, invoice_id,
                movement_group, format!("sale:{}:{}", invoice_id, line_id), session.user_id, now],
        ).map_err(|_| "STOCK_POST_FAILED".to_string())?;
    }

    for (index, payment) in request.payments.iter().enumerate() {
        tx.execute(
            "INSERT INTO payments(id,branch_id,invoice_id,method,amount_minor,currency_code,idempotency_key,created_at)
             VALUES(?1,?2,?3,?4,?5,'EGP',?6,?7)",
            params![Uuid::new_v4().to_string(), session.branch_id, invoice_id, payment.method, payment.amount_minor,
                format!("payment:{}:{}", invoice_id, index), now],
        ).map_err(db_error)?;
    }

    // Store credit is consumed only after the invoice is written, so the
    // spend always points at a real invoice.
    if let Some(customer_id) = request.customer_id.as_deref() {
        let credit_total = request
            .payments
            .iter()
            .filter(|payment| payment.method == "STORE_CREDIT")
            .try_fold(0i64, |sum, payment| sum.checked_add(payment.amount_minor))
            .ok_or_else(|| "AMOUNT_OVERFLOW".to_string())?;
        if credit_total > 0 {
            crate::loyalty::spend_store_credit(
                &tx,
                &session.branch_id,
                customer_id,
                &invoice_id,
                credit_total,
                &session.user_id,
                &now,
            )?;
        }
        // Loyalty is optional: when the programme is off this writes nothing.
        crate::loyalty::award_points_for_invoice(
            &tx,
            &session.branch_id,
            customer_id,
            &invoice_id,
            net_after_discounts,
            &session.user_id,
            &now,
        )?;
    }

    let audit_id = Uuid::new_v4().to_string();
    let details = serde_json::json!({"invoice_no": invoice_no, "line_count": variant_ids.len(), "total_minor": total_minor}).to_string();
    insert_audit(
        &tx,
        &audit_id,
        Some(&session.branch_id),
        Some(&session.user_id),
        "SALE_COMPLETED",
        "invoice",
        Some(&invoice_id),
        None,
        &details,
        &now,
    )?;
    tx.commit().map_err(db_error)?;
    Ok(SaleResult {
        invoice_id,
        invoice_no,
        total_minor,
    })
}

#[tauri::command]
pub fn create_product(
    state: tauri::State<'_, AppState>,
    session_token: String,
    request: CreateProductRequest,
) -> Result<CreateProductResult, String> {
    let session = require_permission(&state, &session_token, "catalog.manage")?;
    let name_ar = request.name_ar.trim();
    let name_en = request.name_en.trim();
    if request.idempotency_key.len() < 8
        || request.idempotency_key.len() > 128
        || name_ar.is_empty()
        || name_en.is_empty()
        || name_ar.len() > 180
        || name_en.len() > 180
        || request.base_cost_minor < 0
        || request.base_price_minor < 0
        || request.variants.is_empty()
        || request.variants.len() > 200
    {
        return Err("PRODUCT_REQUEST_INVALID".to_string());
    }
    let has_opening_stock = request
        .variants
        .iter()
        .any(|variant| variant.opening_quantity != 0);
    if has_opening_stock {
        let db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
        require_permission_with_db(&db, &session, "inventory.adjust")?;
        if request
            .opening_reason
            .as_deref()
            .unwrap_or("")
            .trim()
            .is_empty()
        {
            return Err("OPENING_STOCK_REASON_REQUIRED".to_string());
        }
    }
    let mut skus = HashSet::new();
    let mut barcodes = HashSet::new();
    let mut combinations = HashSet::new();
    for variant in &request.variants {
        let sku = variant.sku.trim();
        if sku.is_empty()
            || sku.len() > 80
            || variant.reorder_level < 0
            || variant.opening_quantity < 0
            || variant.cost_override_minor.is_some_and(|value| value < 0)
            || variant.price_override_minor.is_some_and(|value| value < 0)
            || !skus.insert(sku.to_ascii_uppercase())
            || !combinations.insert((variant.size_id.clone(), variant.color_id.clone()))
        {
            return Err("PRODUCT_VARIANT_INVALID".to_string());
        }
        if let Some(barcode) = variant
            .barcode
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            && (barcode.trim().len() > 64 || !barcodes.insert(barcode.trim().to_string()))
        {
            return Err("BARCODE_DUPLICATE_OR_INVALID".to_string());
        }
    }

    let product_request_hash = request_hash(&request)?;
    let mut db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db_error)?;
    if let Some((product_id,stored_hash)) = tx.query_row(
        "SELECT id,create_request_hash FROM products WHERE branch_id=?1 AND create_idempotency_key=?2",
        params![session.branch_id, request.idempotency_key], |row| Ok((row.get::<_, String>(0)?,row.get::<_,Option<String>>(1)?)),
    ).optional().map_err(db_error)? {
        if stored_hash.as_deref()!=Some(product_request_hash.as_str()) { return Err("IDEMPOTENCY_KEY_REUSED_WITH_DIFFERENT_REQUEST".to_string()); }
        let count: i64 = tx.query_row("SELECT COUNT(*) FROM product_variants WHERE branch_id=?1 AND product_id=?2", params![session.branch_id, product_id], |row| row.get(0)).map_err(db_error)?;
        tx.commit().map_err(db_error)?;
        return Ok(CreateProductResult { product_id, variant_count: count as usize });
    }
    let product_id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    tx.execute(
        "INSERT INTO products(id,branch_id,category_id,brand_id,season_id,name_ar,name_en,description_ar,description_en,base_cost_minor,base_price_minor,create_idempotency_key,create_request_hash,is_active,created_at,updated_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,1,?14,?14)",
        params![product_id, session.branch_id, request.category_id, request.brand_id, request.season_id, name_ar, name_en,
            request.description_ar, request.description_en, request.base_cost_minor, request.base_price_minor, request.idempotency_key, product_request_hash, now],
    ).map_err(|_| "PRODUCT_CREATE_FAILED_CHECK_CATEGORY_BRAND_SEASON".to_string())?;

    for input in &request.variants {
        let variant_id = Uuid::new_v4().to_string();
        let supplied_barcode = input
            .barcode
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let barcode = supplied_barcode
            .map(str::to_string)
            .unwrap_or_else(|| format!("NSJ{}", Uuid::new_v4().simple()));
        tx.execute(
            "INSERT INTO product_variants(id,branch_id,product_id,size_id,color_id,sku,barcode,cost_override_minor,price_override_minor,reorder_level,is_active,created_at,updated_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,1,?11,?11)",
            params![variant_id, session.branch_id, product_id, input.size_id, input.color_id, input.sku.trim(), barcode,
                input.cost_override_minor, input.price_override_minor, input.reorder_level, now],
        ).map_err(|_| "VARIANT_CREATE_FAILED_CHECK_SIZE_COLOR_SKU_BARCODE".to_string())?;
        if input.opening_quantity > 0 {
            tx.execute(
                "INSERT INTO inventory_movements(id,branch_id,variant_id,bucket,movement_type,quantity_delta,unit_cost_minor,source_type,source_id,movement_group_id,idempotency_key,actor_user_id,reason,created_at)
                 VALUES(?1,?2,?3,'SELLABLE','OPENING_BALANCE',?4,?5,'PRODUCT_CREATE',?6,?6,?7,?8,?9,?10)",
                params![Uuid::new_v4().to_string(), session.branch_id, variant_id, input.opening_quantity,
                    input.cost_override_minor.unwrap_or(request.base_cost_minor), product_id,
                    format!("opening:{}:{}", request.idempotency_key, input.sku), session.user_id,
                    request.opening_reason.as_deref().map(str::trim), now],
            ).map_err(|_| "OPENING_STOCK_POST_FAILED".to_string())?;
        }
    }
    let detail = serde_json::json!({"variant_count": request.variants.len()}).to_string();
    insert_audit(
        &tx,
        &Uuid::new_v4().to_string(),
        Some(&session.branch_id),
        Some(&session.user_id),
        "PRODUCT_CREATED",
        "product",
        Some(&product_id),
        None,
        &detail,
        &now,
    )?;
    tx.commit().map_err(db_error)?;
    Ok(CreateProductResult {
        product_id,
        variant_count: request.variants.len(),
    })
}

#[tauri::command]
pub fn get_inventory(
    state: tauri::State<'_, AppState>,
    session_token: String,
) -> Result<Vec<InventoryRow>, String> {
    let session = require_permission(&state, &session_token, "inventory.read")?;
    let db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
    let mut statement = db.prepare(
        "SELECT v.id,v.sku,v.barcode,p.name_ar,p.name_en,s.size_code,c.name_ar,c.name_en,
                COALESCE(v.price_override_minor,p.base_price_minor),
                COALESCE(sell.quantity,0),COALESCE(damaged.quantity,0),v.reorder_level
         FROM product_variants v JOIN products p ON p.branch_id=v.branch_id AND p.id=v.product_id
         JOIN sizes s ON s.branch_id=v.branch_id AND s.id=v.size_id
         JOIN colors c ON c.branch_id=v.branch_id AND c.id=v.color_id
         LEFT JOIN inventory_balances sell ON sell.branch_id=v.branch_id AND sell.variant_id=v.id AND sell.bucket='SELLABLE'
         LEFT JOIN inventory_balances damaged ON damaged.branch_id=v.branch_id AND damaged.variant_id=v.id AND damaged.bucket='DAMAGED'
         WHERE v.branch_id=?1 AND v.is_active=1 AND p.is_active=1
         ORDER BY p.name_ar,s.sort_order,c.name_ar"
    ).map_err(db_error)?;
    let rows = statement
        .query_map([&session.branch_id], |row| {
            let sellable: i64 = row.get(9)?;
            let reorder_level: i64 = row.get(11)?;
            Ok(InventoryRow {
                variant_id: row.get(0)?,
                sku: row.get(1)?,
                barcode: row.get(2)?,
                product_name_ar: row.get(3)?,
                product_name_en: row.get(4)?,
                size_code: row.get(5)?,
                color_ar: row.get(6)?,
                color_en: row.get(7)?,
                price_minor: row.get(8)?,
                sellable,
                damaged: row.get(10)?,
                reorder_level,
                needs_reorder: sellable <= reorder_level,
            })
        })
        .map_err(db_error)?;
    rows.map(|row| row.map_err(db_error)).collect()
}

#[tauri::command]
pub fn adjust_inventory(
    state: tauri::State<'_, AppState>,
    session_token: String,
    request: StockAdjustmentRequest,
) -> Result<AdjustmentResult, String> {
    let session = require_permission(&state, &session_token, "inventory.adjust")?;
    if !matches!(
        request.bucket.as_str(),
        "SELLABLE" | "DAMAGED" | "QUARANTINE"
    ) || request.quantity_delta == 0
        || request.quantity_delta < -10000
        || request.quantity_delta > 10000
        || request.reason.trim().is_empty()
        || request.idempotency_key.len() < 8
        || request.idempotency_key.len() > 128
    {
        return Err("STOCK_ADJUSTMENT_INVALID".to_string());
    }
    let mut db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db_error)?;
    if let Some((movement_id,variant_id,bucket,quantity_delta,reason))=tx.query_row(
        "SELECT id,variant_id,bucket,quantity_delta,COALESCE(reason,'') FROM inventory_movements WHERE branch_id=?1 AND idempotency_key=?2",
        params![session.branch_id, request.idempotency_key], |row|Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,String>(2)?,row.get::<_,i64>(3)?,row.get::<_,String>(4)?)),
    ).optional().map_err(db_error)? {
        if variant_id!=request.variant_id || bucket!=request.bucket || quantity_delta!=request.quantity_delta || reason!=request.reason.trim() {
            return Err("IDEMPOTENCY_KEY_REUSED_WITH_DIFFERENT_REQUEST".to_string());
        }
        let quantity: i64 = tx.query_row("SELECT COALESCE((SELECT quantity FROM inventory_balances WHERE branch_id=?1 AND variant_id=?2 AND bucket=?3),0)", params![session.branch_id, request.variant_id, request.bucket], |row| row.get(0)).map_err(db_error)?;
        tx.commit().map_err(db_error)?;
        return Ok(AdjustmentResult { movement_id, new_quantity: quantity });
    }
    let current: i64 = tx.query_row(
        "SELECT COALESCE((SELECT quantity FROM inventory_balances WHERE branch_id=?1 AND variant_id=?2 AND bucket=?3),0)",
        params![session.branch_id, request.variant_id, request.bucket], |row| row.get(0),
    ).map_err(db_error)?;
    let new_quantity = current
        .checked_add(request.quantity_delta)
        .ok_or_else(|| "STOCK_OVERFLOW".to_string())?;
    if new_quantity < 0 {
        return Err("STOCK_CANNOT_BE_NEGATIVE".to_string());
    }
    let movement_id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    tx.execute(
        "INSERT INTO inventory_movements(id,branch_id,variant_id,bucket,movement_type,quantity_delta,source_type,source_id,movement_group_id,idempotency_key,actor_user_id,reason,created_at)
         VALUES(?1,?2,?3,?4,'COUNT_ADJUSTMENT',?5,'MANUAL_ADJUSTMENT',?6,?6,?7,?8,?9,?10)",
        params![movement_id, session.branch_id, request.variant_id, request.bucket, request.quantity_delta,
            movement_id, request.idempotency_key, session.user_id, request.reason.trim(), now],
    ).map_err(db_error)?;
    let details =
        serde_json::json!({"bucket": request.bucket, "quantity_delta": request.quantity_delta})
            .to_string();
    insert_audit(
        &tx,
        &Uuid::new_v4().to_string(),
        Some(&session.branch_id),
        Some(&session.user_id),
        "INVENTORY_ADJUSTED",
        "variant",
        Some(&request.variant_id),
        Some(request.reason.trim()),
        &details,
        &now,
    )?;
    tx.commit().map_err(db_error)?;
    Ok(AdjustmentResult {
        movement_id,
        new_quantity,
    })
}

#[tauri::command]
pub fn configure_return_policy(
    state: tauri::State<'_, AppState>,
    session_token: String,
    policy: ReturnPolicyConfig,
) -> Result<(), String> {
    let session = require_permission(&state, &session_token, "settings.manage")?;
    let mut db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db_error)?;
    crate::owner_panel::persist_return_policy(&tx, &session.branch_id, &session.user_id, &policy)?;
    tx.commit().map_err(db_error)
}

/// Write the tax settings of a branch and record the change.
///
/// The owner controls this from the control panel; the same rule is used by
/// every invoice, so the tax that is stored here is the tax that is charged.
pub fn persist_tax(
    tx: &Transaction<'_>,
    branch_id: &str,
    actor_user_id: &str,
    tax_rate_bps: i64,
    prices_include_tax: bool,
) -> Result<(), String> {
    if !(0..=10000).contains(&tax_rate_bps) {
        return Err("TAX_RATE_INVALID".to_string());
    }
    tx.execute(
        "UPDATE branches SET tax_rate_bps=?1, prices_include_tax=?2 WHERE id=?3",
        params![
            tax_rate_bps,
            if prices_include_tax { 1 } else { 0 },
            branch_id
        ],
    )
    .map_err(db_error)?;
    let now = Utc::now().to_rfc3339();
    let audit_id = Uuid::new_v4().to_string();
    let details =
        serde_json::json!({"tax_rate_bps": tax_rate_bps, "prices_include_tax": prices_include_tax})
            .to_string();
    insert_audit(
        tx,
        &audit_id,
        Some(branch_id),
        Some(actor_user_id),
        "TAX_SETTINGS_CHANGED",
        "branch",
        Some(branch_id),
        Some("Owner confirmed tax setup"),
        &details,
        &now,
    )?;
    Ok(())
}

/// The return policy that is in force for the branch of the session.
///
/// The returns desk shows the rules to the cashier, and the same rules are
/// what the backend enforces when a return is recorded.
#[tauri::command]
pub fn get_return_policy(
    state: tauri::State<'_, AppState>,
    session_token: String,
) -> Result<ReturnPolicyConfig, String> {
    let session = require_permission(&state, &session_token, "returns.process")?;
    let db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
    let policy_json: Option<String> = db
        .query_row(
            "SELECT setting_value_json FROM system_settings WHERE branch_id=?1 AND setting_key='returns_policy'",
            params![session.branch_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(db_error)?;
    let policy: ReturnPolicyConfig =
        serde_json::from_str(&policy_json.ok_or_else(|| "RETURN_POLICY_REQUIRED".to_string())?)
            .map_err(|_| "RETURN_POLICY_INVALID".to_string())?;
    Ok(policy)
}

#[tauri::command]
pub fn get_returnable_invoice(
    state: tauri::State<'_, AppState>,
    session_token: String,
    invoice_no: String,
) -> Result<ReturnableInvoice, String> {
    let session = require_permission(&state, &session_token, "returns.process")?;
    let invoice_no = invoice_no.trim();
    if invoice_no.is_empty() || invoice_no.len() > 48 {
        return Err("INVOICE_NUMBER_INVALID".to_string());
    }
    let db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
    let (invoice_id, status, created_at): (String, String, String) = db
        .query_row(
            "SELECT id,status,created_at FROM invoices WHERE branch_id=?1 AND invoice_no=?2",
            params![session.branch_id, invoice_no],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(db_error)?
        .ok_or_else(|| "ORIGINAL_INVOICE_NOT_FOUND".to_string())?;
    if !matches!(status.as_str(), "COMPLETED" | "PARTIALLY_RETURNED") {
        return Err("INVOICE_NOT_RETURNABLE".to_string());
    }
    let mut statement = db.prepare(
        "SELECT il.id,il.variant_id,il.sku_snapshot,il.product_name_ar_snapshot,il.product_name_en_snapshot,il.size_snapshot,il.color_snapshot,il.quantity,
                COALESCE(SUM(CASE WHEN rd.status='COMPLETED' THEN rl.quantity ELSE 0 END),0),il.line_total_minor
         FROM invoice_lines il LEFT JOIN return_lines rl ON rl.branch_id=il.branch_id AND rl.original_invoice_line_id=il.id
         LEFT JOIN return_documents rd ON rd.branch_id=rl.branch_id AND rd.id=rl.return_document_id
         WHERE il.branch_id=?1 AND il.invoice_id=?2 GROUP BY il.id ORDER BY il.id"
    ).map_err(db_error)?;
    let rows = statement
        .query_map(params![session.branch_id, invoice_id], |row| {
            let quantity: i64 = row.get(7)?;
            let returned_quantity: i64 = row.get(8)?;
            Ok(ReturnableInvoiceLine {
                invoice_line_id: row.get(0)?,
                variant_id: row.get(1)?,
                sku: row.get(2)?,
                product_name_ar: row.get(3)?,
                product_name_en: row.get(4)?,
                size: row.get(5)?,
                color: row.get(6)?,
                quantity,
                returned_quantity,
                remaining_quantity: quantity.saturating_sub(returned_quantity).max(0),
                line_total_minor: row.get(9)?,
            })
        })
        .map_err(db_error)?;
    let lines = rows
        .map(|row| row.map_err(db_error))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ReturnableInvoice {
        invoice_id,
        invoice_no: invoice_no.to_string(),
        status,
        created_at,
        lines,
    })
}

#[tauri::command]
pub fn record_return(
    state: tauri::State<'_, AppState>,
    session_token: String,
    request: ReturnRequest,
) -> Result<ReturnResult, String> {
    let session = require_permission(&state, &session_token, "returns.process")?;
    if request.idempotency_key.len() < 8
        || request.idempotency_key.len() > 128
        || request.invoice_no.trim().is_empty()
        || request.reason.trim().is_empty()
        || request.lines.is_empty()
        || request.lines.len() > 100
    {
        return Err("RETURN_REQUEST_INVALID".to_string());
    }
    let mut seen_lines = HashSet::new();
    for line in &request.lines {
        if line.quantity <= 0
            || line.quantity > 10000
            || line.reason.trim().is_empty()
            || !matches!(
                line.item_condition.as_str(),
                "UNOPENED" | "NEW_TAGS" | "WORN" | "DAMAGED"
            )
            || !matches!(
                line.disposition.as_str(),
                "RESTOCK" | "DAMAGED" | "QUARANTINE" | "DISCARD"
            )
            || !seen_lines.insert(line.invoice_line_id.clone())
        {
            return Err("RETURN_LINE_INVALID".to_string());
        }
        if line.disposition == "RESTOCK"
            && !matches!(line.item_condition.as_str(), "UNOPENED" | "NEW_TAGS")
        {
            return Err("RETURN_ITEM_NOT_RESTOCKABLE".to_string());
        }
    }
    if request.refunds.len() > 4
        || request.refunds.iter().any(|payment| {
            payment.amount_minor <= 0
                || !matches!(
                    payment.method.as_str(),
                    "CASH" | "CARD" | "WALLET" | "STORE_CREDIT"
                )
        })
    {
        return Err("RETURN_REFUND_LINES_INVALID".to_string());
    }
    let return_request_hash = request_hash(&request)?;

    let mut db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db_error)?;
    if let Some((return_id, return_no, refund_due_minor, stored_hash)) = tx.query_row(
        "SELECT id,return_no,refund_due_minor,request_hash FROM return_documents WHERE branch_id=?1 AND idempotency_key=?2",
        params![session.branch_id, request.idempotency_key],
        |row| Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,i64>(2)?,row.get::<_,String>(3)?)),
    ).optional().map_err(db_error)? {
        if stored_hash!=return_request_hash { return Err("IDEMPOTENCY_KEY_REUSED_WITH_DIFFERENT_REQUEST".to_string()); }
        tx.commit().map_err(db_error)?;
        return Ok(ReturnResult { return_id, return_no, refund_due_minor });
    }
    let policy_json: Option<String> = tx.query_row(
        "SELECT setting_value_json FROM system_settings WHERE branch_id=?1 AND setting_key='returns_policy'",
        [&session.branch_id], |row| row.get(0),
    ).optional().map_err(db_error)?;
    let policy: ReturnPolicyConfig =
        serde_json::from_str(&policy_json.ok_or_else(|| "RETURN_POLICY_REQUIRED".to_string())?)
            .map_err(|_| "RETURN_POLICY_INVALID".to_string())?;
    if request.lines.iter().any(|line| {
        line.disposition == "RESTOCK" && !policy.restock_conditions.contains(&line.item_condition)
    }) {
        return Err("RETURN_POLICY_RESTOCK_CONDITION_NOT_ALLOWED".to_string());
    }
    let (invoice_id, customer_id, created_at, invoice_status, invoice_discounts): (String, Option<String>, String, String, i64) = tx.query_row(
        "SELECT i.id,i.customer_id,i.created_at,i.status,i.item_discount_minor + i.invoice_discount_minor
         FROM invoices i WHERE i.branch_id=?1 AND i.invoice_no=?2",
        params![session.branch_id, request.invoice_no.trim()],
        |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?)),
    ).optional().map_err(db_error)?.ok_or_else(|| "ORIGINAL_INVOICE_NOT_FOUND".to_string())?;
    if !matches!(invoice_status.as_str(), "COMPLETED" | "PARTIALLY_RETURNED") {
        return Err("INVOICE_NOT_RETURNABLE".to_string());
    }
    if invoice_discounts > 0 && !policy.allow_discounted_items {
        return Err("DISCOUNTED_ITEM_RETURN_BLOCKED_BY_POLICY".to_string());
    }
    let sold_at = chrono::DateTime::parse_from_rfc3339(&created_at)
        .map_err(|_| "INVOICE_DATE_INVALID".to_string())?
        .with_timezone(&Utc);
    let age_days = Utc::now().signed_duration_since(sold_at).num_days();
    if age_days < 0 || age_days > policy.period_days {
        return Err("RETURN_WINDOW_EXPIRED".to_string());
    }
    struct RefundSnapshot {
        request: ReturnLineInput,
        variant_id: String,
        line_refund_minor: i64,
    }
    let mut snapshots = Vec::with_capacity(request.lines.len());
    let mut refund_due_minor: i64 = 0;
    for line in request.lines {
        let record = tx.query_row(
            "SELECT variant_id,quantity,line_total_minor FROM invoice_lines WHERE branch_id=?1 AND invoice_id=?2 AND id=?3",
            params![session.branch_id, invoice_id, line.invoice_line_id],
            |row| Ok((row.get::<_,String>(0)?,row.get::<_,i64>(1)?,row.get::<_,i64>(2)?)),
        ).optional().map_err(db_error)?.ok_or_else(|| "RETURN_LINE_NOT_IN_INVOICE".to_string())?;
        let already_returned: i64 = tx.query_row(
            "SELECT COALESCE(SUM(rl.quantity),0) FROM return_lines rl JOIN return_documents rd ON rd.branch_id=rl.branch_id AND rd.id=rl.return_document_id
             WHERE rl.branch_id=?1 AND rl.original_invoice_line_id=?2 AND rd.status='COMPLETED'",
            params![session.branch_id,line.invoice_line_id], |row| row.get(0),
        ).map_err(db_error)?;
        if already_returned > record.1 || line.quantity > record.1 - already_returned {
            return Err("RETURN_QUANTITY_EXCEEDS_SOLD".to_string());
        }
        let returned_before_minor = rounded_ratio(
            record.2 as i128 * already_returned as i128,
            record.1 as i128,
        )?;
        let returned_after_minor = rounded_ratio(
            record.2 as i128 * (already_returned + line.quantity) as i128,
            record.1 as i128,
        )?;
        let line_refund = returned_after_minor
            .checked_sub(returned_before_minor)
            .ok_or_else(|| "AMOUNT_OVERFLOW".to_string())?;
        refund_due_minor = refund_due_minor
            .checked_add(line_refund)
            .ok_or_else(|| "AMOUNT_OVERFLOW".to_string())?;
        snapshots.push(RefundSnapshot {
            request: line,
            variant_id: record.0,
            line_refund_minor: line_refund,
        });
    }
    let payment_total = request.refunds.iter().try_fold(0i64, |sum, p| {
        sum.checked_add(p.amount_minor).ok_or("AMOUNT_OVERFLOW")
    })?;
    if payment_total != refund_due_minor {
        return Err("REFUND_TOTAL_MISMATCH".to_string());
    }

    let return_id = Uuid::new_v4().to_string();
    let local_day = Local::now().format("%Y%m%d").to_string();
    let sequence: i64 = tx
        .query_row(
            "SELECT COUNT(*)+1 FROM return_documents WHERE branch_id=?1 AND return_no LIKE ?2",
            params![session.branch_id, format!("RT-{local_day}-%")],
            |row| row.get(0),
        )
        .map_err(db_error)?;
    let return_no = format!("RT-{local_day}-{sequence:04}");
    let now = Utc::now().to_rfc3339();
    tx.execute(
        "INSERT INTO return_documents(id,branch_id,return_no,original_invoice_id,customer_id,kind,status,reason,refund_due_minor,created_by,created_at,idempotency_key,request_hash)
         VALUES(?1,?2,?3,?4,?5,'RETURN','COMPLETED',?6,?7,?8,?9,?10,?11)",
        params![return_id,session.branch_id,return_no,invoice_id,customer_id,request.reason.trim(),refund_due_minor,session.user_id,now,request.idempotency_key,return_request_hash],
    ).map_err(db_error)?;

    for snapshot in snapshots {
        let line = snapshot.request;
        let line_id = Uuid::new_v4().to_string();
        tx.execute(
            "INSERT INTO return_lines(id,branch_id,return_document_id,original_invoice_line_id,variant_id,quantity,reason,item_condition,disposition,refund_line_minor)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![line_id,session.branch_id,return_id,line.invoice_line_id,snapshot.variant_id,line.quantity,line.reason.trim(),line.item_condition,line.disposition,snapshot.line_refund_minor],
        ).map_err(db_error)?;
        let (bucket, movement_type) = match line.disposition.as_str() {
            "RESTOCK" => (Some("SELLABLE"), Some("RETURN_RESTOCK")),
            "DAMAGED" => (Some("DAMAGED"), Some("RETURN_DAMAGED")),
            "QUARANTINE" => (Some("QUARANTINE"), Some("RETURN_QUARANTINE")),
            _ => (None, None),
        };
        if let (Some(bucket), Some(movement_type)) = (bucket, movement_type) {
            tx.execute(
                "INSERT INTO inventory_movements(id,branch_id,variant_id,bucket,movement_type,quantity_delta,source_type,source_id,movement_group_id,idempotency_key,actor_user_id,reason,created_at)
                 VALUES(?1,?2,?3,?4,?5,?6,'RETURN',?7,?7,?8,?9,?10,?11)",
                params![Uuid::new_v4().to_string(),session.branch_id,snapshot.variant_id,bucket,movement_type,line.quantity,return_id,
                    format!("return:{}:{}",return_id,line_id),session.user_id,line.reason.trim(),now],
            ).map_err(|_| "RETURN_STOCK_POST_FAILED".to_string())?;
        }
    }
    for (index, payment) in request.refunds.iter().enumerate() {
        tx.execute(
            "INSERT INTO return_refunds(id,branch_id,return_document_id,method,amount_minor,idempotency_key,processed_by,created_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![Uuid::new_v4().to_string(),session.branch_id,return_id,payment.method,payment.amount_minor,
                format!("return-refund:{}:{}",return_id,index),session.user_id,now],
        ).map_err(db_error)?;
    }
    // Store credit is the approved default refund for a return. It is issued
    // to the customer account, so a return without a customer can only be
    // refunded by cash, card, or wallet.
    let store_credit_total = request
        .refunds
        .iter()
        .filter(|refund| refund.method == "STORE_CREDIT")
        .try_fold(0i64, |sum, refund| sum.checked_add(refund.amount_minor))
        .ok_or_else(|| "AMOUNT_OVERFLOW".to_string())?;
    if store_credit_total > 0 {
        if customer_id.is_none() {
            return Err("STORE_CREDIT_REQUIRES_CUSTOMER".to_string());
        }
        crate::loyalty::issue_store_credit(
            &tx,
            &session.branch_id,
            customer_id.as_deref(),
            &return_id,
            store_credit_total,
            &session.user_id,
            &now,
        )?;
    }

    let outstanding: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM invoice_lines il LEFT JOIN
           (SELECT rl.original_invoice_line_id,SUM(rl.quantity) q FROM return_lines rl JOIN return_documents rd ON rd.branch_id=rl.branch_id AND rd.id=rl.return_document_id WHERE rd.branch_id=?1 AND rd.status='COMPLETED' GROUP BY rl.original_invoice_line_id) r
           ON r.original_invoice_line_id=il.id WHERE il.branch_id=?1 AND il.invoice_id=?2 AND COALESCE(r.q,0)<il.quantity)",
        params![session.branch_id,invoice_id], |row| row.get(0),
    ).map_err(db_error)?;
    tx.execute(
        "UPDATE invoices SET status=?1 WHERE branch_id=?2 AND id=?3",
        params![
            if outstanding {
                "PARTIALLY_RETURNED"
            } else {
                "RETURNED"
            },
            session.branch_id,
            invoice_id
        ],
    )
    .map_err(db_error)?;
    if !outstanding {
        // A fully returned sale must not leave earned points behind.
        crate::loyalty::reverse_points_for_invoice(
            &tx,
            &session.branch_id,
            &invoice_id,
            &session.user_id,
            &now,
        )?;
    }
    let details = serde_json::json!({"return_no":return_no,"refund_due_minor":refund_due_minor,"line_count":seen_lines.len()}).to_string();
    insert_audit(
        &tx,
        &Uuid::new_v4().to_string(),
        Some(&session.branch_id),
        Some(&session.user_id),
        "RETURN_COMPLETED",
        "return",
        Some(&return_id),
        Some(request.reason.trim()),
        &details,
        &now,
    )?;
    tx.commit().map_err(db_error)?;
    Ok(ReturnResult {
        return_id,
        return_no,
        refund_due_minor,
    })
}

#[tauri::command]
pub fn create_exchange(
    state: tauri::State<'_, AppState>,
    session_token: String,
    request: CreateExchangeRequest,
) -> Result<ExchangeResult, String> {
    let session = require_permission(&state, &session_token, "returns.process")?;
    if request.idempotency_key.len() < 8
        || request.idempotency_key.len() > 128
        || request.invoice_no.trim().is_empty()
        || request.reason.trim().is_empty()
        || request.returned_lines.is_empty()
        || request.returned_lines.len() > 100
        || request.replacement_lines.is_empty()
        || request.replacement_lines.len() > 100
        || !(0..=10000).contains(&request.invoice_discount_bps)
    {
        return Err("EXCHANGE_REQUEST_INVALID".to_string());
    }
    let any_discount = request.invoice_discount_bps > 0
        || request
            .replacement_lines
            .iter()
            .any(|line| line.item_discount_bps > 0);
    {
        let db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
        require_permission_with_db(&db, &session, "sales.create")?;
        if any_discount {
            require_permission_with_db(&db, &session, "sales.discount")?;
            if request
                .discount_reason
                .as_deref()
                .unwrap_or("")
                .trim()
                .is_empty()
            {
                return Err("DISCOUNT_REASON_REQUIRED".to_string());
            }
        }
    }
    let mut returned_line_ids = HashSet::new();
    for line in &request.returned_lines {
        if line.quantity <= 0
            || line.quantity > 10000
            || line.reason.trim().is_empty()
            || !matches!(
                line.item_condition.as_str(),
                "UNOPENED" | "NEW_TAGS" | "WORN" | "DAMAGED"
            )
            || !matches!(
                line.disposition.as_str(),
                "RESTOCK" | "DAMAGED" | "QUARANTINE" | "DISCARD"
            )
            || (line.disposition == "RESTOCK"
                && !matches!(line.item_condition.as_str(), "UNOPENED" | "NEW_TAGS"))
            || !returned_line_ids.insert(line.invoice_line_id.clone())
        {
            return Err("EXCHANGE_RETURN_LINE_INVALID".to_string());
        }
    }
    let mut replacement_variants = HashSet::new();
    for line in &request.replacement_lines {
        if line.quantity <= 0
            || line.quantity > 10000
            || !(0..=10000).contains(&line.item_discount_bps)
            || !replacement_variants.insert(line.variant_id.clone())
        {
            return Err("EXCHANGE_REPLACEMENT_LINE_INVALID".to_string());
        }
    }
    if request.payments.len() > 3
        || request.payments.iter().any(|payment| {
            payment.amount_minor <= 0
                || !matches!(payment.method.as_str(), "CASH" | "CARD" | "WALLET")
        })
        || request.refunds.len() > 3
        || request.refunds.iter().any(|refund| {
            refund.amount_minor <= 0
                || !matches!(
                    refund.method.as_str(),
                    "CASH" | "CARD" | "WALLET" | "STORE_CREDIT"
                )
        })
    {
        return Err("EXCHANGE_SETTLEMENT_LINES_INVALID".to_string());
    }
    let exchange_request_hash = request_hash(&request)?;
    let mut db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db_error)?;
    if let Some((return_id,return_no,returned_value,stored_hash,kind)) = tx.query_row(
        "SELECT id,return_no,refund_due_minor,request_hash,kind FROM return_documents WHERE branch_id=?1 AND idempotency_key=?2",
        params![session.branch_id,request.idempotency_key],
        |row| Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,i64>(2)?,row.get::<_,String>(3)?,row.get::<_,String>(4)?)),
    ).optional().map_err(db_error)? {
        if kind != "EXCHANGE" || stored_hash != exchange_request_hash { return Err("IDEMPOTENCY_KEY_REUSED_WITH_DIFFERENT_REQUEST".to_string()); }
        let (invoice_id,invoice_no,price_difference) = tx.query_row(
            "SELECT i.id,i.invoice_no,eg.price_difference_minor FROM exchange_groups eg
             JOIN invoices i ON i.branch_id=eg.branch_id AND i.id=eg.replacement_invoice_id WHERE eg.branch_id=?1 AND eg.return_document_id=?2",
            params![session.branch_id,return_id], |row| Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,i64>(2)?)),
        ).optional().map_err(db_error)?.ok_or_else(|| "EXCHANGE_IDEMPOTENCY_STATE_INVALID".to_string())?;
        tx.commit().map_err(db_error)?;
        return Ok(ExchangeResult { return_id,return_no,replacement_invoice_id:invoice_id,replacement_invoice_no:invoice_no,returned_value_minor:returned_value,
            replacement_total_minor:returned_value.saturating_add(price_difference),price_difference_minor:price_difference });
    }

    let policy_json: Option<String> = tx.query_row(
        "SELECT setting_value_json FROM system_settings WHERE branch_id=?1 AND setting_key='returns_policy'", [&session.branch_id], |row| row.get(0),
    ).optional().map_err(db_error)?;
    let policy: ReturnPolicyConfig =
        serde_json::from_str(&policy_json.ok_or_else(|| "RETURN_POLICY_REQUIRED".to_string())?)
            .map_err(|_| "RETURN_POLICY_INVALID".to_string())?;
    if request.returned_lines.iter().any(|line| {
        line.disposition == "RESTOCK" && !policy.restock_conditions.contains(&line.item_condition)
    }) {
        return Err("RETURN_POLICY_RESTOCK_CONDITION_NOT_ALLOWED".to_string());
    }
    let (original_invoice_id,customer_id,created_at,invoice_status,invoice_discounts):(String,Option<String>,String,String,i64) = tx.query_row(
        "SELECT id,customer_id,created_at,status,item_discount_minor+invoice_discount_minor FROM invoices WHERE branch_id=?1 AND invoice_no=?2",
        params![session.branch_id,request.invoice_no.trim()],
        |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?)),
    ).optional().map_err(db_error)?.ok_or_else(|| "ORIGINAL_INVOICE_NOT_FOUND".to_string())?;
    if !matches!(invoice_status.as_str(), "COMPLETED" | "PARTIALLY_RETURNED") {
        return Err("INVOICE_NOT_RETURNABLE".to_string());
    }
    if invoice_discounts > 0 && !policy.allow_discounted_items {
        return Err("DISCOUNTED_ITEM_RETURN_BLOCKED_BY_POLICY".to_string());
    }
    let sold_at = chrono::DateTime::parse_from_rfc3339(&created_at)
        .map_err(|_| "INVOICE_DATE_INVALID".to_string())?
        .with_timezone(&Utc);
    let age_days = Utc::now().signed_duration_since(sold_at).num_days();
    if age_days < 0 || age_days > policy.period_days {
        return Err("RETURN_WINDOW_EXPIRED".to_string());
    }

    let mut returned_value_minor = 0i64;
    let mut return_snapshots = Vec::with_capacity(request.returned_lines.len());
    let mut incoming_sellable: HashMap<String, i64> = HashMap::new();
    for line in &request.returned_lines {
        let record = tx.query_row(
            "SELECT variant_id,quantity,line_total_minor FROM invoice_lines WHERE branch_id=?1 AND invoice_id=?2 AND id=?3",
            params![session.branch_id,original_invoice_id,line.invoice_line_id],
            |row| Ok((row.get::<_,String>(0)?,row.get::<_,i64>(1)?,row.get::<_,i64>(2)?)),
        ).optional().map_err(db_error)?.ok_or_else(|| "RETURN_LINE_NOT_IN_INVOICE".to_string())?;
        let already_returned: i64 = tx.query_row(
            "SELECT COALESCE(SUM(rl.quantity),0) FROM return_lines rl JOIN return_documents rd ON rd.branch_id=rl.branch_id AND rd.id=rl.return_document_id
             WHERE rl.branch_id=?1 AND rl.original_invoice_line_id=?2 AND rd.status='COMPLETED'",
            params![session.branch_id,line.invoice_line_id], |row| row.get(0),
        ).map_err(db_error)?;
        if already_returned > record.1 || line.quantity > record.1 - already_returned {
            return Err("RETURN_QUANTITY_EXCEEDS_SOLD".to_string());
        }
        let before = rounded_ratio(
            record.2 as i128 * already_returned as i128,
            record.1 as i128,
        )?;
        let after = rounded_ratio(
            record.2 as i128 * (already_returned + line.quantity) as i128,
            record.1 as i128,
        )?;
        let line_value = after
            .checked_sub(before)
            .ok_or_else(|| "AMOUNT_OVERFLOW".to_string())?;
        returned_value_minor = returned_value_minor
            .checked_add(line_value)
            .ok_or_else(|| "AMOUNT_OVERFLOW".to_string())?;
        if line.disposition == "RESTOCK" {
            let quantity = incoming_sellable.entry(record.0.clone()).or_insert(0);
            *quantity = quantity
                .checked_add(line.quantity)
                .ok_or_else(|| "STOCK_OVERFLOW".to_string())?;
        }
        return_snapshots.push((line, record.0, line_value));
    }

    // Mirrors the sale path: a zero rate means tax is switched off, and an
    // unconfigured branch is treated as zero-rated rather than blocking.
    let (stored_tax_rate_bps, prices_include_tax): (i64, Option<i64>) = tx
        .query_row(
            "SELECT tax_rate_bps,prices_include_tax FROM branches WHERE id=?1",
            [&session.branch_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(db_error)?;
    let tax_rate_bps = stored_tax_rate_bps.clamp(0, 10000);
    let prices_include_tax = prices_include_tax.unwrap_or(0);
    struct ReplacementSnapshot {
        input: SaleLineInput,
        sku: String,
        barcode: Option<String>,
        name_ar: String,
        name_en: String,
        size: String,
        color: String,
        price: i64,
        cost: i64,
    }
    let mut replacement_snapshots = Vec::with_capacity(request.replacement_lines.len());
    let mut subtotal = 0i64;
    let mut item_discount_total = 0i64;
    for line in &request.replacement_lines {
        let snapshot = tx.query_row(
            "SELECT v.sku,v.barcode,p.name_ar,p.name_en,s.size_code,co.name_ar,
                    COALESCE(v.price_override_minor,p.base_price_minor),COALESCE(v.cost_override_minor,p.base_cost_minor),COALESCE(ib.quantity,0)
             FROM product_variants v JOIN products p ON p.branch_id=v.branch_id AND p.id=v.product_id
             JOIN sizes s ON s.branch_id=v.branch_id AND s.id=v.size_id JOIN colors co ON co.branch_id=v.branch_id AND co.id=v.color_id
             LEFT JOIN inventory_balances ib ON ib.branch_id=v.branch_id AND ib.variant_id=v.id AND ib.bucket='SELLABLE'
             WHERE v.branch_id=?1 AND v.id=?2 AND v.is_active=1 AND p.is_active=1",
            params![session.branch_id,line.variant_id],
            |row| Ok((row.get::<_,String>(0)?,row.get::<_,Option<String>>(1)?,row.get::<_,String>(2)?,row.get::<_,String>(3)?,row.get::<_,String>(4)?,row.get::<_,String>(5)?,row.get::<_,i64>(6)?,row.get::<_,i64>(7)?,row.get::<_,i64>(8)?)),
        ).optional().map_err(db_error)?.ok_or_else(|| "VARIANT_NOT_FOUND".to_string())?;
        let available = snapshot
            .8
            .checked_add(*incoming_sellable.get(&line.variant_id).unwrap_or(&0))
            .ok_or_else(|| "STOCK_OVERFLOW".to_string())?;
        if available < line.quantity {
            return Err("INSUFFICIENT_STOCK".to_string());
        }
        let gross = snapshot
            .6
            .checked_mul(line.quantity)
            .ok_or_else(|| "AMOUNT_OVERFLOW".to_string())?;
        let discount = rounded_ratio(gross as i128 * line.item_discount_bps as i128, 10000)?;
        subtotal = subtotal
            .checked_add(gross)
            .ok_or_else(|| "AMOUNT_OVERFLOW".to_string())?;
        item_discount_total = item_discount_total
            .checked_add(discount)
            .ok_or_else(|| "AMOUNT_OVERFLOW".to_string())?;
        replacement_snapshots.push(ReplacementSnapshot {
            input: line.clone(),
            sku: snapshot.0,
            barcode: snapshot.1,
            name_ar: snapshot.2,
            name_en: snapshot.3,
            size: snapshot.4,
            color: snapshot.5,
            price: snapshot.6,
            cost: snapshot.7,
        });
    }
    let after_item_discount = subtotal
        .checked_sub(item_discount_total)
        .ok_or_else(|| "AMOUNT_OVERFLOW".to_string())?;
    let invoice_discount = rounded_ratio(
        after_item_discount as i128 * request.invoice_discount_bps as i128,
        10000,
    )?;
    let net_after_discounts = after_item_discount
        .checked_sub(invoice_discount)
        .ok_or_else(|| "AMOUNT_OVERFLOW".to_string())?;
    let mut invoice_discount_allocations = vec![0i64; replacement_snapshots.len()];
    let mut remainders: Vec<(usize, i128)> = Vec::with_capacity(replacement_snapshots.len());
    let mut allocated_discount = 0i64;
    if after_item_discount > 0 {
        for (index, snapshot) in replacement_snapshots.iter().enumerate() {
            let gross = snapshot
                .price
                .checked_mul(snapshot.input.quantity)
                .ok_or_else(|| "AMOUNT_OVERFLOW".to_string())?;
            let item_discount = rounded_ratio(
                gross as i128 * snapshot.input.item_discount_bps as i128,
                10000,
            )?;
            let line_net = gross
                .checked_sub(item_discount)
                .ok_or_else(|| "AMOUNT_OVERFLOW".to_string())?;
            let numerator = line_net as i128 * invoice_discount as i128;
            let denominator = after_item_discount as i128;
            let floor = i64::try_from(numerator / denominator)
                .map_err(|_| "AMOUNT_OVERFLOW".to_string())?;
            invoice_discount_allocations[index] = floor;
            allocated_discount = allocated_discount
                .checked_add(floor)
                .ok_or_else(|| "AMOUNT_OVERFLOW".to_string())?;
            remainders.push((index, numerator % denominator));
        }
    }
    let residual_discount = usize::try_from(
        invoice_discount
            .checked_sub(allocated_discount)
            .ok_or_else(|| "AMOUNT_OVERFLOW".to_string())?,
    )
    .map_err(|_| "AMOUNT_OVERFLOW".to_string())?;
    remainders.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    if residual_discount > remainders.len() {
        return Err("DISCOUNT_ALLOCATION_FAILED".to_string());
    }
    for (index, _) in remainders.iter().take(residual_discount) {
        invoice_discount_allocations[*index] = invoice_discount_allocations[*index]
            .checked_add(1)
            .ok_or_else(|| "AMOUNT_OVERFLOW".to_string())?;
    }
    struct PostedReplacement {
        snapshot: ReplacementSnapshot,
        discount: i64,
        tax: i64,
        total: i64,
    }
    let mut posted_replacements = Vec::with_capacity(replacement_snapshots.len());
    let mut tax_minor = 0i64;
    for (index, snapshot) in replacement_snapshots.into_iter().enumerate() {
        let gross = snapshot
            .price
            .checked_mul(snapshot.input.quantity)
            .ok_or_else(|| "AMOUNT_OVERFLOW".to_string())?;
        let item_discount = rounded_ratio(
            gross as i128 * snapshot.input.item_discount_bps as i128,
            10000,
        )?;
        let discount = item_discount
            .checked_add(invoice_discount_allocations[index])
            .ok_or_else(|| "AMOUNT_OVERFLOW".to_string())?;
        let tax_base = gross
            .checked_sub(discount)
            .ok_or_else(|| "AMOUNT_OVERFLOW".to_string())?;
        let line_tax = round_tax_to_policy(if prices_include_tax == 1 {
            rounded_ratio(
                tax_base as i128 * tax_rate_bps as i128,
                (10000 + tax_rate_bps) as i128,
            )?
        } else {
            rounded_ratio(tax_base as i128 * tax_rate_bps as i128, 10000)?
        })?;
        let line_total = if prices_include_tax == 1 {
            tax_base
        } else {
            tax_base
                .checked_add(line_tax)
                .ok_or_else(|| "AMOUNT_OVERFLOW".to_string())?
        };
        tax_minor = tax_minor
            .checked_add(line_tax)
            .ok_or_else(|| "AMOUNT_OVERFLOW".to_string())?;
        posted_replacements.push(PostedReplacement {
            snapshot,
            discount,
            tax: line_tax,
            total: line_total,
        });
    }
    let replacement_total = if prices_include_tax == 1 {
        net_after_discounts
    } else {
        net_after_discounts
            .checked_add(tax_minor)
            .ok_or_else(|| "AMOUNT_OVERFLOW".to_string())?
    };
    let price_difference = replacement_total
        .checked_sub(returned_value_minor)
        .ok_or_else(|| "AMOUNT_OVERFLOW".to_string())?;
    let payment_total = request.payments.iter().try_fold(0i64, |sum, payment| {
        sum.checked_add(payment.amount_minor)
            .ok_or("AMOUNT_OVERFLOW")
    })?;
    let refund_total = request.refunds.iter().try_fold(0i64, |sum, refund| {
        sum.checked_add(refund.amount_minor)
            .ok_or("AMOUNT_OVERFLOW")
    })?;
    if (price_difference > 0 && (payment_total != price_difference || refund_total != 0))
        || (price_difference < 0
            && (refund_total != price_difference.checked_neg().ok_or("AMOUNT_OVERFLOW")?
                || payment_total != 0))
        || (price_difference == 0 && (refund_total != 0 || payment_total != 0))
    {
        return Err("EXCHANGE_SETTLEMENT_MISMATCH".to_string());
    }

    let return_id = Uuid::new_v4().to_string();
    let replacement_invoice_id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let local_day = Local::now().format("%Y%m%d").to_string();
    let return_sequence: i64 = tx
        .query_row(
            "SELECT COUNT(*)+1 FROM return_documents WHERE branch_id=?1 AND return_no LIKE ?2",
            params![session.branch_id, format!("EX-{local_day}-%")],
            |row| row.get(0),
        )
        .map_err(db_error)?;
    let return_no = format!("EX-{local_day}-{return_sequence:04}");
    let invoice_sequence: i64 = tx
        .query_row(
            "SELECT COUNT(*)+1 FROM invoices WHERE branch_id=?1 AND invoice_no LIKE ?2",
            params![session.branch_id, format!("NS-{local_day}-%")],
            |row| row.get(0),
        )
        .map_err(db_error)?;
    let replacement_invoice_no = format!("NS-{local_day}-{invoice_sequence:04}");
    tx.execute(
        "INSERT INTO return_documents(id,branch_id,return_no,original_invoice_id,customer_id,kind,status,reason,refund_due_minor,created_by,created_at,idempotency_key,request_hash)
         VALUES(?1,?2,?3,?4,?5,'EXCHANGE','COMPLETED',?6,?7,?8,?9,?10,?11)",
        params![return_id,session.branch_id,return_no,original_invoice_id,customer_id,request.reason.trim(),returned_value_minor,session.user_id,now,request.idempotency_key,exchange_request_hash],
    ).map_err(db_error)?;
    for (line, variant_id, line_value) in return_snapshots {
        let line_id = Uuid::new_v4().to_string();
        tx.execute(
            "INSERT INTO return_lines(id,branch_id,return_document_id,original_invoice_line_id,variant_id,quantity,reason,item_condition,disposition,refund_line_minor)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![line_id,session.branch_id,return_id,line.invoice_line_id,variant_id,line.quantity,line.reason.trim(),line.item_condition,line.disposition,line_value],
        ).map_err(db_error)?;
        let movement = match line.disposition.as_str() {
            "RESTOCK" => Some(("SELLABLE", "RETURN_RESTOCK")),
            "DAMAGED" => Some(("DAMAGED", "RETURN_DAMAGED")),
            "QUARANTINE" => Some(("QUARANTINE", "RETURN_QUARANTINE")),
            _ => None,
        };
        if let Some((bucket, movement_type)) = movement {
            tx.execute("INSERT INTO inventory_movements(id,branch_id,variant_id,bucket,movement_type,quantity_delta,source_type,source_id,movement_group_id,idempotency_key,actor_user_id,reason,created_at)
               VALUES(?1,?2,?3,?4,?5,?6,'EXCHANGE',?7,?7,?8,?9,?10,?11)",
                params![Uuid::new_v4().to_string(),session.branch_id,variant_id,bucket,movement_type,line.quantity,return_id,format!("exchange-return:{}:{}",return_id,line_id),session.user_id,line.reason.trim(),now])
                .map_err(|_| "EXCHANGE_RETURN_STOCK_POST_FAILED".to_string())?;
        }
    }
    let outstanding: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM invoice_lines il LEFT JOIN
           (SELECT rl.original_invoice_line_id,SUM(rl.quantity) q FROM return_lines rl JOIN return_documents rd ON rd.branch_id=rl.branch_id AND rd.id=rl.return_document_id WHERE rd.branch_id=?1 AND rd.status='COMPLETED' GROUP BY rl.original_invoice_line_id) r
           ON r.original_invoice_line_id=il.id WHERE il.branch_id=?1 AND il.invoice_id=?2 AND COALESCE(r.q,0)<il.quantity)",
        params![session.branch_id,original_invoice_id],|row|row.get(0),
    ).map_err(db_error)?;
    tx.execute(
        "UPDATE invoices SET status=?1 WHERE branch_id=?2 AND id=?3",
        params![
            if outstanding {
                "PARTIALLY_RETURNED"
            } else {
                "RETURNED"
            },
            session.branch_id,
            original_invoice_id
        ],
    )
    .map_err(db_error)?;

    tx.execute(
        "INSERT INTO invoices(id,branch_id,invoice_no,idempotency_key,request_hash,customer_id,cashier_user_id,status,currency_code,tax_rate_bps,prices_include_tax,subtotal_minor,item_discount_minor,invoice_discount_minor,tax_minor,total_minor,discount_reason,created_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,'COMPLETED','EGP',?8,?9,?10,?11,?12,?13,?14,?15,?16)",
        params![replacement_invoice_id,session.branch_id,replacement_invoice_no,format!("exchange-sale:{}",request.idempotency_key),exchange_request_hash,customer_id,session.user_id,tax_rate_bps,prices_include_tax,subtotal,item_discount_total,invoice_discount,tax_minor,replacement_total,
            request.discount_reason.as_deref().map(str::trim).filter(|value|!value.is_empty()),now],
    ).map_err(db_error)?;
    for posted in posted_replacements {
        let snapshot = posted.snapshot;
        let line_id = Uuid::new_v4().to_string();
        tx.execute(
            "INSERT INTO invoice_lines(id,branch_id,invoice_id,variant_id,sku_snapshot,barcode_snapshot,product_name_ar_snapshot,product_name_en_snapshot,size_snapshot,color_snapshot,quantity,unit_price_minor,unit_cost_snapshot_minor,line_discount_minor,tax_rate_bps,tax_minor,line_total_minor)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
            params![line_id,session.branch_id,replacement_invoice_id,snapshot.input.variant_id,snapshot.sku,snapshot.barcode,snapshot.name_ar,snapshot.name_en,snapshot.size,snapshot.color,
                snapshot.input.quantity,snapshot.price,snapshot.cost,posted.discount,tax_rate_bps,posted.tax,posted.total],
        ).map_err(db_error)?;
        tx.execute(
            "INSERT INTO inventory_movements(id,branch_id,variant_id,bucket,movement_type,quantity_delta,unit_cost_minor,source_type,source_id,movement_group_id,idempotency_key,actor_user_id,created_at)
             VALUES(?1,?2,?3,'SELLABLE','SALE',?4,?5,'EXCHANGE_INVOICE',?6,?6,?7,?8,?9)",
            params![Uuid::new_v4().to_string(),session.branch_id,snapshot.input.variant_id,-snapshot.input.quantity,snapshot.cost,replacement_invoice_id,
                format!("exchange-sale:{}:{}",replacement_invoice_id,line_id),session.user_id,now],
        ).map_err(|_| "EXCHANGE_SALE_STOCK_POST_FAILED".to_string())?;
    }
    let exchange_credit = returned_value_minor.min(replacement_total);
    if exchange_credit > 0 {
        tx.execute("INSERT INTO payments(id,branch_id,invoice_id,method,amount_minor,currency_code,idempotency_key,created_at) VALUES(?1,?2,?3,'STORE_CREDIT',?4,'EGP',?5,?6)",
            params![Uuid::new_v4().to_string(),session.branch_id,replacement_invoice_id,exchange_credit,format!("exchange-credit:{}",return_id),now]).map_err(db_error)?;
    }
    for (index, payment) in request.payments.iter().enumerate() {
        tx.execute("INSERT INTO payments(id,branch_id,invoice_id,method,amount_minor,currency_code,idempotency_key,created_at) VALUES(?1,?2,?3,?4,?5,'EGP',?6,?7)",
            params![Uuid::new_v4().to_string(),session.branch_id,replacement_invoice_id,payment.method,payment.amount_minor,format!("exchange-payment:{}:{}",return_id,index),now]).map_err(db_error)?;
    }
    for (index, refund) in request.refunds.iter().enumerate() {
        tx.execute("INSERT INTO return_refunds(id,branch_id,return_document_id,method,amount_minor,idempotency_key,processed_by,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![Uuid::new_v4().to_string(),session.branch_id,return_id,refund.method,refund.amount_minor,format!("exchange-refund:{}:{}",return_id,index),session.user_id,now]).map_err(db_error)?;
    }
    let settlement_method = if price_difference < 0 {
        Some("REFUND")
    } else if price_difference == 0 {
        Some("STORE_CREDIT")
    } else if request.payments.len() == 1 {
        Some(request.payments[0].method.as_str())
    } else {
        None
    };
    tx.execute("INSERT INTO exchange_groups(id,branch_id,return_document_id,replacement_invoice_id,price_difference_minor,settlement_method,created_at,created_by) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
        params![Uuid::new_v4().to_string(),session.branch_id,return_id,replacement_invoice_id,price_difference,settlement_method,now,session.user_id]).map_err(db_error)?;
    let details = serde_json::json!({"return_no":return_no,"replacement_invoice_no":replacement_invoice_no,"returned_value_minor":returned_value_minor,"replacement_total_minor":replacement_total,"price_difference_minor":price_difference}).to_string();
    insert_audit(
        &tx,
        &Uuid::new_v4().to_string(),
        Some(&session.branch_id),
        Some(&session.user_id),
        "EXCHANGE_COMPLETED",
        "exchange",
        Some(&return_id),
        Some(request.reason.trim()),
        &details,
        &now,
    )?;
    tx.commit().map_err(db_error)?;
    Ok(ExchangeResult {
        return_id,
        return_no,
        replacement_invoice_id,
        replacement_invoice_no,
        returned_value_minor,
        replacement_total_minor: replacement_total,
        price_difference_minor: price_difference,
    })
}

#[tauri::command]
pub fn list_users(
    state: tauri::State<'_, AppState>,
    session_token: String,
) -> Result<Vec<UserOption>, String> {
    let session = require_permission(&state, &session_token, "users.manage")?;
    let db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
    let mut statement=db.prepare("SELECT u.id,u.username,u.display_name,COALESCE((SELECT ubr.role_code FROM user_branch_roles ubr WHERE ubr.user_id=u.id AND ubr.branch_id=?1 ORDER BY ubr.role_code LIMIT 1),''),u.is_active FROM users u ORDER BY u.username COLLATE NOCASE").map_err(db_error)?;
    let rows = statement
        .query_map([session.branch_id], |row| {
            Ok(UserOption {
                id: row.get(0)?,
                username: row.get(1)?,
                display_name: row.get(2)?,
                role_code: row.get(3)?,
                is_active: row.get::<_, i64>(4)? == 1,
            })
        })
        .map_err(db_error)?;
    rows.map(|row| row.map_err(db_error)).collect()
}

#[tauri::command]
pub fn create_user(
    state: tauri::State<'_, AppState>,
    session_token: String,
    request: CreateUserRequest,
) -> Result<UserOption, String> {
    let session = require_permission(&state, &session_token, "users.manage")?;
    let username = request.username.trim();
    let display_name = request.display_name.trim();
    let role_code = request.role_code.trim().to_ascii_lowercase();
    if username.len() < 3
        || username.len() > 40
        || !username
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
        || display_name.is_empty()
        || display_name.len() > 180
        || request.password.len() < 12
        || request.password.len() > 128
        || !matches!(role_code.as_str(), "cashier" | "manager")
    {
        return Err("USER_INPUT_INVALID".to_string());
    }
    let license = crate::license::ensure_valid(&state)?;
    let password_phc = hash_password(&request.password)?;
    let user_id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let mut db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db_error)?;
    if let Some(max_users) = license.limits.get("max_users") {
        let count: i64 = tx
            .query_row("SELECT COUNT(*) FROM users WHERE is_active=1", [], |row| {
                row.get(0)
            })
            .map_err(db_error)?;
        if count as u64 >= *max_users {
            return Err("LICENSE_USER_LIMIT_REACHED".to_string());
        }
    }
    tx.execute("INSERT INTO users(id,username,display_name,password_phc,is_active,created_at,updated_at) VALUES(?1,?2,?3,?4,1,?5,?5)",
        params![user_id,username,display_name,password_phc,now]).map_err(|_|"USERNAME_ALREADY_EXISTS".to_string())?;
    tx.execute("INSERT INTO user_branch_roles(user_id,branch_id,role_code,granted_by,granted_at) VALUES(?1,?2,?3,?4,?5)",
        params![user_id,session.branch_id,role_code,session.user_id,now]).map_err(db_error)?;
    insert_audit(
        &tx,
        &Uuid::new_v4().to_string(),
        Some(&session.branch_id),
        Some(&session.user_id),
        "USER_CREATED",
        "user",
        Some(&user_id),
        None,
        &serde_json::json!({"role_code":role_code}).to_string(),
        &now,
    )?;
    tx.commit().map_err(db_error)?;
    Ok(UserOption {
        id: user_id,
        username: username.to_string(),
        display_name: display_name.to_string(),
        role_code,
        is_active: true,
    })
}

#[tauri::command]
pub fn change_user_role(
    state: tauri::State<'_, AppState>,
    session_token: String,
    request: ChangeUserRoleRequest,
) -> Result<(), String> {
    let session = require_permission(&state, &session_token, "users.manage")?;
    let role_code = request.role_code.trim().to_ascii_lowercase();
    if !matches!(role_code.as_str(), "cashier" | "manager") || request.user_id == session.user_id {
        return Err("ROLE_CHANGE_NOT_ALLOWED".to_string());
    }
    let mut db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db_error)?;
    let is_owner:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM user_branch_roles WHERE user_id=?1 AND branch_id=?2 AND role_code='owner')",params![request.user_id,session.branch_id],|row|row.get(0)).map_err(db_error)?;
    if is_owner {
        let other_owners:i64=tx.query_row("SELECT COUNT(DISTINCT ubr.user_id) FROM user_branch_roles ubr JOIN users u ON u.id=ubr.user_id WHERE ubr.branch_id=?1 AND ubr.role_code='owner' AND ubr.user_id<>?2 AND u.is_active=1",params![session.branch_id,request.user_id],|row|row.get(0)).map_err(db_error)?;
        if other_owners == 0 {
            return Err("LAST_ACTIVE_OWNER_PROTECTED".to_string());
        }
    }
    tx.execute(
        "DELETE FROM user_branch_roles WHERE user_id=?1 AND branch_id=?2",
        params![request.user_id, session.branch_id],
    )
    .map_err(db_error)?;
    tx.execute("INSERT INTO user_branch_roles(user_id,branch_id,role_code,granted_by,granted_at) VALUES(?1,?2,?3,?4,?5)",params![request.user_id,session.branch_id,role_code,session.user_id,Utc::now().to_rfc3339()]).map_err(db_error)?;
    let now = Utc::now().to_rfc3339();
    insert_audit(
        &tx,
        &Uuid::new_v4().to_string(),
        Some(&session.branch_id),
        Some(&session.user_id),
        "USER_ROLE_CHANGED",
        "user",
        Some(&request.user_id),
        None,
        &serde_json::json!({"role_code":role_code}).to_string(),
        &now,
    )?;
    tx.commit().map_err(db_error)
}

#[tauri::command]
pub fn set_user_active(
    state: tauri::State<'_, AppState>,
    session_token: String,
    request: SetUserActiveRequest,
) -> Result<(), String> {
    let session = require_permission(&state, &session_token, "users.manage")?;
    if request.user_id == session.user_id {
        return Err("CANNOT_CHANGE_OWN_ACTIVE_STATUS".to_string());
    }
    let mut db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db_error)?;
    if !request.is_active {
        let is_owner:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM user_branch_roles WHERE user_id=?1 AND branch_id=?2 AND role_code='owner')",params![request.user_id,session.branch_id],|row|row.get(0)).map_err(db_error)?;
        if is_owner {
            let other_owners:i64=tx.query_row("SELECT COUNT(DISTINCT ubr.user_id) FROM user_branch_roles ubr JOIN users u ON u.id=ubr.user_id WHERE ubr.branch_id=?1 AND ubr.role_code='owner' AND ubr.user_id<>?2 AND u.is_active=1",params![session.branch_id,request.user_id],|row|row.get(0)).map_err(db_error)?;
            if other_owners == 0 {
                return Err("LAST_ACTIVE_OWNER_PROTECTED".to_string());
            }
        }
    }
    let changed=tx.execute("UPDATE users SET is_active=?1,failed_logins=0,locked_until=NULL,updated_at=?2 WHERE id=?3",params![if request.is_active{1}else{0},Utc::now().to_rfc3339(),request.user_id]).map_err(db_error)?;
    if changed == 0 {
        return Err("USER_NOT_FOUND".to_string());
    }
    let now = Utc::now().to_rfc3339();
    insert_audit(
        &tx,
        &Uuid::new_v4().to_string(),
        Some(&session.branch_id),
        Some(&session.user_id),
        "USER_ACTIVE_STATUS_CHANGED",
        "user",
        Some(&request.user_id),
        None,
        &serde_json::json!({"is_active":request.is_active}).to_string(),
        &now,
    )?;
    tx.commit().map_err(db_error)
}

#[tauri::command]
pub fn get_branches(
    state: tauri::State<'_, AppState>,
    session_token: String,
) -> Result<Vec<BranchOption>, String> {
    let session = require_permission(&state, &session_token, "sales.read")?;
    let db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
    let mut statement = db.prepare(
        "SELECT b.id,b.code,b.name_ar,b.name_en FROM branches b
         JOIN user_branch_roles ubr ON ubr.branch_id=b.id AND ubr.user_id=?1
         WHERE EXISTS(SELECT 1 FROM role_permissions rp WHERE rp.role_code=ubr.role_code AND rp.permission_code='sales.read')
         ORDER BY b.code"
    ).map_err(db_error)?;
    let rows = statement
        .query_map([session.user_id], |row| {
            Ok(BranchOption {
                id: row.get(0)?,
                code: row.get(1)?,
                name_ar: row.get(2)?,
                name_en: row.get(3)?,
            })
        })
        .map_err(db_error)?;
    rows.map(|row| row.map_err(db_error)).collect()
}

#[tauri::command]
pub fn create_branch(
    state: tauri::State<'_, AppState>,
    session_token: String,
    request: CreateBranchRequest,
) -> Result<BranchOption, String> {
    let session = require_permission(&state, &session_token, "branches.manage")?;
    let license = crate::license::ensure_valid(&state)?;
    let max_branches = *license
        .limits
        .get("max_branches")
        .ok_or_else(|| "LICENSE_BRANCH_LIMIT_UNSPECIFIED".to_string())?;
    let code = request.code.trim().to_ascii_uppercase();
    let name_ar = request.name_ar.trim();
    let name_en = request.name_en.trim();
    if !(2..=16).contains(&code.len())
        || !code
            .chars()
            .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || matches!(ch, '-' | '_'))
        || name_ar.is_empty()
        || name_ar.len() > 120
        || name_en.is_empty()
        || name_en.len() > 120
    {
        return Err("BRANCH_INPUT_INVALID".to_string());
    }
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let mut db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db_error)?;
    let branch_count: i64 = tx
        .query_row("SELECT COUNT(*) FROM branches", [], |row| row.get(0))
        .map_err(db_error)?;
    if branch_count < 0 || branch_count as u64 >= max_branches {
        return Err("LICENSE_BRANCH_LIMIT_REACHED".to_string());
    }
    let source = tx.query_row(
        "SELECT currency_code,tax_rate_bps,prices_include_tax,timezone FROM branches WHERE id=?1",
        [&session.branch_id], |row| Ok((row.get::<_,String>(0)?,row.get::<_,i64>(1)?,row.get::<_,Option<i64>>(2)?,row.get::<_,String>(3)?)),
    ).optional().map_err(db_error)?.ok_or_else(|| "BRANCH_NOT_FOUND".to_string())?;
    tx.execute(
        "INSERT INTO branches(id,code,name_ar,name_en,currency_code,tax_rate_bps,prices_include_tax,timezone,created_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
        params![id,code,name_ar,name_en,source.0,source.1,source.2,source.3,now],
    ).map_err(|_| "BRANCH_CODE_ALREADY_EXISTS".to_string())?;
    copy_branch_catalog(&tx, &session.branch_id, &id)?;
    tx.execute(
        "INSERT INTO user_branch_roles(user_id,branch_id,role_code,granted_by,granted_at) VALUES(?1,?2,'owner',?1,?3)",
        params![session.user_id,id,now],
    ).map_err(db_error)?;
    insert_audit(
        &tx,
        &Uuid::new_v4().to_string(),
        Some(&session.branch_id),
        Some(&session.user_id),
        "BRANCH_CREATED",
        "branch",
        Some(&id),
        None,
        &serde_json::json!({"code":code,"max_branches":max_branches}).to_string(),
        &now,
    )?;
    tx.commit().map_err(db_error)?;
    Ok(BranchOption {
        id,
        code,
        name_ar: name_ar.to_string(),
        name_en: name_en.to_string(),
    })
}

#[tauri::command]
pub fn switch_branch(
    state: tauri::State<'_, AppState>,
    session_token: String,
    branch_id: String,
) -> Result<AuthResult, String> {
    let session = require_permission(&state, &session_token, "sales.read")?;
    let db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
    let target: Option<(String,String)> = db.query_row(
        "SELECT u.display_name,b.id FROM users u JOIN branches b ON b.id=?2
         WHERE u.id=?1 AND EXISTS(
           SELECT 1 FROM user_branch_roles ubr JOIN role_permissions rp ON rp.role_code=ubr.role_code
           WHERE ubr.user_id=u.id AND ubr.branch_id=b.id AND rp.permission_code='sales.read')",
        params![session.user_id,branch_id], |row| Ok((row.get(0)?,row.get(1)?)),
    ).optional().map_err(db_error)?;
    drop(db);
    let (display_name, branch_id) = target.ok_or_else(|| "BRANCH_ACCESS_DENIED".to_string())?;
    let mut sessions = state
        .sessions
        .lock()
        .map_err(|_| "SESSION_LOCK_FAILED".to_string())?;
    let active = sessions
        .get_mut(&session_token)
        .ok_or_else(|| "SESSION_REQUIRED".to_string())?;
    if Instant::now() > active.expires_at {
        sessions.remove(&session_token);
        return Err("SESSION_EXPIRED".to_string());
    }
    active.branch_id = branch_id.clone();
    active.expires_at = Instant::now() + SESSION_IDLE;
    Ok(AuthResult {
        session_token,
        display_name,
        branch_id,
    })
}

#[tauri::command]
pub fn assign_branch_user(
    state: tauri::State<'_, AppState>,
    session_token: String,
    request: AssignBranchUserRequest,
) -> Result<(), String> {
    let session = require_permission(&state, &session_token, "branches.manage")?;
    let role_code = request.role_code.trim().to_ascii_lowercase();
    if !matches!(role_code.as_str(), "cashier" | "manager") {
        return Err("BRANCH_ROLE_INVALID".to_string());
    }
    let mut db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db_error)?;
    let owner_of_target: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM user_branch_roles WHERE user_id=?1 AND branch_id=?2 AND role_code='owner')",
        params![session.user_id,request.branch_id], |row| row.get(0),
    ).map_err(db_error)?;
    if !owner_of_target {
        return Err("BRANCH_ACCESS_DENIED".to_string());
    }
    let user_exists: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM users WHERE id=?1)",
            [&request.user_id],
            |row| row.get(0),
        )
        .map_err(db_error)?;
    if !user_exists {
        return Err("USER_NOT_FOUND".to_string());
    }
    tx.execute(
        "DELETE FROM user_branch_roles WHERE user_id=?1 AND branch_id=?2",
        params![request.user_id, request.branch_id],
    )
    .map_err(db_error)?;
    tx.execute(
        "INSERT INTO user_branch_roles(user_id,branch_id,role_code,granted_by,granted_at) VALUES(?1,?2,?3,?4,?5)",
        params![request.user_id,request.branch_id,role_code,session.user_id,Utc::now().to_rfc3339()],
    ).map_err(db_error)?;
    let now = Utc::now().to_rfc3339();
    insert_audit(
        &tx,
        &Uuid::new_v4().to_string(),
        Some(&session.branch_id),
        Some(&session.user_id),
        "USER_ASSIGNED_TO_BRANCH",
        "user",
        Some(&request.user_id),
        None,
        &serde_json::json!({"branch_id":request.branch_id,"role_code":role_code}).to_string(),
        &now,
    )?;
    tx.commit().map_err(db_error)
}

fn copy_branch_catalog(
    tx: &Transaction<'_>,
    source_branch: &str,
    target_branch: &str,
) -> Result<(), String> {
    let categories: Vec<(String, String, String, Option<String>, i64, i64)> = {
        let mut statement = tx.prepare("SELECT id,name_ar,name_en,parent_id,is_active,sort_order FROM categories WHERE branch_id=?1 ORDER BY sort_order,name_ar").map_err(db_error)?;
        statement
            .query_map([source_branch], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            })
            .map_err(db_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(db_error)?
    };
    let category_ids: HashMap<String, String> = categories
        .iter()
        .map(|row| (row.0.clone(), Uuid::new_v4().to_string()))
        .collect();
    for (old_id, name_ar, name_en, parent_id, is_active, sort_order) in categories {
        let new_parent = parent_id.and_then(|parent| category_ids.get(&parent).cloned());
        tx.execute("INSERT INTO categories(id,branch_id,name_ar,name_en,parent_id,is_active,sort_order) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![category_ids[&old_id],target_branch,name_ar,name_en,new_parent,is_active,sort_order]).map_err(db_error)?;
    }
    let brands: Vec<(String, String, String, i64)> = {
        let mut statement = tx
            .prepare("SELECT id,name_ar,name_en,is_active FROM brands WHERE branch_id=?1")
            .map_err(db_error)?;
        statement
            .query_map([source_branch], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .map_err(db_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(db_error)?
    };
    for (_, name_ar, name_en, is_active) in brands {
        tx.execute(
            "INSERT INTO brands(id,branch_id,name_ar,name_en,is_active) VALUES(?1,?2,?3,?4,?5)",
            params![
                Uuid::new_v4().to_string(),
                target_branch,
                name_ar,
                name_en,
                is_active
            ],
        )
        .map_err(db_error)?;
    }
    let seasons: Vec<SeasonRow> = {
        let mut statement = tx
            .prepare("SELECT id,name_ar,name_en,starts_on,ends_on FROM seasons WHERE branch_id=?1")
            .map_err(db_error)?;
        statement
            .query_map([source_branch], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })
            .map_err(db_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(db_error)?
    };
    for (_, name_ar, name_en, starts_on, ends_on) in seasons {
        tx.execute("INSERT INTO seasons(id,branch_id,name_ar,name_en,starts_on,ends_on) VALUES(?1,?2,?3,?4,?5,?6)",
            params![Uuid::new_v4().to_string(),target_branch,name_ar,name_en,starts_on,ends_on]).map_err(db_error)?;
    }
    let sizes: Vec<(String, String, String, String, String, i64, i64)> = {
        let mut statement = tx.prepare("SELECT id,profile_code,size_code,name_ar,name_en,sort_order,is_active FROM sizes WHERE branch_id=?1").map_err(db_error)?;
        statement
            .query_map([source_branch], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            })
            .map_err(db_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(db_error)?
    };
    for (_, profile_code, size_code, name_ar, name_en, sort_order, is_active) in sizes {
        tx.execute("INSERT INTO sizes(id,branch_id,profile_code,size_code,name_ar,name_en,sort_order,is_active) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![Uuid::new_v4().to_string(),target_branch,profile_code,size_code,name_ar,name_en,sort_order,is_active]).map_err(db_error)?;
    }
    let colors: Vec<(String, String, String, String, Option<String>, i64)> = {
        let mut statement = tx.prepare("SELECT id,color_code,name_ar,name_en,hex_color,is_active FROM colors WHERE branch_id=?1").map_err(db_error)?;
        statement
            .query_map([source_branch], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            })
            .map_err(db_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(db_error)?
    };
    for (_, color_code, name_ar, name_en, hex_color, is_active) in colors {
        tx.execute("INSERT INTO colors(id,branch_id,color_code,name_ar,name_en,hex_color,is_active) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![Uuid::new_v4().to_string(),target_branch,color_code,name_ar,name_en,hex_color,is_active]).map_err(db_error)?;
    }
    Ok(())
}

#[tauri::command]
pub fn create_customer(
    state: tauri::State<'_, AppState>,
    session_token: String,
    request: CreateCustomerRequest,
) -> Result<CustomerSummary, String> {
    let session = require_permission(&state, &session_token, "customers.manage")?;
    let name = request.name.trim();
    if name.is_empty()
        || name.len() > 180
        || !matches!(request.consent_status.as_str(), "NOT_GIVEN" | "GIVEN")
    {
        return Err("CUSTOMER_INPUT_INVALID".to_string());
    }
    if request
        .email
        .as_ref()
        .is_some_and(|value| value.len() > 254)
        || request.phone.as_ref().is_some_and(|value| value.len() > 40)
        || request
            .notes
            .as_ref()
            .is_some_and(|value| value.len() > 4000)
        || request
            .size_preferences
            .as_ref()
            .is_some_and(|value| value.len() > 2000)
    {
        return Err("CUSTOMER_FIELD_TOO_LONG".to_string());
    }
    if let Some(preferences) = request.size_preferences.as_ref() {
        let parsed: serde_json::Value = serde_json::from_str(preferences)
            .map_err(|_| "SIZE_PREFERENCES_INVALID".to_string())?;
        if !parsed.is_array() {
            return Err("SIZE_PREFERENCES_INVALID".to_string());
        }
    }
    let phone_normalized = request
        .phone
        .as_ref()
        .map(|phone| normalize_phone(phone))
        .transpose()?;
    let phone_hmac = phone_normalized
        .as_ref()
        .map(|phone| db::personal_lookup_hmac(&state.personal_data_key, phone.as_bytes()));
    let encrypt = |value: Option<&str>| -> Result<Option<Vec<u8>>, String> {
        match value.map(str::trim).filter(|value| !value.is_empty()) {
            Some(value) => {
                db::encrypt_personal_field(&state.personal_data_key, value.as_bytes()).map(Some)
            }
            None => Ok(None),
        }
    };
    let name_ciphertext = db::encrypt_personal_field(&state.personal_data_key, name.as_bytes())?;
    let phone_ciphertext = encrypt(phone_normalized.as_deref())?;
    let email_ciphertext = encrypt(request.email.as_deref())?;
    let notes_ciphertext = encrypt(request.notes.as_deref())?;
    let size_preferences_ciphertext = encrypt(request.size_preferences.as_deref())?;
    let customer_id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let consent_at = if request.consent_status == "GIVEN" {
        Some(now.clone())
    } else {
        None
    };
    let mut db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db_error)?;
    if let Some(ref lookup) = phone_hmac {
        let exists:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM customers WHERE branch_id=?1 AND phone_lookup_hmac=?2 AND is_active=1)",params![session.branch_id,lookup],|row|row.get(0)).map_err(db_error)?;
        if exists {
            return Err("CUSTOMER_PHONE_ALREADY_EXISTS".to_string());
        }
    }
    tx.execute("INSERT INTO customers(id,branch_id,name_ciphertext,phone_ciphertext,phone_lookup_hmac,email_ciphertext,notes_ciphertext,size_preferences_ciphertext,consent_status,consent_at,created_at,updated_at,is_active) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?11,1)",
        params![customer_id,session.branch_id,name_ciphertext,phone_ciphertext,phone_hmac,email_ciphertext,notes_ciphertext,size_preferences_ciphertext,request.consent_status,consent_at,now]).map_err(db_error)?;
    insert_audit(
        &tx,
        &Uuid::new_v4().to_string(),
        Some(&session.branch_id),
        Some(&session.user_id),
        "CUSTOMER_CREATED",
        "customer",
        Some(&customer_id),
        None,
        "{\"pii\":\"encrypted\"}",
        &now,
    )?;
    tx.commit().map_err(db_error)?;
    Ok(CustomerSummary {
        id: customer_id,
        name: name.to_string(),
        phone: phone_normalized,
        email: request.email,
        consent_status: request.consent_status,
        loyalty_points: 0,
    })
}

#[tauri::command]
pub fn get_customer_by_phone(
    state: tauri::State<'_, AppState>,
    session_token: String,
    phone: String,
) -> Result<Option<CustomerSummary>, String> {
    let session = require_permission(&state, &session_token, "customers.manage")?;
    let normalized = normalize_phone(&phone)?;
    let lookup = db::personal_lookup_hmac(&state.personal_data_key, normalized.as_bytes());
    let db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
    let row=db.query_row("SELECT id,name_ciphertext,phone_ciphertext,email_ciphertext,consent_status FROM customers WHERE branch_id=?1 AND phone_lookup_hmac=?2 AND is_active=1",params![session.branch_id,lookup],|row|Ok((row.get::<_,String>(0)?,row.get::<_,Option<Vec<u8>>>(1)?,row.get::<_,Option<Vec<u8>>>(2)?,row.get::<_,Option<Vec<u8>>>(3)?,row.get::<_,String>(4)?))).optional().map_err(db_error)?;
    let Some((id, name_blob, phone_blob, email_blob, consent_status)) = row else {
        return Ok(None);
    };
    let decrypt = |value: Option<Vec<u8>>| -> Result<Option<String>, String> {
        match value {
            Some(blob) => {
                String::from_utf8(db::decrypt_personal_field(&state.personal_data_key, &blob)?)
                    .map(Some)
                    .map_err(|_| "CUSTOMER_DATA_INVALID".to_string())
            }
            None => Ok(None),
        }
    };
    let name = decrypt(name_blob)?.ok_or_else(|| "CUSTOMER_NAME_MISSING".to_string())?;
    let phone = decrypt(phone_blob)?;
    let email = decrypt(email_blob)?;
    let loyalty_points:i64=db.query_row("SELECT COALESCE(SUM(points_delta),0) FROM loyalty_ledger WHERE branch_id=?1 AND customer_id=?2",params![session.branch_id,id],|row|row.get(0)).map_err(db_error)?;
    Ok(Some(CustomerSummary {
        id,
        name,
        phone,
        email,
        consent_status,
        loyalty_points,
    }))
}

#[tauri::command]
pub fn create_supplier(
    state: tauri::State<'_, AppState>,
    session_token: String,
    request: CreateSupplierRequest,
) -> Result<SupplierOption, String> {
    let session = require_permission(&state, &session_token, "purchases.manage")?;
    let code = request.supplier_code.trim();
    let name = request.name.trim();
    if code.is_empty() || code.len() > 40 || name.is_empty() || name.len() > 200 {
        return Err("SUPPLIER_INPUT_INVALID".to_string());
    }
    let encode = |value: &Option<String>| -> Result<Option<Vec<u8>>, String> {
        match value.as_ref().map(|v| v.trim()).filter(|v| !v.is_empty()) {
            Some(value) if value.len() <= 4000 => {
                db::encrypt_personal_field(&state.personal_data_key, value.as_bytes()).map(Some)
            }
            Some(_) => Err("SUPPLIER_FIELD_TOO_LONG".to_string()),
            None => Ok(None),
        }
    };
    let phone = encode(&request.phone)?;
    let email = encode(&request.email)?;
    let address = encode(&request.address)?;
    let tax_registration = encode(&request.tax_registration)?;
    let notes = encode(&request.notes)?;
    let supplier_id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let mut db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db_error)?;
    tx.execute(
        "INSERT INTO suppliers(id,branch_id,supplier_code,name,phone_ciphertext,email_ciphertext,address_ciphertext,tax_registration_ciphertext,notes_ciphertext,created_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
        params![supplier_id,session.branch_id,code,name,phone,email,address,tax_registration,notes,now],
    ).map_err(|_| "SUPPLIER_CODE_ALREADY_EXISTS".to_string())?;
    insert_audit(
        &tx,
        &Uuid::new_v4().to_string(),
        Some(&session.branch_id),
        Some(&session.user_id),
        "SUPPLIER_CREATED",
        "supplier",
        Some(&supplier_id),
        None,
        "{}",
        &now,
    )?;
    tx.commit().map_err(db_error)?;
    Ok(SupplierOption {
        id: supplier_id,
        supplier_code: code.to_string(),
        name: name.to_string(),
    })
}

#[tauri::command]
pub fn get_suppliers(
    state: tauri::State<'_, AppState>,
    session_token: String,
) -> Result<Vec<SupplierOption>, String> {
    let session = require_permission(&state, &session_token, "purchases.manage")?;
    let db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
    let mut statement = db.prepare("SELECT id,supplier_code,name FROM suppliers WHERE branch_id=?1 AND is_active=1 ORDER BY name COLLATE NOCASE,id").map_err(db_error)?;
    let rows = statement
        .query_map([session.branch_id], |row| {
            Ok(SupplierOption {
                id: row.get(0)?,
                supplier_code: row.get(1)?,
                name: row.get(2)?,
            })
        })
        .map_err(db_error)?;
    rows.map(|row| row.map_err(db_error)).collect()
}

#[tauri::command]
pub fn get_open_purchase_orders(
    state: tauri::State<'_, AppState>,
    session_token: String,
) -> Result<Vec<PurchaseOrderOpen>, String> {
    let session = require_permission(&state, &session_token, "purchases.manage")?;
    let db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
    let mut statement=db.prepare("SELECT po.id,po.order_no,s.name,po.status,pol.id,v.sku,p.name_ar,p.name_en,sz.size_code,c.name_ar,pol.quantity_ordered-pol.quantity_received,pol.unit_cost_minor
       FROM purchase_orders po JOIN suppliers s ON s.branch_id=po.branch_id AND s.id=po.supplier_id
       JOIN purchase_order_lines pol ON pol.branch_id=po.branch_id AND pol.purchase_order_id=po.id
       JOIN product_variants v ON v.branch_id=pol.branch_id AND v.id=pol.variant_id
       JOIN products p ON p.branch_id=v.branch_id AND p.id=v.product_id
       JOIN sizes sz ON sz.branch_id=v.branch_id AND sz.id=v.size_id
       JOIN colors c ON c.branch_id=v.branch_id AND c.id=v.color_id
       WHERE po.branch_id=?1 AND po.status IN ('ORDERED','PARTIAL') AND pol.quantity_received<pol.quantity_ordered
       ORDER BY po.ordered_at,po.order_no,pol.id").map_err(db_error)?;
    let rows = statement
        .query_map([session.branch_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, String>(8)?,
                row.get::<_, String>(9)?,
                row.get::<_, i64>(10)?,
                row.get::<_, i64>(11)?,
            ))
        })
        .map_err(db_error)?;
    let mut orders: Vec<PurchaseOrderOpen> = Vec::new();
    let mut positions = HashMap::new();
    for row in rows {
        let (
            id,
            order_no,
            supplier_name,
            status,
            line_id,
            sku,
            name_ar,
            name_en,
            size_code,
            color_ar,
            quantity_open,
            unit_cost_minor,
        ) = row.map_err(db_error)?;
        let index = *positions.entry(id.clone()).or_insert_with(|| {
            orders.push(PurchaseOrderOpen {
                id: id.clone(),
                order_no,
                supplier_name,
                status,
                lines: Vec::new(),
            });
            orders.len() - 1
        });
        orders[index].lines.push(PurchaseOrderLineOpen {
            id: line_id,
            sku,
            product_name_ar: name_ar,
            product_name_en: name_en,
            size_code,
            color_ar,
            quantity_open,
            unit_cost_minor,
        });
    }
    Ok(orders)
}

#[tauri::command]
pub fn create_purchase_order(
    state: tauri::State<'_, AppState>,
    session_token: String,
    request: CreatePurchaseOrderRequest,
) -> Result<PurchaseOrderResult, String> {
    let session = require_permission(&state, &session_token, "purchases.manage")?;
    let order_no = request.order_no.trim();
    if order_no.is_empty()
        || order_no.len() > 48
        || request.lines.is_empty()
        || request.lines.len() > 500
    {
        return Err("PURCHASE_ORDER_INPUT_INVALID".to_string());
    }
    if let Some(expected_at) = request.expected_at.as_ref() {
        chrono::DateTime::parse_from_rfc3339(expected_at)
            .map_err(|_| "EXPECTED_DATE_INVALID".to_string())?;
    }
    if request
        .notes
        .as_ref()
        .is_some_and(|notes| notes.len() > 2000)
    {
        return Err("PURCHASE_NOTES_TOO_LONG".to_string());
    }
    let mut seen = HashSet::new();
    for line in &request.lines {
        if line.quantity <= 0
            || line.quantity > 100000
            || line.unit_cost_minor < 0
            || line.tax_rate_bps < 0
            || line.tax_rate_bps > 10000
            || !seen.insert(line.variant_id.clone())
        {
            return Err("PURCHASE_ORDER_LINE_INVALID".to_string());
        }
    }
    let mut db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db_error)?;
    let supplier_active: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM suppliers WHERE branch_id=?1 AND id=?2 AND is_active=1)",
            params![session.branch_id, request.supplier_id],
            |row| row.get(0),
        )
        .map_err(db_error)?;
    if !supplier_active {
        return Err("SUPPLIER_NOT_FOUND".to_string());
    }
    for line in &request.lines {
        let active:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM product_variants WHERE branch_id=?1 AND id=?2 AND is_active=1)",params![session.branch_id,line.variant_id],|row|row.get(0)).map_err(db_error)?;
        if !active {
            return Err("PURCHASE_VARIANT_NOT_FOUND".to_string());
        }
    }
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    tx.execute("INSERT INTO purchase_orders(id,branch_id,supplier_id,order_no,status,ordered_at,expected_at,notes,created_by,created_at,updated_at) VALUES(?1,?2,?3,?4,'ORDERED',?5,?6,?7,?8,?5,?5)",
        params![id,session.branch_id,request.supplier_id,order_no,now,request.expected_at,request.notes,session.user_id]).map_err(|_|"PURCHASE_ORDER_NO_ALREADY_EXISTS".to_string())?;
    for line in &request.lines {
        tx.execute("INSERT INTO purchase_order_lines(id,branch_id,purchase_order_id,variant_id,quantity_ordered,unit_cost_minor,tax_rate_bps) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![Uuid::new_v4().to_string(),session.branch_id,id,line.variant_id,line.quantity,line.unit_cost_minor,line.tax_rate_bps]).map_err(db_error)?;
    }
    insert_audit(
        &tx,
        &Uuid::new_v4().to_string(),
        Some(&session.branch_id),
        Some(&session.user_id),
        "PURCHASE_ORDER_CREATED",
        "purchase_order",
        Some(&id),
        None,
        &serde_json::json!({"order_no":order_no,"line_count":request.lines.len()}).to_string(),
        &now,
    )?;
    tx.commit().map_err(db_error)?;
    Ok(PurchaseOrderResult {
        purchase_order_id: id,
        order_no: order_no.to_string(),
    })
}

#[tauri::command]
pub fn receive_goods(
    state: tauri::State<'_, AppState>,
    session_token: String,
    request: ReceiveGoodsRequest,
) -> Result<GoodsReceiptResult, String> {
    let session = require_permission(&state, &session_token, "purchases.manage")?;
    if request.idempotency_key.len() < 8
        || request.idempotency_key.len() > 128
        || request.receipt_no.trim().is_empty()
        || request.receipt_no.len() > 48
        || request.lines.is_empty()
        || request.lines.len() > 500
    {
        return Err("GOODS_RECEIPT_INPUT_INVALID".to_string());
    }
    if request
        .supplier_invoice_ref
        .as_ref()
        .is_some_and(|value| value.len() > 120)
    {
        return Err("SUPPLIER_INVOICE_REF_TOO_LONG".to_string());
    }
    let mut seen = HashSet::new();
    for line in &request.lines {
        if line.quantity_received <= 0
            || line.quantity_received > 100000
            || !seen.insert(line.purchase_order_line_id.clone())
        {
            return Err("GOODS_RECEIPT_LINE_INVALID".to_string());
        }
    }
    let receipt_request_hash = request_hash(&request)?;
    let mut db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db_error)?;
    if let Some((receipt_id,receipt_no,count,stored_hash))=tx.query_row("SELECT id,receipt_no,(SELECT COUNT(*) FROM goods_receipt_lines WHERE branch_id=goods_receipts.branch_id AND goods_receipt_id=goods_receipts.id),request_hash FROM goods_receipts WHERE branch_id=?1 AND idempotency_key=?2",params![session.branch_id,request.idempotency_key],|row|Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,i64>(2)?,row.get::<_,String>(3)?))).optional().map_err(db_error)? {
        if stored_hash!=receipt_request_hash { return Err("IDEMPOTENCY_KEY_REUSED_WITH_DIFFERENT_REQUEST".to_string()); }
        tx.commit().map_err(db_error)?;
        return Ok(GoodsReceiptResult{receipt_id,receipt_no,received_line_count:count.max(0) as usize});
    }
    let (supplier_id, po_status): (String, String) = tx
        .query_row(
            "SELECT supplier_id,status FROM purchase_orders WHERE branch_id=?1 AND id=?2",
            params![session.branch_id, request.purchase_order_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(db_error)?
        .ok_or_else(|| "PURCHASE_ORDER_NOT_FOUND".to_string())?;
    if !matches!(po_status.as_str(), "ORDERED" | "PARTIAL") {
        return Err("PURCHASE_ORDER_NOT_RECEIVABLE".to_string());
    }
    let mut receipt_lines = Vec::with_capacity(request.lines.len());
    for line in &request.lines {
        let row=tx.query_row("SELECT variant_id,quantity_ordered,quantity_received,unit_cost_minor FROM purchase_order_lines WHERE branch_id=?1 AND purchase_order_id=?2 AND id=?3",params![session.branch_id,request.purchase_order_id,line.purchase_order_line_id],|row|Ok((row.get::<_,String>(0)?,row.get::<_,i64>(1)?,row.get::<_,i64>(2)?,row.get::<_,i64>(3)?))).optional().map_err(db_error)?.ok_or_else(||"PURCHASE_ORDER_LINE_NOT_FOUND".to_string())?;
        if line.quantity_received > row.1 - row.2 {
            return Err("RECEIVED_QUANTITY_EXCEEDS_OPEN_ORDER".to_string());
        }
        receipt_lines.push((
            line.purchase_order_line_id.clone(),
            row.0,
            line.quantity_received,
            row.3,
        ));
    }
    let receipt_id = Uuid::new_v4().to_string();
    let receipt_no = request.receipt_no.trim().to_string();
    let now = Utc::now().to_rfc3339();
    tx.execute("INSERT INTO goods_receipts(id,branch_id,purchase_order_id,supplier_id,receipt_no,supplier_invoice_ref,status,received_at,idempotency_key,request_hash,received_by) VALUES(?1,?2,?3,?4,?5,?6,'POSTED',?7,?8,?9,?10)",
        params![receipt_id,session.branch_id,request.purchase_order_id,supplier_id,receipt_no,request.supplier_invoice_ref,now,request.idempotency_key,receipt_request_hash,session.user_id]).map_err(|_|"RECEIPT_NO_ALREADY_EXISTS_OR_KEY_USED".to_string())?;
    for (po_line_id, variant_id, quantity, cost) in &receipt_lines {
        let receipt_line_id = Uuid::new_v4().to_string();
        tx.execute("INSERT INTO goods_receipt_lines(id,branch_id,goods_receipt_id,purchase_order_line_id,variant_id,quantity_received,unit_cost_minor) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![receipt_line_id,session.branch_id,receipt_id,po_line_id,variant_id,quantity,cost]).map_err(db_error)?;
        tx.execute("UPDATE purchase_order_lines SET quantity_received=quantity_received+?1 WHERE branch_id=?2 AND id=?3",params![quantity,session.branch_id,po_line_id]).map_err(db_error)?;
        tx.execute("INSERT INTO inventory_movements(id,branch_id,variant_id,bucket,movement_type,quantity_delta,unit_cost_minor,source_type,source_id,movement_group_id,idempotency_key,actor_user_id,created_at) VALUES(?1,?2,?3,'SELLABLE','GOODS_RECEIPT',?4,?5,'GOODS_RECEIPT',?6,?6,?7,?8,?9)",
            params![Uuid::new_v4().to_string(),session.branch_id,variant_id,quantity,cost,receipt_id,format!("goods-receipt:{}:{}",receipt_id,receipt_line_id),session.user_id,now]).map_err(|_|"GOODS_RECEIPT_STOCK_POST_FAILED".to_string())?;
    }
    let has_open:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM purchase_order_lines WHERE branch_id=?1 AND purchase_order_id=?2 AND quantity_received<quantity_ordered)",params![session.branch_id,request.purchase_order_id],|row|row.get(0)).map_err(db_error)?;
    tx.execute(
        "UPDATE purchase_orders SET status=?1,updated_at=?2 WHERE branch_id=?3 AND id=?4",
        params![
            if has_open { "PARTIAL" } else { "RECEIVED" },
            now,
            session.branch_id,
            request.purchase_order_id
        ],
    )
    .map_err(db_error)?;
    insert_audit(
        &tx,
        &Uuid::new_v4().to_string(),
        Some(&session.branch_id),
        Some(&session.user_id),
        "GOODS_RECEIVED",
        "goods_receipt",
        Some(&receipt_id),
        None,
        &serde_json::json!({"receipt_no":receipt_no,"line_count":receipt_lines.len()}).to_string(),
        &now,
    )?;
    tx.commit().map_err(db_error)?;
    Ok(GoodsReceiptResult {
        receipt_id,
        receipt_no,
        received_line_count: receipt_lines.len(),
    })
}

#[tauri::command]
pub fn create_encrypted_backup(
    state: tauri::State<'_, AppState>,
    session_token: String,
    destination_path: String,
    password: String,
    destination_kind: String,
) -> Result<BackupOperationResult, String> {
    let session = require_permission(&state, &session_token, "backup.manage")?;
    let destination = validate_backup_path(&destination_path)?;
    if !matches!(destination_kind.as_str(), "LOCAL" | "USB" | "EXTERNAL") {
        return Err("BACKUP_DESTINATION_KIND_INVALID".to_string());
    }
    if destination.exists() {
        return Err("BACKUP_DESTINATION_EXISTS".to_string());
    }
    let backup_id = Uuid::new_v4().to_string();
    let started_at = Utc::now().to_rfc3339();
    let filename = destination
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "BACKUP_PATH_INVALID".to_string())?
        .to_string();
    let mut db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db_error)?;
    tx.execute(
        "INSERT INTO backup_jobs(id,branch_id,status,backup_kind,filename,schema_version,destination_kind,started_at,created_by)
         VALUES(?1,?2,'STARTED','MANUAL',?3,?4,?5,?6,?7)",
        params![backup_id,session.branch_id,filename,db::CURRENT_SCHEMA_VERSION,destination_kind,started_at,session.user_id],
    ).map_err(db_error)?;
    tx.commit().map_err(db_error)?;

    let result =
        crate::backup::create_encrypted_backup(&db, &state.app_data_dir, &destination, &password);
    let finished_at = Utc::now().to_rfc3339();
    match result {
        Ok((byte_size, sha256)) => {
            let tx = db
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(db_error)?;
            tx.execute("UPDATE backup_jobs SET status='SUCCESS',byte_size=?1,sha256=?2,finished_at=?3 WHERE branch_id=?4 AND id=?5",
                params![i64::try_from(byte_size).map_err(|_| "BACKUP_SIZE_INVALID".to_string())?,sha256,finished_at,session.branch_id,backup_id]).map_err(db_error)?;
            insert_audit(&tx,&Uuid::new_v4().to_string(),Some(&session.branch_id),Some(&session.user_id),"BACKUP_CREATED","backup",Some(&backup_id),None,
                &serde_json::json!({"filename":filename,"byte_size":byte_size,"destination_kind":destination_kind}).to_string(),&finished_at)?;
            tx.commit().map_err(db_error)?;
            Ok(BackupOperationResult {
                status: "SUCCESS".to_string(),
                path: destination.to_string_lossy().into_owned(),
                byte_size,
            })
        }
        Err(error) => {
            let tx = db
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(db_error)?;
            tx.execute("UPDATE backup_jobs SET status='FAILED',error_code=?1,finished_at=?2 WHERE branch_id=?3 AND id=?4",
                params![error.chars().take(80).collect::<String>(),finished_at,session.branch_id,backup_id]).map_err(db_error)?;
            tx.commit().map_err(db_error)?;
            Err(error)
        }
    }
}

#[tauri::command]
pub fn restore_encrypted_backup(
    state: tauri::State<'_, AppState>,
    session_token: String,
    source_path: String,
    password: String,
) -> Result<BackupOperationResult, String> {
    let session = require_permission(&state, &session_token, "backup.manage")?;
    let source = validate_backup_path(&source_path)?;
    let source_metadata =
        std::fs::metadata(&source).map_err(|_| "BACKUP_FILE_READ_FAILED".to_string())?;
    let source_size = source_metadata.len();
    let filename = source
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "BACKUP_PATH_INVALID".to_string())?
        .to_string();
    let prepared = crate::backup::prepare_restore(&source, &state.app_data_dir, &password)?;
    let branch_exists: bool = prepared
        .connection()
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM branches WHERE id=?1)",
            [&session.branch_id],
            |row| row.get(0),
        )
        .map_err(|_| "BACKUP_STORE_MISMATCH".to_string())?;
    if !branch_exists {
        return Err("BACKUP_STORE_MISMATCH".to_string());
    }

    let backup_id = Uuid::new_v4().to_string();
    let pre_restore_path = state
        .app_data_dir
        .join("backups")
        .join(format!("pre-restore-{backup_id}.bkz"));
    let mut db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
    let (pre_size, pre_sha256) = crate::backup::create_encrypted_backup(
        &db,
        &state.app_data_dir,
        &pre_restore_path,
        &password,
    )?;
    if let Err(restore_error) = crate::backup::copy_into_current_database(
        prepared.connection(),
        &mut db,
        &state.app_data_dir,
    ) {
        let recovery =
            crate::backup::prepare_restore(&pre_restore_path, &state.app_data_dir, &password)
                .and_then(|prior| {
                    crate::backup::copy_into_current_database(
                        prior.connection(),
                        &mut db,
                        &state.app_data_dir,
                    )
                });
        if recovery.is_err() {
            return Err("RESTORE_FAILED_RECOVERY_REQUIRED".to_string());
        }
        return Err(format!(
            "RESTORE_FAILED_PREVIOUS_DATA_RESTORED:{restore_error}"
        ));
    }

    let completed_at = Utc::now().to_rfc3339();
    let audit_result = (|| -> Result<(), String> {
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_error)?;
        tx.execute(
            "INSERT INTO backup_jobs(id,branch_id,status,backup_kind,filename,schema_version,byte_size,sha256,destination_kind,started_at,finished_at,created_by)
             VALUES(?1,?2,'SUCCESS','PRE_RESTORE',?3,?4,?5,?6,'LOCAL',?7,?8,?9)",
            params![Uuid::new_v4().to_string(),session.branch_id,pre_restore_path.file_name().and_then(|name|name.to_str()).unwrap_or("pre-restore.bkz"),
                db::CURRENT_SCHEMA_VERSION,i64::try_from(pre_size).unwrap_or(i64::MAX),pre_sha256,completed_at,completed_at,session.user_id],
        ).map_err(db_error)?;
        tx.execute(
            "INSERT INTO backup_jobs(id,branch_id,status,backup_kind,filename,schema_version,byte_size,destination_kind,started_at,finished_at,created_by)
             VALUES(?1,?2,'RESTORED','MANUAL',?3,?4,?5,'LOCAL',?6,?7,?8)",
            params![backup_id,session.branch_id,filename,db::CURRENT_SCHEMA_VERSION,i64::try_from(source_size).unwrap_or(i64::MAX),completed_at,completed_at,session.user_id],
        ).map_err(db_error)?;
        insert_audit(&tx,&Uuid::new_v4().to_string(),Some(&session.branch_id),Some(&session.user_id),"BACKUP_RESTORED","backup",Some(&backup_id),None,
            &serde_json::json!({"filename":filename,"pre_restore_backup":pre_restore_path.file_name().and_then(|name|name.to_str()).unwrap_or("pre-restore.bkz")}).to_string(),&completed_at)?;
        tx.commit().map_err(db_error)
    })();
    let status = if audit_result.is_ok() {
        "RESTORED"
    } else {
        "RESTORED_AUDIT_WARNING"
    };
    Ok(BackupOperationResult {
        status: status.to_string(),
        path: source.to_string_lossy().into_owned(),
        byte_size: source_size,
    })
}

#[tauri::command]
pub fn get_backup_history(
    state: tauri::State<'_, AppState>,
    session_token: String,
) -> Result<Vec<BackupHistoryRow>, String> {
    let session = require_permission(&state, &session_token, "backup.manage")?;
    let db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
    let mut statement = db.prepare(
        "SELECT id,status,backup_kind,filename,byte_size,started_at,finished_at FROM backup_jobs
         WHERE branch_id=?1 ORDER BY started_at DESC LIMIT 25"
    ).map_err(db_error)?;
    let rows = statement
        .query_map([session.branch_id], |row| {
            Ok(BackupHistoryRow {
                id: row.get(0)?,
                status: row.get(1)?,
                backup_kind: row.get(2)?,
                filename: row.get(3)?,
                byte_size: row.get::<_, Option<i64>>(4)?.map(|size| size.max(0) as u64),
                started_at: row.get(5)?,
                finished_at: row.get(6)?,
            })
        })
        .map_err(db_error)?;
    rows.map(|row| row.map_err(db_error)).collect()
}

fn validate_backup_path(value: &str) -> Result<PathBuf, String> {
    if value.trim().is_empty() || value.len() > 4096 {
        return Err("BACKUP_PATH_INVALID".to_string());
    }
    let path = PathBuf::from(value.trim());
    if !path.is_absolute()
        || !path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("bkz"))
    {
        return Err("BACKUP_PATH_INVALID".to_string());
    }
    Ok(path)
}

#[tauri::command]
pub fn get_sales_summary(
    state: tauri::State<'_, AppState>,
    session_token: String,
    from_utc: String,
    to_utc: String,
) -> Result<SalesSummary, String> {
    let session = require_permission(&state, &session_token, "reports.sales")?;
    let from = chrono::DateTime::parse_from_rfc3339(&from_utc)
        .map_err(|_| "REPORT_RANGE_INVALID".to_string())?
        .with_timezone(&Utc);
    let to = chrono::DateTime::parse_from_rfc3339(&to_utc)
        .map_err(|_| "REPORT_RANGE_INVALID".to_string())?
        .with_timezone(&Utc);
    if from >= to || to - from > chrono::Duration::days(370) {
        return Err("REPORT_RANGE_INVALID".to_string());
    }
    let db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
    let (invoice_count,gross_minor,discounts_minor,tax_minor):(i64,i64,i64,i64)=db.query_row(
        "SELECT COUNT(*),COALESCE(SUM(total_minor),0),COALESCE(SUM(item_discount_minor+invoice_discount_minor),0),COALESCE(SUM(tax_minor),0)
         FROM invoices WHERE branch_id=?1 AND created_at>=?2 AND created_at<?3 AND status<>'VOIDED'",
        params![session.branch_id,from.to_rfc3339(),to.to_rfc3339()],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?)),
    ).map_err(db_error)?;
    let returns_minor:i64=db.query_row("SELECT COALESCE(SUM(refund_due_minor),0) FROM return_documents WHERE branch_id=?1 AND created_at>=?2 AND created_at<?3 AND status='COMPLETED'",params![session.branch_id,from.to_rfc3339(),to.to_rfc3339()],|row|row.get(0)).map_err(db_error)?;
    Ok(SalesSummary {
        invoice_count,
        gross_minor,
        discounts_minor,
        tax_minor,
        returns_minor,
        net_minor: gross_minor.saturating_sub(returns_minor),
    })
}

#[tauri::command]
pub fn get_profit_summary(
    state: tauri::State<'_, AppState>,
    session_token: String,
    from_utc: String,
    to_utc: String,
) -> Result<ProfitSummary, String> {
    let session = require_permission(&state, &session_token, "reports.profit")?;
    let from = chrono::DateTime::parse_from_rfc3339(&from_utc)
        .map_err(|_| "REPORT_RANGE_INVALID".to_string())?
        .with_timezone(&Utc);
    let to = chrono::DateTime::parse_from_rfc3339(&to_utc)
        .map_err(|_| "REPORT_RANGE_INVALID".to_string())?
        .with_timezone(&Utc);
    if from >= to || to - from > chrono::Duration::days(370) {
        return Err("REPORT_RANGE_INVALID".to_string());
    }
    let db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
    let (revenue_minor,cost_minor):(i64,i64)=db.query_row(
        "SELECT COALESCE(SUM(il.line_total_minor-il.tax_minor),0),COALESCE(SUM(il.unit_cost_snapshot_minor*il.quantity),0)
         FROM invoice_lines il JOIN invoices i ON i.branch_id=il.branch_id AND i.id=il.invoice_id
         WHERE i.branch_id=?1 AND i.created_at>=?2 AND i.created_at<?3 AND i.status<>'VOIDED'",
        params![session.branch_id,from.to_rfc3339(),to.to_rfc3339()],|row|Ok((row.get(0)?,row.get(1)?)),
    ).map_err(db_error)?;
    let (returned_minor,restocked_cost_minor):(i64,i64)=db.query_row(
        "SELECT COALESCE(SUM(rl.refund_line_minor-CAST((il.tax_minor*rl.quantity+il.quantity/2)/il.quantity AS INTEGER)),0),COALESCE(SUM(CASE WHEN rl.disposition='RESTOCK' THEN il.unit_cost_snapshot_minor*rl.quantity ELSE 0 END),0)
         FROM return_lines rl JOIN return_documents rd ON rd.branch_id=rl.branch_id AND rd.id=rl.return_document_id
         JOIN invoices i ON i.branch_id=rd.branch_id AND i.id=rd.original_invoice_id
         JOIN invoice_lines il ON il.branch_id=rl.branch_id AND il.id=rl.original_invoice_line_id
         WHERE rd.branch_id=?1 AND rd.created_at>=?2 AND rd.created_at<?3 AND rd.status='COMPLETED'",
        params![session.branch_id,from.to_rfc3339(),to.to_rfc3339()],|row|Ok((row.get(0)?,row.get(1)?)),
    ).map_err(db_error)?;
    let net_revenue = revenue_minor.saturating_sub(returned_minor);
    let net_cost = cost_minor.saturating_sub(restocked_cost_minor);
    let gross_profit = net_revenue.saturating_sub(net_cost);
    let margin_bps = if net_revenue == 0 {
        0
    } else {
        rounded_signed_ratio(gross_profit as i128 * 10000, net_revenue as i128)?
    };
    Ok(ProfitSummary {
        revenue_minor,
        cost_minor,
        returned_minor,
        restocked_cost_minor,
        gross_profit_minor: gross_profit,
        margin_bps,
    })
}

#[tauri::command]
pub fn configure_tax(
    state: tauri::State<'_, AppState>,

    session_token: String,
    tax_rate_bps: i64,
    prices_include_tax: bool,
) -> Result<(), String> {
    let session = require_permission(&state, &session_token, "settings.manage")?;
    let mut db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db_error)?;
    persist_tax(
        &tx,
        &session.branch_id,
        &session.user_id,
        tax_rate_bps,
        prices_include_tax,
    )?;
    tx.commit().map_err(db_error)
}

#[tauri::command]
pub fn sqlcipher_health(state: tauri::State<'_, AppState>) -> Result<String, String> {
    let db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
    let version: String = db
        .query_row("PRAGMA cipher_version", [], |row| row.get(0))
        .map_err(|_| "SQLCIPHER_REQUIRED".to_string())?;
    if version.trim().is_empty() {
        return Err("SQLCIPHER_REQUIRED".to_string());
    }
    Ok(version)
}

pub(crate) fn require_permission(
    state: &AppState,
    token: &str,
    permission: &str,
) -> Result<Session, String> {
    let license = crate::license::ensure_valid(state)?;
    crate::license::record_trusted_time(state, &license)?;
    let required_feature = match permission {
        "sales.read" | "sales.create" | "sales.discount" | "customers.manage" => Some("pos.sell"),
        "catalog.manage"
        | "inventory.read"
        | "inventory.adjust"
        | "inventory.sell_negative"
        | "purchases.manage"
        | "imports.manage" => Some("inventory.manage"),
        "returns.process" => Some("returns.exchange"),
        "reports.sales" | "audit.read" => Some("reports.basic"),
        "reports.profit" | "commissions.read" => Some("reports.profit"),
        "backup.manage" => Some("backup.restore"),
        _ => None,
    };
    if let Some(feature) = required_feature
        && !license.features.iter().any(|licensed| licensed == feature)
    {
        return Err(format!("LICENSE_FEATURE_NOT_INCLUDED:{feature}"));
    }
    let mut sessions = state
        .sessions
        .lock()
        .map_err(|_| "SESSION_LOCK_FAILED".to_string())?;
    let session = sessions
        .get_mut(token)
        .ok_or_else(|| "SESSION_REQUIRED".to_string())?;
    if Instant::now() > session.expires_at {
        sessions.remove(token);
        return Err("SESSION_EXPIRED".to_string());
    }
    session.expires_at = Instant::now() + SESSION_IDLE;
    let session_copy = Session {
        user_id: session.user_id.clone(),
        branch_id: session.branch_id.clone(),
        expires_at: session.expires_at,
    };
    drop(sessions);
    let db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
    require_permission_with_db(&db, &session_copy, permission)?;
    Ok(session_copy)
}

fn require_permission_with_db(
    db: &Connection,
    session: &Session,
    permission: &str,
) -> Result<(), String> {
    let allowed: bool = db.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM user_branch_roles ubr JOIN role_permissions rp ON rp.role_code=ubr.role_code
           WHERE ubr.user_id=?1 AND ubr.branch_id=?2 AND rp.permission_code=?3
         )",
        params![session.user_id, session.branch_id, permission], |row| row.get(0),
    ).map_err(db_error)?;
    if allowed {
        Ok(())
    } else {
        Err("PERMISSION_DENIED".to_string())
    }
}

fn issue_session(
    state: &AppState,
    user_id: String,
    branch_id: String,
    display_name: String,
) -> AuthResult {
    let token = Uuid::new_v4().to_string() + &Uuid::new_v4().simple().to_string();
    if let Ok(mut sessions) = state.sessions.lock() {
        sessions.insert(
            token.clone(),
            Session {
                user_id,
                branch_id: branch_id.clone(),
                expires_at: Instant::now() + SESSION_IDLE,
            },
        );
    }
    AuthResult {
        session_token: token,
        display_name,
        branch_id,
    }
}

fn hash_password(password: &str) -> Result<String, String> {
    let mut salt_bytes = [0u8; 16];
    getrandom::fill(&mut salt_bytes).map_err(|_| "CSPRNG_FAILED".to_string())?;
    let salt =
        SaltString::encode_b64(&salt_bytes).map_err(|_| "PASSWORD_SALT_FAILED".to_string())?;
    let phc = argon2_instance()?
        .hash_password(password.as_bytes(), &salt)
        .map_err(|_| "PASSWORD_HASH_FAILED".to_string())?;
    Ok(phc.to_string())
}

/// Verify a password against a stored Argon2id hash.
///
/// The owner panel uses the same verification as the sign in, so a password
/// that opens the panel is the password of the owner account.
pub(crate) fn verify_password(password: &str, parsed: &PasswordHash) -> Result<(), String> {
    argon2_instance()?
        .verify_password(password.as_bytes(), parsed)
        .map_err(|_| "PASSWORD_INVALID".to_string())
}

fn argon2_instance() -> Result<Argon2<'static>, String> {
    let params =
        Params::new(64 * 1024, 3, 1, None).map_err(|_| "ARGON2_CONFIG_INVALID".to_string())?;
    Ok(Argon2::new(Algorithm::Argon2id, Version::V0x13, params))
}

fn request_hash<T: Serialize>(value: &T) -> Result<String, String> {
    let bytes = serde_json::to_vec(value).map_err(|_| "REQUEST_HASH_FAILED".to_string())?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

fn rounded_ratio(numerator: i128, denominator: i128) -> Result<i64, String> {
    if denominator <= 0 || numerator < 0 {
        return Err("AMOUNT_INVALID".to_string());
    }
    let rounded = (numerator + denominator / 2) / denominator;
    i64::try_from(rounded).map_err(|_| "AMOUNT_OVERFLOW".to_string())
}

/// Tax is approved at 14% with rounding to the nearest whole EGP.
/// Amounts are held in minor units (piastres), so one EGP is 100 minor units.
fn round_tax_to_policy(tax_minor: i64) -> Result<i64, String> {
    if tax_minor < 0 {
        return Err("AMOUNT_INVALID".to_string());
    }
    let rounded = ((tax_minor as i128 + 50) / 100) * 100;
    i64::try_from(rounded).map_err(|_| "AMOUNT_OVERFLOW".to_string())
}

fn rounded_signed_ratio(numerator: i128, denominator: i128) -> Result<i64, String> {
    if denominator <= 0 {
        return Err("AMOUNT_INVALID".to_string());
    }
    if numerator >= 0 {
        rounded_ratio(numerator, denominator)
    } else {
        rounded_ratio(
            numerator
                .checked_neg()
                .ok_or_else(|| "AMOUNT_OVERFLOW".to_string())?,
            denominator,
        )?
        .checked_neg()
        .ok_or_else(|| "AMOUNT_OVERFLOW".to_string())
    }
}

/// Append one entry to the tamper-evident audit log.
///
/// The parameters mirror the `audit_log` columns one for one, which is why
/// they are kept positional rather than grouped into a struct.
#[allow(clippy::too_many_arguments)]
pub(crate) fn insert_audit(
    tx: &Transaction<'_>,
    id: &str,
    branch_id: Option<&str>,
    actor: Option<&str>,
    action: &str,
    entity_type: &str,
    entity_id: Option<&str>,
    reason: Option<&str>,
    details: &str,
    occurred_at: &str,
) -> Result<(), String> {
    let previous: Option<String> = tx.query_row(
        "SELECT event_hash FROM audit_log WHERE branch_id IS ?1 ORDER BY occurred_at DESC, id DESC LIMIT 1",
        [branch_id], |row| row.get(0),
    ).optional().map_err(db_error)?;
    let content = format!(
        "{}|{}|{}|{}|{}|{}|{}|{}",
        previous.as_deref().unwrap_or(""),
        occurred_at,
        actor.unwrap_or(""),
        action,
        entity_type,
        entity_id.unwrap_or(""),
        reason.unwrap_or(""),
        details
    );
    let event_hash = hex::encode(Sha256::digest(content.as_bytes()));
    tx.execute(
        "INSERT INTO audit_log(id,branch_id,actor_user_id,action_code,entity_type,entity_id,reason,details_json,occurred_at,previous_hash,event_hash)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        params![id, branch_id, actor, action, entity_type, entity_id, reason, details, occurred_at, previous, event_hash],
    ).map_err(db_error)?;
    Ok(())
}

fn normalize_phone(input: &str) -> Result<String, String> {
    let mut normalized = String::new();
    for character in input.trim().chars() {
        // Digits are kept as they are; a leading plus sign is kept too.
        if character.is_ascii_digit() || (character == '+' && normalized.is_empty()) {
            normalized.push(character);
        } else if matches!(character, ' ' | '-' | '(' | ')' | '.') {
            continue;
        } else {
            return Err("PHONE_FORMAT_INVALID".to_string());
        }
    }
    let digits = normalized.strip_prefix('+').unwrap_or(&normalized);
    if !(8..=15).contains(&digits.len()) {
        return Err("PHONE_FORMAT_INVALID".to_string());
    }
    Ok(normalized)
}

pub(crate) fn db_error(error: rusqlite::Error) -> String {
    // Do not expose SQL values, customer data, or DB paths to the WebView.
    format!(
        "DATABASE_ERROR:{}",
        match error {
            rusqlite::Error::SqliteFailure(ref code, _) => format!("{:?}", code.code),
            rusqlite::Error::QueryReturnedNoRows => "NOT_FOUND".to_string(),
            _ => "OPERATION_FAILED".to_string(),
        }
    )
}

#[cfg(test)]
mod tax_policy_tests {
    use super::round_tax_to_policy;

    #[test]
    fn tax_rounds_to_nearest_whole_egp() {
        // Minor units are piastres: 100 minor = 1 EGP.
        assert_eq!(round_tax_to_policy(1_240), Ok(1_000));
        assert_eq!(round_tax_to_policy(1_250), Ok(1_300));
        assert_eq!(round_tax_to_policy(49), Ok(0));
        assert_eq!(round_tax_to_policy(50), Ok(100));
        assert_eq!(round_tax_to_policy(0), Ok(0));
        assert!(round_tax_to_policy(-1).is_err());
    }
}
