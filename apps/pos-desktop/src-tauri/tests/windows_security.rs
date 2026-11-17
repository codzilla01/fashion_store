//! Integration tests for the security-critical storage layer.
//!
//! These tests only assert on Windows. They are the executable form of the
//! release gate "SQLCipher / DPAPI / backup verified on real Windows": they run
//! on a Windows CI runner against the real SQLCipher build, the real Windows
//! DPAPI, and the real `.bkz` container.
//!
//! Run with: `cargo test --test windows_security` on Windows.

#![cfg(windows)]

use naseej_pos::backup;
use naseej_pos::db;
use naseej_pos::rusqlite::Connection;
use std::fs;
use std::path::PathBuf;

fn unique_suffix() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or_default();
    format!("{}-{}", std::process::id(), nanos)
}

fn temp_dir(label: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!("naseej-{label}-{}", unique_suffix()));
    fs::create_dir_all(&directory).expect("temp directory");
    directory
}

/// Run a test body on a thread with a generous stack.
///
/// SQLCipher's key handling and the SQLite backup API both recurse, and the
/// default test-thread stack is small enough for that recursion to overflow
/// it. A larger stack removes the failure mode without changing behaviour.
fn run_with_large_stack(body: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(body)
        .expect("test thread starts")
        .join()
        .expect("test body completes");
}

#[test]
fn opens_an_encrypted_database_and_reports_sqlcipher() {
    eprintln!("[cipher] opening the database");
    let directory = temp_dir("cipher");
    let connection = db::open_database(&directory).expect("database opens");

    let cipher_version: String = connection
        .query_row("PRAGMA cipher_version", [], |row| row.get(0))
        .expect("cipher_version is readable");
    assert!(
        !cipher_version.trim().is_empty(),
        "SQLCipher must be the active engine, not plain SQLite"
    );

    // The file on disk must not be a readable plain SQLite database.
    let raw = fs::read(directory.join("store.db")).expect("database file exists");
    assert!(
        !raw.starts_with(b"SQLite format 3\0"),
        "database file must not start with the plain SQLite magic header"
    );

    fs::remove_dir_all(directory).ok();
}

/// Set on the child process that probes a wrong key.
const WRONG_KEY_CHILD: &str = "NASEEJ_WRONG_KEY_CHILD";

#[test]
fn refuses_to_open_the_database_without_the_protected_key() {
    // SQLCipher can abort the whole process when it is handed a key that does
    // not match the file, so the probe runs in a child process. The parent
    // asserts the outcome that matters: the schema must not be readable.
    if std::env::var_os(WRONG_KEY_CHILD).is_some() {
        run_wrong_key_probe();
        return;
    }

    let executable = std::env::current_exe().expect("test executable path");
    let output = std::process::Command::new(executable)
        .args([
            "--exact",
            "refuses_to_open_the_database_without_the_protected_key",
            "--nocapture",
        ])
        .env(WRONG_KEY_CHILD, "1")
        .output()
        .expect("the wrong key probe runs");
    assert_ne!(
        output.status.code(),
        Some(0),
        "a wrong key must not be able to read the schema"
    );

    // The correct key, recovered through DPAPI, must still work.
    let directory = temp_dir("nokey");
    {
        let _connection = db::open_database(&directory).expect("database opens");
    }
    let key = db::load_database_key(&directory).expect("DPAPI key round trip");
    let connection = Connection::open(directory.join("store.db")).expect("file opens");
    db::set_sqlcipher_key(&connection, &key).expect("key pragma accepted");
    let count: i64 = connection
        .query_row("SELECT COUNT(*) FROM sqlite_master", [], |row| row.get(0))
        .expect("correct key reads the schema");
    assert!(count > 0, "the correct key must open the database");

    fs::remove_dir_all(directory).ok();
}

/// The body of the wrong-key probe.
///
/// It runs in a child process and reports the outcome through the exit code:
/// `0` means the schema was readable, which is a failure.
fn run_wrong_key_probe() {
    let directory = temp_dir("nokey-child");
    {
        let _connection = db::open_database(&directory).expect("database opens");
    }
    let connection = Connection::open(directory.join("store.db")).expect("file opens");
    let wrong_key = [0xABu8; 32];
    db::set_sqlcipher_key(&connection, &wrong_key).expect("key pragma accepted");
    let readable = connection
        .query_row("SELECT COUNT(*) FROM sqlite_master", [], |row| {
            row.get::<_, i64>(0)
        })
        .is_ok();
    fs::remove_dir_all(directory).ok();
    std::process::exit(if readable { 0 } else { 3 });
}

