use crate::db;
use aes_gcm::{
    Aes256Gcm, KeyInit, Nonce,
    aead::{Aead, Payload},
};
use argon2::{Algorithm, Argon2, Params, Version};
use rusqlite::OptionalExtension;
use rusqlite::{Connection, backup::Backup};
use sha2::Digest;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};
use zeroize::{Zeroize, Zeroizing};

const MAGIC: &[u8; 8] = b"NASBKZ01";
const HEADER_LEN: usize = 8 + 16 + 12;
const MAX_BACKUP_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const SNAPSHOT_DIR: &str = "backup-work";

struct TempEncryptedDb {
    path: PathBuf,
    connection: Option<Connection>,
}

impl TempEncryptedDb {
    fn create(directory: &Path, name: &str, key: &[u8; 32]) -> Result<Self, String> {
        fs::create_dir_all(directory).map_err(|_| "BACKUP_WORKDIR_CREATE_FAILED".to_string())?;
        let path = directory.join(format!("{name}-{}.db", uuid::Uuid::new_v4()));
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|_| "BACKUP_TEMP_CREATE_FAILED".to_string())?;
        let connection = (|| {
            let connection =
                Connection::open(&path).map_err(|_| "BACKUP_TEMP_OPEN_FAILED".to_string())?;
            db::set_sqlcipher_key(&connection, key)?;
            Ok::<Connection, String>(connection)
        })();
        match connection {
            Ok(connection) => Ok(Self {
                path,
                connection: Some(connection),
            }),
            Err(error) => {
                let _ = fs::remove_file(&path);
                Err(error)
            }
        }
    }

    fn connection(&self) -> &Connection {
        self.connection
            .as_ref()
            .expect("temporary database is open")
    }
    fn connection_mut(&mut self) -> &mut Connection {
        self.connection
            .as_mut()
            .expect("temporary database is open")
    }

    fn close(&mut self) -> Result<(), String> {
        if let Some(connection) = self.connection.take() {
            connection
                .close()
                .map_err(|_| "BACKUP_TEMP_CLOSE_FAILED".to_string())?;
        }
        Ok(())
    }
}

impl Drop for TempEncryptedDb {
    fn drop(&mut self) {
        self.connection.take();
        let _ = fs::remove_file(&self.path);
        let _ = fs::remove_file(self.path.with_extension("db-wal"));
        let _ = fs::remove_file(self.path.with_extension("db-shm"));
    }
}

pub struct PreparedRestore {
    connection: Option<Connection>,
    temp_path: PathBuf,
}

impl PreparedRestore {
    pub fn connection(&self) -> &Connection {
        self.connection.as_ref().expect("prepared restore is open")
    }
}

impl Drop for PreparedRestore {
    fn drop(&mut self) {
        self.connection.take();
        let _ = fs::remove_file(&self.temp_path);
        let _ = fs::remove_file(self.temp_path.with_extension("db-wal"));
        let _ = fs::remove_file(self.temp_path.with_extension("db-shm"));
    }
}

pub fn create_encrypted_backup(
    source: &Connection,
    app_data_dir: &Path,
    destination: &Path,
    password: &str,
) -> Result<(u64, String), String> {
    validate_password(password)?;
    if destination.exists() {
        return Err("BACKUP_DESTINATION_EXISTS".to_string());
    }
    let parent = destination
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .ok_or_else(|| "BACKUP_PATH_INVALID".to_string())?;
    fs::create_dir_all(parent).map_err(|_| "BACKUP_DIRECTORY_CREATE_FAILED".to_string())?;

    let local_key = db::load_database_key(app_data_dir)?;
    let mut snapshot =
        TempEncryptedDb::create(&app_data_dir.join(SNAPSHOT_DIR), "export", &local_key)?;
    let backup = Backup::new(source, snapshot.connection_mut())
        .map_err(|_| "BACKUP_SNAPSHOT_START_FAILED".to_string())?;
    backup
        .run_to_completion(64, Duration::from_millis(20), None)
        .map_err(|_| "BACKUP_SNAPSHOT_FAILED".to_string())?;
    drop(backup);
    snapshot.close()?;

    let metadata =
        fs::metadata(&snapshot.path).map_err(|_| "BACKUP_SNAPSHOT_MISSING".to_string())?;
    if metadata.len() == 0 || metadata.len() > MAX_BACKUP_BYTES {
        return Err("BACKUP_SIZE_INVALID".to_string());
    }
    let mut encrypted_database =
        fs::read(&snapshot.path).map_err(|_| "BACKUP_SNAPSHOT_READ_FAILED".to_string())?;
    let mut plaintext = Vec::with_capacity(32 + encrypted_database.len());
    plaintext.extend_from_slice(local_key.as_ref());
    plaintext.extend_from_slice(&encrypted_database);
    encrypted_database.zeroize();
    let file_bytes = encrypt_container(&plaintext, password)?;
    plaintext.zeroize();
    write_new_file_atomically(destination, &file_bytes)?;
    Ok((
        file_bytes.len() as u64,
        hex::encode(sha2::Sha256::digest(&file_bytes)),
    ))
}

