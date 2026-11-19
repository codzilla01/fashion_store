//! The owner control panel.
//!
//! The panel is opened with a long press on the application logo. Opening it
//! proves nothing: the owner has to prove who they are, either with the
//! authenticator of the device (Windows Hello on a computer, the platform
//! authenticator on a phone) or with the owner password. Both proofs end in
//! a short lived owner session.
//!
//! The password path is the safe fallback and it is always available. The
//! authenticator path is the convenient one, and it is the path that works
//! on a phone as well, because the ceremony is a standard WebAuthn platform
//! authenticator ceremony that the browser performs and this module
//! verifies.

use crate::{AppState, cbor, webauthn};
use base64::Engine;
use chrono::Utc;
use rusqlite::{OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use uuid::Uuid;

/// How long an owner session stays alive without being used.
pub const OWNER_SESSION_IDLE: Duration = Duration::from_secs(15 * 60);

/// How long a pending ceremony stays valid.
const CHALLENGE_LIFETIME: Duration = Duration::from_secs(3 * 60);

/// How many failed unlock attempts are tolerated before the panel locks.
const MAX_FAILED_ATTEMPTS: u32 = 5;

/// How long the panel stays locked after too many failed attempts.
const LOCKOUT: Duration = Duration::from_secs(10 * 60);

/// An unlocked owner session.
pub struct OwnerSession {
    pub user_id: String,
    pub display_name: String,
    pub auth_method: String,
    pub expires_at: Instant,
}

/// The failed attempt counter of the panel.
#[derive(Default)]
pub struct OwnerAttempts {
    pub failures: u32,
    pub locked_until: Option<Instant>,
}

/// The result of an unlock.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnerAuthResult {
    pub owner_token: String,
    pub display_name: String,
    pub auth_method: String,
    pub expires_in_seconds: i64,
}

/// What the panel knows about itself before the owner unlocked it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnerPanelStatus {
    pub owner_configured: bool,
    pub owners: Vec<OwnerIdentity>,
    pub credentials_enrolled: i64,
    pub this_device_enrolled: bool,
    pub session_active: bool,
    pub locked_until_utc: Option<String>,
    pub rp_id: String,
}

/// The owner account behind the panel.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OwnerIdentity {
    pub user_id: String,
    pub display_name: String,
    pub username: String,
}

/// A pending ceremony.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnerChallenge {
    pub challenge_id: String,
    pub challenge: String,
    pub rp_id: String,
    pub user_id: String,
    pub credential_ids: Vec<String>,
    pub expires_in_seconds: i64,
}

/// An enrolled authenticator.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnerCredential {
    pub id: String,
    pub label: String,
    pub algorithm: String,
    pub rp_id: String,
    pub enrolled_at: String,
    pub last_used_at: Option<String>,
    pub revoked_at: Option<String>,
    pub credential_id: String,
}

/// The return policy the owner controls.
#[derive(Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ReturnPolicy {
    pub period_days: i64,
    pub allow_discounted_items: bool,
    pub restock_conditions: Vec<String>,
}

/// The store settings the owner controls.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnerOverview {
    pub branch_id: String,
    pub branch_code: String,
    pub branch_name_ar: String,
    pub tax_rate_bps: i64,
    pub prices_include_tax: bool,
    pub return_policy: Option<ReturnPolicy>,
    pub branches: Vec<BranchSummary>,
    pub users_by_role: Vec<RoleCount>,
    pub license_valid: bool,
    pub license_status_code: String,
    pub license_expires_at_utc: Option<String>,
    pub plan_code: Option<String>,
    pub customers: i64,
    pub products: i64,
    pub variants: i64,
    pub invoices_total: i64,
    pub owner_name: String,
    pub rp_id: String,
}

/// One branch of the store.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BranchSummary {
    pub id: String,
    pub code: String,
    pub name_ar: String,
    pub name_en: String,
    pub tax_rate_bps: i64,
    pub prices_include_tax: bool,
}

/// How many accounts carry a role.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleCount {
    pub role_code: String,
    pub name_ar: String,
    pub accounts: i64,
}

/// Encode bytes the way the WebAuthn ceremonies carry them.
fn encode(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// Decode bytes that came from the WebView.
fn decode(value: &str) -> Result<Vec<u8>, String> {
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(value.as_bytes())
        .map_err(|_| "OWNER_PANEL_ENCODING_INVALID".to_string())
}

/// Resolve the owner accounts of this store.
fn owner_identities(state: &AppState) -> Result<Vec<OwnerIdentity>, String> {
    let db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
    let mut statement = db
        .prepare(
            "SELECT u.id, u.display_name, u.username
             FROM users u JOIN user_branch_roles ubr ON ubr.user_id = u.id
             WHERE ubr.role_code = 'owner' AND u.is_active = 1
             GROUP BY u.id ORDER BY u.username",
        )
        .map_err(crate::commands::db_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok(OwnerIdentity {
                user_id: row.get(0)?,
                display_name: row.get(1)?,
                username: row.get(2)?,
            })
        })
        .map_err(crate::commands::db_error)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(crate::commands::db_error)
}

