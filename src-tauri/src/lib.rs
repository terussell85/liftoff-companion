pub mod app_state;
pub mod capture;
pub mod commands;
pub mod error;
pub mod gamelog;
pub mod liftoff;
pub mod processing;
pub mod storage;
pub mod telemetry;

use tauri::Manager;

use crate::app_state::AppState;
use crate::error::AppError;
use crate::storage::db;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .try_init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let data_dir = app
                .path()
                .app_data_dir()
                .map_err(|e| AppError::Other(format!("app_data_dir: {}", e)))?;
            std::fs::create_dir_all(&data_dir)?;
            let captures_dir = data_dir.join("captures");
            std::fs::create_dir_all(&captures_dir)?;
            let db_path = data_dir.join("whoop.db");
            let pool = db::open_pool(&db_path)?;
            let state = AppState::new(pool, data_dir, captures_dir);
            app.manage(state);
            // Blackbox-style auto capture: arm the supervisor at launch so
            // telemetry starts a capture the moment Liftoff begins streaming.
            crate::capture::auto::ensure_started(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::capture_commands::start_capture,
            commands::capture_commands::stop_capture,
            commands::capture_commands::add_capture_marker,
            commands::capture_commands::list_captures,
            commands::capture_commands::get_capture,
            commands::capture_commands::update_capture_context,
            commands::capture_commands::current_capture,
            commands::capture_commands::list_race_sessions,
            commands::capture_commands::delete_capture,
            commands::capture_commands::delete_race_session,
            commands::capture_commands::get_auto_capture,
            commands::capture_commands::set_auto_capture,
            commands::processing_commands::list_processing_profiles,
            commands::processing_commands::process_capture,
            commands::processing_commands::get_processing_job,
            commands::processing_commands::list_processed_datasets,
            commands::processing_commands::get_dataset_detail,
            commands::processing_commands::get_session_timing_detail,
            commands::setup_commands::find_liftoff_dirs,
            commands::setup_commands::get_setup_snapshot,
            commands::setup_commands::read_telemetry_config,
            commands::setup_commands::apply_recommended_telemetry_config,
            commands::setup_commands::disable_telemetry_config,
            commands::setup_commands::update_network_config,
            commands::setup_commands::list_game_asset_sources,
            commands::setup_commands::list_game_asset_catalog,
            commands::setup_commands::refresh_race_track_cache,
            commands::setup_commands::resolve_session_course,
            commands::setup_commands::resolve_session_collision_geometry,
            commands::setup_commands::run_test_listener,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
