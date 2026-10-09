//! Online media downloads. This is the only feature that uses the network.
//!
//! Supported:
//! * Direct links to media files (http/https), downloaded natively.
//! * A small allowlist of sites whose terms permit downloading, via yt-dlp.
//!
//! Sites whose terms of service prohibit downloading (e.g. YouTube, Youku,
//! Vimeo without an owner-provided download button) are listed but disabled.
//! DRM-protected streams are never supported and no access controls
//! (logins, cookies, geo-blocks, paywalls) are bypassed.

use crate::deps::{self, Tool};
use crate::error::R;
use crate::ffmpeg::run_process;
use crate::fsutil::{ext_lower, plan_output, sanitize_component, TempGuard};
use crate::jobs::Ctx;
use crate::ui;
use crate::validate::{AUDIO_IN, VIDEO_IN};
use serde::Serialize;
use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub id: &'static str,
    pub name: &'static str,
    pub hosts: &'static [&'static str],
    pub enabled: bool,
    /// Translation key explaining the status / limitations.
    pub note_key: &'static str,
}

pub const SOURCES: &[Source] = &[
    Source { id: "direct", name: "Direct media links (http/https)", hosts: &[], enabled: true, note_key: "download.notes.direct" },
    Source { id: "archive", name: "Internet Archive (archive.org)", hosts: &["archive.org", "www.archive.org"], enabled: true, note_key: "download.notes.archive" },
    Source { id: "wikimedia", name: "Wikimedia Commons (file links)", hosts: &["upload.wikimedia.org"], enabled: true, note_key: "download.notes.wikimedia" },
    Source { id: "youtube", name: "YouTube", hosts: &["youtube.com", "www.youtube.com", "m.youtube.com", "youtu.be"], enabled: false, note_key: "download.notes.youtube" },
    Source { id: "youku", name: "Youku", hosts: &["youku.com", "v.youku.com"], enabled: false, note_key: "download.notes.youku" },
    Source { id: "vimeo", name: "Vimeo", hosts: &["vimeo.com", "player.vimeo.com"], enabled: false, note_key: "download.notes.vimeo" },
    Source { id: "streaming", name: "Subscription streaming services (Netflix, Disney+, etc.)", hosts: &["netflix.com", "www.netflix.com", "disneyplus.com", "www.disneyplus.com", "primevideo.com", "www.primevideo.com"], enabled: false, note_key: "download.notes.drm" },
];

const MAX_BYTES: u64 = 50 * 1024 * 1024 * 1024;

fn host_of(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let host_port = rest.split(['/', '?', '#']).next()?;
    let host = host_port.rsplit('@').next()?.split(':').next()?;
    Some(host.to_ascii_lowercase())
}

pub fn classify(url: &str) -> R<&'static Source> {
    let lower = url.trim().to_ascii_lowercase();
    if !(lower.starts_with("https://") || lower.starts_with("http://")) {
        return Err(ui!("errors.badUrl"));
    }
    let host = host_of(&lower).filter(|h| !h.is_empty()).ok_or_else(|| ui!("errors.badUrl"))?;
    for s in SOURCES.iter().filter(|s| !s.hosts.is_empty()) {
        if s.hosts.iter().any(|h| host == *h || host.ends_with(&format!(".{h}"))) {
            if !s.enabled {
                return Err(ui!("errors.sourceDisabled", "source" => s.name));
            }
            return Ok(s);
        }
    }
    Ok(&SOURCES[0])
}

pub fn run(ctx: &Ctx, url: &str, acknowledged: bool, out_dir: &Path, overwrite: bool) -> R<Vec<PathBuf>> {
    if !acknowledged {
        return Err(ui!("errors.downloadAck"));
    }
    let source = classify(url)?;
    match source.id {
        "archive" => site_download(ctx, url.trim(), out_dir, overwrite),
        _ => direct_download(ctx, url.trim(), out_dir, overwrite),
    }
}