/// Refuse to work while the panel is locked after too many failed attempts.
fn ensure_not_locked(state: &AppState) -> Result<(), String> {
    let attempts = state
        .owner_attempts
        .lock()
        .map_err(|_| "OWNER_PANEL_LOCK_FAILED".to_string())?;
    if let Some(until) = attempts.locked_until
        && Instant::now() < until
    {
        return Err("OWNER_PANEL_LOCKED".to_string());
    }
    Ok(())
}

/// Count a failed unlock attempt and lock the panel when the limit is passed.
fn register_failure(state: &AppState) -> Result<(), String> {
    let mut attempts = state
        .owner_attempts
        .lock()
        .map_err(|_| "OWNER_PANEL_LOCK_FAILED".to_string())?;
    attempts.failures = attempts.failures.saturating_add(1);
    if attempts.failures >= MAX_FAILED_ATTEMPTS {
        attempts.locked_until = Some(Instant::now() + LOCKOUT);
        attempts.failures = 0;
    }
    Ok(())
}

/// Forget the failed attempt counter after a successful unlock.
fn clear_failures(state: &AppState) -> Result<(), String> {
    let mut attempts = state
        .owner_attempts
        .lock()
        .map_err(|_| "OWNER_PANEL_LOCK_FAILED".to_string())?;
    attempts.failures = 0;
    attempts.locked_until = None;
    Ok(())
}

/// Issue an owner session.
fn issue_owner_session(
    state: &AppState,
    user_id: String,
    display_name: String,
    auth_method: &str,
) -> Result<OwnerAuthResult, String> {
    // The panel belongs to the licensed product, so it only opens while the
    // licence is valid.
    let license = crate::license::ensure_valid(state)?;
    crate::license::record_trusted_time(state, &license)?;
    let token = Uuid::new_v4().to_string() + &Uuid::new_v4().simple().to_string();
    let mut sessions = state
        .owner_sessions
        .lock()
        .map_err(|_| "OWNER_PANEL_LOCK_FAILED".to_string())?;
    sessions.insert(
        token.clone(),
        OwnerSession {
            user_id,
            display_name: display_name.clone(),
            auth_method: auth_method.to_string(),
            expires_at: Instant::now() + OWNER_SESSION_IDLE,
        },
    );
    Ok(OwnerAuthResult {
        owner_token: token,
        display_name,
        auth_method: auth_method.to_string(),
        expires_in_seconds: OWNER_SESSION_IDLE.as_secs() as i64,
    })
}

/// Resolve an owner session, refreshing its idle timer.
pub fn resolve_owner_session(
    state: &AppState,
    token: &str,
) -> Result<Option<OwnerSession>, String> {
    let mut sessions = state
        .owner_sessions
        .lock()
        .map_err(|_| "OWNER_PANEL_LOCK_FAILED".to_string())?;
    let Some(session) = sessions.get_mut(token) else {
        return Ok(None);
    };
    if Instant::now() > session.expires_at {
        sessions.remove(token);
        return Ok(None);
    }
    session.expires_at = Instant::now() + OWNER_SESSION_IDLE;
    Ok(Some(OwnerSession {
        user_id: session.user_id.clone(),
        display_name: session.display_name.clone(),
        auth_method: session.auth_method.clone(),
        expires_at: session.expires_at,
    }))
}

/// Resolve an owner session, refusing the request when there is none.
pub fn require_owner_session(state: &AppState, token: &str) -> Result<OwnerSession, String> {
    // The panel is part of the licensed product, so it is only reachable
    // while the licence is valid.
    let license = crate::license::ensure_valid(state)?;
    crate::license::record_trusted_time(state, &license)?;
    match resolve_owner_session(state, token)? {
        Some(session) => Ok(session),
        None => Err("OWNER_SESSION_REQUIRED".to_string()),
    }
}

