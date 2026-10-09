//! Screen recording through FFmpeg's platform capture devices:
//! Windows `gdigrab`, macOS `avfoundation`, Linux X11 `x11grab`.
//!
//! Capture permission is enforced by the OS (macOS asks the user the first
//! time; denial surfaces as an FFmpeg failure we translate). Wayland sessions
//! are reported as unsupported because they require the PipeWire portal.

use crate::deps::{self, Tool};
use crate::error::R;
use crate::fsutil::{plan_output, TempGuard};
use crate::ui;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Child, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Display {
    pub id: String,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Serialize, Clone)]
pub struct Window {
    pub id: String,
    pub title: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Targets {
    pub supported: bool,
    pub reason_key: Option<String>,
    pub window_capture: bool,
    pub displays: Vec<Display>,
    pub windows: Vec<Window>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordOptions {
    /// "display" or "window"
    pub kind: String,
    pub id: String,
    pub fps: u32,
    pub format: String,
    pub output_dir: String,
    pub file_name: String,
    #[serde(default)]
    pub overwrite: bool,
}

struct Active {
    child: Child,
    tmp: TempGuard,
    final_path: PathBuf,
    started: Instant,
}

#[derive(Default)]
pub struct Recorder(Mutex<Option<Active>>);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecState {
    pub recording: bool,
    pub elapsed_secs: f64,
}

fn wayland() -> bool {
    cfg!(target_os = "linux")
        && (std::env::var("XDG_SESSION_TYPE").is_ok_and(|v| v.eq_ignore_ascii_case("wayland"))
            || std::env::var("WAYLAND_DISPLAY").is_ok() && std::env::var("DISPLAY").is_err())
}

pub fn targets(monitors: Vec<Display>) -> Targets {
    if deps::find(Tool::Ffmpeg).is_none() {
        return Targets { supported: false, reason_key: Some("recorder.reasons.noFfmpeg".into()), window_capture: false, displays: vec![], windows: vec![] };
    }
    if wayland() {
        return Targets { supported: false, reason_key: Some("recorder.reasons.wayland".into()), window_capture: false, displays: vec![], windows: vec![] };
    }
    let mut displays = monitors;
    if cfg!(target_os = "macos") {
        displays = mac_screens(&displays);
    }
    Targets {
        supported: !displays.is_empty(),
        reason_key: displays.is_empty().then(|| "recorder.reasons.noDisplays".to_string()),
        window_capture: !cfg!(target_os = "macos"),
        windows: list_windows(),
        displays,
    }
}

/// avfoundation addresses screens by device index ("Capture screen N").
fn mac_screens(monitors: &[Display]) -> Vec<Display> {
    let Some(ff) = deps::find(Tool::Ffmpeg) else { return vec![] };
    let Ok(out) = deps::command(&ff).args(["-hide_banner", "-f", "avfoundation", "-list_devices", "true", "-i", ""]).output() else {
        return vec![];
    };
    let text = String::from_utf8_lossy(&out.stderr);
    let mut v = vec![];
    for line in text.lines() {
        if let Some(pos) = line.find("Capture screen") {
            let idx = line[..pos].rsplit('[').next().and_then(|s| s.split(']').next()).unwrap_or("").trim().to_string();
            let n = v.len();
            let m = monitors.get(n);
            v.push(Display {
                id: idx,
                name: line[pos..].trim().to_string(),
                x: m.map_or(0, |m| m.x),
                y: m.map_or(0, |m| m.y),
                width: m.map_or(0, |m| m.width),
                height: m.map_or(0, |m| m.height),
            });
        }
    }
    v
}

fn list_windows() -> Vec<Window> {
    #[cfg(windows)]
    {
        let ps = "Get-Process | Where-Object { $_.MainWindowTitle } | ForEach-Object { $_.MainWindowTitle }";
        if let Ok(out) = deps::command(std::path::Path::new("powershell")).args(["-NoProfile", "-Command", ps]).output() {
            return String::from_utf8_lossy(&out.stdout)
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(|t| Window { id: t.to_string(), title: t.to_string() })
                .collect();
        }
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(wm) = which::which("wmctrl") {
            if let Ok(out) = deps::command(&wm).arg("-l").output() {
                return String::from_utf8_lossy(&out.stdout)
                    .lines()
                    .filter_map(|l| {
                        // "<id> <desktop> <host> <title...>"
                        let parts: Vec<&str> = l.split_whitespace().collect();
                        let id = parts.first()?.to_string();
                        let title = parts.get(3..).map(|t| t.join(" ")).filter(|t| !t.is_empty())?;
                        Some(Window { id, title })
                    })
                    .collect();
            }
        }
    }
    vec![]
}

fn capture_args(opts: &RecordOptions, displays: &[Display]) -> R<Vec<String>> {
    let fps = opts.fps.clamp(1, 60).to_string();
    let display = || displays.iter().find(|d| d.id == opts.id).ok_or_else(|| ui!("errors.captureTarget"));
    let mut a: Vec<String> = vec![];
    if cfg!(windows) {
        a.extend(["-f", "gdigrab", "-framerate", &fps, "-draw_mouse", "1"].map(String::from));
        if opts.kind == "window" {
            a.extend(["-i".into(), format!("title={}", opts.id)]);
        } else {
            let d = display()?;
            a.extend([
                "-offset_x".into(), d.x.to_string(), "-offset_y".into(), d.y.to_string(),
                "-video_size".into(), format!("{}x{}", d.width, d.height), "-i".into(), "desktop".into(),
            ]);
        }
    } else if cfg!(target_os = "macos") {
        if opts.kind == "window" {
            return Err(ui!("errors.windowCaptureUnsupported"));
        }
        let d = display()?;
        a.extend(["-f", "avfoundation", "-capture_cursor", "1", "-framerate", &fps].map(String::from));
        a.extend(["-i".into(), format!("{}:none", d.id)]);
    } else {
        let disp = std::env::var("DISPLAY").unwrap_or_else(|_| ":0".into());
        a.extend(["-f", "x11grab", "-framerate", &fps, "-draw_mouse", "1"].map(String::from));
        if opts.kind == "window" {
            a.extend(["-window_id".into(), opts.id.clone(), "-i".into(), disp]);
        } else {
            let d = display()?;
            a.extend(["-video_size".into(), format!("{}x{}", d.width, d.height), "-i".into(), format!("{disp}+{},{}", d.x, d.y)]);
        }
    }
    Ok(a)
}

impl Recorder {
    pub fn start(&self, opts: RecordOptions, displays: Vec<Display>) -> R<String> {
        let mut g = self.0.lock().unwrap();
        if g.is_some() {
            return Err(ui!("errors.alreadyRecording"));
        }
        let ff = deps::require(Tool::Ffmpeg)?;
        let format = if opts.format == "mkv" { "mkv" } else { "mp4" };
        let name = crate::fsutil::sanitize_component(opts.file_name.trim());
        let name = if name.is_empty() { "recording".to_string() } else { name };
        let final_path = plan_output(std::path::Path::new(&opts.output_dir), &name, format, opts.overwrite)?;
        let tmp = TempGuard::file_for(&final_path);

        let displays = if cfg!(target_os = "macos") { mac_screens(&displays) } else { displays };
        let mut args: Vec<String> = vec!["-hide_banner".into(), "-loglevel".into(), "error".into(), "-y".into()];
        args.extend(capture_args(&opts, &displays)?);
        let enc = ["libx264", "libopenh264", "h264_mf"]
            .into_iter()
            .find(|e| crate::ffmpeg::has_encoder(e))
            .unwrap_or("mpeg4");
        args.extend(["-c:v".into(), enc.into()]);
        if enc == "libx264" {
            args.extend(["-preset", "ultrafast", "-crf", "23"].map(String::from));
        }
        args.extend(["-pix_fmt", "yuv420p", "-vf", "scale=trunc(iw/2)*2:trunc(ih/2)*2"].map(String::from));
        args.push(tmp.path.display().to_string());

        let mut child = deps::command(&ff)
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| ui!("errors.spawnFailed", "tool" => "ffmpeg", "detail" => e))?;

        // Permission problems and bad targets make FFmpeg exit immediately.
        std::thread::sleep(Duration::from_millis(1200));
        if let Ok(Some(_)) = child.try_wait() {
            let mut err = String::new();
            if let Some(mut s) = child.stderr.take() {
                let _ = s.read_to_string(&mut err);
            }
            let detail: String = err.lines().rev().take(4).collect::<Vec<_>>().join(" | ");
            return Err(ui!("errors.captureFailed", "detail" => detail));
        }
        // Drain stderr so FFmpeg never blocks on a full pipe.
        if let Some(mut s) = child.stderr.take() {
            std::thread::spawn(move || {
                let mut sink = vec![];
                let _ = s.read_to_end(&mut sink);
            });
        }
        let path = final_path.display().to_string();
        *g = Some(Active { child, tmp, final_path, started: Instant::now() });
        Ok(path)
    }

    pub fn stop(&self) -> R<String> {
        let mut active = self.0.lock().unwrap().take().ok_or_else(|| ui!("errors.notRecording"))?;
        if let Some(stdin) = active.child.stdin.as_mut() {
            let _ = stdin.write_all(b"q");
            let _ = stdin.flush();
        }
        drop(active.child.stdin.take());
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            match active.child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(100)),
                _ => {
                    let _ = active.child.kill();
                    let _ = active.child.wait();
                    break;
                }
            }
        }
        let ok = std::fs::metadata(&active.tmp.path).map(|m| m.len() > 0).unwrap_or(false);
        if !ok {
            return Err(ui!("errors.recordingEmpty"));
        }
        let final_path = active.final_path.clone();
        active.tmp.commit(&final_path)?;
        Ok(final_path.display().to_string())
    }

    pub fn state(&self) -> RecState {
        let g = self.0.lock().unwrap();
        RecState { recording: g.is_some(), elapsed_secs: g.as_ref().map_or(0.0, |a| a.started.elapsed().as_secs_f64()) }
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        if let Ok(mut g) = self.0.lock() {
            if let Some(mut a) = g.take() {
                let _ = a.child.kill();
            }
        }
    }
}
