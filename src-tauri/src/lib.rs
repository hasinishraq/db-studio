//! The desktop app: wires the crates together and exposes typed commands.

mod audit_sink;
mod commands;
mod error;
mod logging;
pub(crate) mod setup;
mod state;

use tauri_specta::{collect_commands, collect_events, Builder};

pub use state::AppState;

/// Every command and event the frontend can use.
pub fn specta_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new()
        .commands(collect_commands![
            commands::profiles::list_profiles,
            commands::profiles::save_profile,
            commands::profiles::delete_profile,
            commands::secrets::set_secret,
            commands::secrets::clear_secret,
            commands::sessions::test_connection,
            commands::sessions::connect,
            commands::sessions::list_databases,
            commands::sessions::use_database,
            commands::sessions::disconnect,
            commands::schema::list_schemas,
            commands::schema::list_objects,
            commands::schema::columns,
            commands::schema::table_structure,
            commands::schema::table_page,
            commands::schema::table_count,
            commands::query::prepare_query,
            commands::query::approve_query,
            commands::query::run_query,
            commands::query::cancel_run,
            commands::query::set_tx_mode,
            commands::query::commit_tx,
            commands::query::rollback_tx,
            commands::query::tx_state,
            commands::query::explain_query,
            commands::history::history_list,
            commands::history::history_clear,
            commands::drafts::drafts_list,
            commands::drafts::drafts_save,
            commands::drafts::drafts_delete,
            commands::settings::settings_get,
            commands::settings::settings_set,
            commands::rows::plan_row_change,
            commands::rows::plan_create_table,
            commands::schema_io::export_schema,
            commands::schema_io::open_schema_file,
            commands::schema_io::outline_schema,
            commands::transfer::export_query,
            commands::transfer::export_rows,
            commands::transfer::export_table,
            commands::transfer::import_pick_file,
            commands::transfer::import_plan,
            commands::transfer::import_csv,
            commands::audit::audit_list,
            commands::audit::audit_verify,
            commands::audit::audit_export,
            commands::ai::ai_send,
            commands::ai::ai_review,
            commands::ai::ai_cancel,
            commands::ai::ai_cancel_query,
            commands::ai::ai_forget,
            commands::ai::ai_conversations,
            commands::ai::ai_turns,
            commands::ai::ai_rename,
            commands::ai::ai_test_provider,
            commands::ai::masking_list,
            commands::ai::masking_set,
            commands::ssh::trust_host_key,
            commands::system::diagnostics,
            commands::system::import_legacy,
        ])
        .events(collect_events![state::ConnectionLost, state::SecretStoreWarning])
        // Only counts and millisecond timestamps are 64-bit here, all far below 2^53. Database
        // 64-bit values travel as strings in `Value::BigInt`, so nothing can lose precision.
        .dangerously_cast_bigints_to_number()
        .disable_serde_phases()
}

/// Starts the desktop app.
#[allow(clippy::expect_used)] // documented startup invariants
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = specta_builder();

    #[cfg(debug_assertions)]
    builder
        .export(specta_typescript::Typescript::default(), "../src/bindings.ts")
        .expect("export TypeScript bindings");

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            builder.mount_events(app);
            setup::init(app)?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running AI DB Studio");
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    #[test]
    fn export_bindings() {
        super::specta_builder()
            .export(specta_typescript::Typescript::default(), "../src/bindings.ts")
            .expect("export bindings");
    }
}