pub fn prepare_restore(
    source_path: &Path,
    app_data_dir: &Path,
    password: &str,
) -> Result<PreparedRestore, String> {
    validate_password(password)?;
    let file_size = fs::metadata(source_path)
        .map_err(|_| "BACKUP_FILE_READ_FAILED".to_string())?
        .len();
    if file_size < (HEADER_LEN + 16 + 32 + 512) as u64
        || file_size > MAX_BACKUP_BYTES + (HEADER_LEN + 16 + 32) as u64
    {
        return Err("BACKUP_SIZE_INVALID".to_string());
    }
    let mut bytes = fs::read(source_path).map_err(|_| "BACKUP_FILE_READ_FAILED".to_string())?;
    let mut plaintext = decrypt_container(&bytes, password)?;
    bytes.zeroize();
    if plaintext.len() <= 32 || (plaintext.len() - 32) as u64 > MAX_BACKUP_BYTES {
        plaintext.zeroize();
        return Err("BACKUP_PAYLOAD_INVALID".to_string());
    }
    let mut imported_key = [0u8; 32];
    imported_key.copy_from_slice(&plaintext[..32]);
    let mut temp_path = app_data_dir.join(SNAPSHOT_DIR);
    fs::create_dir_all(&temp_path).map_err(|_| "BACKUP_WORKDIR_CREATE_FAILED".to_string())?;
    temp_path.push(format!("restore-{}.db", uuid::Uuid::new_v4()));
    {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
            .map_err(|_| "BACKUP_TEMP_CREATE_FAILED".to_string())?;
        file.write_all(&plaintext[32..])
            .map_err(|_| "BACKUP_TEMP_WRITE_FAILED".to_string())?;
        file.sync_all()
            .map_err(|_| "BACKUP_TEMP_SYNC_FAILED".to_string())?;
    }
    plaintext.zeroize();
    let imported_key = Zeroizing::new(imported_key);
    let connection = (|| {
        let connection =
            Connection::open(&temp_path).map_err(|_| "BACKUP_DATABASE_OPEN_FAILED".to_string())?;
        db::set_sqlcipher_key(&connection, &imported_key)?;
        validate_database(&connection)?;
        Ok::<Connection, String>(connection)
    })();
    match connection {
        Ok(connection) => Ok(PreparedRestore {
            connection: Some(connection),
            temp_path,
        }),
        Err(error) => {
            let _ = fs::remove_file(&temp_path);
            Err(error)
        }
    }
}

pub fn copy_into_current_database(
    restored: &Connection,
    destination: &mut Connection,
    app_data_dir: &Path,
) -> Result<(), String> {
    let local_key = db::load_database_key(app_data_dir)?;
    let mut staged =
        TempEncryptedDb::create(&app_data_dir.join(SNAPSHOT_DIR), "install", &local_key)?;
    let stage_backup = Backup::new(restored, staged.connection_mut())
        .map_err(|_| "RESTORE_STAGE_START_FAILED".to_string())?;
    stage_backup
        .run_to_completion(64, Duration::from_millis(20), None)
        .map_err(|_| "RESTORE_STAGE_FAILED".to_string())?;
    drop(stage_backup);
    validate_database(staged.connection())?;
    let install = Backup::new(staged.connection(), destination)
        .map_err(|_| "RESTORE_INSTALL_START_FAILED".to_string())?;
    install
        .run_to_completion(64, Duration::from_millis(20), None)
        .map_err(|_| "RESTORE_INSTALL_FAILED".to_string())?;
    drop(install);
    validate_database(destination)
}

