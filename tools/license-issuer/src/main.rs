use clap::{Parser, Subcommand, ValueEnum};
use ed25519_dalek::SigningKey;
use naseej_license_registry::{
    IssueOptions, SubscriptionStatus, default_registry_dir, issue_license, read_records,
};
use std::{
    fs,
    path::{Path, PathBuf},
};
use zeroize::Zeroize;

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
    fn registry_plan(self) -> naseej_license_registry::Plan {
        match self {
            Self::Trial7d => naseej_license_registry::Plan::Trial7d,
            Self::Annual12m => naseej_license_registry::Plan::Annual12m,
        }
    }
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
        } => {
            let registry_dir = registry.unwrap_or(default_registry_dir()?);
            // This file-based key source is for controlled development and
            // internal use only. Production signing belongs in an HSM or in
            // Windows CNG with audited access controls.
            let signing_key = read_signing_key(&private_key_file)?;
            let outcome = issue_license(
                &signing_key,
                IssueOptions {
                    request_path: request,
                    customer_id,
                    plan: plan.registry_plan(),
                    starts_at_utc,
                    requested_features: features,
                    max_users,
                    max_branches,
                    registry_dir,
                    output_dir: None,
                },
            )?;
            for warning in &outcome.warnings {
                eprintln!("{warning}");
            }
            println!("PLAN:{}", outcome.plan_label);
            println!("LICENSE_ID:{}", outcome.license_id);
            println!("EXPIRES_AT_UTC:{}", outcome.expires_at_utc);
            println!("LICENSE_WRITTEN:{}", outcome.license_path.display());
            println!("REGISTRY_DIR:{}", outcome.registry_dir.display());
            Ok(())
        }
        Command::List { registry } => {
            let registry_dir = registry.unwrap_or(default_registry_dir()?);
            list_subscriptions(registry_dir)
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

fn list_subscriptions(registry_dir: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let records = read_records(&registry_dir)?;
    println!("registry: {}", registry_dir.display());
    println!("records: {}", records.len());
    println!("license_id\tcustomer_id\tplan\tstatus\texpires_at_utc\tfingerprint\tlicense_file");
    let now = chrono::Utc::now();
    for record in records {
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
            SubscriptionStatus::of(&record, now).code(),
            record.expires_at_utc,
            fingerprint,
            record.license_file,
        );
    }
    Ok(())
}
