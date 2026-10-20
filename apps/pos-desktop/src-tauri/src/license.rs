use crate::db;
use chrono::Utc;
use naseej_license_format::{
    ActivationRequest, SignedLicense, create_request, device_info, embedded_public_key,
    validate_license_for_device,
};
use rusqlite::OptionalExtension;
use serde::Serialize;
use std::{collections::BTreeMap, env, fs, path::PathBuf};

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LicenseStatus {
    pub valid: bool,
    pub status_code: String,
    pub device_fingerprint: Option<String>,
    pub license_id: Option<String>,
    pub customer_id: Option<String>,
    pub plan_code: Option<String>,
    pub expires_at_utc: Option<String>,
    pub features: Vec<String>,
    pub limits: BTreeMap<String, u64>,
    pub public_key_configured: bool,
}

pub fn current_status() -> LicenseStatus {
    let public_key = match embedded_public_key() {
        Ok(key) => key,
        Err(_) => return empty_status("PUBLIC_KEY_NOT_CONFIGURED", None, false),
    };
    let device = match device_info() {
        Ok(device) => device,
        Err(_) => return empty_status("DEVICE_FINGERPRINT_UNAVAILABLE", None, true),
    };
    let path = match shared_license_path() {
        Ok(path) => path,
        Err(_) => {
            return empty_status(
                "LICENSE_STORAGE_UNAVAILABLE",
                Some(device.fingerprint),
                true,
            );
        }
    };
    if !path.exists() {
        return empty_status("ACTIVATION_REQUIRED", Some(device.fingerprint), true);
    }
    let protected = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(_) => return empty_status("LICENSE_READ_FAILED", Some(device.fingerprint), true),
    };
    let clear = match db::unprotect_current_user_data(&protected) {
        Ok(bytes) => bytes,
        Err(_) => return empty_status("LICENSE_DECRYPT_FAILED", Some(device.fingerprint), true),
    };
    let envelope: SignedLicense = match serde_json::from_slice(&clear) {
        Ok(envelope) => envelope,
        Err(_) => return empty_status("LICENSE_FORMAT_INVALID", Some(device.fingerprint), true),
    };
    if let Err(error) =
        validate_license_for_device(&envelope, &public_key, &device.fingerprint, Utc::now())
    {
        let code = match error {
            naseej_license_format::LicenseError::Expired => "LICENSE_EXPIRED",
            naseej_license_format::LicenseError::NotYetValid => "LICENSE_NOT_YET_VALID",
            naseej_license_format::LicenseError::WrongDevice => "LICENSE_DEVICE_MISMATCH",
            naseej_license_format::LicenseError::WrongProduct => "LICENSE_PRODUCT_MISMATCH",
            naseej_license_format::LicenseError::InvalidSignature => "LICENSE_SIGNATURE_INVALID",
            _ => "LICENSE_INVALID",
        };
        return empty_status(code, Some(device.fingerprint), true);
    }
    LicenseStatus {
        valid: true,
        status_code: "ACTIVE".to_string(),
        device_fingerprint: Some(device.fingerprint),
        license_id: Some(envelope.payload.license_id),
        customer_id: Some(envelope.payload.customer_id),
        plan_code: envelope.payload.plan_code,
        expires_at_utc: Some(envelope.payload.expires_at_utc),
        features: envelope.payload.features.into_iter().collect(),
        limits: envelope.payload.limits,
        public_key_configured: true,
    }
}

pub fn ensure_valid() -> Result<LicenseStatus, String> {
    let status = current_status();
    if status.valid {
        Ok(status)
    } else {
        Err(format!("LICENSE_REQUIRED:{}", status.status_code))
    }
}