fn filename_from(url: &str, disposition: Option<&str>) -> String {
    if let Some(d) = disposition {
        if let Some(i) = d.find("filename=") {
            let n = d[i + 9..].trim_matches(|c| c == '"' || c == '\'' || c == ';' || c == ' ');
            let n = n.split(';').next().unwrap_or(n).trim_matches('"');
            if !n.is_empty() {
                return sanitize_component(n);
            }
        }
    }
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let last = path.rsplit('/').next().unwrap_or("download");
    let decoded = percent_decode(last);
    sanitize_component(if decoded.is_empty() { "download" } else { &decoded })
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("");
            if let Ok(v) = u8::from_str_radix(hex, 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

fn direct_download(ctx: &Ctx, url: &str, out_dir: &Path, overwrite: bool) -> R<Vec<PathBuf>> {
    ctx.set_engine("Native (HTTP)");
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(20))
        .timeout(None)
        .user_agent(concat!("ConvertHub/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let mut resp = client.get(url).send().map_err(|e| ui!("errors.network", "detail" => e))?;
    if !resp.status().is_success() {
        return Err(ui!("errors.httpStatus", "status" => resp.status().as_u16()));
    }
    let ctype = resp.headers().get(reqwest::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("").to_ascii_lowercase();
    let disp = resp.headers().get(reqwest::header::CONTENT_DISPOSITION).and_then(|v| v.to_str().ok()).map(String::from);
    let mut name = filename_from(resp.url().as_str(), disp.as_deref());
    let mut ext = ext_lower(Path::new(&name));
    if !(VIDEO_IN.contains(&ext.as_str()) || AUDIO_IN.contains(&ext.as_str())) {
        // Fall back to the content type for extension-less URLs.
        let guessed = match ctype.split(';').next().unwrap_or("") {
            "video/mp4" => "mp4",
            "video/webm" => "webm",
            "video/x-matroska" => "mkv",
            "video/quicktime" => "mov",
            "audio/mpeg" => "mp3",
            "audio/ogg" => "ogg",
            "audio/wav" | "audio/x-wav" => "wav",
            "audio/flac" => "flac",
            "audio/mp4" => "m4a",
            _ => return Err(ui!("errors.notMediaUrl", "type" => if ctype.is_empty() { "?".to_string() } else { ctype.clone() })),
        };
        name = format!("{name}.{guessed}");
        ext = guessed.into();
    }
    if ctype.starts_with("text/html") {
        return Err(ui!("errors.notMediaUrl", "type" => ctype));
    }
    let stem = Path::new(&name).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "download".into());
    let final_path = plan_output(out_dir, &stem, &ext, overwrite)?;
    let total = resp.content_length();
    if total.is_some_and(|t| t > MAX_BYTES) {
        return Err(ui!("errors.tooLarge", "path" => &name, "limitGb" => MAX_BYTES / (1 << 30)));
    }
    let tmp = TempGuard::file_for(&final_path);
    let mut f = std::io::BufWriter::new(std::fs::File::create(&tmp.path)?);
    let mut buf = vec![0u8; 256 * 1024];
    let mut done: u64 = 0;
    loop {
        ctx.check()?;
        let n = resp.read(&mut buf).map_err(|e| ui!("errors.network", "detail" => e))?;
        if n == 0 {
            break;
        }
        f.write_all(&buf[..n])?;
        done += n as u64;
        if done > MAX_BYTES {
            return Err(ui!("errors.tooLarge", "path" => &name, "limitGb" => MAX_BYTES / (1 << 30)));
        }
        if let Some(t) = total.filter(|t| *t > 0) {
            ctx.progress(done as f64 / t as f64);
        }
    }
    f.flush()?;
    drop(f);
    if total.is_some_and(|t| t != done) {
        return Err(ui!("errors.downloadIncomplete"));
    }
    tmp.commit(&final_path)?;
    Ok(vec![final_path])
}

fn site_download(ctx: &Ctx, url: &str, out_dir: &Path, overwrite: bool) -> R<Vec<PathBuf>> {
    let ytdlp = deps::find(Tool::YtDlp).ok_or_else(|| ui!("errors.toolMissing", "tool" => "yt-dlp"))?;
    ctx.set_engine("yt-dlp");
    let work = TempGuard::in_system_temp()?;
    let mut args: Vec<OsString> = vec![
        "--no-playlist".into(),
        "--newline".into(),
        "--no-cookies-from-browser".into(),
        "--restrict-filenames".into(),
        "--progress-template".into(),
        "download:CHPROG %(progress._percent_str)s".into(),
        "-P".into(),
        work.path.clone().into(),
        "-o".into(),
        "%(title).120B.%(ext)s".into(),
    ];
    if let Some(ff) = deps::find(Tool::Ffmpeg) {
        args.push("--ffmpeg-location".into());
        args.push(ff.into());
    }
    args.push("--".into());
    args.push(url.into());
    run_process(
        ctx,
        &ytdlp,
        &args,
        |line| {
            if let Some(p) = line.strip_prefix("CHPROG ") {
                if let Ok(v) = p.trim().trim_end_matches('%').trim().parse::<f64>() {
                    ctx.progress(v / 100.0 * 0.98);
                }
            }
        },
        None,
    )?;
    let mut outputs = vec![];
    for e in std::fs::read_dir(&work.path)?.filter_map(|e| e.ok()) {
        let p = e.path();
        if !p.is_file() {
            continue;
        }
        let stem = crate::fsutil::file_stem(&p);
        let dst = plan_output(out_dir, &stem, &ext_lower(&p), overwrite)?;
        if dst.exists() {
            std::fs::remove_file(&dst)?;
        }
        std::fs::copy(&p, &dst)?;
        outputs.push(dst);
    }
    if outputs.is_empty() {
        return Err(ui!("errors.noOutput"));
    }
    Ok(outputs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classification() {
        assert!(classify("ftp://x/y.mp4").is_err());
        assert!(classify("file:///etc/passwd").is_err());
        assert_eq!(classify("https://www.youtube.com/watch?v=x").unwrap_err().0.key, "errors.sourceDisabled");
        assert_eq!(classify("https://youtu.be/x").unwrap_err().0.key, "errors.sourceDisabled");
        assert_eq!(classify("https://v.youku.com/v_show/id_x").unwrap_err().0.key, "errors.sourceDisabled");
        assert_eq!(classify("https://archive.org/details/x").unwrap().id, "archive");
        assert_eq!(classify("https://example.com/a.mp4").unwrap().id, "direct");
        // Host lookalikes must not match.
        assert_eq!(classify("https://notyoutube.com/a.mp4").unwrap().id, "direct");
        assert_eq!(classify("https://user@www.youtube.com:443/x").unwrap_err().0.key, "errors.sourceDisabled");
    }

    #[test]
    fn filenames() {
        assert_eq!(filename_from("https://h/a/b%20c.mp4?x=1", None), "b c.mp4");
        assert_eq!(filename_from("https://h/x", Some("attachment; filename=\"../evil.mp4\"")), ".._evil.mp4");
        assert_eq!(filename_from("https://h/", None), "download");
    }
}
