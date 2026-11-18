//! The subscription registry and the issuing flow of Naseej licences.
//!
//! The registry is the internal book of the developer: every licence that was
//! issued from this workstation is recorded here, together with the customer,
//! the device it is bound to and the plan it carries. The registry never
//! holds the signing secret, only the public facts about the licences.
//!
//! The crate is shared by the command line issuer and by the developer
//! dashboard, so both write the same records in the same format and a
//! licence issued from one is visible in the other.

use chrono::{DateTime, Duration, Months, Utc};
use ed25519_dalek::{Signer, SigningKey};
use naseej_license_format::{
    ACTIVATION_REQUEST_SCHEMA_VERSION, ActivationRequest, LICENSE_SCHEMA_VERSION, LicensePayload,
    PLAN_ANNUAL_12M, PLAN_TRIAL_7D, PRODUCT_CODE, SignedLicense,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    path::{Path, PathBuf},
};
use uuid::Uuid;

/// The version of the registry record format.
pub const REGISTRY_SCHEMA_VERSION: u32 = 1;

/// The subscription plans that can be issued.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plan {
    /// A signed seven day trial.
    Trial7d,
    /// A signed subscription of twelve calendar months.
    Annual12m,
}

impl Plan {
    /// The plan code that goes into the licence.
    pub fn code(self) -> &'static str {
        match self {
            Self::Trial7d => PLAN_TRIAL_7D,
            Self::Annual12m => PLAN_ANNUAL_12M,
        }
    }

    /// A human readable name for the plan.
    pub fn label(self) -> &'static str {
        match self {
            Self::Trial7d => "7-day trial",
            Self::Annual12m => "annual (12 calendar months)",
        }
    }

    /// The moment the subscription ends.
    pub fn expires_at(self, starts_at: DateTime<Utc>) -> Result<DateTime<Utc>, String> {
        match self {
            Self::Trial7d => starts_at
                .checked_add_signed(Duration::days(7))
                .ok_or_else(|| "trial expiry overflow".to_string()),
            Self::Annual12m => starts_at
                .checked_add_months(Months::new(12))
                .ok_or_else(|| "annual expiry overflow".to_string()),
        }
    }
}

/// One issued subscription.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubscriptionRecord {
    pub schema_version: u32,
    pub license_id: String,
    pub request_id: String,
    pub product: String,
    pub customer_id: String,
    pub device_fingerprint: String,
    pub plan_code: String,
    pub issued_at_utc: String,
    pub not_before_utc: String,
    pub expires_at_utc: String,
    pub license_file: String,
    pub features: Vec<String>,
    pub limits: BTreeMap<String, u64>,
}

/// The state of a subscription at a given moment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SubscriptionStatus {
    /// The subscription has not started yet.
    Scheduled,
    /// The subscription is running.
    Active,
    /// The subscription has ended.
    Expired,
    /// The record carries a date that cannot be read.
    DateInvalid,
}

impl SubscriptionStatus {
    /// The status of a record at the given moment.
    pub fn of(record: &SubscriptionRecord, now: DateTime<Utc>) -> Self {
        let Ok(not_before) = DateTime::parse_from_rfc3339(&record.not_before_utc) else {
            return Self::DateInvalid;
        };
        if now < not_before.with_timezone(&Utc) {
            return Self::Scheduled;
        }
        match DateTime::parse_from_rfc3339(&record.expires_at_utc) {
            Ok(expiry) if now >= expiry.with_timezone(&Utc) => Self::Expired,
            Ok(_) => Self::Active,
            Err(_) => Self::DateInvalid,
        }
    }

    /// The status code as it is written in the command line output.
    pub fn code(self) -> &'static str {
        match self {
            Self::Scheduled => "SCHEDULED",
            Self::Active => "ACTIVE",
            Self::Expired => "EXPIRED",
            Self::DateInvalid => "DATE_INVALID",
        }
    }
}

