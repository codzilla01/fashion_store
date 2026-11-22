use crate::db;
use chrono::{DateTime, Duration, Utc};
use naseej_license_format::{
    ActivationRequest, PLAN_TRIAL_LOCAL, SignedLicense, create_request, device_info,
    embedded_public_key, grace_period_days, local_trial_features, local_trial_limits,
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
    /// True while the application runs on an automatically started local
    /// evaluation instead of a signed licence.
    pub local_trial: bool,
    /// True while the licence is expired but still inside the grace period.
    pub grace_period: bool,
    /// Instant at which the grace period ends, when one applies.
    pub grace_expires_at_utc: Option<String>,
    /// False when this build carries no real production licence key.
    ///
    /// A development build must never be handed to a paying customer, so the
    /// user interface surfaces this prominently.
    pub production_build: bool,
}

/// Outcome of looking for a signed licence on disk.
///
/// The status is boxed so the enum stays small: `Absent` carries no data and
/// the common case should not pay for the size of a full status.
enum SignedLookup {
    /// No licence file exists yet.
    Absent,
    /// A licence file exists and was evaluated.
    Present(Box<LicenseStatus>),
}

/// Current licensing state of the installation.
///
/// A signed licence always wins. When no signed licence is present at all the
/// application falls back to a local, device-bound evaluation so the product
/// can be tried immediately; once that evaluation and its grace period have
/// elapsed, a signed licence is required.
pub fn current_status(state: &crate::AppState) -> LicenseStatus {
    let fingerprint = match device_info() {
        Ok(device) => device.fingerprint,
        Err(_) => return empty_status("DEVICE_FINGERPRINT_UNAVAILABLE", None, true),
    };
    match signed_license_status(Some(fingerprint.clone())) {
        SignedLookup::Present(status) => *status,
        SignedLookup::Absent => match local_trial_status(state, fingerprint) {
            Ok(status) => status,
            Err(code) => empty_status(&code, None, true),
        },
    }
}

/// Status derived from a signed licence file, if one is present and readable.
///
/// A licence file that exists but fails validation is reported as-is; the
/// application never silently downgrades a broken licence to an evaluation.
fn signed_license_status(fingerprint: Option<String>) -> SignedLookup {
    let public_key = match embedded_public_key() {
        Ok(key) => key,
        Err(_) => {
            return SignedLookup::Present(Box::new(empty_status(
                "PUBLIC_KEY_NOT_CONFIGURED",
                fingerprint,
                false,
            )));
        }
    };
    let device = match device_info() {
        Ok(device) => device,
        Err(_) => {
            return SignedLookup::Present(Box::new(empty_status(
                "DEVICE_FINGERPRINT_UNAVAILABLE",
                fingerprint,
                true,
            )));
        }
    };
    let path = match shared_license_path() {
        Ok(path) => path,
        Err(_) => {
            return SignedLookup::Present(Box::new(empty_status(
                "LICENSE_STORAGE_UNAVAILABLE",
                Some(device.fingerprint),
                true,
            )));
        }
    };
    if !path.exists() {
        return SignedLookup::Absent;
    }
    let protected = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(_) => {
            return SignedLookup::Present(Box::new(empty_status(
                "LICENSE_READ_FAILED",
                Some(device.fingerprint),
                true,
            )));
        }
    };
    let clear = match db::unprotect_current_user_data(&protected) {
        Ok(bytes) => bytes,
        Err(_) => {
            return SignedLookup::Present(Box::new(empty_status(
                "LICENSE_DECRYPT_FAILED",
                Some(device.fingerprint),
                true,
            )));
        }
    };
    let envelope: SignedLicense = match serde_json::from_slice(&clear) {
        Ok(envelope) => envelope,
        Err(_) => {
            return SignedLookup::Present(Box::new(empty_status(
                "LICENSE_FORMAT_INVALID",
                Some(device.fingerprint),
                true,
            )));
        }
    };
    let now = Utc::now();
    match validate_license_for_device(&envelope, &public_key, &device.fingerprint, now) {
        Ok(()) => SignedLookup::Present(Box::new(LicenseStatus {
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
            local_trial: false,
            grace_period: false,
            grace_expires_at_utc: None,
            production_build: naseej_license_format::is_production_key_configured(),
        })),
        Err(error) => {
            let code = match error {
                naseej_license_format::LicenseError::Expired => "LICENSE_EXPIRED",
                naseej_license_format::LicenseError::NotYetValid => "LICENSE_NOT_YET_VALID",
                naseej_license_format::LicenseError::WrongDevice => "LICENSE_DEVICE_MISMATCH",
                naseej_license_format::LicenseError::WrongProduct => "LICENSE_PRODUCT_MISMATCH",
                naseej_license_format::LicenseError::InvalidSignature => {
                    "LICENSE_SIGNATURE_INVALID"
                }
                _ => "LICENSE_INVALID",
            };
            // An expired licence still grants the grace period, so a merchant
            // is not locked out of their own shop in the middle of a day.
            let grace_end = if code == "LICENSE_EXPIRED" {
                DateTime::parse_from_rfc3339(&envelope.payload.expires_at_utc)
                    .ok()
                    .map(|expires| {
                        expires.with_timezone(&Utc) + Duration::days(grace_period_days())
                    })
            } else {
                None
            };
            if let Some(grace_end) = grace_end.filter(|end| now < *end) {
                return SignedLookup::Present(Box::new(LicenseStatus {
                    valid: true,
                    status_code: "GRACE_PERIOD".to_string(),
                    device_fingerprint: Some(device.fingerprint),
                    license_id: Some(envelope.payload.license_id),
                    customer_id: Some(envelope.payload.customer_id),
                    plan_code: envelope.payload.plan_code,
                    expires_at_utc: Some(envelope.payload.expires_at_utc),
                    features: envelope.payload.features.into_iter().collect(),
                    limits: envelope.payload.limits,
                    public_key_configured: true,
                    local_trial: false,
                    grace_period: true,
                    grace_expires_at_utc: Some(grace_end.to_rfc3339()),
                    production_build: naseej_license_format::is_production_key_configured(),
                }));
            }
            SignedLookup::Present(Box::new(empty_status(code, Some(device.fingerprint), true)))
        }
    }
}

