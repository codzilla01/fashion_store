use naseej_pos::AppState;
use tauri::Manager;

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let app_data_dir = app.path().app_data_dir()?;
            let state = AppState::open(app_data_dir).map_err(std::io::Error::other)?;
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            naseej_pos::commands::get_license_status,
            naseej_pos::commands::create_activation_request,
            naseej_pos::commands::import_activation_license,
            naseej_pos::commands::app_status,
            naseej_pos::commands::bootstrap_owner,
            naseej_pos::commands::login,
            naseej_pos::commands::logout,
            naseej_pos::commands::get_catalog,
            naseej_pos::commands::get_catalog_setup_options,
            naseej_pos::commands::create_sale,
            naseej_pos::commands::create_product,
            naseej_pos::commands::get_inventory,
            naseej_pos::commands::adjust_inventory,
            naseej_pos::commands::list_users,
            naseej_pos::commands::create_user,
            naseej_pos::commands::change_user_role,
            naseej_pos::commands::set_user_active,
            naseej_pos::commands::get_branches,
            naseej_pos::commands::create_branch,
            naseej_pos::commands::switch_branch,
            naseej_pos::commands::assign_branch_user,
            naseej_pos::commands::create_customer,
            naseej_pos::commands::get_customer_by_phone,
            naseej_pos::commands::create_supplier,
            naseej_pos::commands::get_suppliers,
            naseej_pos::commands::get_open_purchase_orders,
            naseej_pos::commands::create_purchase_order,
            naseej_pos::commands::receive_goods,
            naseej_pos::commands::get_sales_summary,
            naseej_pos::commands::get_profit_summary,
            naseej_pos::commands::get_return_policy,
            naseej_pos::commands::get_returnable_invoice,
            naseej_pos::commands::record_return,
            naseej_pos::commands::create_exchange,
            naseej_pos::commands::configure_return_policy,
            naseej_pos::commands::configure_tax,
            naseej_pos::owner_panel::owner_panel_status,
            naseej_pos::owner_panel::owner_panel_unlock_with_password,
            naseej_pos::owner_panel::owner_panel_begin_assertion,
            naseej_pos::owner_panel::owner_panel_finish_assertion,
            naseej_pos::owner_panel::owner_panel_begin_registration,
            naseej_pos::owner_panel::owner_panel_finish_registration,
            naseej_pos::owner_panel::owner_panel_credentials,
            naseej_pos::owner_panel::owner_panel_revoke_credential,
            naseej_pos::owner_panel::owner_panel_overview,
            naseej_pos::owner_panel::owner_panel_set_return_policy,
            naseej_pos::owner_panel::owner_panel_set_tax,
            naseej_pos::owner_panel::owner_panel_lock,
            naseej_pos::commands::sqlcipher_health,
            naseej_pos::commands::create_encrypted_backup,
            naseej_pos::commands::restore_encrypted_backup,
            naseej_pos::commands::get_backup_history,
            naseej_pos::commands::print_invoice,
            naseej_pos::commands::preview_invoice_receipt,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Naseej POS");
}