/// The features a licence may carry.
pub fn allowed_features() -> BTreeSet<String> {
    BTreeSet::from([
        "pos.sell".to_string(),
        "inventory.manage".to_string(),
        "returns.exchange".to_string(),
        "reports.basic".to_string(),
        "reports.profit".to_string(),
        "backup.restore".to_string(),
    ])
}

/// Everything needed to issue one licence.
#[derive(Debug, Clone)]
pub struct IssueOptions {
    pub request_path: PathBuf,
    pub customer_id: String,
    pub plan: Plan,
    pub starts_at_utc: Option<String>,
    pub requested_features: Vec<String>,
    pub max_users: u64,
    pub max_branches: u64,
    pub registry_dir: PathBuf,
    /// Where the licence file is written. Defaults to the folder of the request.
    pub output_dir: Option<PathBuf>,
}

/// The result of issuing a licence.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueOutcome {
    pub license_id: String,
    pub customer_id: String,
    pub plan_code: String,
    pub plan_label: String,
    pub issued_at_utc: String,
    pub not_before_utc: String,
    pub expires_at_utc: String,
    pub license_path: PathBuf,
    pub registry_dir: PathBuf,
    pub features: Vec<String>,
    pub limits: BTreeMap<String, u64>,
    pub warnings: Vec<String>,
}

/// Read every record of a registry directory.
pub fn read_records(registry_dir: &Path) -> Result<Vec<SubscriptionRecord>, String> {
    if !registry_dir.exists() {
        return Ok(Vec::new());
    }
    let mut records = Vec::new();
    for entry in fs::read_dir(registry_dir).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }
        let bytes = fs::read(&path).map_err(|error| error.to_string())?;
        let record: SubscriptionRecord =
            serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
        if record.schema_version != REGISTRY_SCHEMA_VERSION {
            return Err(format!("unsupported registry record: {}", path.display()));
        }
        records.push(record);
    }
    records.sort_by(|left, right| left.issued_at_utc.cmp(&right.issued_at_utc));
    Ok(records)
}

/// Write one record into a registry directory.
pub fn write_record(registry_dir: &Path, record: &SubscriptionRecord) -> Result<(), String> {
    fs::create_dir_all(registry_dir).map_err(|error| error.to_string())?;
    let final_path = registry_dir.join(format!("{}.json", record.license_id));
    let temp_path = registry_dir.join(format!(".{}.json.part", record.license_id));
    let bytes = serde_json::to_vec_pretty(record).map_err(|error| error.to_string())?;
    {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
            .map_err(|error| error.to_string())?;
        file.write_all(&bytes).map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
    }
    if let Err(error) = fs::rename(&temp_path, &final_path) {
        let _ = fs::remove_file(&temp_path);
        return Err(error.to_string());
    }
    Ok(())
}

/// Check that an activation request is well formed.
pub fn validate_request(request: &ActivationRequest) -> Result<(), String> {
    if request.schema_version != ACTIVATION_REQUEST_SCHEMA_VERSION
        || request.product != PRODUCT_CODE
        || request.device_fingerprint.len() != 64
        || hex::decode(&request.device_fingerprint).is_err()
        || Uuid::parse_str(&request.request_id).is_err()
    {
        return Err("request schema/product/request ID/fingerprint is invalid".to_string());
    }
    DateTime::parse_from_rfc3339(&request.requested_at_utc)
        .map_err(|_| "request timestamp is invalid".to_string())?;
    Ok(())
}

/// Read an activation request from a file.
pub fn read_request(path: &Path) -> Result<ActivationRequest, String> {
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    if bytes.len() > 64 * 1024 {
        return Err("activation request is too large".to_string());
    }
    let request: ActivationRequest =
        serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    validate_request(&request)?;
    Ok(request)
}

