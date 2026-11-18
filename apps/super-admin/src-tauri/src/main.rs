//! The developer dashboard of Naseej.
//!
//! This application is the internal side of the licensing system: it holds
//! the signing key in a vault, it issues the subscriptions of the customers
//! and it keeps the registry of everything that was issued.
//!
//! It is deliberately a separate application. The customer application only
//! ever knows the public key, so a copy of the POS installer on a customer
//! machine can never be turned into a licence signer.

mod dpapi;
mod vault;

use chrono::Utc;
use naseej_license_format::ActivationRequest;
use naseej_license_registry::{
    IssueOptions, Plan, SubscriptionStatus, allowed_features, issue_license, read_records,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, path::PathBuf, sync::Mutex};
use tauri::Manager;
use zeroize::Zeroizing;

/// The state of the dashboard.
struct AppState {
    app_data_dir: PathBuf,
    registry_dir: PathBuf,
    /// The signing seed while the vault is unlocked.
    seed: Mutex<Option<Zeroizing<[u8; 32]>>>,
}

/// What the dashboard knows about the vault.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VaultStatus {
    exists: bool,
    unlocked: bool,
    public_key_hex: Option<String>,
    registry_dir: String,
    app_data_dir: String,
}

/// One issued subscription, as the dashboard shows it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SubscriptionRow {
    license_id: String,
    request_id: String,
    customer_id: String,
    plan_code: String,
    issued_at_utc: String,
    not_before_utc: String,
    expires_at_utc: String,
    device_fingerprint: String,
    license_file: String,
    features: Vec<String>,
    limits: BTreeMap<String, u64>,
    status: String,
    revoked: bool,
}

/// The revocation marker of a licence.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RevocationRecord {
    license_id: String,
    revoked_at_utc: String,
    reason: String,
}

/// The features a licence may carry.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FeatureCatalog {
    features: Vec<String>,
    plans: Vec<PlanOption>,
}

/// One plan the dashboard can issue.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PlanOption {
    code: String,
    label: String,
    days: i64,
}

