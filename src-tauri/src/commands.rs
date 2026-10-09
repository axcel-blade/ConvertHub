//! Tauri commands exposed to the frontend. Anything that may block (process
//! spawning, file probing) runs on a blocking thread so the UI stays fluid.

use crate::deps;
use crate::error::R;
use crate::gpu::{self, HwInfo};
use crate::jobs::{Job, JobManager, JobRequest};
use crate::recorder::{self, Recorder};
use crate::validate::{self, Category, InputCheck};
use crate::{ffmpeg, ops, presets};
use serde::Serialize;
use tauri::{AppHandle, Manager, State};

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> R<T> + Send + 'static) -> R<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(crate::error::generic)?
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemInfo {
    os: &'static str,
    arch: &'static str,
    version: &'static str,
    deps: Vec<deps::DepStatus>,
}

#[tauri::command]
pub async fn system_info() -> R<SystemInfo> {
    blocking(|| {
        Ok(SystemInfo {
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
            version: env!("CARGO_PKG_VERSION"),
            deps: deps::status_all(),
        })
    })
    .await
}

#[tauri::command]
pub async fn hardware_info(refresh: bool) -> R<HwInfo> {
    blocking(move || Ok(gpu::info(refresh))).await
}

#[tauri::command]
pub async fn validate_inputs(paths: Vec<String>, category: Category) -> R<Vec<InputCheck>> {
    blocking(move || Ok(validate::check_many(&paths, category))).await
}

#[tauri::command]
pub fn check_conflicts(requests: Vec<JobRequest>) -> Vec<String> {
    requests.iter().flat_map(ops::conflicts).collect()
}

#[tauri::command]
pub fn submit_jobs(jobs: State<'_, JobManager>, requests: Vec<JobRequest>) -> Vec<String> {
    requests.into_iter().map(|r| jobs.submit(r)).collect()
}

#[tauri::command]
pub fn start_queue(jobs: State<'_, JobManager>) {
    jobs.start()
}

#[tauri::command]
pub fn pause_queue(jobs: State<'_, JobManager>) {
    jobs.pause()
}

#[tauri::command]
pub fn queue_state(jobs: State<'_, JobManager>) -> crate::jobs::QueueState {
    jobs.state()
}

#[tauri::command]
pub fn list_jobs(jobs: State<'_, JobManager>) -> Vec<Job> {
    jobs.list()
}

#[tauri::command]
pub fn cancel_job(jobs: State<'_, JobManager>, id: String) {
    jobs.cancel(&id)
}

#[tauri::command]
pub fn retry_job(jobs: State<'_, JobManager>, id: String) -> Option<String> {
    jobs.retry(&id)
}

#[tauri::command]
pub fn remove_job(jobs: State<'_, JobManager>, id: String) {
    jobs.remove(&id)
}

#[tauri::command]
pub fn clear_finished(jobs: State<'_, JobManager>) {
    jobs.clear_finished()
}

#[tauri::command]
pub fn recent_jobs(jobs: State<'_, JobManager>) -> Vec<Job> {
    jobs.recent()
}

#[tauri::command]
pub fn clear_recent(jobs: State<'_, JobManager>) {
    jobs.clear_recent()
}

#[tauri::command]
pub async fn probe_media(path: String) -> R<ffmpeg::Probe> {
    blocking(move || ffmpeg::probe(std::path::Path::new(&path))).await
}

#[tauri::command]
pub async fn image_info(path: String) -> R<ops::image::ImageInfo> {
    blocking(move || {
        validate::check_file(std::path::Path::new(&path), Category::Image)?;
        ops::image::info(std::path::Path::new(&path))
    })
    .await
}

#[tauri::command]
pub fn list_presets() -> &'static [presets::Preset] {
    presets::PRESETS
}

#[tauri::command]
pub fn download_sources() -> &'static [ops::download::Source] {
    ops::download::SOURCES
}

#[derive(Serialize)]
pub struct FormatGuide {
    video_in: &'static [&'static str],
    video_out: &'static [&'static str],
    audio_in: &'static [&'static str],
    audio_out: &'static [&'static str],
    image_in: &'static [&'static str],
    image_out: &'static [&'static str],
    archive_in: &'static [&'static str],
    pdf_out: &'static [&'static str],
}

#[tauri::command]
pub fn format_guide() -> FormatGuide {
    FormatGuide {
        video_in: validate::VIDEO_IN,
        video_out: validate::VIDEO_OUT,
        audio_in: validate::AUDIO_IN,
        audio_out: validate::AUDIO_OUT,
        image_in: validate::IMAGE_IN,
        image_out: validate::IMAGE_OUT,
        archive_in: validate::ARCHIVE_IN,
        pdf_out: &["txt", "docx", "doc", "xlsx", "xls", "html", "htm", "jpg/png (images)"],
    }
}

fn monitors(app: &AppHandle) -> Vec<recorder::Display> {
    let Some(w) = app.get_webview_window("main") else { return vec![] };
    w.available_monitors()
        .unwrap_or_default()
        .into_iter()
        .enumerate()
        .map(|(i, m)| recorder::Display {
            id: i.to_string(),
            name: m.name().cloned().unwrap_or_else(|| format!("Display {}", i + 1)),
            x: m.position().x,
            y: m.position().y,
            width: m.size().width,
            height: m.size().height,
        })
        .collect()
}

#[tauri::command]
pub async fn capture_targets(app: AppHandle) -> R<recorder::Targets> {
    let mons = monitors(&app);
    blocking(move || Ok(recorder::targets(mons))).await
}

#[tauri::command]
pub async fn start_recording(app: AppHandle, options: recorder::RecordOptions) -> R<String> {
    let mons = monitors(&app);
    blocking(move || app.state::<Recorder>().start(options, mons)).await
}

#[tauri::command]
pub async fn stop_recording(app: AppHandle) -> R<String> {
    blocking(move || app.state::<Recorder>().stop()).await
}

#[tauri::command]
pub fn recording_state(rec: State<'_, Recorder>) -> recorder::RecState {
    rec.state()
}

#[tauri::command]
pub fn default_output_dir(app: AppHandle) -> Option<String> {
    let p = app.path();
    let base = p.video_dir().or_else(|_| p.download_dir()).or_else(|_| p.home_dir()).ok()?;
    // A dedicated subfolder keeps results out of the user's own videos.
    let dir = base.join("ConvertHub");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.display().to_string())
}

#[tauri::command]
pub fn path_exists(path: String) -> bool {
    std::path::Path::new(&path).exists()
}