/// Issue a signed licence for an activation request.
///
/// The signing key is passed in, so the caller decides where the secret
/// comes from and how long it lives in memory. The function never writes the
/// secret anywhere.
pub fn issue_license(
    signing_key: &SigningKey,
    options: IssueOptions,
) -> Result<IssueOutcome, String> {
    let customer_id = options.customer_id.trim().to_string();
    if customer_id.is_empty()
        || customer_id.len() > 128
        || customer_id.chars().any(char::is_control)
    {
        return Err("customer ID must be 1-128 printable characters".to_string());
    }
    if options.max_users == 0 || options.max_branches == 0 {
        return Err("license limits must be explicit positive integers".to_string());
    }

    let request = read_request(&options.request_path)?;
    let mut records = read_records(&options.registry_dir)?;
    if records
        .iter()
        .any(|record| record.request_id == request.request_id)
    {
        return Err(
            "this request ID has already been processed; create a fresh .req for renewal"
                .to_string(),
        );
    }

    let mut warnings = Vec::new();
    let prior_trials = records
        .iter()
        .filter(|record| {
            record.plan_code == PLAN_TRIAL_7D
                && (record.device_fingerprint == request.device_fingerprint
                    || record.customer_id == customer_id)
        })
        .count();
    if prior_trials > 0 && options.plan == Plan::Trial7d {
        warnings.push(format!(
            "ISSUER_WARNING: {prior_trials} prior trial record(s) exist for this customer or device; verify trial eligibility before issuing again."
        ));
    }

    let allowed = allowed_features();
    let features: BTreeSet<String> = options.requested_features.into_iter().collect();
    if features.is_empty() || features.iter().any(|feature| !allowed.contains(feature)) {
        return Err("features must be selected from the documented feature list".to_string());
    }

    let issued = Utc::now();
    let starts_at = match options.starts_at_utc {
        Some(value) => DateTime::parse_from_rfc3339(&value)
            .map_err(|_| "subscription start is not a valid RFC3339 instant".to_string())?
            .with_timezone(&Utc),
        None => issued,
    };
    if starts_at < issued {
        return Err("subscription start cannot be earlier than issue time".to_string());
    }
    let expires = options.plan.expires_at(starts_at)?;

    let license_id = Uuid::new_v4().to_string();
    let payload = LicensePayload {
        schema_version: LICENSE_SCHEMA_VERSION,
        product: PRODUCT_CODE.to_string(),
        license_id: license_id.clone(),
        customer_id: customer_id.clone(),
        issued_at_utc: issued.to_rfc3339(),
        not_before_utc: starts_at.to_rfc3339(),
        expires_at_utc: expires.to_rfc3339(),
        device_fingerprint: request.device_fingerprint.clone(),
        features: features.clone(),
        limits: BTreeMap::from([
            ("max_users".to_string(), options.max_users),
            ("max_branches".to_string(), options.max_branches),
        ]),
        plan_code: Some(options.plan.code().to_string()),
    };
    let signature = signing_key.sign(&naseej_license_format::canonical_payload(&payload)?);
    let envelope = SignedLicense {
        payload: payload.clone(),
        signature_ed25519_hex: hex::encode(signature.to_bytes()),
    };

    let output_dir = options
        .output_dir
        .clone()
        .or_else(|| options.request_path.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."));
    let output = output_dir.join(format!(
        "naseej-{plan_code}-{license_id}.lic",
        plan_code = options.plan.code()
    ));
    let temp = output.with_file_name(format!(".{license_id}.lic.part"));
    let bytes = serde_json::to_vec_pretty(&envelope).map_err(|error| error.to_string())?;
    {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|error| error.to_string())?;
        file.write_all(&bytes).map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
    }
    if let Err(error) = fs::rename(&temp, &output) {
        let _ = fs::remove_file(&temp);
        return Err(error.to_string());
    }

    let record = SubscriptionRecord {
        schema_version: REGISTRY_SCHEMA_VERSION,
        license_id: license_id.clone(),
        request_id: request.request_id,
        product: PRODUCT_CODE.to_string(),
        customer_id: customer_id.clone(),
        device_fingerprint: request.device_fingerprint,
        plan_code: options.plan.code().to_string(),
        issued_at_utc: payload.issued_at_utc.clone(),
        not_before_utc: payload.not_before_utc.clone(),
        expires_at_utc: payload.expires_at_utc.clone(),
        license_file: output.display().to_string(),
        features: features.iter().cloned().collect(),
        limits: payload.limits.clone(),
    };
    if let Err(error) = write_record(&options.registry_dir, &record) {
        let _ = fs::remove_file(&output);
        return Err(error);
    }
    records.push(record);

    Ok(IssueOutcome {
        license_id,
        customer_id,
        plan_code: options.plan.code().to_string(),
        plan_label: options.plan.label().to_string(),
        issued_at_utc: payload.issued_at_utc,
        not_before_utc: payload.not_before_utc,
        expires_at_utc: payload.expires_at_utc,
        license_path: output,
        registry_dir: options.registry_dir,
        features: features.into_iter().collect(),
        limits: payload.limits,
        warnings,
    })
}

