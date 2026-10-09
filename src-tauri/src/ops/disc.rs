//! DVD, Blu-ray and audio CD ripping for discs the user is authorized to copy.
//!
//! ConvertHub never bypasses DRM or copy protection (CSS, AACS, BD+): it
//! reads only unencrypted VOB/M2TS streams and audio tracks the OS exposes.
//! Protected discs fail with a clear explanation.

use crate::deps::{self, Tool};
use crate::error::{R, UiMsg};
use crate::ffmpeg::{self, os, run_process_in};
use crate::fsutil::{ext_lower, TempGuard};
use crate::jobs::{Ctx, Operation as Op};
use crate::ops::{audio, video};
use crate::ui;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

pub fn run(ctx: &Ctx, op: &Op, src: &Path, out: &Path) -> R<Vec<PathBuf>> {
    match op {
        Op::DvdRip { format, preset, authorized } => {
            if !authorized {
                return Err(ui!("errors.discAuthorization"));
            }
            let vobs = dvd_main_title(src)?;
            let url = format!("concat:{}", vobs.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join("|"));
            rip_video(ctx, &url, format, preset.as_deref(), out)?;
        }
        Op::BlurayRip { format, preset, authorized } => {
            if !authorized {
                return Err(ui!("errors.discAuthorization"));
            }
            let m2ts = bluray_main_stream(src)?;
            rip_video(ctx, &m2ts.display().to_string(), format, preset.as_deref(), out)?;
        }
        Op::CdRip { format, bitrate_k } => return rip_cd(ctx, src, format, *bitrate_k, out),
        _ => unreachable!(),
    }
    Ok(vec![out.to_path_buf()])
}

fn rip_video(ctx: &Ctx, input: &str, format: &str, preset: Option<&str>, out: &Path) -> R<()> {
    let probe = ffmpeg::probe(Path::new(input)).map_err(|_| ui!("errors.discProtected"))?;
    if !probe.has_video {
        return Err(ui!("errors.discProtected"));
    }
    let enc = video::enc_for(format, preset.filter(|p| !p.is_empty()), &probe)?;
    let tmp = TempGuard::file_for(out);
    let args: Vec<OsString> = vec!["-fflags".into(), "+genpts".into(), "-i".into(), input.into()];
    video::encode(ctx, &args, &enc, &tmp.path, probe.duration, probe.has_audio).map_err(|e| {
        if e.0.key == "errors.corruptInput" { ui!("errors.discProtected") } else { e }
    })?;
    tmp.commit(out)
}

fn find_dir(root: &Path, name: &str) -> Option<PathBuf> {
    if root.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(name)) {
        return Some(root.to_path_buf());
    }
    std::fs::read_dir(root)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.is_dir() && p.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(name)))
}

/// The title set with the most data is almost always the main feature.
/// VTS_NN_0.VOB is the menu and is skipped.
pub fn dvd_main_title(root: &Path) -> R<Vec<PathBuf>> {
    let dir = find_dir(root, "VIDEO_TS").ok_or_else(|| ui!("errors.dvdNotFound"))?;
    let mut sets: std::collections::BTreeMap<String, (u64, Vec<PathBuf>)> = Default::default();
    for e in std::fs::read_dir(&dir)?.filter_map(|e| e.ok()) {
        let name = e.file_name().to_string_lossy().to_uppercase();
        let Some(rest) = name.strip_prefix("VTS_").and_then(|r| r.strip_suffix(".VOB")) else { continue };
        let Some((set, part)) = rest.split_once('_') else { continue };
        if part == "0" {
            continue;
        }
        let size = e.metadata().map(|m| m.len()).unwrap_or(0);
        let entry = sets.entry(set.to_string()).or_default();
        entry.0 += size;
        entry.1.push(e.path());
    }
    let (_, (_, mut files)) = sets.into_iter().max_by_key(|(_, (s, _))| *s).ok_or_else(|| ui!("errors.dvdNotFound"))?;
    files.sort();
    Ok(files)
}

