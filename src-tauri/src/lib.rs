//! ConvertHub: a local, cross-platform multimedia conversion toolkit.

pub mod commands;
pub mod deps;
pub mod error;
pub mod ffmpeg;
pub mod fsutil;
pub mod gpu;
pub mod jobs;
pub mod ops;
pub mod presets;
pub mod recorder;
pub mod sync;
pub mod validate;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            deps::set_resource_bin(app.path().resource_dir().ok().map(|d| d.join("bin")));
            let data_dir = app.path().app_data_dir().ok();
            let workers = std::thread::available_parallelism().map(|n| (n.get() / 4).clamp(1, 3)).unwrap_or(1);
            app.manage(jobs::JobManager::new(app.handle().clone(), data_dir, workers));
            app.manage(recorder::Recorder::default());
            // Warm up hardware detection in the background (spawns test encodes).
            std::thread::spawn(|| {
                gpu::info(false);
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::system_info,
            commands::hardware_info,
            commands::validate_inputs,
            commands::check_conflicts,
            commands::submit_jobs,
            commands::start_queue,
            commands::pause_queue,
            commands::queue_state,
            commands::list_jobs,
            commands::cancel_job,
            commands::retry_job,
            commands::remove_job,
            commands::clear_finished,
            commands::recent_jobs,
            commands::clear_recent,
            commands::retry_failed,
            commands::restorable_jobs,
            commands::restore_jobs,
            commands::get_workers,
            commands::set_workers,
            commands::updater_configured,
            commands::probe_media,
            commands::image_info,
            commands::list_presets,
            commands::download_sources,
            commands::format_guide,
            commands::capture_targets,
            commands::start_recording,
            commands::stop_recording,
            commands::recording_state,
            commands::default_output_dir,
            commands::path_exists,
        ])
        .run(tauri::generate_context!())
        .expect("error while running ConvertHub");
}