/// The default registry directory of this workstation.
pub fn default_registry_dir() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("NASEEJ_ISSUER_REGISTRY") {
        return Ok(PathBuf::from(path));
    }
    #[cfg(windows)]
    {
        let root = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .ok_or("LOCALAPPDATA is unavailable")?;
        Ok(root.join("NaseejLicenseIssuer").join("subscriptions"))
    }
    #[cfg(not(windows))]
    {
        let root = std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state"))
            })
            .ok_or("XDG_STATE_HOME and HOME are unavailable")?;
        Ok(root.join("naseej-license-issuer").join("subscriptions"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn trial_term_is_exactly_seven_days() {
        let start = DateTime::parse_from_rfc3339("2026-10-14T09:30:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let end = Plan::Trial7d.expires_at(start).unwrap();
        assert_eq!(end - start, Duration::days(7));
    }

    #[test]
    fn annual_term_is_twelve_calendar_months() {
        let start = DateTime::parse_from_rfc3339("2026-10-14T09:30:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let end = Plan::Annual12m.expires_at(start).unwrap();
        assert_eq!(end.to_rfc3339(), "2027-10-14T09:30:00+00:00");
    }

    #[test]
    fn subscription_registry_round_trips_records() {
        let directory = env::temp_dir().join(format!("naseej-registry-test-{}", Uuid::new_v4()));
        let record = SubscriptionRecord {
            schema_version: REGISTRY_SCHEMA_VERSION,
            license_id: "license-test".into(),
            request_id: "request-test".into(),
            product: PRODUCT_CODE.into(),
            customer_id: "customer-test".into(),
            device_fingerprint: "a".repeat(64),
            plan_code: PLAN_TRIAL_7D.into(),
            issued_at_utc: "2026-10-14T00:00:00Z".into(),
            not_before_utc: "2026-10-14T00:00:00Z".into(),
            expires_at_utc: "2026-10-21T00:00:00Z".into(),
            license_file: "trial.lic".into(),
            features: vec!["pos.sell".into()],
            limits: BTreeMap::from([("max_users".into(), 1)]),
        };
        write_record(&directory, &record).unwrap();
        let records = read_records(&directory).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].customer_id, "customer-test");
        fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn a_subscription_reports_its_status() {
        let record = SubscriptionRecord {
            schema_version: REGISTRY_SCHEMA_VERSION,
            license_id: "license-test".into(),
            request_id: "request-test".into(),
            product: PRODUCT_CODE.into(),
            customer_id: "customer-test".into(),
            device_fingerprint: "a".repeat(64),
            plan_code: PLAN_TRIAL_7D.into(),
            issued_at_utc: "2026-10-14T00:00:00Z".into(),
            not_before_utc: "2026-10-14T00:00:00Z".into(),
            expires_at_utc: "2026-10-21T00:00:00Z".into(),
            license_file: "trial.lic".into(),
            features: vec!["pos.sell".into()],
            limits: BTreeMap::from([("max_users".into(), 1)]),
        };
        let now = DateTime::parse_from_rfc3339("2026-10-18T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(
            SubscriptionStatus::of(&record, now),
            SubscriptionStatus::Active
        );
        let later = DateTime::parse_from_rfc3339("2026-11-18T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(
            SubscriptionStatus::of(&record, later),
            SubscriptionStatus::Expired
        );
    }
}
