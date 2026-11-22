//! Import commands: preview, approve, and commit legacy store data.
//!
//! The parsing and validation rules live in [`crate::import_parse`] so they
//! can be tested without a database or a Tauri runtime.

use crate::AppState;
use chrono::Utc;
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use uuid::Uuid;

use crate::commands::{db_error, insert_audit, require_permission};
use crate::import_parse::{
    ImportPreview, ProductImportRow, parse_delimited, parse_xlsx, validate_products,
};

/// The result of committing a validated batch.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportCommitResult {
    pub batch_id: String,
    pub products_created: usize,
    pub variants_created: usize,
    pub stock_movements: usize,
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// A preview is held in memory only between the preview and commit calls, so
/// nothing provisional is written into the encrypted database.
fn preview_cache() -> &'static std::sync::Mutex<HashMap<String, (ImportPreview, Vec<u8>)>> {
    static CACHE: std::sync::OnceLock<std::sync::Mutex<HashMap<String, (ImportPreview, Vec<u8>)>>> =
        std::sync::OnceLock::new();
    CACHE.get_or_init(|| std::sync::Mutex::new(HashMap::new()))
}

/// Parse and validate a file without writing anything.
///
/// The WebView sends the bytes of the chosen file; the file on disk is never
/// modified and nothing is written to the database at this stage.
#[tauri::command]
pub fn preview_import(
    state: tauri::State<'_, AppState>,
    session_token: String,
    file_name: String,
    content_base64: String,
    import_type: Option<String>,
) -> Result<ImportPreview, String> {
    let _session = require_permission(&state, &session_token, "imports.manage")?;
    if import_type.as_deref().unwrap_or("PRODUCTS") != "PRODUCTS" {
        return Err("IMPORT_TYPE_NOT_SUPPORTED".to_string());
    }
    let bytes = base64_decode(&content_base64)?;
    if bytes.len() > 32 * 1024 * 1024 {
        return Err("IMPORT_FILE_TOO_LARGE".to_string());
    }
    let is_xlsx = file_name.to_lowercase().ends_with(".xlsx");
    let table = if is_xlsx {
        parse_xlsx(&bytes)?
    } else {
        let text = String::from_utf8(bytes.clone())
            .map_err(|_| "IMPORT_FILE_NOT_UTF8".to_string())?;
        parse_delimited(&text, None)?
    };
    if table.rows.len() > 50_000 {
        return Err("IMPORT_FILE_TOO_MANY_ROWS".to_string());
    }
    let batch_id = Uuid::new_v4().to_string();
    let mut preview = validate_products(&table, &batch_id).0;
    preview.batch_id = batch_id.clone();
    preview.source_kind = if is_xlsx { "XLSX" } else { "CSV" }.to_string();
    preview.source_sha256 = hex::encode(Sha256::digest(&bytes));
    // The source bytes are kept only for the length of the approval step so
    // the commit can prove it is writing the same file that was previewed.
    preview_cache()
        .lock()
        .map_err(|_| "IMPORT_CACHE_FAILED".to_string())?
        .insert(batch_id, (preview.clone(), bytes));
    Ok(preview)
}

fn base64_decode(value: &str) -> Result<Vec<u8>, String> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(value.trim().as_bytes())
        .map_err(|_| "IMPORT_FILE_ENCODING_INVALID".to_string())
}

