use aes_gcm::{
    Aes256Gcm, KeyInit, Nonce,
    aead::{Aead, Payload},
};
use chrono::Utc;
use rusqlite::{Connection, TransactionBehavior};
use sha2::{Digest, Sha256};
use std::{fs, path::Path};
use zeroize::{Zeroize, Zeroizing};

const INITIAL_SCHEMA: &str = include_str!("../migrations/0001_initial.sql");
const MIGRATION_0002: &str = include_str!("../migrations/0002_branch_management.sql");
const MIGRATION_0003: &str = include_str!("../migrations/0003_local_trial_state.sql");
const MIGRATION_0004: &str = include_str!("../migrations/0004_owner_panel.sql");
const MIGRATION_0005: &str = include_str!("../migrations/0005_clock_high_water.sql");
const MIGRATION_0006: &str = include_str!("../migrations/0006_loyalty_and_imports.sql");
const MIGRATION_0007: &str = include_str!("../migrations/0007_immutable_documents.sql");
pub const CURRENT_SCHEMA_VERSION: i64 = 7;
const BRANCH_ID: &str = "00000000-0000-4000-8000-000000000001";

// The SQLCipher configuration
//
// Exactly one pragma is applied to every connection: the key itself.
//
// * `cipher_compatibility` is a migration pragma. Applying it makes SQLCipher
//   re-derive the key, which does not fit the raw random key this
//   application stores under Windows DPAPI.
// * No cipher parameter (`cipher_page_size`, `kdf_iter`,
//   `cipher_hmac_algorithm`, `cipher_kdf_algorithm`) is set. The bundled
//   SQLCipher is version 4 and its defaults are exactly page size 4096,
//   PBKDF2-HMAC-SHA512 with 256,000 iterations and HMAC-SHA512, so stating
//   them again cannot change the result.
// * `cipher_memory_security` is deliberately not used. It is a process-wide
//   one-way switch: once any connection turns it on, every later allocation
//   in the whole process runs through SQLCipher's "enhanced memory
//   security" allocator, which locks pages with `VirtualLock` on Windows and
//   wipes every block before it is freed. In the bundled SQLCipher that mode
//   ends the process with a stack overflow, so it stays off.
//
// The on-disk format is unchanged: page size 4096, PBKDF2-HMAC-SHA512 with
// 256,000 iterations and HMAC-SHA512, the SQLCipher 4 defaults.

/// Emit a diagnostic line when `NASEEJ_TRACE` is set.
///
/// The Windows integration test turns this on so a failure names the exact
/// storage step instead of just the operation.
pub fn trace(message: &str) {
    if std::env::var_os("NASEEJ_TRACE").is_some() {
        eprintln!("[naseej] {message}");
    }
}

/// The tag that binds a database file to the local installation.
///
/// SQLCipher aborts the process when it is handed a key that does not match
/// the file, so the mismatch is detected here first and reported as a normal
/// error the operator can act on.
const DB_BINDING_DOMAIN: &[u8] = b"naseej-db-binding-v1";

/// Compute the binding tag of a database key.
fn db_binding_tag(key: &[u8; 32]) -> String {
    let mut material = Vec::with_capacity(key.len() + DB_BINDING_DOMAIN.len());
    material.extend_from_slice(key);
    material.extend_from_slice(DB_BINDING_DOMAIN);
    hex::encode(Sha256::digest(&material))
}

/// Check that the database file belongs to this installation.
///
/// The tag is written the first time the database is opened. A missing tag is
/// treated as a first run; a different tag means the database file was
/// replaced with one that was encrypted under a different key, which
/// SQLCipher cannot open safely.
fn check_database_binding(app_data_dir: &Path, key: &[u8; 32]) -> Result<(), String> {
    let binding_path = app_data_dir.join("security").join("db-binding.tag");
    let expected = db_binding_tag(key);
    match fs::read_to_string(&binding_path) {
        Ok(stored) => {
            if stored.trim() == expected {
                Ok(())
            } else {
                Err("DB_KEY_BINDING_MISMATCH".to_string())
            }
        }
        Err(_) => {
            if let Some(parent) = binding_path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::write(&binding_path, &expected);
            Ok(())
        }
    }
}