/// The result of an issue operation.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct IssueResult {
    license_id: String,
    customer_id: String,
    plan_code: String,
    plan_label: String,
    issued_at_utc: String,
    expires_at_utc: String,
    license_path: String,
    license_content: String,
    warnings: Vec<String>,
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let app_data_dir = app.path().app_data_dir()?;
            let registry_dir = app_data_dir.join("subscriptions");
            fs::create_dir_all(&registry_dir)?;
            app.manage(AppState {
                app_data_dir,
                registry_dir,
                seed: Mutex::new(None),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            vault_status,
            vault_create,
            vault_import_seed,
            vault_unlock,
            vault_lock,
            feature_catalog,
            list_subscriptions,
            issue_subscription,
            revoke_subscription,
            read_license,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run the Naseej developer dashboard");
}

/// The path of the vault file.
fn vault_path(state: &AppState) -> PathBuf {
    state.app_data_dir.join("vault.bin")
}

/// The state of the vault.
fn status_of(state: &AppState) -> Result<VaultStatus, String> {
    let guard = state
        .seed
        .lock()
        .map_err(|_| "VAULT_LOCK_FAILED".to_string())?;
    let public_key_hex = guard.as_ref().map(|seed| vault::public_key_hex(seed));
    Ok(VaultStatus {
        exists: vault_path(state).exists(),
        unlocked: guard.is_some(),
        public_key_hex,
        registry_dir: state.registry_dir.display().to_string(),
        app_data_dir: state.app_data_dir.display().to_string(),
    })
}

/// The state of the vault.
#[tauri::command]
fn vault_status(state: tauri::State<'_, AppState>) -> Result<VaultStatus, String> {
    status_of(&state)
}

/// Create a new vault with a freshly generated signing key.
#[tauri::command]
fn vault_create(
    state: tauri::State<'_, AppState>,
    passphrase: String,
) -> Result<VaultStatus, String> {
    let path = vault_path(&state);
    if path.exists() {
        return Err("VAULT_ALREADY_EXISTS".to_string());
    }
    let seed = vault::generate_seed()?;
    vault::create(&path, &seed, &passphrase)?;
    {
        let mut guard = state
            .seed
            .lock()
            .map_err(|_| "VAULT_LOCK_FAILED".to_string())?;
        *guard = Some(seed);
    }
    status_of(&state)
}

/// Import an existing signing seed into the vault.
#[tauri::command]
fn vault_import_seed(
    state: tauri::State<'_, AppState>,
    passphrase: String,
    seed_hex: String,
) -> Result<VaultStatus, String> {
    let path = vault_path(&state);
    if path.exists() {
        return Err("VAULT_ALREADY_EXISTS".to_string());
    }
    let seed = vault::seed_from_hex(&seed_hex)?;
    vault::create(&path, &seed, &passphrase)?;
    {
        let mut guard = state
            .seed
            .lock()
            .map_err(|_| "VAULT_LOCK_FAILED".to_string())?;
        *guard = Some(seed);
    }
    status_of(&state)
}

/// Unlock the vault.
#[tauri::command]
fn vault_unlock(
    state: tauri::State<'_, AppState>,
    passphrase: String,
) -> Result<VaultStatus, String> {
    let seed = vault::unlock(&vault_path(&state), &passphrase)?;
    {
        let mut guard = state
            .seed
            .lock()
            .map_err(|_| "VAULT_LOCK_FAILED".to_string())?;
        *guard = Some(seed);
    }
    status_of(&state)
}

/// Lock the vault and wipe the seed from memory.
#[tauri::command]
fn vault_lock(state: tauri::State<'_, AppState>) -> Result<VaultStatus, String> {
    {
        let mut guard = state
            .seed
            .lock()
            .map_err(|_| "VAULT_LOCK_FAILED".to_string())?;
        *guard = None;
    }
    status_of(&state)
}

/// The features and the plans the dashboard can issue.
#[tauri::command]
fn feature_catalog() -> Result<FeatureCatalog, String> {
    Ok(FeatureCatalog {
        features: allowed_features().into_iter().collect(),
        plans: vec![
            PlanOption {
                code: Plan::Trial7d.code().to_string(),
                label: Plan::Trial7d.label().to_string(),
                days: 7,
            },
            PlanOption {
                code: Plan::Annual12m.code().to_string(),
                label: Plan::Annual12m.label().to_string(),
                days: 365,
            },
        ],
    })
}

/// The path of the revocation marker of a licence.
fn revocation_path(state: &AppState, license_id: &str) -> PathBuf {
    state
        .registry_dir
        .join("revoked")
        .join(format!("{license_id}.json"))
}

/// Every subscription this workstation issued.
#[tauri::command]
fn list_subscriptions(state: tauri::State<'_, AppState>) -> Result<Vec<SubscriptionRow>, String> {
    let records = read_records(&state.registry_dir)?;
    let now = Utc::now();
    let rows = records
        .into_iter()
        .map(|record| {
            let revoked = revocation_path(&state, &record.license_id).exists();
            let status = SubscriptionStatus::of(&record, now).code().to_string();
            SubscriptionRow {
                license_id: record.license_id,
                request_id: record.request_id,
                customer_id: record.customer_id,
                plan_code: record.plan_code,
                issued_at_utc: record.issued_at_utc,
                not_before_utc: record.not_before_utc,
                expires_at_utc: record.expires_at_utc,
                device_fingerprint: record.device_fingerprint,
                license_file: record.license_file,
                features: record.features,
                limits: record.limits,
                status,
                revoked,
            }
        })
        .collect();
    Ok(rows)
}

/// Issue a subscription for an activation request.
///
/// The request arrives as the text of the file the customer sent, so the
/// dashboard never depends on where that file happens to sit.
#[tauri::command]
fn issue_subscription(
    state: tauri::State<'_, AppState>,
    request_json: String,
    customer_id: String,
    plan_code: String,
    features: Vec<String>,
    max_users: u64,
    max_branches: u64,
) -> Result<IssueResult, String> {
    {
        let guard = state
            .seed
            .lock()
            .map_err(|_| "VAULT_LOCK_FAILED".to_string())?;
        if guard.is_none() {
            return Err("VAULT_LOCKED".to_string());
        }
    }
    if request_json.len() > 64 * 1024 {
        return Err("the activation request is too large".to_string());
    }
    let request: ActivationRequest =
        serde_json::from_str(&request_json).map_err(|_| "REQUEST_INVALID".to_string())?;
    naseej_license_registry::validate_request(&request)?;

    let requests_dir = state.app_data_dir.join("requests");
    fs::create_dir_all(&requests_dir).map_err(|error| error.to_string())?;
    let request_path = requests_dir.join(format!("{}.req", request.request_id));
    fs::write(&request_path, request_json.as_bytes()).map_err(|error| error.to_string())?;

    let licenses_dir = state.app_data_dir.join("licenses");
    fs::create_dir_all(&licenses_dir).map_err(|error| error.to_string())?;

    let plan = match plan_code.as_str() {
        code if code == Plan::Trial7d.code() => Plan::Trial7d,
        code if code == Plan::Annual12m.code() => Plan::Annual12m,
        _ => return Err("PLAN_UNKNOWN".to_string()),
    };

    let signing_key = {
        let guard = state
            .seed
            .lock()
            .map_err(|_| "VAULT_LOCK_FAILED".to_string())?;
        let seed = guard.as_ref().ok_or_else(|| "VAULT_LOCKED".to_string())?;
        ed25519_dalek::SigningKey::from_bytes(seed)
    };

    let outcome = issue_license(
        &signing_key,
        IssueOptions {
            request_path,
            customer_id,
            plan,
            starts_at_utc: None,
            requested_features: features,
            max_users,
            max_branches,
            registry_dir: state.registry_dir.clone(),
            output_dir: Some(licenses_dir),
        },
    )?;

    let license_content =
        fs::read_to_string(&outcome.license_path).map_err(|error| error.to_string())?;
    Ok(IssueResult {
        license_id: outcome.license_id,
        customer_id: outcome.customer_id,
        plan_code: outcome.plan_code,
        plan_label: outcome.plan_label,
        issued_at_utc: outcome.issued_at_utc,
        expires_at_utc: outcome.expires_at_utc,
        license_path: outcome.license_path.display().to_string(),
        license_content,
        warnings: outcome.warnings,
    })
}

/// Record a revocation.
///
/// The dashboard is offline, so a revocation is a note in the registry of
/// this workstation: it stops the licence from being renewed or reissued for
/// that customer, and it does not reach a licence that was already handed
/// over.
#[tauri::command]
fn revoke_subscription(
    state: tauri::State<'_, AppState>,
    license_id: String,
    reason: String,
) -> Result<(), String> {
    let trimmed = license_id.trim();
    if trimmed.is_empty() || trimmed.len() > 128 {
        return Err("LICENSE_ID_INVALID".to_string());
    }
    let path = revocation_path(&state, trimmed);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let record = RevocationRecord {
        license_id: trimmed.to_string(),
        revoked_at_utc: Utc::now().to_rfc3339(),
        reason: reason.trim().to_string(),
    };
    let bytes = serde_json::to_vec_pretty(&record).map_err(|error| error.to_string())?;
    fs::write(&path, bytes).map_err(|error| error.to_string())?;
    Ok(())
}

/// The licence file of a subscription, so it can be handed over again.
#[tauri::command]
fn read_license(state: tauri::State<'_, AppState>, license_id: String) -> Result<String, String> {
    let records = read_records(&state.registry_dir)?;
    let record = records
        .into_iter()
        .find(|record| record.license_id == license_id.trim())
        .ok_or_else(|| "LICENSE_NOT_FOUND".to_string())?;
    fs::read_to_string(&record.license_file).map_err(|_| "LICENSE_FILE_MISSING".to_string())
}