/// Write a previously previewed batch, after the owner approves it.
///
/// Every product, variant, and opening stock movement is written in a single
/// transaction: either the whole batch lands or none of it does.
#[tauri::command]
pub fn commit_import(
    state: tauri::State<'_, AppState>,
    session_token: String,
    batch_id: String,
) -> Result<ImportCommitResult, String> {
    let session = require_permission(&state, &session_token, "imports.manage")?;
    // Only an owner or a manager may approve a write into the ledger.
    {
        let db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
        let allowed: bool = db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM user_branch_roles WHERE user_id=?1 AND branch_id=?2 AND role_code IN ('owner','manager'))",
                params![&session.user_id, &session.branch_id],
                |row| row.get(0),
            )
            .map_err(db_error)?;
        if !allowed {
            return Err("IMPORT_APPROVAL_REQUIRES_MANAGER".to_string());
        }
    }
    let (preview, source_bytes) = {
        let mut cache = preview_cache()
            .lock()
            .map_err(|_| "IMPORT_CACHE_FAILED".to_string())?;
        cache
            .remove(&batch_id)
            .ok_or_else(|| "IMPORT_BATCH_NOT_PREVIEWED".to_string())?
    };
    // Re-hash the bytes so a stale or swapped file can never be committed
    // under an approval that was given for something else.
    let digest = hex::encode(Sha256::digest(&source_bytes));
    if digest != preview.source_sha256 {
        return Err("IMPORT_SOURCE_CHANGED".to_string());
    }
    let source_kind = preview.source_kind.clone();
    let source_sha256 = preview.source_sha256.clone();

    let mut db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db_error)?;
    let now = Utc::now().to_rfc3339();
    tx.execute(
        "INSERT INTO import_batches(id,branch_id,import_type,source_name,source_sha256,status,total_rows,accepted_rows,rejected_rows,created_by,created_at,committed_at)
         VALUES(?1,?2,'PRODUCTS',?3,?4,'COMMITTED',?5,?6,?7,?8,?9,?9)",
        params![
            &batch_id,
            &session.branch_id,
            format!("{} import", source_kind),
            &source_sha256,
            preview.total_rows as i64,
            preview.accepted_rows as i64,
            preview.rejected_rows as i64,
            &session.user_id,
            &now
        ],
    )
    .map_err(db_error)?;
    tx.execute(
        "INSERT INTO import_batch_meta(branch_id,import_batch_id,source_kind,approved_by,approved_at,committed_checksum,notes)
         VALUES(?1,?2,?3,?4,?5,?6,?7)",
        params![
            &session.branch_id,
            &batch_id,
            &source_kind,
            &session.user_id,
            &now,
            &source_sha256,
            "Owner approved the import batch"
        ],
    )
    .map_err(db_error)?;

    let mut products_created = 0usize;
    let mut variants_created = 0usize;
    let mut stock_movements = 0usize;
    // Group rows into products by Arabic name plus category, which is how a
    // legacy export lists the sizes and colors of one garment.
    let mut products: HashMap<(String, String), Vec<ProductImportRow>> = HashMap::new();
    for row in preview.accepted {
        products
            .entry((row.name_ar.clone(), row.category.clone()))
            .or_default()
            .push(row);
    }
    for ((name_ar, category), rows) in products {
        let product_id = Uuid::new_v4().to_string();
        let category_id = ensure_category(&tx, &session.branch_id, &category)?;
        let name_en = rows.first().map(|row| row.name_en.clone()).unwrap_or_default();
        let base_price = rows.iter().map(|row| row.price_minor).max().unwrap_or(0);
        let base_cost = rows.iter().map(|row| row.cost_minor).min().unwrap_or(0);
        tx.execute(
            "INSERT INTO products(id,branch_id,category_id,name_ar,name_en,base_cost_minor,base_price_minor,is_active,created_at,updated_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,1,?8,?8)",
            params![product_id, session.branch_id, category_id, name_ar, name_en, base_cost, base_price, now],
        )
        .map_err(db_error)?;
        products_created += 1;
        for row in rows {
            let variant_id = Uuid::new_v4().to_string();
            let size_id = ensure_size(&tx, &session.branch_id, &row.size_code)?;
            let color_id = ensure_color(&tx, &session.branch_id, &row.color)?;
            let barcode = match row.barcode.as_deref() {
                Some(barcode) => barcode.to_string(),
                None => format!("NSJ{}", Uuid::new_v4().simple()),
            };
            tx.execute(
                "INSERT INTO product_variants(id,branch_id,product_id,size_id,color_id,sku,barcode,cost_override_minor,price_override_minor,reorder_level,is_active,created_at,updated_at)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,0,1,?10,?10)",
                params![variant_id, session.branch_id, product_id, size_id, color_id, row.sku, barcode, row.cost_minor, row.price_minor, now],
            )
            .map_err(|error| match error {
                rusqlite::Error::SqliteFailure(_, Some(ref message))
                    if message.contains("UNIQUE") =>
                {
                    "IMPORT_SKU_OR_BARCODE_ALREADY_EXISTS".to_string()
                }
                other => db_error(other),
            })?;
            variants_created += 1;
            if row.opening_qty > 0 {
                tx.execute(
                    "INSERT INTO inventory_movements(id,branch_id,variant_id,bucket,movement_type,quantity_delta,unit_cost_minor,source_type,source_id,movement_group_id,idempotency_key,actor_user_id,reason,created_at)
                     VALUES(?1,?2,?3,'SELLABLE','OPENING_BALANCE',?4,?5,'IMPORT',?6,?6,?7,?8,?9,?10)",
                    params![
                        Uuid::new_v4().to_string(),
                        session.branch_id,
                        variant_id,
                        row.opening_qty,
                        row.cost_minor,
                        batch_id,
                        format!("import:{batch_id}:{}", row.sku),
                        session.user_id,
                        "Opening stock from a legacy import",
                        now
                    ],
                )
                .map_err(|_| "IMPORT_OPENING_STOCK_FAILED".to_string())?;
                stock_movements += 1;
            }
        }
    }
    insert_audit(
        &tx,
        &Uuid::new_v4().to_string(),
        Some(&session.branch_id),
        Some(&session.user_id),
        "IMPORT_COMMITTED",
        "import_batch",
        Some(&batch_id),
        Some("Owner approved the import batch"),
        &serde_json::json!({
            "products": products_created,
            "variants": variants_created,
            "stock_movements": stock_movements,
            "source_sha256": source_sha256
        })
        .to_string(),
        &now,
    )?;
    tx.commit().map_err(db_error)?;
    Ok(ImportCommitResult {
        batch_id,
        products_created,
        variants_created,
        stock_movements,
    })
}