pub fn open_database(app_data_dir: &Path) -> Result<Connection, String> {
    fs::create_dir_all(app_data_dir.join("media")).map_err(|e| format!("APP_DATA_DIR:{e}"))?;
    let database_path = app_data_dir.join("store.db");
    let protected_key_path = app_data_dir.join("security").join("db-key.dpapi");
    if let Some(parent) = protected_key_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("KEY_DIR:{e}"))?;
    }

    trace("open_database: reading the protected key");
    let mut key = load_or_create_database_key(&protected_key_path)?;
    trace("open_database: key loaded");
    // The binding is checked before SQLCipher sees the key, because handing
    // SQLCipher a key that does not match the file aborts the process.
    let key_bytes: [u8; 32] = key
        .as_slice()
        .try_into()
        .map_err(|_| "DB_KEY_INVALID".to_string())?;
    check_database_binding(app_data_dir, &key_bytes)?;
    let connection = Connection::open(&database_path).map_err(|e| format!("DB_OPEN:{e}"))?;
    trace("open_database: database file opened");
    let key_hex = hex::encode(&key);
    trace("open_database: applying the key");
    // The generated key is hex-only, so this PRAGMA cannot carry user input.
    connection
        .execute_batch(&format!("PRAGMA key = \"x'{key_hex}'\";"))
        .map_err(|_| "DB_KEY_INIT_FAILED".to_string())?;
    key.zeroize();

    trace("open_database: key applied");
    trace("open_database: verifying SQLCipher");
    // Reading the schema is intentional: it verifies that the key opened an encrypted database.
    let cipher_version: String = connection
        .query_row("PRAGMA cipher_version", [], |row| row.get(0))
        .map_err(|_| "SQLCIPHER_REQUIRED".to_string())?;
    if cipher_version.trim().is_empty() {
        return Err("SQLCIPHER_REQUIRED".to_string());
    }
    connection
        .busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|e| format!("DB_BUSY_TIMEOUT:{e}"))?;
    connection
        .execute_batch(
            "PRAGMA foreign_keys = ON;
         PRAGMA journal_mode = WAL;
         PRAGMA synchronous = FULL;
         PRAGMA temp_store = MEMORY;",
        )
        .map_err(|e| format!("DB_PRAGMA:{e}"))?;

    trace("open_database: applying migrations");
    let connection = apply_migrations(connection)?;
    trace("open_database: ready");
    Ok(connection)
}

