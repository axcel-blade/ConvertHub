//! FFmpeg/ffprobe process management: running with live progress and
//! cancellation, probing inputs, and listing available encoders.

use crate::deps::{self, Tool};
use crate::error::R;
use crate::jobs::Ctx;
use crate::ui;
use serde::Serialize;
use std::collections::VecDeque;
use std::ffi::OsString;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Stdio};
use std::sync::mpsc;
use std::sync::OnceLock;
use std::time::Duration;

/// Generic external-process runner used by every tool integration.
/// `on_line` receives each stdout line (FFmpeg `-progress` output, yt-dlp
/// progress lines, ...). Cancellation kills the child process.
pub fn run_process(
    ctx: &Ctx,
    program: &Path,
    args: &[OsString],
    on_line: impl FnMut(&str),
    timeout: Option<Duration>,
) -> R<Vec<String>> {
    run_process_in(ctx, program, args, None, on_line, timeout)
}

/// [`run_process`] with an explicit working directory for the child.
pub fn run_process_in(
    ctx: &Ctx,
    program: &Path,
    args: &[OsString],
    cwd: Option<&Path>,
    mut on_line: impl FnMut(&str),
    timeout: Option<Duration>,
) -> R<Vec<String>> {
    let mut cmd = deps::command(program);
    if let Some(d) = cwd {
        cmd.current_dir(d);
    }
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child: Child = cmd.spawn().map_err(|e| ui!("errors.spawnFailed", "tool" => program.display(), "detail" => e))?;

    let stdout = child.stdout.take().expect("piped");
    let stderr = child.stderr.take().expect("piped");
    let (tx, rx) = mpsc::channel::<String>();
    let out_thread = std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let err_thread = std::thread::spawn(move || {
        let mut tail = VecDeque::with_capacity(40);
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            if tail.len() == 40 {
                tail.pop_front();
            }
            tail.push_back(line);
        }
        tail.into_iter().collect::<Vec<_>>()
    });

    let started = std::time::Instant::now();
    loop {
        match rx.recv_timeout(Duration::from_millis(150)) {
            Ok(line) => on_line(&line),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        if ctx.is_cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(ui!("errors.cancelled"));
        }
        if let Some(t) = timeout {
            if started.elapsed() > t {
                let _ = child.kill();
                let _ = child.wait();
                return Err(ui!("errors.timeout", "seconds" => t.as_secs()));
            }
        }
    }
    let status = child.wait()?;
    let _ = out_thread.join();
    let tail = err_thread.join().unwrap_or_default();
    if ctx.is_cancelled() {
        return Err(ui!("errors.cancelled"));
    }
    if !status.success() {
        return Err(classify_failure(program, &tail));
    }
    Ok(tail)
}

fn classify_failure(program: &Path, tail: &[String]) -> crate::error::UiError {
    let text = tail.join("\n");
    let last: String = tail.iter().rev().take(6).rev().cloned().collect::<Vec<_>>().join("\n");
    let tool = program.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    if text.contains("Unknown encoder") || text.contains("Encoder not found") {
        ui!("errors.encoderMissing", "detail" => last)
    } else if text.contains("moov atom not found")
        || text.contains("Invalid data found when processing input")
        || text.contains("could not find codec parameters")
    {
        ui!("errors.corruptInput", "detail" => last)
    } else if text.contains("Permission denied") {
        ui!("errors.permissionDenied", "detail" => last)
    } else if text.contains("No space left") {
        ui!("errors.diskFull")
    } else {
        ui!("errors.toolFailed", "tool" => tool, "detail" => last)
    }
}

/// Run FFmpeg, reporting progress over `[from, to]` of the job's bar.
pub fn run(ctx: &Ctx, args: Vec<OsString>, duration: Option<f64>, from: f64, to: f64) -> R<()> {
    let ffmpeg = deps::require(Tool::Ffmpeg)?;
    let mut full: Vec<OsString> =
        ["-hide_banner", "-nostdin", "-y", "-progress", "pipe:1", "-nostats"].iter().map(OsString::from).collect();
    full.extend(args);
    run_process(
        ctx,
        &ffmpeg,
        &full,
        |line| {
            if let (Some(d), Some(v)) = (duration, line.strip_prefix("out_time_us=")) {
                if let Ok(us) = v.trim().parse::<f64>() {
                    if d > 0.0 {
                        let frac = (us / 1_000_000.0 / d).clamp(0.0, 1.0);
                        ctx.progress(from + (to - from) * frac);
                    }
                }
            }
        },
        None,
    )?;
    ctx.progress(to);
    Ok(())
}