/// Record the start of the local evaluation.
///
/// Called once when the application state is created. The row is written only
/// the first time; every later start reads the existing row, so the trial
/// window cannot be extended by restarting the application.
pub fn initialize_local_trial(state: &crate::AppState) -> Result<(), String> {
    let fingerprint = match device_info() {
        Ok(device) => device.fingerprint,
        Err(_) => return Ok(()),
    };
    load_or_start_local_trial(state, &fingerprint).map(|_| ())
}

/// Local, device-bound evaluation entitlement.
///
/// The evaluation is recorded once in the encrypted database so it cannot be
/// restarted by deleting files, and it is bound to the device fingerprint.
fn local_trial_status(
    state: &crate::AppState,
    fingerprint: String,
) -> Result<LicenseStatus, String> {
    let trial = load_or_start_local_trial(state, &fingerprint)?;
    let now = Utc::now();
    let trial_expires = parse_utc(&trial.trial_expires_at_utc)?;
    let grace_expires = parse_utc(&trial.grace_expires_at_utc)?;
    let public_key_configured = embedded_public_key().is_ok();
    if now < trial_expires {
        return Ok(LicenseStatus {
            valid: true,
            status_code: "TRIAL_ACTIVE".to_string(),
            device_fingerprint: Some(fingerprint),
            license_id: Some(trial.trial_id),
            customer_id: None,
            plan_code: Some(PLAN_TRIAL_LOCAL.to_string()),
            expires_at_utc: Some(trial_expires.to_rfc3339()),
            features: local_trial_features().into_iter().collect(),
            limits: local_trial_limits(),
            public_key_configured,
            local_trial: true,
            grace_period: false,
            grace_expires_at_utc: None,
            production_build: naseej_license_format::is_production_key_configured(),
        });
    }
    if now < grace_expires {
        return Ok(LicenseStatus {
            valid: true,
            status_code: "TRIAL_GRACE_PERIOD".to_string(),
            device_fingerprint: Some(fingerprint),
            license_id: Some(trial.trial_id),
            customer_id: None,
            plan_code: Some(PLAN_TRIAL_LOCAL.to_string()),
            expires_at_utc: Some(trial_expires.to_rfc3339()),
            features: local_trial_features().into_iter().collect(),
            limits: local_trial_limits(),
            public_key_configured,
            local_trial: true,
            grace_period: true,
            grace_expires_at_utc: Some(grace_expires.to_rfc3339()),
            production_build: naseej_license_format::is_production_key_configured(),
        });
    }
    Ok(empty_status("LICENSE_REQUIRED", Some(fingerprint), true))
}

struct LocalTrial {
    trial_id: String,
    trial_expires_at_utc: String,
    grace_expires_at_utc: String,
}

fn parse_utc(value: &str) -> Result<DateTime<Utc>, String> {
    DateTime::parse_from_rfc3339(value)
        .map(|parsed| parsed.with_timezone(&Utc))
        .map_err(|_| "LICENSE_CLOCK_STATE_INVALID".to_string())
}