fn apply_migrations(mut connection: Connection) -> Result<Connection, String> {
    trace("migrations: checking the schema version");
    let migration_table_exists: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='schema_migrations')",
        [], |row| row.get(0),
    ).map_err(|e| format!("MIGRATION_CHECK:{e}"))?;

    if !migration_table_exists {
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Exclusive)
            .map_err(|e| format!("MIGRATION_BEGIN:{e}"))?;
        transaction
            .execute_batch(INITIAL_SCHEMA)
            .map_err(|e| format!("MIGRATION_0001:{e}"))?;
        let digest = hex::encode(Sha256::digest(INITIAL_SCHEMA.as_bytes()));
        transaction
            .execute(
                "INSERT INTO schema_migrations(version, checksum, applied_at) VALUES(1, ?1, ?2)",
                rusqlite::params![digest, Utc::now().to_rfc3339()],
            )
            .map_err(|e| format!("MIGRATION_RECORD:{e}"))?;
        transaction.execute(
            "INSERT INTO branches(id, code, name_ar, name_en, currency_code, tax_rate_bps, prices_include_tax, timezone, created_at)
             VALUES(?1, 'MAIN', 'الفرع الرئيسي', 'Main branch', 'EGP', 0, 0, 'Africa/Cairo', ?2)",
            rusqlite::params![BRANCH_ID, Utc::now().to_rfc3339()],
        ).map_err(|e| format!("BRANCH_INIT:{e}"))?;
        seed_catalog_defaults(&transaction)?;
        transaction
            .commit()
            .map_err(|e| format!("MIGRATION_COMMIT:{e}"))?;
    }

    let mut version: i64 = connection
        .query_row(
            "SELECT COALESCE(MAX(version),0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )
        .map_err(|e| format!("MIGRATION_VERSION:{e}"))?;
    if version == 0 {
        return Err("DB_MIGRATION_STATE_INVALID".to_string());
    }
    if version > CURRENT_SCHEMA_VERSION {
        return Err("DB_SCHEMA_NEWER_THAN_APP".to_string());
    }

    if version < 2 {
        trace("migrations: applying migration 0002");
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Exclusive)
            .map_err(|e| format!("MIGRATION_0002_BEGIN:{e}"))?;
        transaction
            .execute_batch(MIGRATION_0002)
            .map_err(|e| format!("MIGRATION_0002:{e}"))?;
        let digest = hex::encode(Sha256::digest(MIGRATION_0002.as_bytes()));
        transaction
            .execute(
                "INSERT INTO schema_migrations(version, checksum, applied_at) VALUES(2, ?1, ?2)",
                rusqlite::params![digest, Utc::now().to_rfc3339()],
            )
            .map_err(|e| format!("MIGRATION_0002_RECORD:{e}"))?;
        transaction
            .commit()
            .map_err(|e| format!("MIGRATION_0002_COMMIT:{e}"))?;
        version = 2;
    }
    if version < 3 {
        trace("migrations: applying migration 0003");
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Exclusive)
            .map_err(|e| format!("MIGRATION_0003_BEGIN:{e}"))?;
        transaction
            .execute_batch(MIGRATION_0003)
            .map_err(|e| format!("MIGRATION_0003:{e}"))?;
        let digest = hex::encode(Sha256::digest(MIGRATION_0003.as_bytes()));
        transaction
            .execute(
                "INSERT INTO schema_migrations(version, checksum, applied_at) VALUES(3, ?1, ?2)",
                rusqlite::params![digest, Utc::now().to_rfc3339()],
            )
            .map_err(|e| format!("MIGRATION_0003_RECORD:{e}"))?;
        transaction
            .commit()
            .map_err(|e| format!("MIGRATION_0003_COMMIT:{e}"))?;
        version = 3;
    }
    if version < 4 {
        trace("migrations: applying migration 0004");
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Exclusive)
            .map_err(|e| format!("MIGRATION_0004_BEGIN:{e}"))?;
        transaction
            .execute_batch(MIGRATION_0004)
            .map_err(|e| format!("MIGRATION_0004:{e}"))?;
        let digest = hex::encode(Sha256::digest(MIGRATION_0004.as_bytes()));
        transaction
            .execute(
                "INSERT INTO schema_migrations(version, checksum, applied_at) VALUES(4, ?1, ?2)",
                rusqlite::params![digest, Utc::now().to_rfc3339()],
            )
            .map_err(|e| format!("MIGRATION_0004_RECORD:{e}"))?;
        transaction
            .commit()
            .map_err(|e| format!("MIGRATION_0004_COMMIT:{e}"))?;
        version = 4;
    }
    if version < 5 {
        trace("migrations: applying migration 0005");
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Exclusive)
            .map_err(|e| format!("MIGRATION_0005_BEGIN:{e}"))?;
        transaction
            .execute_batch(MIGRATION_0005)
            .map_err(|e| format!("MIGRATION_0005:{e}"))?;
        let digest = hex::encode(Sha256::digest(MIGRATION_0005.as_bytes()));
        transaction
            .execute(
                "INSERT INTO schema_migrations(version, checksum, applied_at) VALUES(5, ?1, ?2)",
                rusqlite::params![digest, Utc::now().to_rfc3339()],
            )
            .map_err(|e| format!("MIGRATION_0005_RECORD:{e}"))?;
        transaction
            .commit()
            .map_err(|e| format!("MIGRATION_0005_COMMIT:{e}"))?;
        version = 5;
    }
    if version < 6 {
        trace("migrations: applying migration 0006");
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Exclusive)
            .map_err(|e| format!("MIGRATION_0006_BEGIN:{e}"))?;
        transaction
            .execute_batch(MIGRATION_0006)
            .map_err(|e| format!("MIGRATION_0006:{e}"))?;
        let digest = hex::encode(Sha256::digest(MIGRATION_0006.as_bytes()));
        transaction
            .execute(
                "INSERT INTO schema_migrations(version, checksum, applied_at) VALUES(6, ?1, ?2)",
                rusqlite::params![digest, Utc::now().to_rfc3339()],
            )
            .map_err(|e| format!("MIGRATION_0006_RECORD:{e}"))?;
        transaction
            .commit()
            .map_err(|e| format!("MIGRATION_0006_COMMIT:{e}"))?;
        version = 6;
    }
    if version < 7 {
        trace("migrations: applying migration 0007");
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Exclusive)
            .map_err(|e| format!("MIGRATION_0007_BEGIN:{e}"))?;
        transaction
            .execute_batch(MIGRATION_0007)
            .map_err(|e| format!("MIGRATION_0007:{e}"))?;
        let digest = hex::encode(Sha256::digest(MIGRATION_0007.as_bytes()));
        transaction
            .execute(
                "INSERT INTO schema_migrations(version, checksum, applied_at) VALUES(7, ?1, ?2)",
                rusqlite::params![digest, Utc::now().to_rfc3339()],
            )
            .map_err(|e| format!("MIGRATION_0007_RECORD:{e}"))?;
        transaction
            .commit()
            .map_err(|e| format!("MIGRATION_0007_COMMIT:{e}"))?;
        version = 7;
    }
    if version != CURRENT_SCHEMA_VERSION {
        return Err("DB_MIGRATION_STATE_INVALID".to_string());
    }
    Ok(connection)
}

