//! Naseej POS application library.
//!
//! The binary crate (`main.rs`) is a thin Tauri shell. Everything that touches
//! the encrypted database, the licensing state, or the backup format lives here
//! so that integration tests can exercise the real code paths on a real
//! Windows machine (SQLCipher, DPAPI, `.bkz` backup/restore) instead of a
//! mock.

pub mod backup;
pub mod cbor;
pub mod commands;
pub mod db;
pub mod import_parse;
pub mod imports;
pub mod license;
pub mod loyalty;
pub mod owner_panel;
pub mod printing;
pub mod webauthn;

/// Re-exported so integration tests can query the encrypted database
/// without adding a second `rusqlite` dependency.
pub use rusqlite;

use rusqlite::Connection;
use std::{collections::HashMap, path::PathBuf, sync::Mutex, time::Instant};
use zeroize::Zeroizing;

pub struct Session {
    pub user_id: String,
    pub branch_id: String,
    pub expires_at: Instant,
}

pub struct AppState {
    pub db: Mutex<Connection>,
    pub sessions: Mutex<HashMap<String, Session>>,
    /// The sessions of the owner control panel.
    ///
    /// They are kept apart from the working sessions on purpose: the panel
    /// is a separate surface with its own lifetime, and a panel session must
    /// never be mistaken for a signed in cashier.
    pub owner_sessions: Mutex<HashMap<String, owner_panel::OwnerSession>>,
    /// The failed unlock attempts of the owner control panel.
    pub owner_attempts: Mutex<owner_panel::OwnerAttempts>,
    pub app_data_dir: PathBuf,
    pub personal_data_key: Zeroizing<[u8; 32]>,
}

impl AppState {
    /// Build the shared application state from a directory.
    ///
    /// This is the single place where the encrypted database and the
    /// DPAPI-protected personal-data key are opened, so both the Tauri shell
    /// and the integration tests exercise identical startup behaviour.
    pub fn open(app_data_dir: PathBuf) -> Result<Self, String> {
        let db = db::open_database(&app_data_dir)?;
        let personal_data_key = db::load_or_create_personal_data_key(&app_data_dir)?;
        let state = Self {
            db: Mutex::new(db),
            sessions: Mutex::new(HashMap::new()),
            owner_sessions: Mutex::new(HashMap::new()),
            owner_attempts: Mutex::new(owner_panel::OwnerAttempts::default()),
            app_data_dir,
            personal_data_key: Zeroizing::new(personal_data_key),
        };
        // Record the start of the local evaluation once, at first launch.
        // Doing it here keeps the licence check itself read-only on the hot
        // path, so it never has to take the database lock while a command
        // already holds it.
        license::initialize_local_trial(&state)?;
        Ok(state)
    }
}