/// Read the recorded evaluation, starting it on first use.
///
/// The row is inserted once and never updated: the append-only trigger on
/// `local_trial_state` enforces that the start instant cannot be moved.
fn load_or_start_local_trial(
    state: &crate::AppState,
    fingerprint: &str,
) -> Result<LocalTrial, String> {
    let now = Utc::now();
    let trial_expires = now + Duration::days(naseej_license_format::LOCAL_TRIAL_DAYS);
    let grace_expires = trial_expires + Duration::days(grace_period_days());
    let trial_id = format!("trial-{}", &uuid::Uuid::new_v4().to_string()[..8]);

    let mut db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
    let tx = db
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|_| "LICENSE_CLOCK_STATE_FAILED".to_string())?;
    let existing: Option<(String, String)> = tx
        .query_row(
            "SELECT trial_expires_at_utc, grace_expires_at_utc
             FROM local_trial_state WHERE singleton=1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|_| "LICENSE_CLOCK_STATE_FAILED".to_string())?;
    let result = match existing {
        Some((trial_expires_at_utc, grace_expires_at_utc)) => LocalTrial {
            trial_id,
            trial_expires_at_utc,
            grace_expires_at_utc,
        },
        None => {
            tx.execute(
                "INSERT INTO local_trial_state(
                   singleton, device_fingerprint, trial_started_at_utc,
                   trial_expires_at_utc, grace_expires_at_utc, created_at
                 ) VALUES(1, ?1, ?2, ?3, ?4, ?2)",
                rusqlite::params![
                    fingerprint,
                    now.to_rfc3339(),
                    trial_expires.to_rfc3339(),
                    grace_expires.to_rfc3339()
                ],
            )
            .map_err(|_| "LICENSE_CLOCK_STATE_FAILED".to_string())?;
            LocalTrial {
                trial_id,
                trial_expires_at_utc: trial_expires.to_rfc3339(),
                grace_expires_at_utc: grace_expires.to_rfc3339(),
            }
        }
    };
    tx.commit()
        .map_err(|_| "LICENSE_CLOCK_STATE_FAILED".to_string())?;
    Ok(result)
}

pub fn ensure_valid(state: &crate::AppState) -> Result<LicenseStatus, String> {
    let status = current_status(state);
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
    let high_water: Option<String> = tx
        .query_row(
            "SELECT high_water_utc FROM clock_high_water WHERE singleton=1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|_| "LICENSE_CLOCK_STATE_FAILED".to_string())?;

    // Both the last observation and the all-time high-water mark are trusted.
    // A forward jump followed by a return to real time is therefore detected
    // just like an ordinary rollback.
    let mut reference = previous.as_deref().map(parse_utc).transpose()?;
    if let Some(recorded_high) = high_water.as_deref().map(parse_utc).transpose()? {
        reference = Some(match reference {
            Some(value) => value.max(recorded_high),
            None => recorded_high,
        });
    }
    if let Some(reference) = reference {
        if now + chrono::Duration::minutes(5) < reference {
            return Err("CLOCK_ROLLBACK_DETECTED".to_string());
        }
    }
    let observed_high = reference
        .map(|value| value.max(now).to_rfc3339())
        .unwrap_or_else(|| now_text.clone());

    tx.execute(
        "INSERT INTO clock_high_water(singleton,high_water_utc,updated_at)
         VALUES(1,?1,?2)
         ON CONFLICT(singleton) DO UPDATE SET high_water_utc=excluded.high_water_utc,updated_at=excluded.updated_at",
        rusqlite::params![observed_high, now_text],
    )
    .map_err(|_| "LICENSE_CLOCK_STATE_FAILED".to_string())?;
    tx.execute(
        "INSERT INTO local_license_state(singleton,license_id,signed_payload_ciphertext,last_seen_utc,last_invoice_counter,state_updated_at)
         VALUES(1,?1,NULL,?2,0,?2)
         ON CONFLICT(singleton) DO UPDATE SET license_id=excluded.license_id,last_seen_utc=excluded.last_seen_utc,state_updated_at=excluded.state_updated_at",
        rusqlite::params![status.license_id, now_text.clone()],
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

pub fn import_signed_license(
    state: &crate::AppState,
    license_json: &str,
) -> Result<LicenseStatus, String> {
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
    Ok(current_status(state))
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
        local_trial: false,
        grace_period: false,
        grace_expires_at_utc: None,
        production_build: naseej_license_format::is_production_key_configured(),
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
