use chrono::{DateTime, Duration, Months, Utc};
use clap::{Parser, Subcommand, ValueEnum};
use ed25519_dalek::{Signer, SigningKey};
use naseej_license_format::{
    ACTIVATION_REQUEST_SCHEMA_VERSION, ActivationRequest, LICENSE_SCHEMA_VERSION, LicensePayload,
    PLAN_ANNUAL_12M, PLAN_TRIAL_7D, PRODUCT_CODE, SignedLicense,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    io::Write,
    path::{Path, PathBuf},
};
use uuid::Uuid;
use zeroize::Zeroize;

const REGISTRY_SCHEMA_VERSION: u32 = 1;

#[derive(Parser)]
#[command(
    name = "naseej-license-issuer",
    about = "Internal offline subscription manager and license signer"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Issue a device-bound trial or annual subscription from a .req file.
    Issue {
        #[arg(long)]
        request: PathBuf,
        #[arg(long)]
        customer_id: String,
        #[arg(long, value_enum)]
        plan: Plan,
        /// Optional RFC3339 start instant; defaults to issue time. Future only.
        #[arg(long)]
        starts_at_utc: Option<String>,
        #[arg(long)]
        private_key_file: PathBuf,
        #[arg(long, value_delimiter = ',', required = true)]
        features: Vec<String>,
        #[arg(long)]
        max_users: u64,
        #[arg(long)]
        max_branches: u64,
        /// Internal registry directory. Defaults to the current user's app-data directory.
        #[arg(long)]
        registry: Option<PathBuf>,
    },
    /// List subscriptions recorded by this issuer workstation.
    List {
        /// Internal registry directory. Defaults to the current user's app-data directory.
        #[arg(long)]
        registry: Option<PathBuf>,
    },
    /// Print the public verification key corresponding to a private seed file.
    PublicKey {
        #[arg(long)]
        private_key_file: PathBuf,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Plan {
    #[value(name = "trial-7d")]
    Trial7d,
    #[value(name = "annual-12m")]
    Annual12m,
}

impl Plan {
    fn code(self) -> &'static str {
        match self {
            Self::Trial7d => PLAN_TRIAL_7D,
            Self::Annual12m => PLAN_ANNUAL_12M,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Trial7d => "7-day trial",
            Self::Annual12m => "annual (12 calendar months)",
        }
    }

    fn expires_at(self, starts_at: DateTime<Utc>) -> Result<DateTime<Utc>, String> {
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

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SubscriptionRecord {
    schema_version: u32,
    license_id: String,
    request_id: String,
    product: String,
    customer_id: String,
    device_fingerprint: String,
    plan_code: String,
    issued_at_utc: String,
    not_before_utc: String,
    expires_at_utc: String,
    license_file: String,
    features: Vec<String>,
    limits: BTreeMap<String, u64>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("ISSUER_ERROR: {error}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    match Cli::parse().command {
        Command::Issue {
            request,
            customer_id,
            plan,
            starts_at_utc,
            private_key_file,
            features,
            max_users,
            max_branches,
            registry,
        } => issue(
            request,
            customer_id,
            plan,
            starts_at_utc,
            private_key_file,
            features,
            max_users,
            max_branches,
            registry.unwrap_or(default_registry_dir()?),
        ),
        Command::List { registry } => {
            list_subscriptions(registry.unwrap_or(default_registry_dir()?))
        }
        Command::PublicKey { private_key_file } => {
            let signing_key = read_signing_key(&private_key_file)?;
            println!(
                "NASEEJ_LICENSE_PUBLIC_KEY_HEX={}",
                hex::encode(signing_key.verifying_key().to_bytes())
            );
            Ok(())
        }
    }
}

fn issue(
    request_path: PathBuf,
    customer_id: String,
    plan: Plan,
    starts_at_utc: Option<String>,
    private_key_path: PathBuf,
    requested_features: Vec<String>,
    max_users: u64,
    max_branches: u64,
    registry_dir: PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    let customer_id = customer_id.trim();
    if customer_id.is_empty()
        || customer_id.len() > 128
        || customer_id.chars().any(char::is_control)
    {
        return Err("customer ID must be 1-128 printable characters".into());
    }
    if max_users == 0 || max_branches == 0 {
        return Err("license limits must be explicit positive integers".into());
    }

    let request_bytes = fs::read(&request_path)?;
    if request_bytes.len() > 64 * 1024 {
        return Err("activation request is too large".into());
    }
    let request: ActivationRequest = serde_json::from_slice(&request_bytes)?;
    validate_request(&request)?;
    let mut records = read_records(&registry_dir)?;
    if records
        .iter()
        .any(|record| record.request_id == request.request_id)
    {
        return Err(
            "this request ID has already been processed; create a fresh .req for renewal".into(),
        );
    }
    let prior_trials = records
        .iter()
        .filter(|record| {
            record.plan_code == PLAN_TRIAL_7D
                && (record.device_fingerprint == request.device_fingerprint
                    || record.customer_id == customer_id)
        })
        .count();
    if prior_trials > 0 && matches!(plan, Plan::Trial7d) {
        eprintln!(
            "ISSUER_WARNING: {prior_trials} prior trial record(s) exist for this customer or device; verify trial eligibility before issuing again."
        );
    }

    let allowed_features = BTreeSet::from([
        "pos.sell".to_string(),
        "inventory.manage".to_string(),
        "returns.exchange".to_string(),
        "reports.basic".to_string(),
        "reports.profit".to_string(),
        "backup.restore".to_string(),
    ]);
    let features: BTreeSet<String> = requested_features.into_iter().collect();
    if features.is_empty()
        || features
            .iter()
            .any(|feature| !allowed_features.contains(feature))
    {
        return Err("features must be selected from the documented feature list".into());
    }

    let issued = Utc::now();
    let starts_at = match starts_at_utc {
        Some(value) => DateTime::parse_from_rfc3339(&value)?.with_timezone(&Utc),
        None => issued,
    };
    if starts_at < issued {
        return Err("subscription start cannot be earlier than issue time".into());
    }
    let expires = plan
        .expires_at(starts_at)
        .map_err(|error| format!("subscription term invalid: {error}"))?;

    // This file-based key source is for controlled development/internal use only.
    // Production signing must move to HSM/Windows CNG with audited access controls.
    let signing_key = read_signing_key(&private_key_path)?;

    let license_id = Uuid::new_v4().to_string();
    let payload = LicensePayload {
        schema_version: LICENSE_SCHEMA_VERSION,
        product: PRODUCT_CODE.to_string(),
        license_id: license_id.clone(),
        customer_id: customer_id.to_string(),
        issued_at_utc: issued.to_rfc3339(),
        not_before_utc: starts_at.to_rfc3339(),
        expires_at_utc: expires.to_rfc3339(),
        device_fingerprint: request.device_fingerprint.clone(),
        features: features.clone(),
        limits: BTreeMap::from([
            ("max_users".to_string(), max_users),
            ("max_branches".to_string(), max_branches),
        ]),
        plan_code: Some(plan.code().to_string()),
    };
    let signature = signing_key.sign(&naseej_license_format::canonical_payload(&payload)?);
    let envelope = SignedLicense {
        payload: payload.clone(),
        signature_ed25519_hex: hex::encode(signature.to_bytes()),
    };
    let output_dir = request_path.parent().unwrap_or_else(|| Path::new("."));
    let output = output_dir.join(format!(
        "naseej-{plan_code}-{license_id}.lic",
        plan_code = plan.code()
    ));
    let temp = output.with_file_name(format!(".{license_id}.lic.part"));
    let bytes = serde_json::to_vec_pretty(&envelope)?;
    {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
    }
    if let Err(error) = fs::rename(&temp, &output) {
        let _ = fs::remove_file(&temp);
        return Err(error.into());
    }

    let record = SubscriptionRecord {
        schema_version: REGISTRY_SCHEMA_VERSION,
        license_id,
        request_id: request.request_id,
        product: PRODUCT_CODE.to_string(),
        customer_id: customer_id.to_string(),
        device_fingerprint: request.device_fingerprint,
        plan_code: plan.code().to_string(),
        issued_at_utc: payload.issued_at_utc,
        not_before_utc: payload.not_before_utc,
        expires_at_utc: payload.expires_at_utc,
        license_file: output.display().to_string(),
        features: features.into_iter().collect(),
        limits: payload.limits,
    };
    if let Err(error) = write_record(&registry_dir, &record) {
        let _ = fs::remove_file(&output);
        return Err(error);
    }

    records.push(record);
    println!("PLAN:{}", plan.label());
    println!(
        "LICENSE_ID:{}",
        records.last().expect("record appended").license_id
    );
    println!(
        "EXPIRES_AT_UTC:{}",
        records.last().expect("record appended").expires_at_utc
    );
    println!("LICENSE_WRITTEN:{}", output.display());
    println!("REGISTRY_DIR:{}", registry_dir.display());
    Ok(())
}

fn read_signing_key(path: &Path) -> Result<SigningKey, Box<dyn std::error::Error>> {
    let mut seed = fs::read(path)?;
    if seed.len() != 32 {
        seed.zeroize();
        return Err("private key file must contain exactly 32 seed bytes".into());
    }
    let mut seed_array: [u8; 32] = seed
        .as_slice()
        .try_into()
        .map_err(|_| "private key is invalid")?;
    let signing_key = SigningKey::from_bytes(&seed_array);
    seed.zeroize();
    seed_array.zeroize();
    Ok(signing_key)
}

fn validate_request(request: &ActivationRequest) -> Result<(), Box<dyn std::error::Error>> {
    if request.schema_version != ACTIVATION_REQUEST_SCHEMA_VERSION
        || request.product != PRODUCT_CODE
        || request.device_fingerprint.len() != 64
        || hex::decode(&request.device_fingerprint).is_err()
        || Uuid::parse_str(&request.request_id).is_err()
    {
        return Err("request schema/product/request ID/fingerprint is invalid".into());
    }
    DateTime::parse_from_rfc3339(&request.requested_at_utc)?;
    Ok(())
}

fn default_registry_dir() -> Result<PathBuf, Box<dyn std::error::Error>> {
    if let Some(path) = env::var_os("NASEEJ_ISSUER_REGISTRY") {
        return Ok(PathBuf::from(path));
    }
    #[cfg(windows)]
    {
        let root = env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .ok_or("LOCALAPPDATA is unavailable")?;
        Ok(root.join("NaseejLicenseIssuer").join("subscriptions"))
    }
    #[cfg(not(windows))]
    {
        let root = env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")))
            .ok_or("XDG_STATE_HOME and HOME are unavailable")?;
        Ok(root.join("naseej-license-issuer").join("subscriptions"))
    }
}

fn read_records(
    registry_dir: &Path,
) -> Result<Vec<SubscriptionRecord>, Box<dyn std::error::Error>> {
    if !registry_dir.exists() {
        return Ok(Vec::new());
    }
    let mut records = Vec::new();
    for entry in fs::read_dir(registry_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let record: SubscriptionRecord = serde_json::from_slice(&fs::read(&path)?)?;
        if record.schema_version != REGISTRY_SCHEMA_VERSION {
            return Err(format!("unsupported registry record: {}", path.display()).into());
        }
        records.push(record);
    }
    records.sort_by(|left, right| left.issued_at_utc.cmp(&right.issued_at_utc));
    Ok(records)
}

fn write_record(
    registry_dir: &Path,
    record: &SubscriptionRecord,
) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(registry_dir)?;
    let final_path = registry_dir.join(format!("{}.json", record.license_id));
    let temp_path = registry_dir.join(format!(".{}.json.part", record.license_id));
    let bytes = serde_json::to_vec_pretty(record)?;
    {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
    }
    if let Err(error) = fs::rename(&temp_path, &final_path) {
        let _ = fs::remove_file(&temp_path);
        return Err(error.into());
    }
    Ok(())
}

fn list_subscriptions(registry_dir: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let records = read_records(&registry_dir)?;
    println!("registry: {}", registry_dir.display());
    println!("records: {}", records.len());
    println!("license_id\tcustomer_id\tplan\tstatus\texpires_at_utc\tfingerprint\tlicense_file");
    let now = Utc::now();
    for record in records {
        let status = if let Ok(not_before) = DateTime::parse_from_rfc3339(&record.not_before_utc) {
            if now < not_before.with_timezone(&Utc) {
                "SCHEDULED"
            } else if DateTime::parse_from_rfc3339(&record.expires_at_utc)
                .map(|expiry| now >= expiry.with_timezone(&Utc))
                .unwrap_or(true)
            {
                "EXPIRED"
            } else {
                "ACTIVE"
            }
        } else {
            "DATE_INVALID"
        };
        let fingerprint = if record.device_fingerprint.len() > 16 {
            format!("{}…", &record.device_fingerprint[..16])
        } else {
            record.device_fingerprint.clone()
        };
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}",
            record.license_id,
            record.customer_id,
            record.plan_code,
            status,
            record.expires_at_utc,
            fingerprint,
            record.license_file,
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let directory = env::temp_dir().join(format!("naseej-issuer-test-{}", Uuid::new_v4()));
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
        assert_eq!(records[0].license_id, record.license_id);
        assert_eq!(records[0].plan_code, PLAN_TRIAL_7D);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn issuing_trial_creates_signed_seven_day_license_and_registry_entry() {
        let directory = env::temp_dir().join(format!("naseej-issuer-e2e-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let request_path = directory.join("activation.req");
        let key_path = directory.join("test-seed.bin");
        let registry = directory.join("registry");
        let request = ActivationRequest {
            schema_version: ACTIVATION_REQUEST_SCHEMA_VERSION,
            request_id: Uuid::new_v4().to_string(),
            product: PRODUCT_CODE.into(),
            app_version: "test".into(),
            requested_at_utc: Utc::now().to_rfc3339(),
            device_fingerprint: "ab".repeat(32),
        };
        fs::write(&request_path, serde_json::to_vec(&request).unwrap()).unwrap();
        fs::write(&key_path, [7u8; 32]).unwrap();
        issue(
            request_path,
            "customer-test".into(),
            Plan::Trial7d,
            None,
            key_path.clone(),
            vec!["pos.sell".into()],
            1,
            1,
            registry.clone(),
        )
        .unwrap();
        let annual_request_path = directory.join("renewal.req");
        let annual_request = ActivationRequest {
            schema_version: ACTIVATION_REQUEST_SCHEMA_VERSION,
            request_id: Uuid::new_v4().to_string(),
            product: PRODUCT_CODE.into(),
            app_version: "test".into(),
            requested_at_utc: Utc::now().to_rfc3339(),
            device_fingerprint: "ab".repeat(32),
        };
        fs::write(
            &annual_request_path,
            serde_json::to_vec(&annual_request).unwrap(),
        )
        .unwrap();
        issue(
            annual_request_path,
            "customer-test".into(),
            Plan::Annual12m,
            None,
            key_path,
            vec!["pos.sell".into()],
            2,
            1,
            registry.clone(),
        )
        .unwrap();

        let records = read_records(&registry).unwrap();
        assert_eq!(records.len(), 2);
        let trial_record = records
            .iter()
            .find(|record| record.plan_code == PLAN_TRIAL_7D)
            .unwrap();
        let annual_record = records
            .iter()
            .find(|record| record.plan_code == PLAN_ANNUAL_12M)
            .unwrap();
        let trial: SignedLicense =
            serde_json::from_slice(&fs::read(&trial_record.license_file).unwrap()).unwrap();
        let annual: SignedLicense =
            serde_json::from_slice(&fs::read(&annual_record.license_file).unwrap()).unwrap();
        let verifying_key = SigningKey::from_bytes(&[7u8; 32]).verifying_key();
        for (license, expected_plan) in [(&trial, PLAN_TRIAL_7D), (&annual, PLAN_ANNUAL_12M)] {
            assert_eq!(license.payload.plan_code.as_deref(), Some(expected_plan));
            assert!(naseej_license_format::verify_license(license, &verifying_key).is_ok());
        }
        let trial_start = DateTime::parse_from_rfc3339(&trial.payload.not_before_utc)
            .unwrap()
            .with_timezone(&Utc);
        let trial_end = DateTime::parse_from_rfc3339(&trial.payload.expires_at_utc)
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(trial_end - trial_start, Duration::days(7));
        let annual_start = DateTime::parse_from_rfc3339(&annual.payload.not_before_utc)
            .unwrap()
            .with_timezone(&Utc);
        let annual_end = DateTime::parse_from_rfc3339(&annual.payload.expires_at_utc)
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(
            annual_end,
            annual_start.checked_add_months(Months::new(12)).unwrap()
        );
        fs::remove_dir_all(directory).unwrap();
    }
}
