use chrono::{DateTime, Utc};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
#[cfg(windows)]
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;
use uuid::Uuid;

pub const PRODUCT_CODE: &str = "naseej-pos";
pub const ACTIVATION_REQUEST_SCHEMA_VERSION: u32 = 1;
pub const LEGACY_LICENSE_SCHEMA_VERSION: u32 = 1;
pub const LICENSE_SCHEMA_VERSION: u32 = 2;
pub const PLAN_TRIAL_7D: &str = "trial_7d";
pub const PLAN_ANNUAL_12M: &str = "annual_12m";

/// Locally generated evaluation entitlement.
///
/// Unlike [`PLAN_TRIAL_7D`] this plan is **not** signed by the issuer. It is
/// created by the application on the device itself so a customer can evaluate
/// the product without waiting for an offline activation round-trip. It is
/// deliberately weaker than a signed license and is documented as such.
pub const PLAN_TRIAL_LOCAL: &str = "trial_local";

/// Length of the automatically started local evaluation, in days.
pub const LOCAL_TRIAL_DAYS: i64 = 7;

/// Number of days a merchant may keep working after a licence expires.
///
/// Hard-locking a shop in the middle of a trading day is worse for the
/// customer than a short unpaid extension, so expiry moves the application
/// into a clearly marked grace period before it locks.
pub const DEFAULT_GRACE_DAYS: i64 = 14;

/// Features granted by the automatically started local evaluation.
///
/// `reports.profit` and `commissions.read` are intentionally excluded: margin
/// reporting stays a paid capability.
pub fn local_trial_features() -> BTreeSet<String> {
    BTreeSet::from([
        "pos.sell".to_string(),
        "inventory.manage".to_string(),
        "returns.exchange".to_string(),
        "reports.basic".to_string(),
        "backup.restore".to_string(),
    ])
}

/// Limits granted by the automatically started local evaluation.
pub fn local_trial_limits() -> BTreeMap<String, u64> {
    BTreeMap::from([
        ("max_users".to_string(), 3),
        ("max_branches".to_string(), 1),
    ])
}

/// Grace period in days, overridable at build time with
/// `NASEEJ_LICENSE_GRACE_DAYS`.
pub fn grace_period_days() -> i64 {
    option_env!("NASEEJ_LICENSE_GRACE_DAYS")
        .and_then(|value| value.trim().parse::<i64>().ok())
        .filter(|value| (0..=365).contains(value))
        .unwrap_or(DEFAULT_GRACE_DAYS)
}