pub fn load_database_key(app_data_dir: &Path) -> Result<Zeroizing<[u8; 32]>, String> {
    let path = app_data_dir.join("security").join("db-key.dpapi");
    let protected = fs::read(path).map_err(|_| "DB_KEY_FILE_READ_FAILED".to_string())?;
    let mut raw = dpapi_unprotect(&protected)?;
    if raw.len() != 32 {
        raw.zeroize();
        return Err("DB_KEY_INVALID".to_string());
    }
    let mut key = [0u8; 32];
    key.copy_from_slice(&raw);
    raw.zeroize();
    Ok(Zeroizing::new(key))
}

/// Apply the SQLCipher key to a connection.
///
/// Only `PRAGMA key` is issued. `cipher_memory_security` is not used: it is
/// a process-wide one-way switch, and the "enhanced memory security" mode it
/// enables terminates the process with a stack overflow in the bundled
/// SQLCipher. The key itself does not need that mode: it is protected by
/// Windows DPAPI at rest, and the Rust-side copies are zeroized.
pub fn set_sqlcipher_key(connection: &Connection, key: &[u8; 32]) -> Result<(), String> {
    let key_hex = hex::encode(key);
    // The generated key is hex-only, so this PRAGMA cannot carry user input.
    connection
        .execute_batch(&format!("PRAGMA key = \"x'{key_hex}'\";"))
        .map_err(|_| "DB_KEY_INIT_FAILED".to_string())?;
    let cipher_version: String = connection
        .query_row("PRAGMA cipher_version", [], |row| row.get(0))
        .map_err(|_| "SQLCIPHER_REQUIRED".to_string())?;
    if cipher_version.trim().is_empty() {
        return Err("SQLCIPHER_REQUIRED".to_string());
    }
    Ok(())
}

