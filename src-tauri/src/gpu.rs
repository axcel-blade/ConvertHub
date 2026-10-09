//! Hardware-acceleration detection.
//!
//! An encoder being *compiled into* FFmpeg says nothing about whether a
//! compatible GPU and driver exist, so each candidate is verified with a tiny
//! real test encode. Only verified encoders are offered to the user.

use crate::deps::{self, Tool};
use crate::ffmpeg;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Accel {
    #[default]
    Auto,
    Cpu,
    Gpu,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuEncoder {
    pub name: String,
    /// "h264" or "hevc"
    pub codec: String,
    pub vendor: String,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HwInfo {
    pub ffmpeg_found: bool,
    pub hwaccels: Vec<String>,
    /// Compiled-in hardware encoders (not necessarily usable).
    pub compiled: Vec<String>,
    /// Encoders that passed a real test encode on this machine.
    pub verified: Vec<GpuEncoder>,
}

const CANDIDATES: &[(&str, &str, &str)] = &[
    ("h264_nvenc", "h264", "NVIDIA NVENC"),
    ("hevc_nvenc", "hevc", "NVIDIA NVENC"),
    ("h264_qsv", "h264", "Intel Quick Sync"),
    ("hevc_qsv", "hevc", "Intel Quick Sync"),
    ("h264_amf", "h264", "AMD AMF"),
    ("hevc_amf", "hevc", "AMD AMF"),
    ("h264_videotoolbox", "h264", "Apple VideoToolbox"),
    ("hevc_videotoolbox", "hevc", "Apple VideoToolbox"),
    ("h264_vaapi", "h264", "VA-API"),
    ("hevc_vaapi", "hevc", "VA-API"),
];

static CACHE: Mutex<Option<HwInfo>> = Mutex::new(None);

pub fn info(refresh: bool) -> HwInfo {
    let mut g = CACHE.lock().unwrap();
    if refresh || g.is_none() {
        *g = Some(detect());
    }
    g.clone().unwrap()
}

pub const VAAPI_DEVICE: &str = "/dev/dri/renderD128";

/// Extra FFmpeg arguments an encoder needs *before* the inputs / as filters.
pub fn pre_input_args(encoder: &str) -> Vec<String> {
    if encoder.ends_with("_vaapi") {
        vec!["-vaapi_device".into(), VAAPI_DEVICE.into()]
    } else {
        vec![]
    }
}

pub fn upload_filter(encoder: &str) -> Option<&'static str> {
    encoder.ends_with("_vaapi").then_some("format=nv12,hwupload")
}

/// Quality arguments mapped from a CRF-like 0-51 value (lower = better).
pub fn quality_args(encoder: &str, crf: u32) -> Vec<String> {
    let crf = crf.min(51);
    let s = crf.to_string();
    if encoder.contains("nvenc") {
        vec!["-rc".into(), "vbr".into(), "-cq".into(), s, "-b:v".into(), "0".into()]
    } else if encoder.contains("qsv") {
        vec!["-global_quality".into(), s]
    } else if encoder.contains("amf") {
        vec!["-rc".into(), "cqp".into(), "-qp_i".into(), s.clone(), "-qp_p".into(), s]
    } else if encoder.contains("videotoolbox") {
        // VideoToolbox uses 1-100 (higher = better).
        let q = (100 - (crf * 100 / 51)).clamp(1, 100);
        vec!["-q:v".into(), q.to_string()]
    } else if encoder.contains("vaapi") {
        vec!["-qp".into(), s]
    } else {
        vec!["-crf".into(), s]
    }
}

fn detect() -> HwInfo {
    let Some(ff) = deps::find(Tool::Ffmpeg) else { return HwInfo::default() };
    let hwaccels = deps::command(&ff)
        .args(["-hide_banner", "-hwaccels"])
        .output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .skip(1)
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect()
        })
        .unwrap_or_default();

    let compiled: Vec<String> =
        CANDIDATES.iter().map(|c| c.0.to_string()).filter(|n| ffmpeg::has_encoder(n)).collect();

    let verified = CANDIDATES
        .iter()
        .filter(|(n, _, _)| compiled.iter().any(|c| c == n))
        .filter(|(n, _, _)| test_encode(&ff, n))
        .map(|(n, c, v)| GpuEncoder { name: n.to_string(), codec: c.to_string(), vendor: v.to_string() })
        .collect();

    HwInfo { ffmpeg_found: true, hwaccels, compiled, verified }
}

fn test_encode(ff: &std::path::Path, encoder: &str) -> bool {
    let mut args: Vec<String> = vec!["-hide_banner".into(), "-loglevel".into(), "error".into()];
    args.extend(pre_input_args(encoder));
    args.extend(
        ["-f", "lavfi", "-i", "color=c=black:s=256x256:r=25:d=0.2"].iter().map(|s| s.to_string()),
    );
    if let Some(f) = upload_filter(encoder) {
        args.extend(["-vf".into(), f.into()]);
    }
    args.extend(["-c:v", encoder, "-frames:v", "5", "-f", "null", "-"].iter().map(|s| s.to_string()));
    let mut child = match deps::command(ff)
        .args(&args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return false,
    };
    // Broken drivers can hang; give each probe a bounded time.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        match child.try_wait() {
            Ok(Some(st)) => return st.success(),
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(50))
            }
            _ => {
                let _ = child.kill();
                return false;
            }
        }
    }
}

/// Pick the verified GPU encoder for `codec` ("h264"/"hevc"), if any.
pub fn pick(codec: &str) -> Option<String> {
    info(false).verified.into_iter().find(|e| e.codec == codec).map(|e| e.name)
}
