//! Audio operations: convert, trim, split, join, mix, repair.

use crate::error::{R, UiMsg};
use crate::ffmpeg::{self, os};
use crate::fsutil::{ext_lower, TempGuard};
use crate::jobs::{Ctx, Operation as Op};
use crate::ops::video::{audio_encoder, split, trim_range};
use crate::ui;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// (encoder, lossless) for an output extension.
pub fn codec_for(ext: &str) -> R<(&'static str, bool)> {
    Ok(match ext {
        "mp3" => ("libmp3lame", false),
        "m4a" | "aac" => ("aac", false),
        "ogg" | "oga" => ("libvorbis", false),
        "opus" => ("libopus", false),
        "flac" => ("flac", true),
        "wav" => ("pcm_s16le", true),
        "aiff" | "aif" => ("pcm_s16be", true),
        "wma" => ("wmav2", false),
        "ac3" => ("ac3", false),
        "mka" => ("flac", true),
        "mp2" => ("mp2", false),
        other => return Err(ui!("errors.unsupportedOutput", "format" => other)),
    })
}

pub fn repair_ext(input_ext: &str) -> String {
    if codec_for(input_ext).is_ok() { input_ext.to_string() } else { "m4a".into() }
}

fn codec_args(ext: &str, bitrate_k: Option<u32>, sample_rate: Option<u32>, channels: Option<u32>) -> R<Vec<OsString>> {
    let (enc, lossless) = codec_for(ext)?;
    let enc = audio_encoder(enc);
    ffmpeg::require_encoder(enc)?;
    let mut a = os(&["-c:a", enc]);
    if !lossless {
        let mut b = bitrate_k.unwrap_or(192);
        if enc == "libopus" {
            b = b.min(256);
        }
        a.extend(os(&["-b:a", &format!("{b}k")]));
    }
    // Opus only supports a fixed set of rates; let FFmpeg default it to 48 kHz.
    if let Some(sr) = sample_rate.filter(|_| enc != "libopus") {
        a.extend(os(&["-ar", &sr.to_string()]));
    }
    if let Some(ch) = channels {
        a.extend(os(&["-ac", &ch.to_string()]));
    }
    Ok(a)
}

fn input(p: &Path) -> Vec<OsString> {
    vec!["-i".into(), p.into()]
}

fn engine(ext: &str) -> String {
    format!("CPU ({})", codec_for(ext).map(|c| audio_encoder(c.0).to_string()).unwrap_or_default())
}

pub fn run(ctx: &Ctx, op: &Op, inputs: &[PathBuf], out: &Path) -> R<Vec<PathBuf>> {
    let first = &inputs[0];
    let out_ext = ext_lower(out);
    match op {
        Op::AudioConvert { bitrate_k, sample_rate, channels, .. } => {
            let p = ffmpeg::probe(first)?;
            if !p.has_audio {
                return Err(ui!("errors.noAudioStream"));
            }
            let tmp = TempGuard::file_for(out);
            let mut args = input(first);
            args.extend(os(&["-map", "0:a:0", "-vn", "-map_metadata", "0"]));
            args.extend(codec_args(&out_ext, *bitrate_k, *sample_rate, *channels)?);
            args.push(tmp.path.clone().into());
            ctx.set_engine(engine(&out_ext));
            ffmpeg::run(ctx, args, p.duration, 0.0, 1.0)?;
            tmp.commit(out)?;
        }
        Op::AudioTrim { start, end } => {
            let p = ffmpeg::probe(first)?;
            let (s, d) = trim_range(start, end, p.duration)?;
            let tmp = TempGuard::file_for(out);
            let mut args = os(&["-ss", &format!("{s:.3}")]);
            args.extend(input(first));
            if let Some(d) = d {
                args.extend(os(&["-t", &format!("{d:.3}")]));
            }
            args.extend(os(&["-map", "0:a:0", "-vn", "-map_metadata", "0"]));
            args.extend(codec_args(&repair_ext(&out_ext), None, None, None)?);
            args.push(tmp.path.clone().into());
            ctx.set_engine(engine(&out_ext));
            ffmpeg::run(ctx, args, d.or(p.duration.map(|t| t - s)), 0.0, 1.0)?;
            tmp.commit(out)?;
        }
        Op::AudioSplit { segment_seconds } => return split(ctx, first, out, *segment_seconds, false, false),
        Op::AudioJoin { .. } | Op::AudioMix { .. } => {
            let probes: Vec<_> = inputs.iter().map(|p| ffmpeg::probe(p)).collect::<R<_>>()?;
            if probes.iter().any(|p| !p.has_audio) {
                return Err(ui!("errors.noAudioStream"));
            }
            let mut args = vec![];
            let mut fc = String::new();
            let mut ins = String::new();
            for (i, p) in inputs.iter().enumerate() {
                args.extend(input(p));
                fc.push_str(&format!("[{i}:a:0]aresample=48000,aformat=channel_layouts=stereo[a{i}];"));
                ins.push_str(&format!("[a{i}]"));
            }
            let n = inputs.len();
            let (filter, duration) = if matches!(op, Op::AudioJoin { .. }) {
                (format!("{ins}concat=n={n}:v=0:a=1[out]"), probes.iter().filter_map(|p| p.duration).sum::<f64>())
            } else {
                (
                    format!("{ins}amix=inputs={n}:duration=longest:dropout_transition=0[out]"),
                    probes.iter().filter_map(|p| p.duration).fold(0.0, f64::max),
                )
            };
            fc.push_str(&filter);
            let tmp = TempGuard::file_for(out);
            args.extend(os(&["-filter_complex", &fc, "-map", "[out]"]));
            args.extend(codec_args(&out_ext, None, None, None)?);
            args.push(tmp.path.clone().into());
            ctx.set_engine(engine(&out_ext));
            ffmpeg::run(ctx, args, Some(duration), 0.0, 1.0)?;
            tmp.commit(out)?;
        }
        Op::AudioRepair => {
            let original = ffmpeg::probe(first).ok();
            let tmp = TempGuard::file_for(out);
            let mut args = os(&["-err_detect", "ignore_err", "-fflags", "+discardcorrupt+genpts"]);
            args.extend(input(first));
            args.extend(os(&["-map", "0:a:0?", "-vn"]));
            args.extend(codec_args(&out_ext, Some(256), None, None)?);
            args.push(tmp.path.clone().into());
            ctx.stage(UiMsg::new("stage.repairReencode"));
            ctx.set_engine(engine(&out_ext));
            let res = ffmpeg::run(ctx, args, original.as_ref().and_then(|p| p.duration), 0.0, 1.0);
            ctx.check()?;
            let recovered = res
                .ok()
                .and_then(|_| ffmpeg::probe(&tmp.path).ok())
                .filter(|p| p.has_audio && p.duration.unwrap_or(0.0) > 0.0);
            let Some(p) = recovered else { return Err(ui!("errors.repairFailed")) };
            tmp.commit(out)?;
            ctx.stage(UiMsg::new("result.repaired").var("seconds", format!("{:.1}", p.duration.unwrap_or(0.0))));
        }
        _ => unreachable!(),
    }
    Ok(vec![out.to_path_buf()])
}