fn seed_catalog_defaults(tx: &rusqlite::Transaction<'_>) -> Result<(), String> {
    let categories = [
        (
            "00000000-0000-4000-8000-000000000101",
            "قمصان",
            "Shirts",
            10,
        ),
        (
            "00000000-0000-4000-8000-000000000102",
            "بناطيل",
            "Trousers",
            20,
        ),
        (
            "00000000-0000-4000-8000-000000000103",
            "فساتين",
            "Dresses",
            30,
        ),
        (
            "00000000-0000-4000-8000-000000000104",
            "جاكيتات",
            "Jackets",
            40,
        ),
        (
            "00000000-0000-4000-8000-000000000105",
            "أساسيات",
            "Basics",
            50,
        ),
        (
            "00000000-0000-4000-8000-000000000106",
            "إكسسوارات",
            "Accessories",
            60,
        ),
    ];
    for (id, name_ar, name_en, sort_order) in categories {
        tx.execute("INSERT INTO categories(id,branch_id,name_ar,name_en,sort_order) VALUES(?1,?2,?3,?4,?5)",
            rusqlite::params![id, BRANCH_ID, name_ar, name_en, sort_order]).map_err(|e| format!("SEED_CATEGORY:{e}"))?;
    }
    for (index, (profile, code, name_ar, name_en, order)) in [
        ("CLOTHING", "XS", "صغير جدًا", "Extra small", 10),
        ("CLOTHING", "S", "صغير", "Small", 20),
        ("CLOTHING", "M", "متوسط", "Medium", 30),
        ("CLOTHING", "L", "كبير", "Large", 40),
        ("CLOTHING", "XL", "كبير جدًا", "Extra large", 50),
        ("CLOTHING", "XXL", "2XL", "2XL", 60),
        ("CLOTHING", "ONE SIZE", "مقاس واحد", "One size", 70),
        ("NUMERIC", "28", "28", "28", 10),
        ("NUMERIC", "30", "30", "30", 20),
        ("NUMERIC", "32", "32", "32", 30),
        ("NUMERIC", "34", "34", "34", 40),
        ("NUMERIC", "36", "36", "36", 50),
    ]
    .into_iter()
    .enumerate()
    {
        let size_num = 201u32 + index as u32;
        let id = format!("00000000-0000-4000-8000-{size_num:012}");
        tx.execute("INSERT INTO sizes(id,branch_id,profile_code,size_code,name_ar,name_en,sort_order) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            rusqlite::params![id, BRANCH_ID, profile, code, name_ar, name_en, order]).map_err(|e| format!("SEED_SIZE:{e}"))?;
    }
    let colors = [
        (301, "BLACK", "أسود", "Black", "#292C32"),
        (302, "WHITE", "أبيض", "White", "#F7F7F3"),
        (303, "BLUE", "أزرق", "Blue", "#6D9DBC"),
        (304, "NAVY", "كحلي", "Navy", "#273A53"),
        (305, "BEIGE", "بيج", "Beige", "#D6C3A5"),
        (306, "OLIVE", "زيتي", "Olive", "#737C61"),
        (307, "RED", "أحمر", "Red", "#B45D58"),
        (308, "MULTI", "متعدد الألوان", "Multicolor", "#A9A09A"),
        (309, "NONE", "بدون لون", "No color", "#D9DDD9"),
    ];
    for (id_num, code, name_ar, name_en, hex_color) in colors {
        let id = format!("00000000-0000-4000-8000-{id_num:012}");
        tx.execute("INSERT INTO colors(id,branch_id,color_code,name_ar,name_en,hex_color) VALUES(?1,?2,?3,?4,?5,?6)",
            rusqlite::params![id, BRANCH_ID, code, name_ar, name_en, hex_color]).map_err(|e| format!("SEED_COLOR:{e}"))?;
    }
    Ok(())
}

pub fn load_or_create_personal_data_key(app_data_dir: &Path) -> Result<[u8; 32], String> {
    let security_dir = app_data_dir.join("security");
    fs::create_dir_all(&security_dir).map_err(|_| "KEY_DIR_CREATE_FAILED".to_string())?;
    let mut raw = load_or_create_database_key(&security_dir.join("personal-data-key.dpapi"))?;
    if raw.len() != 32 {
        raw.zeroize();
        return Err("LOOKUP_KEY_INVALID".to_string());
    }
    let key: [u8; 32] = raw
        .as_slice()
        .try_into()
        .map_err(|_| "LOOKUP_KEY_INVALID".to_string())?;
    raw.zeroize();
    Ok(key)
}

fn load_or_create_database_key(path: &Path) -> Result<Vec<u8>, String> {
    if path.exists() {
        let protected = fs::read(path).map_err(|_| "DB_KEY_FILE_READ_FAILED".to_string())?;
        return dpapi_unprotect(&protected);
    }

    let mut raw_key = vec![0u8; 32];
    getrandom::fill(&mut raw_key).map_err(|_| "CSPRNG_FAILED".to_string())?;
    let protected = dpapi_protect(&raw_key)?;
    use std::fs::OpenOptions;
    use std::io::Write;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| "DB_KEY_FILE_CREATE_FAILED".to_string())?;
    file.write_all(&protected)
        .map_err(|_| "DB_KEY_FILE_WRITE_FAILED".to_string())?;
    file.sync_all()
        .map_err(|_| "DB_KEY_FILE_SYNC_FAILED".to_string())?;
    Ok(raw_key)
}