#[test]
fn rejects_a_database_that_belongs_to_another_installation() {
    let directory = temp_dir("binding");
    {
        let _connection = db::open_database(&directory).expect("database opens");
    }
    // The binding tag belongs to this installation.
    let binding = directory.join("security").join("db-binding.tag");
    assert!(
        binding.exists(),
        "the database binding tag must be written on first open"
    );

    // A database file taken from another installation is rejected before
    // SQLCipher is handed a key that does not match it.
    let foreign = temp_dir("binding-foreign");
    let foreign_connection = db::open_database(&foreign).expect("database opens");
    drop(foreign_connection);
    fs::copy(foreign.join("store.db"), directory.join("store.db"))
        .expect("the foreign database is copied over");

    let result = db::open_database(&directory);
    assert!(
        result.is_err(),
        "a database from another installation must be rejected"
    );

    fs::remove_dir_all(directory).ok();
    fs::remove_dir_all(foreign).ok();
}

#[test]
fn dpapi_protects_and_unprotects_the_database_key() {
    let secret = b"naseej-dpapi-secret-payload";

    let protected = db::protect_current_user_data(secret).expect("DPAPI protect");
    assert_ne!(
        protected.as_slice(),
        secret,
        "protected bytes must not equal the plaintext"
    );
    assert!(
        !protected
            .windows(secret.len())
            .any(|window| window == secret),
        "plaintext must not be recoverable from the protected blob"
    );

    let clear = db::unprotect_current_user_data(&protected).expect("DPAPI unprotect");
    assert_eq!(clear.as_slice(), secret, "round trip must be exact");

    // The same protection must apply to the personal-data key material.
    let directory = temp_dir("dpapi-key");
    let key = db::load_or_create_personal_data_key(&directory).expect("personal key");
    let protected_key = db::protect_current_user_data(&key).expect("protect key");
    let clear_key = db::unprotect_current_user_data(&protected_key).expect("unprotect key");
    assert_eq!(clear_key.as_slice(), key.as_slice());

    fs::remove_dir_all(directory).ok();
}

#[test]
fn personal_fields_round_trip_through_the_derived_key() {
    let directory = temp_dir("personal");
    let key = db::load_or_create_personal_data_key(&directory).expect("personal key");
    let plaintext = "عميل تجريبي 01000000000".as_bytes();

    let ciphertext = db::encrypt_personal_field(&key, plaintext).expect("encrypt");
    assert_ne!(ciphertext.as_slice(), plaintext);
    let clear = db::decrypt_personal_field(&key, &ciphertext).expect("decrypt");
    assert_eq!(clear.as_slice(), plaintext);

    // A different key must not decrypt the field.
    let other_key = [0x11u8; 32];
    assert!(
        db::decrypt_personal_field(&other_key, &ciphertext).is_err(),
        "a different key must not decrypt the field"
    );

    fs::remove_dir_all(directory).ok();
}

#[test]
fn creates_and_restores_an_encrypted_backup() {
    run_with_large_stack(|| {
        let directory = temp_dir("backup");
        eprintln!("[backup] opening the source database");
        let connection = db::open_database(&directory).expect("database opens");
        let destination = directory.join("exports").join("store.bkz");

        eprintln!("[backup] creating the encrypted container");
        let (size, checksum) = backup::create_encrypted_backup(
            &connection,
            &directory,
            &destination,
            "correct horse battery staple",
        )
        .expect("backup is created");
        assert!(size > 0, "backup must not be empty");
        assert_eq!(checksum.len(), 64, "checksum must be a SHA-256 hex digest");

        // The container must not leak the plaintext database.
        eprintln!("[backup] checking the container does not leak plaintext");
        let raw = fs::read(&destination).expect("backup file exists");
        assert!(
            !raw.starts_with(b"SQLite format 3\0"),
            "backup must not contain a plain SQLite header"
        );

        // A wrong password must be rejected.
        eprintln!("[backup] checking that a wrong password is rejected");
        let wrong = backup::prepare_restore(&destination, &directory, "wrong password entirely");
        assert!(
            wrong.is_err(),
            "a wrong password must not restore the backup"
        );

        // The correct password must restore into a usable database.
        eprintln!("[backup] restoring with the correct password");
        let prepared =
            backup::prepare_restore(&destination, &directory, "correct horse battery staple")
                .expect("restore prepares");
        eprintln!("[backup] querying the restored database");
        let branch_count: i64 = prepared
            .connection()
            .query_row("SELECT COUNT(*) FROM branches", [], |row| row.get(0))
            .expect("restored database is queryable");
        assert!(
            branch_count > 0,
            "restored database must contain the seed branch"
        );

        eprintln!("[backup] done");
        fs::remove_dir_all(directory).ok();
    });
}

#[test]
fn migrations_are_applied_once_and_are_idempotent_on_reopen() {
    let directory = temp_dir("migrate");
    {
        let connection = db::open_database(&directory).expect("database opens");
        let version: i64 = connection
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .expect("migration version");
        assert_eq!(version, db::CURRENT_SCHEMA_VERSION);
    }
    {
        let connection = db::open_database(&directory).expect("database reopens");
        let count: i64 = connection
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .expect("migration rows");
        assert_eq!(
            count as i64,
            db::CURRENT_SCHEMA_VERSION,
            "reopening must not re-apply migrations"
        );
        let integrity: String = connection
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))
            .expect("integrity check");
        assert_eq!(integrity, "ok");
    }

    fs::remove_dir_all(directory).ok();
}