pub fn record_trusted_time(state: &crate::AppState, status: &LicenseStatus) -> Result<(), String> {
    if !status.valid {
        return Err("LICENSE_REQUIRED".to_string());
    }
    let now = Utc::now();
    let now_text = now.to_rfc3339();
    let mut db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
    let tx = db
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|_| "LICENSE_CLOCK_STATE_FAILED".to_string())?;
    let previous: Option<String> = tx
        .query_row(
            "SELECT last_seen_utc FROM local_license_state WHERE singleton=1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|_| "LICENSE_CLOCK_STATE_FAILED".to_string())?;
    if let Some(previous) = previous {
        let previous = chrono::DateTime::parse_from_rfc3339(&previous)
            .map_err(|_| "LICENSE_CLOCK_STATE_INVALID".to_string())?
            .with_timezone(&Utc);
        if now + chrono::Duration::minutes(5) < previous {
            return Err("CLOCK_ROLLBACK_DETECTED".to_string());
        }
    }
    tx.execute(
        "INSERT INTO local_license_state(singleton,license_id,signed_payload_ciphertext,last_seen_utc,last_invoice_counter,state_updated_at)
         VALUES(1,?1,NULL,?2,0,?2)
         ON CONFLICT(singleton) DO UPDATE SET license_id=excluded.license_id,last_seen_utc=excluded.last_seen_utc,state_updated_at=excluded.state_updated_at",
        rusqlite::params![status.license_id, now_text],
    ).map_err(|_| "LICENSE_CLOCK_STATE_FAILED".to_string())?;
    tx.commit()
        .map_err(|_| "LICENSE_CLOCK_STATE_FAILED".to_string())
}

pub fn activation_request() -> Result<ActivationRequest, String> {
    create_request(env!("CARGO_PKG_VERSION")).map_err(|error| match error {
        naseej_license_format::LicenseError::FingerprintUnavailable => {
            "DEVICE_FINGERPRINT_UNAVAILABLE".to_string()
        }
        other => other.to_string(),
    })
}

pub fn import_signed_license(license_json: &str) -> Result<LicenseStatus, String> {
    if license_json.len() > 64 * 1024 {
        return Err("LICENSE_FILE_TOO_LARGE".to_string());
    }
    let envelope: SignedLicense =
        serde_json::from_str(license_json).map_err(|_| "LICENSE_FORMAT_INVALID".to_string())?;
    let public_key = embedded_public_key().map_err(|_| "PUBLIC_KEY_NOT_CONFIGURED".to_string())?;
    let fingerprint = device_info()
        .map_err(|_| "DEVICE_FINGERPRINT_UNAVAILABLE".to_string())?
        .fingerprint;
    validate_license_for_device(&envelope, &public_key, &fingerprint, Utc::now())
        .map_err(|error| error.to_string())?;
    let clear =
        serde_json::to_vec(&envelope).map_err(|_| "LICENSE_SERIALIZE_FAILED".to_string())?;
    let protected = db::protect_current_user_data(&clear)?;
    let path = shared_license_path()?;
    let parent = path
        .parent()
        .ok_or_else(|| "LICENSE_PATH_INVALID".to_string())?;
    fs::create_dir_all(parent).map_err(|_| "LICENSE_DIR_CREATE_FAILED".to_string())?;
    let temp = parent.join(format!("license.state.{}.part", std::process::id()));
    {
        use std::io::Write;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&temp)
            .map_err(|_| "LICENSE_TEMP_CREATE_FAILED".to_string())?;
        file.write_all(&protected)
            .map_err(|_| "LICENSE_TEMP_WRITE_FAILED".to_string())?;
        file.sync_all()
            .map_err(|_| "LICENSE_TEMP_SYNC_FAILED".to_string())?;
    }
    fs::rename(&temp, &path).map_err(|_| {
        let _ = fs::remove_file(&temp);
        "LICENSE_ATOMIC_REPLACE_FAILED".to_string()
    })?;
    Ok(current_status())
}

fn empty_status(
    code: &str,
    fingerprint: Option<String>,
    public_key_configured: bool,
) -> LicenseStatus {
    LicenseStatus {
        valid: false,
        status_code: code.to_string(),
        device_fingerprint: fingerprint,
        license_id: None,
        customer_id: None,
        plan_code: None,
        expires_at_utc: None,
        features: Vec::new(),
        limits: BTreeMap::new(),
        public_key_configured,
    }
}

fn shared_license_path() -> Result<PathBuf, String> {
    let local = env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| "LOCALAPPDATA_UNAVAILABLE".to_string())?;
    Ok(local
        .join("NaseejPOS")
        .join("Licensing")
        .join("license.state.dpapi"))
}