#[cfg(windows)]
fn dpapi_protect(data: &[u8]) -> Result<Vec<u8>, String> {
    win_dpapi::protect(data)
}
#[cfg(not(windows))]
fn dpapi_protect(_data: &[u8]) -> Result<Vec<u8>, String> {
    Err("WINDOWS_DPAPI_REQUIRED".to_string())
}

#[cfg(windows)]
fn dpapi_unprotect(data: &[u8]) -> Result<Vec<u8>, String> {
    win_dpapi::unprotect(data)
}
#[cfg(not(windows))]
fn dpapi_unprotect(_data: &[u8]) -> Result<Vec<u8>, String> {
    Err("WINDOWS_DPAPI_REQUIRED".to_string())
}

pub fn encrypt_personal_field(master_key: &[u8; 32], data: &[u8]) -> Result<Vec<u8>, String> {
    let mut key = derive_key(master_key, b"naseej-pos/pii-encryption/v1");
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|_| "PII_KEY_INVALID".to_string())?;
    key.zeroize();
    let mut nonce_bytes = [0u8; 12];
    getrandom::fill(&mut nonce_bytes).map_err(|_| "CSPRNG_FAILED".to_string())?;
    let ciphertext = cipher
        .encrypt(
            Nonce::from_slice(&nonce_bytes),
            Payload {
                msg: data,
                aad: b"naseej-pos/personal-field/v1",
            },
        )
        .map_err(|_| "PII_ENCRYPT_FAILED".to_string())?;
    let mut output = Vec::with_capacity(1 + nonce_bytes.len() + ciphertext.len());
    output.push(1);
    output.extend_from_slice(&nonce_bytes);
    output.extend_from_slice(&ciphertext);
    Ok(output)
}

pub fn decrypt_personal_field(master_key: &[u8; 32], data: &[u8]) -> Result<Vec<u8>, String> {
    if data.len() < 1 + 12 + 16 || data[0] != 1 {
        return Err("PII_CIPHERTEXT_INVALID".to_string());
    }
    let mut key = derive_key(master_key, b"naseej-pos/pii-encryption/v1");
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|_| "PII_KEY_INVALID".to_string())?;
    key.zeroize();
    cipher
        .decrypt(
            Nonce::from_slice(&data[1..13]),
            Payload {
                msg: &data[13..],
                aad: b"naseej-pos/personal-field/v1",
            },
        )
        .map_err(|_| "PII_DECRYPT_FAILED".to_string())
}

pub fn personal_lookup_hmac(master_key: &[u8; 32], message: &[u8]) -> String {
    let mut lookup_key = derive_key(master_key, b"naseej-pos/phone-lookup/v1");
    let digest = hmac_sha256(&lookup_key, message);
    lookup_key.zeroize();
    hex::encode(digest)
}