pub fn os(v: &[&str]) -> Vec<OsString> {
    v.iter().map(OsString::from).collect()
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Probe {
    pub duration: Option<f64>,
    pub has_video: bool,
    pub has_audio: bool,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub format_name: Option<String>,
}

pub fn probe(path: &Path) -> R<Probe> {
    let ffprobe = deps::require(Tool::Ffprobe)?;
    let out = deps::command(&ffprobe)
        .args(["-v", "error", "-print_format", "json", "-show_format", "-show_streams"])
        .arg(path)
        .output()?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(ui!("errors.corruptInput", "detail" => err));
    }
    let v: serde_json::Value = serde_json::from_slice(&out.stdout)?;
    let mut p = Probe {
        duration: v["format"]["duration"].as_str().and_then(|s| s.parse().ok()),
        format_name: v["format"]["format_name"].as_str().map(String::from),
        ..Default::default()
    };
    for s in v["streams"].as_array().into_iter().flatten() {
        match s["codec_type"].as_str() {
            Some("video") if !p.has_video && s["disposition"]["attached_pic"].as_i64() != Some(1) => {
                p.has_video = true;
                p.width = s["width"].as_u64().map(|x| x as u32);
                p.height = s["height"].as_u64().map(|x| x as u32);
                p.video_codec = s["codec_name"].as_str().map(String::from);
            }
            Some("audio") if !p.has_audio => {
                p.has_audio = true;
                p.audio_codec = s["codec_name"].as_str().map(String::from);
            }
            _ => {}
        }
        if p.duration.is_none() {
            p.duration = s["duration"].as_str().and_then(|s| s.parse().ok());
        }
    }
    Ok(p)
}

static ENCODERS: OnceLock<Vec<String>> = OnceLock::new();

/// Encoder names compiled into the detected FFmpeg build.
pub fn encoders() -> &'static [String] {
    ENCODERS.get_or_init(|| {
        let Some(ff) = deps::find(Tool::Ffmpeg) else { return vec![] };
        let Ok(out) = deps::command(&ff).args(["-hide_banner", "-encoders"]).output() else { return vec![] };
        parse_encoders(&String::from_utf8_lossy(&out.stdout))
    })
}

fn parse_encoders(text: &str) -> Vec<String> {
    let mut seen_sep = false;
    let mut v = vec![];
    for line in text.lines() {
        if line.trim_start().starts_with("------") {
            seen_sep = true;
            continue;
        }
        if !seen_sep {
            continue;
        }
        let mut parts = line.split_whitespace();
        if let (Some(_flags), Some(name)) = (parts.next(), parts.next()) {
            v.push(name.to_string());
        }
    }
    v
}

pub fn has_encoder(name: &str) -> bool {
    encoders().iter().any(|e| e == name)
}

pub fn require_encoder(name: &str) -> R<()> {
    if encoders().is_empty() || has_encoder(name) {
        // If the list could not be read, let FFmpeg report the problem itself.
        Ok(())
    } else {
        Err(ui!("errors.encoderMissing", "detail" => name))
    }
}

/// Parse "90", "1:30", "01:02:03.5" into seconds.
pub fn parse_time(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let mut total = 0.0;
    for part in s.split(':') {
        let v: f64 = part.trim().parse().ok()?;
        if v < 0.0 {
            return None;
        }
        total = total * 60.0 + v;
    }
    Some(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times() {
        assert_eq!(parse_time("90"), Some(90.0));
        assert_eq!(parse_time("1:30"), Some(90.0));
        assert_eq!(parse_time("01:00:01.5"), Some(3601.5));
        assert_eq!(parse_time("abc"), None);
        assert_eq!(parse_time(""), None);
    }

    #[test]
    fn encoder_list() {
        let text = "Encoders:\n V..... = Video\n ------\n V....D libx264  H.264\n A....D aac   AAC\n";
        assert_eq!(parse_encoders(text), vec!["libx264", "aac"]);
    }
}
