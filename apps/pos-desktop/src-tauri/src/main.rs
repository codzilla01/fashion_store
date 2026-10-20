mod backup;
mod commands;
mod db;
mod license;

use rusqlite::Connection;
use std::{collections::HashMap, path::PathBuf, sync::Mutex, time::Instant};
use tauri::Manager;
use zeroize::Zeroizing;

pub struct Session {
    pub user_id: String,
    pub branch_id: String,
    pub expires_at: Instant,
}

pub struct AppState {
    pub db: Mutex<Connection>,
    pub sessions: Mutex<HashMap<String, Session>>,
    pub app_data_dir: PathBuf,
    pub personal_data_key: Zeroizing<[u8; 32]>,
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let app_data_dir = app.path().app_data_dir()?;
            let db = db::open_database(&app_data_dir).map_err(std::io::Error::other)?;
            let personal_data_key = db::load_or_create_personal_data_key(&app_data_dir)
                .map_err(std::io::Error::other)?;
            app.manage(AppState {
                db: Mutex::new(db),
                sessions: Mutex::new(HashMap::new()),
                app_data_dir,
                personal_data_key: Zeroizing::new(personal_data_key),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_license_status,
            commands::create_activation_request,
            commands::import_activation_license,
            commands::app_status,
            commands::bootstrap_owner,
            commands::login,
            commands::logout,
            commands::get_catalog,
            commands::get_catalog_setup_options,
            commands::create_sale,
            commands::create_product,
            commands::get_inventory,
            commands::adjust_inventory,
            commands::list_users,
            commands::create_user,
            commands::change_user_role,
            commands::set_user_active,
            commands::get_branches,
            commands::create_branch,
            commands::switch_branch,
            commands::assign_branch_user,
            commands::create_customer,
            commands::get_customer_by_phone,
            commands::create_supplier,
            commands::get_suppliers,
            commands::get_open_purchase_orders,
            commands::create_purchase_order,
            commands::receive_goods,
            commands::get_sales_summary,
            commands::get_profit_summary,
            commands::get_returnable_invoice,
            commands::record_return,
            commands::create_exchange,
            commands::configure_return_policy,
            commands::configure_tax,
            commands::sqlcipher_health,
            commands::create_encrypted_backup,
            commands::restore_encrypted_backup,
            commands::get_backup_history,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Naseej POS");
}
