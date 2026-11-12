mod commands;
mod config;
mod convert;
mod db;
mod device;
mod dragout;
mod plugins;
mod store;

use rusqlite::Connection;
use serde_json::Value;
use std::{path::PathBuf, sync::Mutex};
use tauri::Manager;

pub struct AppState {
    pub config: Mutex<config::AppConfig>,
    pub config_path: PathBuf,
    pub db: Mutex<Option<Connection>>,
    pub device_info: Mutex<Option<Value>>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let config_dir = app.path().app_config_dir()?;
            std::fs::create_dir_all(&config_dir)?;
            let config_path = config::config_path(&config_dir);
            let config = config::load(&config_path);

            app.manage(AppState {
                config: Mutex::new(config),
                config_path,
                db: Mutex::new(None),
                device_info: Mutex::new(None),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::library_status,
            commands::library_default_path,
            commands::library_list,
            commands::library_create,
            commands::library_open,
            commands::library_remove,
            commands::library_close,
            commands::library_rename,
            commands::library_update_settings,
            commands::tags_list,
            commands::tags_add,
            commands::tags_remove,
            commands::folders_list,
            commands::folders_create,
            commands::folders_update,
            commands::folders_move,
            commands::folders_delete,
            commands::models_move_folder,
            commands::models_list,
            commands::models_upload_by_path,
            commands::models_upload_with_aux,
            commands::models_stage_model,
            commands::models_stage_aux,
            commands::models_overwrite,
            commands::models_batch_upload_by_paths,
            commands::models_read_file_data,
            commands::models_load_file,
            commands::models_add_aux_file,
            commands::models_remove_aux_file,
            commands::models_save_image,
            commands::models_read_image,
            commands::models_save,
            commands::models_update,
            commands::models_delete,
            commands::models_delete_batch,
            commands::models_list_deleted,
            commands::models_restore,
            commands::models_delete_forever,
            commands::models_empty_recycle,
            commands::models_export,
            commands::models_export_batch,
            commands::logs_query,
            commands::logs_export,
            commands::logs_write_file,
            commands::logs_clear,
            commands::logs_get_device_info,
            commands::resources_start_drag,
            plugins::plugin_storage_get,
            plugins::plugin_storage_set,
            plugins::plugin_storage_remove
        ])
        .run(tauri::generate_context!())
        .expect("error while running tsrbox");
}