/// Whether the panel is unlocked right now.
#[tauri::command]
pub fn owner_panel_status(state: tauri::State<'_, AppState>) -> Result<OwnerPanelStatus, String> {
    let owners = owner_identities(&state)?;
    let fingerprint = naseej_license_format::device_fingerprint().unwrap_or_default();
    let (credentials_enrolled, this_device_enrolled): (i64, i64) = {
        let db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
        let total: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM owner_panel_credentials WHERE revoked_at IS NULL",
                [],
                |row| row.get(0),
            )
            .map_err(crate::commands::db_error)?;
        let on_this_device: i64 = if fingerprint.is_empty() {
            0
        } else {
            db.query_row(
                "SELECT COUNT(*) FROM owner_panel_devices WHERE device_fingerprint=?1 AND revoked_at IS NULL",
                params![fingerprint],
                |row| row.get(0),
            )
            .map_err(crate::commands::db_error)?
        };
        (total, on_this_device)
    };
    let session_active = {
        let sessions = state
            .owner_sessions
            .lock()
            .map_err(|_| "OWNER_PANEL_LOCK_FAILED".to_string())?;
        sessions
            .values()
            .any(|session| Instant::now() < session.expires_at)
    };
    let locked_until_utc = {
        let attempts = state
            .owner_attempts
            .lock()
            .map_err(|_| "OWNER_PANEL_LOCK_FAILED".to_string())?;
        attempts.locked_until.map(|until| {
            let remaining = until.saturating_duration_since(Instant::now());
            (Utc::now() + chrono::Duration::from_std(remaining).unwrap_or_default()).to_rfc3339()
        })
    };
    Ok(OwnerPanelStatus {
        owner_configured: !owners.is_empty(),
        owners,
        credentials_enrolled,
        this_device_enrolled: this_device_enrolled > 0,
        session_active,
        locked_until_utc,
        rp_id: webauthn::default_rp_id().to_string(),
    })
}

/// Unlock the panel with the owner password.
#[tauri::command]
pub fn owner_panel_unlock_with_password(
    state: tauri::State<'_, AppState>,
    username: String,
    password: String,
) -> Result<OwnerAuthResult, String> {
    ensure_not_locked(&state)?;
    let username = username.trim().to_string();
    let record = {
        let db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
        db.query_row(
            "SELECT u.id, u.display_name, u.password_phc, u.is_active,
                    EXISTS(SELECT 1 FROM user_branch_roles ubr WHERE ubr.user_id=u.id AND ubr.role_code='owner')
             FROM users u WHERE u.username=?1 COLLATE NOCASE",
            params![username],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, bool>(4)?,
                ))
            },
        )
        .optional()
        .map_err(crate::commands::db_error)?
    };
    let Some((user_id, display_name, password_phc, active, is_owner)) = record else {
        register_failure(&state)?;
        return Err("OWNER_LOGIN_FAILED".to_string());
    };
    if active != 1 || !is_owner {
        register_failure(&state)?;
        return Err("OWNER_LOGIN_FAILED".to_string());
    }
    let parsed = password_hash::PasswordHash::new(&password_phc)
        .map_err(|_| "PASSWORD_HASH_INVALID".to_string())?;
    if crate::commands::verify_password(&password, &parsed).is_err() {
        register_failure(&state)?;
        return Err("OWNER_LOGIN_FAILED".to_string());
    }
    clear_failures(&state)?;
    issue_owner_session(&state, user_id, display_name, "PASSWORD")
}