pub fn validate_database(connection: &Connection) -> Result<(), String> {
    let version: String = connection
        .query_row("PRAGMA cipher_version", [], |row| row.get(0))
        .map_err(|_| "SQLCIPHER_REQUIRED".to_string())?;
    if version.trim().is_empty() {
        return Err("SQLCIPHER_REQUIRED".to_string());
    }
    let check: String = connection
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .map_err(|_| "BACKUP_INTEGRITY_CHECK_FAILED".to_string())?;
    if check != "ok" {
        return Err("BACKUP_INTEGRITY_CHECK_FAILED".to_string());
    }
    let foreign_key_error: Option<String> = connection
        .query_row(
            "SELECT \"table\" FROM pragma_foreign_key_check LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|_| "BACKUP_FOREIGN_KEY_CHECK_FAILED".to_string())?;
    if foreign_key_error.is_some() {
        return Err("BACKUP_FOREIGN_KEY_CHECK_FAILED".to_string());
    }
    let schema_version: i64 = connection
        .query_row(
            "SELECT COALESCE(MAX(version),0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )
        .map_err(|_| "BACKUP_SCHEMA_INVALID".to_string())?;
    if schema_version != db::CURRENT_SCHEMA_VERSION {
        return Err("BACKUP_SCHEMA_UNSUPPORTED".to_string());
    }
    Ok(())
}

fn validate_password(password: &str) -> Result<(), String> {
    if password.chars().count() < 12 || password.len() > 256 {
        return Err("BACKUP_PASSWORD_POLICY_FAILED".to_string());
    }
    Ok(())
}

fn derive_key(password: &str, salt: &[u8; 16]) -> Result<Zeroizing<[u8; 32]>, String> {
    let params = Params::new(64 * 1024, 3, 1, Some(32))
        .map_err(|_| "BACKUP_KDF_CONFIG_FAILED".to_string())?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = Zeroizing::new([0u8; 32]);
    argon
        .hash_password_into(password.as_bytes(), salt, key.as_mut())
        .map_err(|_| "BACKUP_KDF_FAILED".to_string())?;
    Ok(key)
}

fn encrypt_container(plaintext: &[u8], password: &str) -> Result<Vec<u8>, String> {
    let mut salt = [0u8; 16];
    let mut nonce = [0u8; 12];
    getrandom::fill(&mut salt).map_err(|_| "CSPRNG_FAILED".to_string())?;
    getrandom::fill(&mut nonce).map_err(|_| "CSPRNG_FAILED".to_string())?;
    let key = derive_key(password, &salt)?;
    let cipher = Aes256Gcm::new_from_slice(key.as_ref())
        .map_err(|_| "BACKUP_CIPHER_INIT_FAILED".to_string())?;
    let mut header = Vec::with_capacity(HEADER_LEN);
    header.extend_from_slice(MAGIC);
    header.extend_from_slice(&salt);
    header.extend_from_slice(&nonce);
    let encrypted = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: plaintext,
                aad: &header,
            },
        )
        .map_err(|_| "BACKUP_ENCRYPT_FAILED".to_string())?;
    let mut output = header;
    output.extend_from_slice(&encrypted);
    Ok(output)
}

fn decrypt_container(bytes: &[u8], password: &str) -> Result<Vec<u8>, String> {
    if bytes.len() < HEADER_LEN + 16 || &bytes[..8] != MAGIC {
        return Err("BACKUP_FORMAT_INVALID".to_string());
    }
    let salt: [u8; 16] = bytes[8..24]
        .try_into()
        .map_err(|_| "BACKUP_FORMAT_INVALID".to_string())?;
    let nonce: [u8; 12] = bytes[24..36]
        .try_into()
        .map_err(|_| "BACKUP_FORMAT_INVALID".to_string())?;
    let key = derive_key(password, &salt)?;
    let cipher = Aes256Gcm::new_from_slice(key.as_ref())
        .map_err(|_| "BACKUP_CIPHER_INIT_FAILED".to_string())?;
    cipher
        .decrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: &bytes[HEADER_LEN..],
                aad: &bytes[..HEADER_LEN],
            },
        )
        .map_err(|_| "BACKUP_AUTHENTICATION_FAILED".to_string())
}

fn write_new_file_atomically(destination: &Path, bytes: &[u8]) -> Result<(), String> {
    if destination.exists() {
        return Err("BACKUP_DESTINATION_EXISTS".to_string());
    }
    let parent = destination
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .ok_or_else(|| "BACKUP_PATH_INVALID".to_string())?;
    let filename = destination
        .file_name()
        .ok_or_else(|| "BACKUP_PATH_INVALID".to_string())?
        .to_string_lossy();
    let temporary = parent.join(format!(".{filename}.{}.part", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|_| "BACKUP_TEMP_CREATE_FAILED".to_string())?;
        file.write_all(bytes)
            .map_err(|_| "BACKUP_TEMP_WRITE_FAILED".to_string())?;
        file.sync_all()
            .map_err(|_| "BACKUP_TEMP_SYNC_FAILED".to_string())?;
        fs::rename(&temporary, destination)
            .map_err(|_| "BACKUP_ATOMIC_RENAME_FAILED".to_string())?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}