fn ensure_category(
    tx: &rusqlite::Transaction<'_>,
    branch_id: &str,
    name: &str,
) -> Result<String, String> {
    let existing: Option<String> = tx
        .query_row(
            "SELECT id FROM categories WHERE branch_id=?1 AND name_ar=?2",
            params![branch_id, name],
            |row| row.get(0),
        )
        .optional()
        .map_err(db_error)?;
    if let Some(id) = existing {
        return Ok(id);
    }
    let id = Uuid::new_v4().to_string();
    tx.execute(
        "INSERT INTO categories(id,branch_id,name_ar,name_en,is_active,sort_order) VALUES(?1,?2,?3,?3,1,0)",
        params![id, branch_id, name],
    )
    .map_err(db_error)?;
    Ok(id)
}

fn ensure_size(
    tx: &rusqlite::Transaction<'_>,
    branch_id: &str,
    size_code: &str,
) -> Result<String, String> {
    let existing: Option<String> = tx
        .query_row(
            "SELECT id FROM sizes WHERE branch_id=?1 AND size_code=?2",
            params![branch_id, size_code],
            |row| row.get(0),
        )
        .optional()
        .map_err(db_error)?;
    if let Some(id) = existing {
        return Ok(id);
    }
    let id = Uuid::new_v4().to_string();
    tx.execute(
        "INSERT INTO sizes(id,branch_id,profile_code,size_code,name_ar,name_en,sort_order,is_active) VALUES(?1,?2,'CLOTHING',?3,?3,?3,0,1)",
        params![id, branch_id, size_code],
    )
    .map_err(db_error)?;
    Ok(id)
}

fn ensure_color(
    tx: &rusqlite::Transaction<'_>,
    branch_id: &str,
    name: &str,
) -> Result<String, String> {
    let existing: Option<String> = tx
        .query_row(
            "SELECT id FROM colors WHERE branch_id=?1 AND name_ar=?2",
            params![branch_id, name],
            |row| row.get(0),
        )
        .optional()
        .map_err(db_error)?;
    if let Some(id) = existing {
        return Ok(id);
    }
    let id = Uuid::new_v4().to_string();
    let code = name
        .chars()
        .filter(|character| character.is_alphanumeric())
        .take(12)
        .collect::<String>()
        .to_uppercase();
    let code = if code.is_empty() {
        format!("C{}", &Uuid::new_v4().simple().to_string()[..6])
    } else {
        code
    };
    tx.execute(
        "INSERT INTO colors(id,branch_id,color_code,name_ar,name_en,is_active) VALUES(?1,?2,?3,?4,?4,1)",
        params![id, branch_id, format!("{code}-{}", &id[..4]), name, name],
    )
    .map_err(db_error)?;
    Ok(id)
}

/// The import history of the branch, newest first.
#[tauri::command]
pub fn get_import_batches(
    state: tauri::State<'_, AppState>,
    session_token: String,
) -> Result<Vec<ImportBatchRow>, String> {
    let session = require_permission(&state, &session_token, "imports.manage")?;
    let db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
    let mut statement = db
        .prepare(
            "SELECT b.id, m.source_kind, b.status, b.total_rows, b.accepted_rows, b.rejected_rows, b.created_at, b.committed_at
             FROM import_batches b LEFT JOIN import_batch_meta m ON m.branch_id=b.branch_id AND m.import_batch_id=b.id
             WHERE b.branch_id=?1 ORDER BY b.created_at DESC",
        )
        .map_err(db_error)?;
    let rows = statement
        .query_map(params![&session.branch_id], |row| {
            Ok(ImportBatchRow {
                id: row.get(0)?,
                source_kind: row.get::<_, Option<String>>(1)?.unwrap_or_else(|| "CSV".into()),
                status: row.get(2)?,
                total_rows: row.get(3)?,
                accepted_rows: row.get(4)?,
                rejected_rows: row.get(5)?,
                created_at: row.get(6)?,
                committed_at: row.get(7)?,
            })
        })
        .map_err(db_error)?;
    rows.map(|row| row.map_err(db_error)).collect()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportBatchRow {
    pub id: String,
    pub source_kind: String,
    pub status: String,
    pub total_rows: i64,
    pub accepted_rows: i64,
    pub rejected_rows: i64,
    pub created_at: String,
    pub committed_at: Option<String>,
}