#[derive(Debug, Error)]
pub enum LicenseError {
    #[error("license serialization is invalid")]
    Serialization,
    #[error("license signature is invalid")]
    InvalidSignature,
    #[error("license public key is not configured in this build")]
    PublicKeyNotConfigured,
    #[error("license schema is not supported")]
    UnsupportedSchema,
    #[error("license product does not match")]
    WrongProduct,
    #[error("license was issued for another device")]
    WrongDevice,
    #[error("license is not yet valid")]
    NotYetValid,
    #[error("license has expired")]
    Expired,
    #[error("license plan is invalid")]
    InvalidPlan,
    #[error("license dates are invalid")]
    InvalidDates,
    #[error("device fingerprint is unavailable")]
    FingerprintUnavailable,
    #[error("activation request is invalid")]
    InvalidRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActivationRequest {
    pub schema_version: u32,
    pub request_id: String,
    pub product: String,
    pub app_version: String,
    pub requested_at_utc: String,
    pub device_fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LicensePayload {
    pub schema_version: u32,
    pub product: String,
    pub license_id: String,
    pub customer_id: String,
    pub issued_at_utc: String,
    pub not_before_utc: String,
    pub expires_at_utc: String,
    pub device_fingerprint: String,
    pub features: BTreeSet<String>,
    pub limits: BTreeMap<String, u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedLicense {
    pub payload: LicensePayload,
    pub signature_ed25519_hex: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    pub fingerprint: String,
    pub algorithm: &'static str,
}

pub fn create_request(app_version: &str) -> Result<ActivationRequest, LicenseError> {
    let fingerprint = device_fingerprint()?;
    Ok(ActivationRequest {
        schema_version: ACTIVATION_REQUEST_SCHEMA_VERSION,
        request_id: Uuid::new_v4().to_string(),
        product: PRODUCT_CODE.to_string(),
        app_version: app_version.to_string(),
        requested_at_utc: Utc::now().to_rfc3339(),
        device_fingerprint: fingerprint,
    })
}

pub fn canonical_payload(payload: &LicensePayload) -> Result<Vec<u8>, LicenseError> {
    // Struct field order is fixed; BTreeSet/BTreeMap keep nested entries deterministic.
    serde_json::to_vec(payload).map_err(|_| LicenseError::Serialization)
}

pub fn verify_license(
    envelope: &SignedLicense,
    public_key: &VerifyingKey,
) -> Result<(), LicenseError> {
    match envelope.payload.schema_version {
        LEGACY_LICENSE_SCHEMA_VERSION if envelope.payload.plan_code.is_none() => {}
        LEGACY_LICENSE_SCHEMA_VERSION => return Err(LicenseError::InvalidPlan),
        LICENSE_SCHEMA_VERSION => {
            if !matches!(
                envelope.payload.plan_code.as_deref(),
                Some(PLAN_TRIAL_7D | PLAN_ANNUAL_12M)
            ) {
                return Err(LicenseError::InvalidPlan);
            }
        }
        _ => return Err(LicenseError::UnsupportedSchema),
    }
    if envelope.payload.product != PRODUCT_CODE {
        return Err(LicenseError::WrongProduct);
    }
    let bytes = canonical_payload(&envelope.payload)?;
    let signature_bytes =
        hex::decode(&envelope.signature_ed25519_hex).map_err(|_| LicenseError::InvalidSignature)?;
    let signature =
        Signature::from_slice(&signature_bytes).map_err(|_| LicenseError::InvalidSignature)?;
    public_key
        .verify(&bytes, &signature)
        .map_err(|_| LicenseError::InvalidSignature)
}

pub fn validate_license_for_device(
    envelope: &SignedLicense,
    public_key: &VerifyingKey,
    expected_fingerprint: &str,
    now: DateTime<Utc>,
) -> Result<(), LicenseError> {
    verify_license(envelope, public_key)?;
    if envelope.payload.device_fingerprint != expected_fingerprint {
        return Err(LicenseError::WrongDevice);
    }
    let not_before = DateTime::parse_from_rfc3339(&envelope.payload.not_before_utc)
        .map_err(|_| LicenseError::Serialization)?
        .with_timezone(&Utc);
    let expires = DateTime::parse_from_rfc3339(&envelope.payload.expires_at_utc)
        .map_err(|_| LicenseError::Serialization)?
        .with_timezone(&Utc);
    if expires <= not_before {
        return Err(LicenseError::InvalidDates);
    }
    if now < not_before {
        return Err(LicenseError::NotYetValid);
    }
    if now >= expires {
        return Err(LicenseError::Expired);
    }
    Ok(())
}

/// Public key baked into CI and development builds.
///
/// A build that carries this key cannot validate any licence issued by the
/// production issuer, so it must never be handed to a paying customer.
pub const PLACEHOLDER_PUBLIC_KEY_HEX: &str =
    "0000000000000000000000000000000000000000000000000000000000000000";

/// True when this build carries a real production licence key.
///
/// Returns `false` for development builds, for builds without the
/// `NASEEJ_LICENSE_PUBLIC_KEY_HEX` environment variable, and for builds that
/// still carry the CI placeholder key.
pub fn is_production_key_configured() -> bool {
    match option_env!("NASEEJ_LICENSE_PUBLIC_KEY_HEX") {
        Some(value) => {
            let trimmed = value.trim();
            !trimmed.is_empty() && !trimmed.eq_ignore_ascii_case(PLACEHOLDER_PUBLIC_KEY_HEX)
        }
        None => false,
    }
}

pub fn embedded_public_key() -> Result<VerifyingKey, LicenseError> {
    let encoded =
        option_env!("NASEEJ_LICENSE_PUBLIC_KEY_HEX").ok_or(LicenseError::PublicKeyNotConfigured)?;
    let bytes = hex::decode(encoded).map_err(|_| LicenseError::PublicKeyNotConfigured)?;
    let public: [u8; 32] = bytes
        .try_into()
        .map_err(|_| LicenseError::PublicKeyNotConfigured)?;
    VerifyingKey::from_bytes(&public).map_err(|_| LicenseError::PublicKeyNotConfigured)
}

pub fn device_info() -> Result<DeviceInfo, LicenseError> {
    Ok(DeviceInfo {
        fingerprint: device_fingerprint()?,
        algorithm: "SHA-256(SMBIOS System UUID, v1)",
    })
}

#[cfg(windows)]
pub fn device_fingerprint() -> Result<String, LicenseError> {
    use std::{ffi::c_void, ptr::null_mut};
    #[link(name = "Kernel32")]
    unsafe extern "system" {
        fn GetSystemFirmwareTable(
            provider: u32,
            table_id: u32,
            buffer: *mut c_void,
            buffer_size: u32,
        ) -> u32;
    }
    let provider = u32::from_le_bytes(*b"RSMB");
    let needed = unsafe { GetSystemFirmwareTable(provider, 0, null_mut(), 0) } as usize;
    if needed < 9 || needed > 1024 * 1024 {
        return Err(LicenseError::FingerprintUnavailable);
    }
    let mut raw = vec![0u8; needed];
    let written = unsafe {
        GetSystemFirmwareTable(
            provider,
            0,
            raw.as_mut_ptr() as *mut c_void,
            raw.len() as u32,
        )
    } as usize;
    if written < 9 || written > raw.len() {
        return Err(LicenseError::FingerprintUnavailable);
    }
    raw.truncate(written);
    let table_len = u32::from_le_bytes(
        raw[4..8]
            .try_into()
            .map_err(|_| LicenseError::FingerprintUnavailable)?,
    ) as usize;
    let limit = raw.len().min(8usize.saturating_add(table_len));
    let mut offset = 8usize;
    let mut system_uuid: Option<[u8; 16]> = None;
    while offset + 4 <= limit {
        let structure_type = raw[offset];
        let formatted_len = raw[offset + 1] as usize;
        if formatted_len < 4 || offset + formatted_len > limit {
            break;
        }
        if structure_type == 1 && formatted_len >= 0x18 {
            let bytes: [u8; 16] = raw[offset + 8..offset + 24]
                .try_into()
                .map_err(|_| LicenseError::FingerprintUnavailable)?;
            if bytes.iter().any(|byte| *byte != 0) && bytes.iter().any(|byte| *byte != 0xff) {
                system_uuid = Some(bytes);
                break;
            }
        }
        let mut next = offset + formatted_len;
        while next + 1 < limit && !(raw[next] == 0 && raw[next + 1] == 0) {
            next += 1;
        }
        offset = next.saturating_add(2);
    }
    let uuid = system_uuid.ok_or(LicenseError::FingerprintUnavailable)?;
    let mut digest = Sha256::new();
    digest.update(b"naseej-pos-device-fingerprint-v1\0");
    digest.update(uuid);
    Ok(hex::encode(digest.finalize()))
}

#[cfg(not(windows))]
pub fn device_fingerprint() -> Result<String, LicenseError> {
    Err(LicenseError::FingerprintUnavailable)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    #[test]
    fn verifies_signed_payload_and_rejects_tampering() {
        let signing = SigningKey::from_bytes(&[7u8; 32]);
        let payload = LicensePayload {
            schema_version: LICENSE_SCHEMA_VERSION,
            product: PRODUCT_CODE.into(),
            license_id: "lic-test".into(),
            customer_id: "customer-test".into(),
            issued_at_utc: "2026-01-01T00:00:00Z".into(),
            not_before_utc: "2026-01-01T00:00:00Z".into(),
            expires_at_utc: "2027-01-01T00:00:00Z".into(),
            device_fingerprint: "device-hash".into(),
            features: BTreeSet::from(["pos.sell".into()]),
            limits: BTreeMap::new(),
            plan_code: Some(PLAN_ANNUAL_12M.into()),
        };
        let signature = signing.sign(&canonical_payload(&payload).unwrap());
        let envelope = SignedLicense {
            payload: payload.clone(),
            signature_ed25519_hex: hex::encode(signature.to_bytes()),
        };
        assert!(verify_license(&envelope, &signing.verifying_key()).is_ok());
        let mut changed = envelope;
        changed.payload.customer_id = "tampered".into();
        assert!(matches!(
            verify_license(&changed, &signing.verifying_key()),
            Err(LicenseError::InvalidSignature)
        ));
    }

    #[test]
    fn accepts_legacy_v1_envelopes_without_plan_code() {
        let signing = SigningKey::from_bytes(&[9u8; 32]);
        let payload = LicensePayload {
            schema_version: LEGACY_LICENSE_SCHEMA_VERSION,
            product: PRODUCT_CODE.into(),
            license_id: "legacy-license".into(),
            customer_id: "customer-test".into(),
            issued_at_utc: "2026-01-01T00:00:00Z".into(),
            not_before_utc: "2026-01-01T00:00:00Z".into(),
            expires_at_utc: "2027-01-01T00:00:00Z".into(),
            device_fingerprint: "device-hash".into(),
            features: BTreeSet::from(["pos.sell".into()]),
            limits: BTreeMap::new(),
            plan_code: None,
        };
        let signature = signing.sign(&canonical_payload(&payload).unwrap());
        let envelope = SignedLicense {
            payload,
            signature_ed25519_hex: hex::encode(signature.to_bytes()),
        };
        assert!(verify_license(&envelope, &signing.verifying_key()).is_ok());
    }

    #[test]
    fn rejects_v2_without_a_known_subscription_plan() {
        let signing = SigningKey::from_bytes(&[11u8; 32]);
        let payload = LicensePayload {
            schema_version: LICENSE_SCHEMA_VERSION,
            product: PRODUCT_CODE.into(),
            license_id: "bad-plan".into(),
            customer_id: "customer-test".into(),
            issued_at_utc: "2026-01-01T00:00:00Z".into(),
            not_before_utc: "2026-01-01T00:00:00Z".into(),
            expires_at_utc: "2027-01-01T00:00:00Z".into(),
            device_fingerprint: "device-hash".into(),
            features: BTreeSet::from(["pos.sell".into()]),
            limits: BTreeMap::new(),
            plan_code: Some("unknown".into()),
        };
        let signature = signing.sign(&canonical_payload(&payload).unwrap());
        let envelope = SignedLicense {
            payload,
            signature_ed25519_hex: hex::encode(signature.to_bytes()),
        };
        assert!(matches!(
            verify_license(&envelope, &signing.verifying_key()),
            Err(LicenseError::InvalidPlan)
        ));
    }

    #[test]
    fn local_evaluation_limits_and_features_are_bounded() {
        let features = local_trial_features();
        assert!(features.contains("pos.sell"));
        assert!(!features.is_empty());

        let limits = local_trial_limits();
        assert_eq!(limits.get("max_users"), Some(&3));
        assert_eq!(limits.get("max_branches"), Some(&1));
    }

    #[test]
    fn grace_period_is_a_positive_bounded_number_of_days() {
        // black_box keeps the assertion from being folded away at compile
        // time, so it really checks the value this build will use.
        let days = std::hint::black_box(grace_period_days());
        assert!(days > 0, "grace period must be positive");
        assert!(days <= 365, "grace period must stay bounded");
    }

    #[test]
    fn placeholder_key_is_never_treated_as_a_production_key() {
        // The CI/dev placeholder must never be reported as production ready.
        let configured = option_env!("NASEEJ_LICENSE_PUBLIC_KEY_HEX").map(|v| v.trim());
        if configured.is_none()
            || configured == Some(PLACEHOLDER_PUBLIC_KEY_HEX)
            || configured == Some("")
        {
            assert!(!is_production_key_configured());
        }
    }

    #[test]
    fn local_trial_window_is_shorter_than_a_year() {
        let days = std::hint::black_box(LOCAL_TRIAL_DAYS);
        assert!(days > 0, "the local evaluation must last at least one day");
        assert!(days <= 31, "the local evaluation must stay within a month");
    }
}
