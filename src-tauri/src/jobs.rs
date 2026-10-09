//! Job queue: a fixed pool of worker threads processes queued jobs, emitting
//! `job-updated` events to the frontend. Finished jobs are appended to a
//! "recent jobs" history file in the app data directory.

use crate::error::{UiError, UiMsg};
use crate::gpu::Accel;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum Operation {
    VideoConvert {
        format: String,
        #[serde(default)]
        preset: Option<String>,
        #[serde(default)]
        codec: Option<String>,
        #[serde(default)]
        accel: Accel,
        #[serde(default)]
        crf: Option<u32>,
        #[serde(default)]
        video_bitrate_k: Option<u32>,
        #[serde(default)]
        audio_bitrate_k: Option<u32>,
        #[serde(default)]
        max_width: Option<u32>,
        #[serde(default)]
        max_height: Option<u32>,
        #[serde(default)]
        target_size_mb: Option<f64>,
    },
    VideoTrim { start: String, #[serde(default)] end: String, #[serde(default)] precise: bool },
    VideoSplit { segment_seconds: f64, #[serde(default)] precise: bool },
    VideoJoin { format: String },
    VideoMux { audio: String },
    VideoCrop { x: u32, y: u32, w: u32, h: u32 },
    VideoDelogo { x: u32, y: u32, w: u32, h: u32, #[serde(default)] authorized: bool },
    VideoRepair,
    AudioConvert {
        format: String,
        #[serde(default)]
        bitrate_k: Option<u32>,
        #[serde(default)]
        sample_rate: Option<u32>,
        #[serde(default)]
        channels: Option<u32>,
    },
    AudioTrim { start: String, #[serde(default)] end: String },
    AudioSplit { segment_seconds: f64 },
    AudioJoin { format: String },
    AudioMix { format: String },
    AudioRepair,
    MediaTags {
        #[serde(default)]
        title: String,
        #[serde(default)]
        artist: String,
        #[serde(default)]
        album: String,
        #[serde(default)]
        year: String,
        #[serde(default)]
        comment: String,
    },
    ImageConvert {
        format: String,
        #[serde(default)]
        quality: Option<u8>,
        #[serde(default)]
        width: Option<u32>,
        #[serde(default)]
        height: Option<u32>,
        #[serde(default)]
        percent: Option<f64>,
        #[serde(default)]
        rotate: Option<i32>,
        #[serde(default)]
        flip_h: bool,
        #[serde(default)]
        flip_v: bool,
        #[serde(default)]
        keep_metadata: bool,
    },
    ImageTags {
        #[serde(default)]
        title: String,
        #[serde(default)]
        artist: String,
        #[serde(default)]
        copyright: String,
        #[serde(default)]
        comment: String,
    },
    PdfMerge,
    PdfConvert { target: String },
    PdfImages { #[serde(default)] as_jpg: bool },
    DvdRip { format: String, #[serde(default)] preset: Option<String>, #[serde(default)] authorized: bool },
    BlurayRip { format: String, #[serde(default)] preset: Option<String>, #[serde(default)] authorized: bool },
    CdRip { format: String, #[serde(default)] bitrate_k: Option<u32> },
    Download { url: String, #[serde(default)] acknowledged: bool },
    ArchiveExtract,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobRequest {
    pub inputs: Vec<String>,
    pub output_dir: String,
    #[serde(default)]
    pub output_name: Option<String>,
    #[serde(default)]
    pub overwrite: bool,
    pub operation: Operation,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Queued,
    Running,
    Done,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: String,
    pub request: JobRequest,
    pub status: Status,
    pub progress: f64,
    pub message: Option<UiMsg>,
    pub outputs: Vec<String>,
    /// What actually processed the job, e.g. "CPU (libx264)" or "GPU (h264_nvenc)".
    pub engine: Option<String>,
    pub created_at: u64,
    pub finished_at: Option<u64>,
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

struct Entry {
    job: Job,
    cancel: Arc<AtomicBool>,
}

struct Inner {
    app: AppHandle,
    entries: Mutex<Vec<Entry>>,
    cv: Condvar,
    recent_path: Option<PathBuf>,
    /// Workers only pick up queued jobs while this is set. Adding jobs never
    /// starts processing; the user presses Start. It resets automatically
    /// when the queue drains, so later additions wait again.
    running: AtomicBool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueState {
    pub running: bool,
}

#[derive(Clone)]
pub struct JobManager(Arc<Inner>);

/// Per-job handle given to processing code.
pub struct Ctx {
    /// `None` only in unit tests, where no app/event bus exists.
    inner: Option<Arc<Inner>>,
    pub id: String,
    cancel: Arc<AtomicBool>,
    last_emit: Mutex<Instant>,
    /// Last engine reported (also kept when there is no app, for tests).
    pub engine_seen: Mutex<Option<String>>,
}

impl Ctx {
    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
    pub fn check(&self) -> crate::error::R<()> {
        if self.is_cancelled() { Err(crate::ui!("errors.cancelled")) } else { Ok(()) }
    }
    pub fn progress(&self, p: f64) {
        let p = p.clamp(0.0, 1.0);
        let mut last = self.last_emit.lock().unwrap();
        let force = p >= 1.0;
        if let Some(i) = &self.inner {
            i.update(&self.id, |j| j.progress = p.max(j.progress));
        }
        if force || last.elapsed().as_millis() >= 200 {
            *last = Instant::now();
            if let Some(i) = &self.inner {
                i.emit(&self.id);
            }
        }
    }
    pub fn stage(&self, msg: UiMsg) {
        if let Some(i) = &self.inner {
            i.update(&self.id, |j| j.message = Some(msg));
            i.emit(&self.id);
        }
    }
    pub fn set_engine(&self, engine: impl Into<String>) {
        let e = engine.into();
        *self.engine_seen.lock().unwrap() = Some(e.clone());
        if let Some(i) = &self.inner {
            i.update(&self.id, |j| j.engine = Some(e));
            i.emit(&self.id);
        }
    }

    #[cfg(test)]
    pub fn cancel_for_test(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    #[cfg(test)]
    pub fn for_test() -> Self {
        Ctx { inner: None, id: "test".into(), cancel: Arc::new(AtomicBool::new(false)), last_emit: Mutex::new(Instant::now()), engine_seen: Mutex::new(None) }
    }
}

impl Inner {
    fn update(&self, id: &str, f: impl FnOnce(&mut Job)) {
        let mut g = self.entries.lock().unwrap();
        if let Some(e) = g.iter_mut().find(|e| e.job.id == id) {
            f(&mut e.job);
        }
    }
    fn emit(&self, id: &str) {
        let job = {
            let g = self.entries.lock().unwrap();
            g.iter().find(|e| e.job.id == id).map(|e| e.job.clone())
        };
        if let Some(job) = job {
            let _ = self.app.emit("job-updated", job);
        }
    }
    fn emit_queue_state(&self) {
        let _ = self.app.emit("queue-state", QueueState { running: self.running.load(Ordering::Relaxed) });
    }

    fn append_recent(&self, job: &Job) {
        let Some(path) = &self.recent_path else { return };
        let mut list: Vec<Job> = std::fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        list.insert(0, job.clone());
        list.truncate(100);
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(bytes) = serde_json::to_vec_pretty(&list) {
            let tmp = path.with_extension("json.tmp");
            if std::fs::write(&tmp, bytes).is_ok() {
                let _ = std::fs::rename(&tmp, path);
            }
        }
    }
}

impl JobManager {
    pub fn new(app: AppHandle, recent_path: Option<PathBuf>, workers: usize) -> Self {
        let inner = Arc::new(Inner { app, entries: Mutex::new(vec![]), cv: Condvar::new(), recent_path, running: AtomicBool::new(false) });
        for _ in 0..workers.max(1) {
            let inner = inner.clone();
            std::thread::spawn(move || worker(inner));
        }
        Self(inner)
    }

    pub fn submit(&self, req: JobRequest) -> String {
        let id = uuid::Uuid::new_v4().simple().to_string();
        let job = Job {
            id: id.clone(),
            request: req,
            status: Status::Queued,
            progress: 0.0,
            message: None,
            outputs: vec![],
            engine: None,
            created_at: now_ms(),
            finished_at: None,
        };
        self.0.entries.lock().unwrap().push(Entry { job, cancel: Arc::new(AtomicBool::new(false)) });
        self.0.emit(&id);
        self.0.cv.notify_one();
        id
    }

    /// Start processing every queued job.
    pub fn start(&self) {
        let has_queued = self.0.entries.lock().unwrap().iter().any(|e| e.job.status == Status::Queued);
        self.0.running.store(has_queued, Ordering::Relaxed);
        self.0.cv.notify_all();
        self.0.emit_queue_state();
    }

    /// Stop picking up new jobs; jobs already converting finish normally.
    pub fn pause(&self) {
        self.0.running.store(false, Ordering::Relaxed);
        self.0.emit_queue_state();
    }

    pub fn state(&self) -> QueueState {
        QueueState { running: self.0.running.load(Ordering::Relaxed) }
    }

    pub fn list(&self) -> Vec<Job> {
        self.0.entries.lock().unwrap().iter().map(|e| e.job.clone()).collect()
    }

    pub fn cancel(&self, id: &str) {
        let mut g = self.0.entries.lock().unwrap();
        if let Some(e) = g.iter_mut().find(|e| e.job.id == id) {
            e.cancel.store(true, Ordering::Relaxed);
            if e.job.status == Status::Queued {
                e.job.status = Status::Cancelled;
                e.job.finished_at = Some(now_ms());
            }
        }
        drop(g);
        self.0.emit(id);
    }

    pub fn retry(&self, id: &str) -> Option<String> {
        let req = {
            let g = self.0.entries.lock().unwrap();
            g.iter()
                .find(|e| e.job.id == id && matches!(e.job.status, Status::Failed | Status::Cancelled))
                .map(|e| e.job.request.clone())
        }?;
        self.remove(id);
        Some(self.submit(req))
    }

    pub fn remove(&self, id: &str) {
        self.0.entries.lock().unwrap().retain(|e| e.job.id != id || e.job.status == Status::Running);
    }

    pub fn clear_finished(&self) {
        self.0
            .entries
            .lock()
            .unwrap()
            .retain(|e| matches!(e.job.status, Status::Queued | Status::Running));
    }

    pub fn recent(&self) -> Vec<Job> {
        self.0
            .recent_path
            .as_ref()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn clear_recent(&self) {
        if let Some(p) = &self.0.recent_path {
            let _ = std::fs::remove_file(p);
        }
    }
}

fn worker(inner: Arc<Inner>) {
    loop {
        let (id, req, cancel) = {
            let mut g = inner.entries.lock().unwrap();
            loop {
                let running = inner.running.load(Ordering::Relaxed);
                if let Some(e) = g.iter_mut().find(|e| running && e.job.status == Status::Queued) {
                    e.job.status = Status::Running;
                    break (e.job.id.clone(), e.job.request.clone(), e.cancel.clone());
                }
                g = inner.cv.wait(g).unwrap();
            }
        };
        inner.emit(&id);
        let ctx = Ctx { inner: Some(inner.clone()), id: id.clone(), cancel, last_emit: Mutex::new(Instant::now()), engine_seen: Mutex::new(None) };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| crate::ops::run(&ctx, &req)))
            .unwrap_or_else(|_| Err(crate::error::generic("internal error (panic)")));
        let cancelled = ctx.is_cancelled();
        inner.update(&id, |j| {
            j.finished_at = Some(now_ms());
            match result {
                Ok(outputs) => {
                    j.status = Status::Done;
                    j.progress = 1.0;
                    j.outputs = outputs.iter().map(|p| p.display().to_string()).collect();
                    // Keep informational messages (e.g. partial repair) from the run.
                    if j.message.as_ref().is_some_and(|m| !m.key.starts_with("result.")) {
                        j.message = None;
                    }
                }
                Err(UiError(_)) if cancelled => {
                    j.status = Status::Cancelled;
                    j.message = None;
                }
                Err(UiError(msg)) => {
                    j.status = Status::Failed;
                    j.message = Some(msg);
                }
            }
        });
        inner.emit(&id);
        let job = inner.entries.lock().unwrap().iter().find(|e| e.job.id == id).map(|e| e.job.clone());
        if let Some(job) = job {
            inner.append_recent(&job);
        }
        // Queue drained: stop, so newly added jobs wait for Start again.
        let busy = inner.entries.lock().unwrap().iter().any(|e| matches!(e.job.status, Status::Queued | Status::Running));
        if !busy && inner.running.swap(false, Ordering::Relaxed) {
            inner.emit_queue_state();
        }
    }
}