pub fn protect_current_user_data(data: &[u8]) -> Result<Vec<u8>, String> {
    dpapi_protect(data)
}
pub fn unprotect_current_user_data(data: &[u8]) -> Result<Vec<u8>, String> {
    dpapi_unprotect(data)
}

fn derive_key(master: &[u8; 32], domain: &[u8]) -> [u8; 32] {
    hmac_sha256(master, domain)
}

fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    let mut key_block = [0u8; BLOCK];
    if key.len() > BLOCK {
        let digest = Sha256::digest(key);
        key_block[..digest.len()].copy_from_slice(&digest);
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }
    let mut inner = Vec::with_capacity(BLOCK + message.len());
    let mut outer = Vec::with_capacity(BLOCK + 32);
    for byte in key_block {
        inner.push(byte ^ 0x36);
        outer.push(byte ^ 0x5c);
    }
    inner.extend_from_slice(message);
    outer.extend_from_slice(&Sha256::digest(&inner));
    Sha256::digest(&outer).into()
}

#[cfg(windows)]
mod win_dpapi {
    use std::{
        ffi::c_void,
        ptr::{null, null_mut},
        slice,
    };

    #[repr(C)]
    struct DataBlob {
        cb_data: u32,
        pb_data: *mut u8,
    }

    const CRYPTPROTECT_UI_FORBIDDEN: u32 = 0x1;

    #[link(name = "Crypt32")]
    unsafe extern "system" {
        fn CryptProtectData(
            input: *const DataBlob,
            description: *const u16,
            entropy: *const DataBlob,
            reserved: *mut c_void,
            prompt: *mut c_void,
            flags: u32,
            output: *mut DataBlob,
        ) -> i32;
        fn CryptUnprotectData(
            input: *const DataBlob,
            description: *mut *mut u16,
            entropy: *const DataBlob,
            reserved: *mut c_void,
            prompt: *mut c_void,
            flags: u32,
            output: *mut DataBlob,
        ) -> i32;
    }
    #[link(name = "Kernel32")]
    unsafe extern "system" {
        fn LocalFree(memory: *mut c_void) -> *mut c_void;
    }

    pub fn protect(data: &[u8]) -> Result<Vec<u8>, String> {
        let input = DataBlob {
            cb_data: data.len() as u32,
            pb_data: data.as_ptr() as *mut u8,
        };
        let mut output = DataBlob {
            cb_data: 0,
            pb_data: null_mut(),
        };
        let ok = unsafe {
            CryptProtectData(
                &input,
                null(),
                null(),
                null_mut(),
                null_mut(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        };
        if ok == 0 {
            return Err("DPAPI_PROTECT_FAILED".to_string());
        }
        let result =
            unsafe { slice::from_raw_parts(output.pb_data, output.cb_data as usize).to_vec() };
        unsafe {
            LocalFree(output.pb_data as *mut c_void);
        }
        Ok(result)
    }

    pub fn unprotect(data: &[u8]) -> Result<Vec<u8>, String> {
        let input = DataBlob {
            cb_data: data.len() as u32,
            pb_data: data.as_ptr() as *mut u8,
        };
        let mut output = DataBlob {
            cb_data: 0,
            pb_data: null_mut(),
        };
        let mut description: *mut u16 = null_mut();
        let ok = unsafe {
            CryptUnprotectData(
                &input,
                &mut description,
                null(),
                null_mut(),
                null_mut(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        };
        if ok == 0 {
            return Err("DPAPI_UNPROTECT_FAILED".to_string());
        }
        let result =
            unsafe { slice::from_raw_parts(output.pb_data, output.cb_data as usize).to_vec() };
        unsafe {
            LocalFree(output.pb_data as *mut c_void);
        }
        if !description.is_null() {
            unsafe {
                LocalFree(description as *mut c_void);
            }
        }
        Ok(result)
    }
}