/// Unlock the panel with the device authenticator.
///
/// The browser performs the ceremony; this command only hands out the
/// challenge and the credential handles, and refuses to start a ceremony when
/// no authenticator is enrolled.
#[tauri::command]
pub fn owner_panel_begin_assertion(
    state: tauri::State<'_, AppState>,
    owner_user_id: Option<String>,
    rp_id: Option<String>,
) -> Result<OwnerChallenge, String> {
    ensure_not_locked(&state)?;
    let rp_id = resolve_rp_id(rp_id)?;
    let owners = owner_identities(&state)?;
    let owner = match owner_user_id {
        Some(user_id) => owners
            .into_iter()
            .find(|owner| owner.user_id == user_id)
            .ok_or_else(|| "OWNER_NOT_FOUND".to_string())?,
        None => {
            if owners.len() != 1 {
                return Err("OWNER_ACCOUNT_REQUIRED".to_string());
            }
            owners.into_iter().next().unwrap()
        }
    };
    let credentials: Vec<(Vec<u8>, String)> = {
        let db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
        let mut statement = db
            .prepare(
                "SELECT credential_id, rp_id FROM owner_panel_credentials
                 WHERE owner_user_id=?1 AND revoked_at IS NULL ORDER BY enrolled_at",
            )
            .map_err(crate::commands::db_error)?;
        let rows = statement
            .query_map(params![owner.user_id], |row| {
                Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(crate::commands::db_error)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(crate::commands::db_error)?
    };
    if credentials.is_empty() {
        return Err("OWNER_NO_AUTHENTICATOR_ENROLLED".to_string());
    }
    // Every enrolled authenticator belongs to the origin it was enrolled
    // under, so the ceremony has to run under that same origin.
    if credentials
        .iter()
        .any(|(_, credential_rp_id)| credential_rp_id != &rp_id)
    {
        return Err("OWNER_RELYING_PARTY_MISMATCH".to_string());
    }
    let mut challenge_bytes = [0u8; 32];
    getrandom::fill(&mut challenge_bytes).map_err(|_| "CSPRNG_FAILED".to_string())?;
    let challenge_id = Uuid::new_v4().to_string();
    let now = Utc::now();
    {
        let db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
        db.execute(
            "INSERT INTO owner_panel_challenges(challenge_id, owner_user_id, purpose, challenge, rp_id, created_at, expires_at)
             VALUES(?1,?2,'assert',?3,?4,?5,?6)",
            params![
                challenge_id,
                owner.user_id,
                challenge_bytes.to_vec(),
                rp_id,
                now.to_rfc3339(),
                (now + chrono::Duration::from_std(CHALLENGE_LIFETIME).unwrap_or_default())
                    .to_rfc3339()
            ],
        )
        .map_err(crate::commands::db_error)?;
    }
    Ok(OwnerChallenge {
        challenge_id,
        challenge: encode(&challenge_bytes),
        rp_id,
        user_id: owner.user_id,
        credential_ids: credentials.iter().map(|(bytes, _)| encode(bytes)).collect(),
        expires_in_seconds: CHALLENGE_LIFETIME.as_secs() as i64,
    })
}

/// The result of an assertion, as the browser reported it.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssertionInput {
    pub challenge_id: String,
    pub credential_id: String,
    pub authenticator_data: String,
    pub client_data_json: String,
    pub signature: String,
}

/// Finish an assertion ceremony and unlock the panel.
#[tauri::command]
pub fn owner_panel_finish_assertion(
    state: tauri::State<'_, AppState>,
    input: AssertionInput,
) -> Result<OwnerAuthResult, String> {
    ensure_not_locked(&state)?;
    let authenticator_data = decode(&input.authenticator_data)?;
    let client_data_json = decode(&input.client_data_json)?;
    let signature = decode(&input.signature)?;
    let credential_id = decode(&input.credential_id)?;

    let challenge = {
        let db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
        db.query_row(
            "SELECT owner_user_id, challenge, rp_id FROM owner_panel_challenges
             WHERE challenge_id=?1 AND purpose='assert' AND consumed_at IS NULL",
            params![input.challenge_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()
        .map_err(crate::commands::db_error)?
    };
    let Some((owner_user_id, expected_challenge, challenge_rp_id)) = challenge else {
        register_failure(&state)?;
        return Err("OWNER_CHALLENGE_INVALID".to_string());
    };
    let still_valid: bool = {
        let db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
        db.query_row(
            "SELECT expires_at > ?1 FROM owner_panel_challenges WHERE challenge_id=?2",
            params![Utc::now().to_rfc3339(), input.challenge_id],
            |row| row.get(0),
        )
        .map_err(crate::commands::db_error)?
    };
    if !still_valid {
        return Err("OWNER_CHALLENGE_EXPIRED".to_string());
    }

    let stored = {
        let db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
        db.query_row(
            "SELECT owner_user_id, public_key_x, public_key_y, rp_id, revoked_at
             FROM owner_panel_credentials WHERE credential_id=?1",
            params![credential_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, Vec<u8>>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            },
        )
        .optional()
        .map_err(crate::commands::db_error)?
    };
    let Some((credential_owner, x_bytes, y_bytes, rp_id, revoked_at)) = stored else {
        register_failure(&state)?;
        return Err("OWNER_CREDENTIAL_UNKNOWN".to_string());
    };
    if credential_owner != owner_user_id {
        register_failure(&state)?;
        return Err("OWNER_CREDENTIAL_UNKNOWN".to_string());
    }
    if rp_id != challenge_rp_id {
        register_failure(&state)?;
        return Err("OWNER_RELYING_PARTY_MISMATCH".to_string());
    }
    if revoked_at.is_some() {
        return Err("OWNER_CREDENTIAL_REVOKED".to_string());
    }
    if x_bytes.len() != 32 || y_bytes.len() != 32 {
        return Err("OWNER_CREDENTIAL_INVALID".to_string());
    }

    let parsed = webauthn::parse_authenticator_data(&authenticator_data)?;
    if parsed.flags & webauthn::FLAG_USER_PRESENT == 0 {
        register_failure(&state)?;
        return Err("OWNER_PRESENCE_NOT_VERIFIED".to_string());
    }
    // The authenticator must have verified the person, not only seen them.
    if parsed.flags & webauthn::FLAG_USER_VERIFIED == 0 {
        register_failure(&state)?;
        return Err("OWNER_BIOMETRIC_NOT_VERIFIED".to_string());
    }
    if parsed.rp_id_hash != webauthn::rp_id_hash(&rp_id) {
        register_failure(&state)?;
        return Err("OWNER_RELYING_PARTY_MISMATCH".to_string());
    }
    let client_data: webauthn::ClientData = serde_json::from_slice(&client_data_json)
        .map_err(|_| "OWNER_CLIENT_DATA_INVALID".to_string())?;
    if webauthn::verify_client_data(&client_data, "webauthn.get", &encode(&expected_challenge))
        .is_err()
    {
        register_failure(&state)?;
        return Err("OWNER_CLIENT_DATA_INVALID".to_string());
    }
    let mut x = [0u8; 32];
    let mut y = [0u8; 32];
    x.copy_from_slice(&x_bytes);
    y.copy_from_slice(&y_bytes);
    let public_key = webauthn::Es256PublicKey { x, y };
    let message = webauthn::assertion_message(&authenticator_data, &client_data_json);
    if webauthn::verify_es256(&public_key, &message, &signature).is_err() {
        register_failure(&state)?;
        return Err("OWNER_SIGNATURE_INVALID".to_string());
    }

    let now = Utc::now().to_rfc3339();
    {
        let db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
        db.execute(
            "UPDATE owner_panel_challenges SET consumed_at=?1 WHERE challenge_id=?2",
            params![now, input.challenge_id],
        )
        .map_err(crate::commands::db_error)?;
        db.execute(
            "UPDATE owner_panel_credentials SET last_used_at=?1 WHERE credential_id=?2",
            params![now, credential_id.to_vec()],
        )
        .map_err(crate::commands::db_error)?;
    }
    clear_failures(&state)?;
    let display_name = {
        let db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
        db.query_row(
            "SELECT display_name FROM users WHERE id=?1",
            params![owner_user_id],
            |row| row.get::<_, String>(0),
        )
        .map_err(crate::commands::db_error)?
    };
    let fingerprint = naseej_license_format::device_fingerprint().unwrap_or_default();
    if !fingerprint.is_empty() {
        let db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
        let _ = db.execute(
            "UPDATE owner_panel_devices SET last_unlocked_at=?1 WHERE device_fingerprint=?2",
            params![now, fingerprint],
        );
    }
    issue_owner_session(&state, owner_user_id, display_name, "BIOMETRIC")
}

/// Start the registration of the authenticator of this device.
#[tauri::command]
pub fn owner_panel_begin_registration(
    state: tauri::State<'_, AppState>,
    owner_token: String,
    rp_id: Option<String>,
) -> Result<OwnerChallenge, String> {
    let session = require_owner_session(&state, &owner_token)?;
    let rp_id = resolve_rp_id(rp_id)?;
    let mut challenge_bytes = [0u8; 32];
    getrandom::fill(&mut challenge_bytes).map_err(|_| "CSPRNG_FAILED".to_string())?;
    let challenge_id = Uuid::new_v4().to_string();
    let now = Utc::now();
    {
        let db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
        db.execute(
            "INSERT INTO owner_panel_challenges(challenge_id, owner_user_id, purpose, challenge, rp_id, created_at, expires_at)
             VALUES(?1,?2,'register',?3,?4,?5,?6)",
            params![
                challenge_id,
                session.user_id,
                challenge_bytes.to_vec(),
                rp_id,
                now.to_rfc3339(),
                (now + chrono::Duration::from_std(CHALLENGE_LIFETIME).unwrap_or_default())
                    .to_rfc3339()
            ],
        )
        .map_err(crate::commands::db_error)?;
    }
    Ok(OwnerChallenge {
        challenge_id,
        challenge: encode(&challenge_bytes),
        rp_id,
        user_id: session.user_id,
        credential_ids: Vec::new(),
        expires_in_seconds: CHALLENGE_LIFETIME.as_secs() as i64,
    })
}

/// The result of a registration ceremony, as the browser reported it.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistrationInput {
    pub challenge_id: String,
    /// The session of the panel that started the ceremony.
    pub owner_token: String,
    pub credential_id: String,
    pub attestation_object: String,
    pub client_data_json: String,
    pub label: Option<String>,
}

/// Finish a registration ceremony and store the authenticator.
#[tauri::command]
pub fn owner_panel_finish_registration(
    state: tauri::State<'_, AppState>,
    input: RegistrationInput,
) -> Result<OwnerCredential, String> {
    // The ceremony only counts when the panel is unlocked, and the pending
    // ceremony has to belong to the owner of that session.
    let session = require_owner_session(&state, &input.owner_token)?;
    let attestation_object = decode(&input.attestation_object)?;
    let client_data_json = decode(&input.client_data_json)?;
    let credential_id = decode(&input.credential_id)?;

    let expected_challenge = {
        let db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
        db.query_row(
            "SELECT owner_user_id, challenge, rp_id FROM owner_panel_challenges
             WHERE challenge_id=?1 AND purpose='register' AND consumed_at IS NULL",
            params![input.challenge_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()
        .map_err(crate::commands::db_error)?
    };
    let Some((challenge_owner, expected_challenge, rp_id)) = expected_challenge else {
        return Err("OWNER_CHALLENGE_INVALID".to_string());
    };
    if challenge_owner != session.user_id {
        return Err("OWNER_CHALLENGE_INVALID".to_string());
    }
    let client_data: webauthn::ClientData = serde_json::from_slice(&client_data_json)
        .map_err(|_| "OWNER_CLIENT_DATA_INVALID".to_string())?;
    webauthn::verify_client_data(
        &client_data,
        "webauthn.create",
        &encode(&expected_challenge),
    )?;

    let authenticator_data = extract_authenticator_data(&attestation_object)?;
    let parsed = webauthn::parse_authenticator_data(&authenticator_data)?;
    let attested = parsed
        .attested_credential
        .ok_or_else(|| "OWNER_ATTESTATION_INCOMPLETE".to_string())?;
    if attested.credential_id != credential_id {
        return Err("OWNER_CREDENTIAL_MISMATCH".to_string());
    }
    let public_key = webauthn::parse_cose_es256_public_key(&attested.public_key_cose)?;
    if parsed.rp_id_hash != webauthn::rp_id_hash(&rp_id) {
        return Err("OWNER_RELYING_PARTY_MISMATCH".to_string());
    }

    let fingerprint = naseej_license_format::device_fingerprint().unwrap_or_default();
    let now = Utc::now().to_rfc3339();
    let device_id = Uuid::new_v4().to_string();
    let credential_row_id = Uuid::new_v4().to_string();
    let label = input
        .label
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| {
            if cfg!(windows) {
                "Windows Hello".to_string()
            } else {
                "Device authenticator".to_string()
            }
        });
    {
        let db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
        db.execute(
            "INSERT OR IGNORE INTO owner_panel_devices(id, owner_user_id, label, platform, device_fingerprint, enrolled_at)
             VALUES(?1,?2,?3,?4,?5,?6)",
            params![
                device_id,
                session.user_id,
                label,
                if cfg!(windows) { "windows" } else { "other" },
                fingerprint,
                now
            ],
        )
        .map_err(crate::commands::db_error)?;
        db.execute(
            "INSERT INTO owner_panel_credentials(id, owner_user_id, device_id, credential_id, public_key_cose, public_key_x, public_key_y, algorithm, rp_id, label, enrolled_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![
                credential_row_id,
                session.user_id,
                device_id,
                credential_id,
                attested.public_key_cose,
                public_key.x.to_vec(),
                public_key.y.to_vec(),
                "ES256",
                rp_id,
                label,
                now
            ],
        )
        .map_err(crate::commands::db_error)?;
        db.execute(
            "UPDATE owner_panel_challenges SET consumed_at=?1 WHERE challenge_id=?2",
            params![now, input.challenge_id],
        )
        .map_err(crate::commands::db_error)?;
    }
    Ok(OwnerCredential {
        id: credential_row_id,
        label,
        algorithm: "ES256".to_string(),
        rp_id,
        enrolled_at: now,
        last_used_at: None,
        revoked_at: None,
        credential_id: encode(&credential_id),
    })
}

/// Read the authenticator data out of an attestation object.
fn extract_authenticator_data(attestation_object: &[u8]) -> Result<Vec<u8>, String> {
    let document =
        cbor::decode(attestation_object).map_err(|_| "OWNER_ATTESTATION_INVALID".to_string())?;
    let entries = document
        .as_map()
        .ok_or_else(|| "OWNER_ATTESTATION_INVALID".to_string())?;
    for (label, value) in entries {
        if label.as_text() == Some("authData")
            && let Some(bytes) = value.as_bytes()
        {
            return Ok(bytes.to_vec());
        }
    }
    Err("OWNER_ATTESTATION_INVALID".to_string())
}

/// The authenticators that are enrolled for the owner.
#[tauri::command]
pub fn owner_panel_credentials(
    state: tauri::State<'_, AppState>,
    owner_token: String,
) -> Result<Vec<OwnerCredential>, String> {
    let session = require_owner_session(&state, &owner_token)?;
    let db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
    let mut statement = db
        .prepare(
            "SELECT id, label, algorithm, rp_id, enrolled_at, last_used_at, revoked_at, credential_id
             FROM owner_panel_credentials WHERE owner_user_id=?1 ORDER BY enrolled_at DESC",
        )
        .map_err(crate::commands::db_error)?;
    let rows = statement
        .query_map(params![session.user_id], |row| {
            Ok(OwnerCredential {
                id: row.get(0)?,
                label: row.get(1)?,
                algorithm: row.get(2)?,
                rp_id: row.get(3)?,
                enrolled_at: row.get(4)?,
                last_used_at: row.get(5)?,
                revoked_at: row.get(6)?,
                credential_id: encode(&row.get::<_, Vec<u8>>(7)?),
            })
        })
        .map_err(crate::commands::db_error)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(crate::commands::db_error)
}

/// Revoke an enrolled authenticator.
#[tauri::command]
pub fn owner_panel_revoke_credential(
    state: tauri::State<'_, AppState>,
    owner_token: String,
    credential_id: String,
) -> Result<(), String> {
    let session = require_owner_session(&state, &owner_token)?;
    let db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
    let now = Utc::now().to_rfc3339();
    db.execute(
        "UPDATE owner_panel_credentials SET revoked_at=?1 WHERE id=?2 AND owner_user_id=?3",
        params![now, credential_id, session.user_id],
    )
    .map_err(crate::commands::db_error)?;
    Ok(())
}

/// Lock the panel.
#[tauri::command]
pub fn owner_panel_lock(
    state: tauri::State<'_, AppState>,
    owner_token: String,
) -> Result<(), String> {
    state
        .owner_sessions
        .lock()
        .map_err(|_| "OWNER_PANEL_LOCK_FAILED".to_string())?
        .remove(&owner_token);
    Ok(())
}

/// The store settings the owner controls.
#[tauri::command]
pub fn owner_panel_overview(
    state: tauri::State<'_, AppState>,
    owner_token: String,
    branch_id: Option<String>,
) -> Result<OwnerOverview, String> {
    let session = require_owner_session(&state, &owner_token)?;
    let license = crate::license::ensure_valid(&state)?;
    let db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;

    let mut branch_query = db
        .prepare(
            "SELECT id, code, name_ar, name_en, tax_rate_bps, COALESCE(prices_include_tax, 0)
             FROM branches ORDER BY code",
        )
        .map_err(crate::commands::db_error)?;
    let branches = branch_query
        .query_map([], |row| {
            Ok(BranchSummary {
                id: row.get(0)?,
                code: row.get(1)?,
                name_ar: row.get(2)?,
                name_en: row.get(3)?,
                tax_rate_bps: row.get(4)?,
                prices_include_tax: row.get::<_, i64>(5)? == 1,
            })
        })
        .map_err(crate::commands::db_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(crate::commands::db_error)?;

    let selected = branch_id
        .and_then(|id| branches.iter().find(|branch| branch.id == id).cloned())
        .or_else(|| branches.first().cloned())
        .ok_or_else(|| "OWNER_NO_BRANCH".to_string())?;

    let policy_json: Option<String> = db
        .query_row(
            "SELECT setting_value_json FROM system_settings WHERE branch_id=?1 AND setting_key='returns_policy'",
            params![selected.id],
            |row| row.get(0),
        )
        .optional()
        .map_err(crate::commands::db_error)?;
    let return_policy =
        policy_json.and_then(|json| serde_json::from_str::<ReturnPolicy>(&json).ok());

    let users_by_role = {
        let mut statement = db
            .prepare(
                "SELECT r.code, r.name_ar, COUNT(ubr.user_id)
                 FROM roles r LEFT JOIN user_branch_roles ubr ON ubr.role_code = r.code
                 GROUP BY r.code ORDER BY r.code",
            )
            .map_err(crate::commands::db_error)?;
        statement
            .query_map([], |row| {
                Ok(RoleCount {
                    role_code: row.get(0)?,
                    name_ar: row.get(1)?,
                    accounts: row.get(2)?,
                })
            })
            .map_err(crate::commands::db_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(crate::commands::db_error)?
    };

    let (customers, products, variants, invoices): (i64, i64, i64, i64) = db
        .query_row(
            "SELECT (SELECT COUNT(*) FROM customers),
                    (SELECT COUNT(*) FROM products),
                    (SELECT COUNT(*) FROM product_variants),
                    (SELECT COUNT(*) FROM invoices)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .map_err(crate::commands::db_error)?;

    Ok(OwnerOverview {
        branch_id: selected.id,
        branch_code: selected.code,
        branch_name_ar: selected.name_ar,
        tax_rate_bps: selected.tax_rate_bps,
        prices_include_tax: selected.prices_include_tax,
        return_policy,
        branches,
        users_by_role,
        license_valid: license.valid,
        license_status_code: license.status_code.clone(),
        license_expires_at_utc: license.expires_at_utc.clone(),
        plan_code: license.plan_code.clone(),
        customers,
        products,
        variants,
        invoices_total: invoices,
        owner_name: session.display_name,
        rp_id: webauthn::default_rp_id().to_string(),
    })
}

/// Resolve the relying party of the ceremony.
///
/// The application asks for the origin it runs under; the panel only accepts
/// the origins the desktop application and the development server use, so a
/// ceremony that was started somewhere else cannot be finished here.
fn resolve_rp_id(rp_id: Option<String>) -> Result<String, String> {
    match rp_id {
        Some(value) => {
            let value = value.trim().to_ascii_lowercase();
            if !webauthn::is_allowed_rp_id(&value) {
                return Err("OWNER_RELYING_PARTY_NOT_ALLOWED".to_string());
            }
            Ok(value)
        }
        None => Ok(webauthn::default_rp_id().to_string()),
    }
}

/// Check that a branch exists before a setting is written to it.
fn require_branch(state: &AppState, branch_id: &str) -> Result<(), String> {
    let db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
    let exists: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM branches WHERE id=?1)",
            params![branch_id],
            |row| row.get(0),
        )
        .map_err(crate::commands::db_error)?;
    if exists {
        Ok(())
    } else {
        Err("OWNER_BRANCH_NOT_FOUND".to_string())
    }
}

/// Set the return policy of a branch from the control panel.
#[tauri::command]
pub fn owner_panel_set_return_policy(
    state: tauri::State<'_, AppState>,
    owner_token: String,
    branch_id: String,
    policy: ReturnPolicy,
) -> Result<(), String> {
    let session = require_owner_session(&state, &owner_token)?;
    require_branch(&state, &branch_id)?;
    let mut db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
    let tx = db
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(crate::commands::db_error)?;
    persist_return_policy(&tx, &branch_id, &session.user_id, &policy)?;
    tx.commit().map_err(crate::commands::db_error)
}

/// Set the tax settings of a branch from the control panel.
#[tauri::command]
pub fn owner_panel_set_tax(
    state: tauri::State<'_, AppState>,
    owner_token: String,
    branch_id: String,
    tax_rate_bps: i64,
    prices_include_tax: bool,
) -> Result<(), String> {
    let session = require_owner_session(&state, &owner_token)?;
    require_branch(&state, &branch_id)?;
    let mut db = state.db.lock().map_err(|_| "DB_LOCK_FAILED".to_string())?;
    let tx = db
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(crate::commands::db_error)?;
    crate::commands::persist_tax(
        &tx,
        &branch_id,
        &session.user_id,
        tax_rate_bps,
        prices_include_tax,
    )?;
    tx.commit().map_err(crate::commands::db_error)
}

/// Write the return policy of a branch.
///
/// The owner controls this from the control panel; the same rule is used by
/// the returns desk, so the policy that is stored here is the policy that is
/// enforced.
pub fn persist_return_policy(
    tx: &Transaction<'_>,
    branch_id: &str,
    actor_user_id: &str,
    policy: &ReturnPolicy,
) -> Result<(), String> {
    if !(1..=180).contains(&policy.period_days)
        || policy.restock_conditions.is_empty()
        || policy
            .restock_conditions
            .iter()
            .any(|condition| !matches!(condition.as_str(), "UNOPENED" | "NEW_TAGS"))
    {
        return Err("RETURN_POLICY_INVALID".to_string());
    }
    let value =
        serde_json::to_string(policy).map_err(|_| "RETURN_POLICY_SERIALIZE_FAILED".to_string())?;
    let now = Utc::now().to_rfc3339();
    tx.execute(
        "INSERT INTO system_settings(branch_id,setting_key,setting_value_json,version,changed_by,changed_at)
         VALUES(?1,'returns_policy',?2,1,?3,?4)
         ON CONFLICT(branch_id,setting_key) DO UPDATE SET setting_value_json=excluded.setting_value_json,version=system_settings.version+1,changed_by=excluded.changed_by,changed_at=excluded.changed_at",
        params![branch_id, value, actor_user_id, now],
    )
    .map_err(crate::commands::db_error)?;
    crate::commands::insert_audit(
        tx,
        &Uuid::new_v4().to_string(),
        Some(branch_id),
        Some(actor_user_id),
        "RETURN_POLICY_CHANGED",
        "setting",
        Some("returns_policy"),
        Some("Owner configured the return rules"),
        "{}",
        &now,
    )?;
    Ok(())
}