pub fn bluray_main_stream(root: &Path) -> R<PathBuf> {
    let bdmv = find_dir(root, "BDMV").ok_or_else(|| ui!("errors.blurayNotFound"))?;
    let stream = find_dir(&bdmv, "STREAM").ok_or_else(|| ui!("errors.blurayNotFound"))?;
    std::fs::read_dir(stream)?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().to_lowercase().ends_with(".m2ts"))
        .max_by_key(|e| e.metadata().map(|m| m.len()).unwrap_or(0))
        .map(|e| e.path())
        .ok_or_else(|| ui!("errors.blurayNotFound"))
}

fn rip_cd(ctx: &Ctx, src: &Path, format: &str, bitrate_k: Option<u32>, out_dir: &Path) -> R<Vec<PathBuf>> {
    audio::codec_for(format)?;
    let mut tracks: Vec<PathBuf> = std::fs::read_dir(src)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| matches!(ext_lower(p).as_str(), "aiff" | "aif" | "wav" | "flac"))
        .collect();
    let has_cda = std::fs::read_dir(src)?.filter_map(|e| e.ok()).any(|e| ext_lower(&e.path()) == "cda");
    let _rip_tmp; // keeps cdparanoia output alive until conversion is done
    if tracks.is_empty() {
        let Some(cdp) = deps::find(Tool::Cdparanoia) else {
            return Err(if has_cda { ui!("errors.cdUnsupportedPlatform") } else { ui!("errors.cdNoTracks") });
        };
        ctx.stage(UiMsg::new("stage.readingDisc"));
        ctx.set_engine("cdparanoia");
        let tmp = TempGuard::in_system_temp()?;
        // cdparanoia -B writes trackNN.cdda.wav into the working directory.
        let args: Vec<OsString> = vec!["-B".into(), "--".into(), "1-".into()];
        run_process_in(ctx, &cdp, &args, Some(&tmp.path), |_| {}, None)?;
        tracks = std::fs::read_dir(&tmp.path)?.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| ext_lower(p) == "wav").collect();
        _rip_tmp = Some(tmp);
        if tracks.is_empty() {
            return Err(ui!("errors.cdNoTracks"));
        }
    } else {
        _rip_tmp = None;
    }
    tracks.sort();
    let tmp = TempGuard::dir_for(out_dir)?;
    let n = tracks.len();
    for (i, t) in tracks.iter().enumerate() {
        ctx.check()?;
        let dst = tmp.path.join(format!("Track {:02}.{format}", i + 1));
        let mut args = vec!["-i".into(), t.into()];
        args.extend(os(&["-map", "0:a:0", "-vn"]));
        let (enc, lossless) = audio::codec_for(format)?;
        args.extend(os(&["-c:a", video::audio_encoder(enc)]));
        if !lossless {
            args.extend(os(&["-b:a", &format!("{}k", bitrate_k.unwrap_or(256))]));
        }
        args.push(dst.into());
        let d = ffmpeg::probe(t).ok().and_then(|p| p.duration);
        ffmpeg::run(ctx, args, d, i as f64 / n as f64, (i + 1) as f64 / n as f64)?;
    }
    ctx.set_engine(format!("CPU ({})", audio::codec_for(format)?.0));
    tmp.commit(out_dir)?;
    Ok(vec![out_dir.to_path_buf()])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_largest_title_set() {
        let d = tempfile::tempdir().unwrap();
        let v = d.path().join("VIDEO_TS");
        std::fs::create_dir(&v).unwrap();
        std::fs::write(v.join("VTS_01_0.VOB"), vec![0u8; 50]).unwrap();
        std::fs::write(v.join("VTS_01_1.VOB"), vec![0u8; 10]).unwrap();
        std::fs::write(v.join("VTS_02_1.VOB"), vec![0u8; 30]).unwrap();
        std::fs::write(v.join("VTS_02_2.VOB"), vec![0u8; 30]).unwrap();
        let t = dvd_main_title(d.path()).unwrap();
        assert_eq!(t.len(), 2);
        assert!(t[0].ends_with("VTS_02_1.VOB"));
    }
}
