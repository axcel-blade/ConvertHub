//! Video operations on top of FFmpeg: convert/compress (with device presets
//! and GPU encoding), trim, split, join, mux, crop, delogo, repair, and tags.

use crate::error::{R, UiMsg};
use crate::ffmpeg::{self, os, parse_time, Probe};
use crate::fsutil::{ext_lower, file_stem, TempGuard};
use crate::gpu::{self, Accel};
use crate::jobs::{Ctx, Operation as Op};
use crate::ops::audio;
use crate::presets;
use crate::ui;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Encoding settings shared by every re-encoding video operation.
#[derive(Clone, Default)]
pub struct Enc {
    pub format: String,
    pub codec: Option<String>,
    pub accel: Accel,
    pub crf: Option<u32>,
    pub video_bitrate_k: Option<u32>,
    pub audio_bitrate_k: Option<u32>,
    pub max_width: Option<u32>,
    pub max_height: Option<u32>,
    pub profile: Option<String>,
    pub level: Option<String>,
    /// Extra video filters applied before scaling (crop, delogo, ...).
    pub filters: Vec<String>,
}

/// Codec family used for a container: (family, audio codec).
fn container_defaults(format: &str, requested: Option<&str>) -> (&'static str, &'static str) {
    match format {
        "webm" => ("vp9", "libopus"),
        "avi" => ("mpeg4", "libmp3lame"),
        "wmv" => ("wmv2", "wmav2"),
        "mpg" => ("mpeg2", "mp2"),
        "flv" | "3gp" => ("h264", "aac"),
        "mkv" | "mp4" | "m4v" | "mov" | "ts" if requested == Some("hevc") => ("hevc", "aac"),
        _ => ("h264", "aac"),
    }
}

/// CPU encoders per family, in order of preference. LGPL FFmpeg builds lack
/// libx264/libx265, so OS-provided encoders are tried as fallbacks.
fn cpu_encoder(family: &str) -> R<&'static str> {
    let options: &[&str] = match family {
        "h264" => &["libx264", "libopenh264", "h264_mf"],
        "hevc" => &["libx265", "hevc_mf"],
        "vp9" => &["libvpx-vp9"],
        "mpeg4" => &["mpeg4"],
        "wmv2" => &["wmv2"],
        "mpeg2" => &["mpeg2video"],
        _ => &["libx264"],
    };
    if ffmpeg::encoders().is_empty() {
        return Ok(options[0]);
    }
    options
        .iter()
        .copied()
        .find(|e| ffmpeg::has_encoder(e))
        .ok_or_else(|| ui!("errors.encoderMissing", "detail" => options.join(" / ")))
}

pub fn audio_encoder(name: &str) -> &str {
    if name == "libmp3lame" && !ffmpeg::encoders().is_empty() && !ffmpeg::has_encoder("libmp3lame") {
        "mp3_mf"
    } else {
        name
    }
}

fn scale_filter(w: Option<u32>, h: Option<u32>) -> Option<String> {
    if w.is_none() && h.is_none() {
        return None;
    }
    let w = w.map(|v| format!("min(iw\\,{v})")).unwrap_or_else(|| "iw".into());
    let h = h.map(|v| format!("min(ih\\,{v})")).unwrap_or_else(|| "ih".into());
    Some(format!("scale=w={w}:h={h}:force_original_aspect_ratio=decrease:force_divisible_by=2"))
}

struct Attempt {
    encoder: String,
    gpu: bool,
}

fn attempts(family: &str, accel: Accel) -> R<Vec<Attempt>> {
    let gpu_enc = if family == "h264" || family == "hevc" { gpu::pick(family) } else { None };
    let cpu = || cpu_encoder(family).map(|e| Attempt { encoder: e.into(), gpu: false });
    Ok(match accel {
        Accel::Cpu => vec![cpu()?],
        Accel::Gpu => match gpu_enc {
            Some(e) => vec![Attempt { encoder: e, gpu: true }],
            None => return Err(ui!("errors.gpuUnavailable", "codec" => family)),
        },
        Accel::Auto => {
            let mut v = vec![];
            if let Some(e) = gpu_enc {
                v.push(Attempt { encoder: e, gpu: true });
            }
            if let Ok(c) = cpu() {
                v.push(c);
            }
            if v.is_empty() {
                cpu()?;
            }
            v
        }
    })
}

fn video_codec_args(a: &Attempt, enc: &Enc, family: &str) -> (Vec<String>, Vec<String>) {
    let mut pre = vec![];
    let mut args = vec!["-c:v".to_string(), a.encoder.clone()];
    let crf = enc.crf.unwrap_or(23);
    if let Some(b) = enc.video_bitrate_k {
        args.extend([
            "-b:v".into(),
            format!("{b}k"),
            "-maxrate".into(),
            format!("{}k", b * 3 / 2),
            "-bufsize".into(),
            format!("{}k", b * 2),
        ]);
    } else {
        match a.encoder.as_str() {
            "libx264" | "libx265" => args.extend(["-crf".into(), crf.to_string(), "-preset".into(), "medium".into()]),
            "libvpx-vp9" => args.extend(["-crf".into(), crf.max(15).to_string(), "-b:v".into(), "0".into(), "-row-mt".into(), "1".into()]),
            "mpeg4" | "wmv2" | "mpeg2video" => {
                // 2 (best) .. 31 (worst), mapped from CRF 0..51.
                let q = (2 + crf * 29 / 51).clamp(2, 31);
                args.extend(["-q:v".into(), q.to_string()]);
            }
            e if a.gpu => args.extend(gpu::quality_args(e, crf)),
            // libopenh264 / Media Foundation: no CRF, use a sane bitrate.
            _ => args.extend(["-b:v".into(), "4000k".into()]),
        }
    }
    if family == "h264" && (a.encoder == "libx264" || a.gpu) && !a.encoder.contains("vaapi") {
        if let Some(p) = &enc.profile {
            args.extend(["-profile:v".into(), p.clone()]);
        }
        if let Some(l) = &enc.level {
            if a.encoder == "libx264" || a.encoder.contains("nvenc") || a.encoder.contains("qsv") {
                args.extend(["-level".into(), l.clone()]);
            }
        }
    }
    if family == "hevc" && matches!(enc.format.as_str(), "mp4" | "m4v" | "mov") {
        args.extend(["-tag:v".into(), "hvc1".into()]); // required for Apple playback
    }
    if a.gpu {
        pre.extend(gpu::pre_input_args(&a.encoder));
    }
    (pre, args)
}

fn container_args(format: &str) -> Vec<String> {
    match format {
        "mp4" | "m4v" | "mov" => vec!["-movflags".into(), "+faststart".into()],
        _ => vec![],
    }
}

/// Build and run a full re-encode of `inputs` (already prepared input args)
/// into `out`, trying GPU first when allowed and falling back to CPU.
pub fn encode(
    ctx: &Ctx,
    input_args: &[OsString],
    enc: &Enc,
    out: &Path,
    duration: Option<f64>,
    has_audio: bool,
) -> R<()> {
    if enc.format == "gif" {
        let mut vf = enc.filters.clone();
        vf.push(scale_filter(enc.max_width.or(Some(480)), enc.max_height).unwrap_or_default());
        vf.push("fps=12".into());
        let chain = vf.into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(",");
        let fc = format!("[0:v]{chain},split[a][b];[a]palettegen[p];[b][p]paletteuse");
        let mut args = input_args.to_vec();
        args.extend(os(&["-filter_complex", &fc, "-loop", "0"]));
        args.push(out.into());
        ctx.set_engine("CPU (gif)");
        return ffmpeg::run(ctx, args, duration, 0.0, 1.0);
    }

    let (family, acodec) = container_defaults(&enc.format, enc.codec.as_deref());
    let list = attempts(family, enc.accel)?;
    let mut last_err = None;
    for (i, a) in list.iter().enumerate() {
        ctx.check()?;
        let (pre, vargs) = video_codec_args(a, enc, family);
        let mut filters = enc.filters.clone();
        if let Some(s) = scale_filter(enc.max_width, enc.max_height) {
            filters.push(s);
        }
        if a.gpu {
            if let Some(up) = gpu::upload_filter(&a.encoder) {
                filters.push(up.into());
            } else {
                filters.push("format=nv12".into());
            }
        } else if family == "h264" || family == "hevc" {
            filters.push("format=yuv420p".into());
        }
        let mut args: Vec<OsString> = pre.iter().map(OsString::from).collect();
        args.extend(input_args.iter().cloned());
        args.extend(os(&["-map", "0:v:0"]));
        if has_audio {
            args.extend(os(&["-map", "0:a:0?"]));
        }
        if !filters.is_empty() {
            args.extend(os(&["-vf", &filters.join(",")]));
        }
        args.extend(vargs.iter().map(OsString::from));
        if has_audio {
            let ab = enc.audio_bitrate_k.unwrap_or(160);
            args.extend(os(&["-c:a", audio_encoder(acodec), "-b:a", &format!("{ab}k")]));
            if acodec == "libopus" || acodec == "aac" {
                args.extend(os(&["-ac", "2"]));
            }
        } else {
            args.push("-an".into());
        }
        args.extend(container_args(&enc.format).iter().map(OsString::from));
        args.push(out.into());

        let label = if a.gpu { format!("GPU ({})", a.encoder) } else { format!("CPU ({})", a.encoder) };
        let label = if i > 0 { format!("{label} — GPU fallback") } else { label };
        ctx.set_engine(label);
        match ffmpeg::run(ctx, args, duration, 0.0, 1.0) {
            Ok(()) => return Ok(()),
            Err(e) if a.gpu && enc.accel == Accel::Auto && !ctx.is_cancelled() => {
                ctx.stage(UiMsg::new("stage.gpuFallback"));
                last_err = Some(e);
            }
            Err(e) => return Err(e),
        }
    }
    Err(last_err.unwrap_or_else(|| ui!("errors.encoderMissing", "detail" => family)))
}

fn input(path: &Path) -> Vec<OsString> {
    vec!["-i".into(), path.into()]
}

/// Encoding settings for a target format, optionally from a device preset.
pub fn enc_for(format: &str, preset: Option<&str>, probe: &Probe) -> R<Enc> {
    enc_from_convert(
        &Op::VideoConvert {
            format: format.into(),
            preset: preset.map(String::from),
            codec: None,
            accel: Accel::Auto,
            crf: Some(21),
            video_bitrate_k: None,
            audio_bitrate_k: None,
            max_width: None,
            max_height: None,
            target_size_mb: None,
        },
        probe,
    )
}

fn enc_from_convert(op: &Op, probe: &Probe) -> R<Enc> {
    let Op::VideoConvert {
        format, preset, codec, accel, crf, video_bitrate_k, audio_bitrate_k, max_width, max_height, target_size_mb,
    } = op
    else {
        unreachable!()
    };
    let mut enc = Enc {
        format: format.clone(),
        codec: codec.clone(),
        accel: *accel,
        crf: *crf,
        video_bitrate_k: *video_bitrate_k,
        audio_bitrate_k: *audio_bitrate_k,
        max_width: *max_width,
        max_height: *max_height,
        ..Default::default()
    };
    if let Some(p) = preset.as_deref().filter(|p| !p.is_empty()) {
        let p = presets::get(p).ok_or_else(|| ui!("errors.unknownPreset", "preset" => p))?;
        enc.format = p.format.into();
        enc.codec = Some(p.codec.into());
        enc.profile = p.profile.map(String::from);
        enc.level = p.level.map(String::from);
        enc.max_width = Some(enc.max_width.map_or(p.max_width, |w| w.min(p.max_width)));
        enc.max_height = Some(enc.max_height.map_or(p.max_height, |h| h.min(p.max_height)));
        enc.crf = enc.crf.or(Some(p.crf));
        enc.video_bitrate_k = enc.video_bitrate_k.or(p.video_bitrate_k);
        enc.audio_bitrate_k = enc.audio_bitrate_k.or(Some(p.audio_bitrate_k));
    }
    if let Some(mb) = target_size_mb.filter(|m| *m > 0.0) {
        let d = probe.duration.filter(|d| *d > 0.0).ok_or_else(|| ui!("errors.durationUnknown"))?;
        let audio_k = if probe.has_audio { enc.audio_bitrate_k.unwrap_or(128) } else { 0 };
        let total_k = mb * 8192.0 / d; // MB -> kbit, per second
        let video_k = (total_k * 0.97 - audio_k as f64).floor();
        if video_k < 50.0 {
            return Err(ui!("errors.targetTooSmall", "minMb" => format!("{:.1}", (audio_k as f64 + 50.0) * d / 8192.0)));
        }
        enc.video_bitrate_k = Some(video_k as u32);
    }
    Ok(enc)
}

/// Re-encode settings that keep an input's container ("same format").
fn same_format_enc(path: &Path) -> Enc {
    let ext = ext_lower(path);
    let format = if crate::validate::VIDEO_OUT.contains(&ext.as_str()) { ext } else { "mkv".into() };
    Enc { format, crf: Some(20), ..Default::default() }
}

pub fn repair_ext(input_ext: &str) -> String {
    if crate::validate::AUDIO_IN.contains(&input_ext) {
        audio::repair_ext(input_ext)
    } else {
        "mkv".into()
    }
}

pub fn run(ctx: &Ctx, op: &Op, inputs: &[PathBuf], out: &Path) -> R<Vec<PathBuf>> {
    let first = &inputs[0];
    match op {
        Op::VideoConvert { .. } => {
            let probe = ffmpeg::probe(first)?;
            if !probe.has_video {
                return Err(ui!("errors.noVideoStream"));
            }
            let enc = enc_from_convert(op, &probe)?;
            let tmp = TempGuard::file_for(out);
            encode(ctx, &input(first), &enc, &tmp.path, probe.duration, probe.has_audio)?;
            tmp.commit(out)?;
        }
        Op::VideoTrim { start, end, precise } => {
            let probe = ffmpeg::probe(first)?;
            let (s, d) = trim_range(start, end, probe.duration)?;
            let tmp = TempGuard::file_for(out);
            let mut args = os(&["-ss", &format!("{s:.3}")]);
            args.extend(input(first));
            if let Some(d) = d {
                args.extend(os(&["-t", &format!("{d:.3}")]));
            }
            if *precise {
                let enc = same_format_enc(out);
                encode(ctx, &args, &enc, &tmp.path, d.or(probe.duration.map(|x| x - s)), probe.has_audio)?;
            } else {
                ctx.set_engine("Stream copy");
                args.extend(os(&["-map", "0", "-c", "copy", "-avoid_negative_ts", "make_zero"]));
                args.push(tmp.path.clone().into());
                ffmpeg::run(ctx, args, d.or(probe.duration.map(|x| x - s)), 0.0, 1.0)?;
            }
            tmp.commit(out)?;
        }
        Op::VideoSplit { segment_seconds, precise } => {
            return split(ctx, first, out, *segment_seconds, *precise, true);
        }
        Op::VideoJoin { format } => {
            let enc = Enc { format: format.clone(), crf: Some(20), ..Default::default() };
            join(ctx, inputs, &enc, out)?;
        }
        Op::VideoMux { audio } => {
            let probe = ffmpeg::probe(first)?;
            if !probe.has_video {
                return Err(ui!("errors.noVideoStream"));
            }
            let ap = ffmpeg::probe(Path::new(audio))?;
            if !ap.has_audio {
                return Err(ui!("errors.noAudioStream"));
            }
            let tmp = TempGuard::file_for(out);
            let mut args = input(first);
            args.extend(input(Path::new(audio)));
            args.extend(os(&["-map", "0:v:0", "-map", "1:a:0", "-c:v", "copy", "-c:a", "aac", "-b:a", "192k", "-shortest"]));
            args.extend(container_args(&ext_lower(out)).iter().map(OsString::from));
            args.push(tmp.path.clone().into());
            ctx.set_engine("Stream copy + AAC");
            ffmpeg::run(ctx, args, probe.duration, 0.0, 1.0)?;
            tmp.commit(out)?;
        }
        Op::VideoCrop { x, y, w, h } => {
            let probe = ffmpeg::probe(first)?;
            check_rect(&probe, *x, *y, *w, *h, 0)?;
            let mut enc = same_format_enc(out);
            enc.accel = Accel::Auto;
            enc.filters.push(format!("crop={w}:{h}:{x}:{y}"));
            let tmp = TempGuard::file_for(out);
            encode(ctx, &input(first), &enc, &tmp.path, probe.duration, probe.has_audio)?;
            tmp.commit(out)?;
        }
        Op::VideoDelogo { x, y, w, h, authorized } => {
            if !authorized {
                return Err(ui!("errors.delogoAuthorization"));
            }
            let probe = ffmpeg::probe(first)?;
            // FFmpeg's delogo needs a 1px border inside the frame.
            check_rect(&probe, *x, *y, *w, *h, 1)?;
            let mut enc = same_format_enc(out);
            enc.filters.push(format!("delogo=x={x}:y={y}:w={w}:h={h}"));
            let tmp = TempGuard::file_for(out);
            encode(ctx, &input(first), &enc, &tmp.path, probe.duration, probe.has_audio)?;
            tmp.commit(out)?;
        }
        Op::VideoRepair => repair(ctx, first, out)?,
        Op::MediaTags { title, artist, album, year, comment } => {
            let probe = ffmpeg::probe(first)?;
            let tmp = TempGuard::file_for(out);
            let mut args = input(first);
            args.extend(os(&["-map", "0", "-map_metadata", "0", "-c", "copy"]));
            for (k, v) in [("title", title), ("artist", artist), ("album", album), ("date", year), ("comment", comment)] {
                if !v.trim().is_empty() {
                    args.extend(os(&["-metadata", &format!("{k}={}", v.trim())]));
                }
            }
            args.push(tmp.path.clone().into());
            ctx.set_engine("Stream copy");
            ffmpeg::run(ctx, args, probe.duration, 0.0, 1.0)?;
            tmp.commit(out)?;
        }
        _ => unreachable!(),
    }
    Ok(vec![out.to_path_buf()])
}

fn check_rect(p: &Probe, x: u32, y: u32, w: u32, h: u32, margin: u32) -> R<()> {
    let (Some(fw), Some(fh)) = (p.width, p.height) else { return Ok(()) };
    if w == 0 || h == 0 || x < margin || y < margin || x + w + margin > fw || y + h + margin > fh {
        return Err(ui!("errors.rectOutOfBounds", "width" => fw, "height" => fh));
    }
    Ok(())
}

/// Returns (start, optional duration) in seconds.
pub fn trim_range(start: &str, end: &str, total: Option<f64>) -> R<(f64, Option<f64>)> {
    let s = if start.trim().is_empty() { 0.0 } else { parse_time(start).ok_or_else(|| ui!("errors.badTime", "value" => start))? };
    let e = if end.trim().is_empty() { None } else { Some(parse_time(end).ok_or_else(|| ui!("errors.badTime", "value" => end))?) };
    if let Some(e) = e {
        if e <= s {
            return Err(ui!("errors.endBeforeStart"));
        }
    }
    if let Some(t) = total {
        if s >= t {
            return Err(ui!("errors.startAfterEnd", "duration" => format!("{t:.1}")));
        }
    }
    Ok((s, e.map(|e| e - s)))
}

pub fn split(ctx: &Ctx, input_path: &Path, out_dir: &Path, seg: f64, precise: bool, video: bool) -> R<Vec<PathBuf>> {
    if !(seg >= 1.0) {
        return Err(ui!("errors.segmentTooShort"));
    }
    let probe = ffmpeg::probe(input_path)?;
    let tmp = TempGuard::dir_for(out_dir)?;
    let ext = ext_lower(input_path);
    let pattern = tmp.path.join(format!("{}_%03d.{ext}", file_stem(input_path)));
    let mut args = input(input_path);
    args.extend(os(&["-map", "0"]));
    if precise && video {
        let enc = same_format_enc(input_path);
        let (family, acodec) = container_defaults(&enc.format, None);
        let venc = cpu_encoder(family)?;
        args.extend(os(&["-c:v", venc, "-c:a", audio_encoder(acodec), "-force_key_frames", &format!("expr:gte(t,n_forced*{seg})")]));
        ctx.set_engine(format!("CPU ({venc})"));
    } else {
        args.extend(os(&["-c", "copy"]));
        ctx.set_engine("Stream copy");
    }
    args.extend(os(&["-f", "segment", "-segment_time", &seg.to_string(), "-reset_timestamps", "1"]));
    args.push(pattern.into());
    ffmpeg::run(ctx, args, probe.duration, 0.0, 1.0)?;
    let mut parts: Vec<PathBuf> = std::fs::read_dir(&tmp.path)?.filter_map(|e| e.ok().map(|e| e.path())).collect();
    parts.sort();
    if parts.is_empty() {
        return Err(ui!("errors.noOutput"));
    }
    tmp.commit(out_dir)?;
    Ok(vec![out_dir.to_path_buf()])
}

fn join(ctx: &Ctx, inputs: &[PathBuf], enc: &Enc, out: &Path) -> R<()> {
    let probes: Vec<Probe> = inputs.iter().map(|p| ffmpeg::probe(p)).collect::<R<_>>()?;
    if probes.iter().any(|p| !p.has_video) {
        return Err(ui!("errors.noVideoStream"));
    }
    let w = probes[0].width.unwrap_or(1280) / 2 * 2;
    let h = probes[0].height.unwrap_or(720) / 2 * 2;
    let total: f64 = probes.iter().filter_map(|p| p.duration).sum();
    let any_audio = probes.iter().any(|p| p.has_audio);

    let mut args: Vec<OsString> = vec![];
    for p in inputs {
        args.extend(input(p));
    }
    let mut extra = inputs.len();
    let mut fc = String::new();
    let mut concat_in = String::new();
    for (i, p) in probes.iter().enumerate() {
        fc.push_str(&format!(
            "[{i}:v:0]scale={w}:{h}:force_original_aspect_ratio=decrease,pad={w}:{h}:(ow-iw)/2:(oh-ih)/2,setsar=1,fps=30,format=yuv420p[v{i}];"
        ));
        concat_in.push_str(&format!("[v{i}]"));
        if any_audio {
            let src = if p.has_audio {
                format!("[{i}:a:0]")
            } else {
                // Silent filler so the concat filter gets an audio pad.
                args.extend(os(&["-f", "lavfi", "-t", &format!("{:.3}", p.duration.unwrap_or(1.0)), "-i", "anullsrc=r=48000:cl=stereo"]));
                let s = format!("[{extra}:a]");
                extra += 1;
                s
            };
            fc.push_str(&format!("{src}aresample=48000,aformat=channel_layouts=stereo[a{i}];"));
            concat_in.push_str(&format!("[a{i}]"));
        }
    }
    let a = if any_audio { 1 } else { 0 };
    fc.push_str(&format!("{concat_in}concat=n={}:v=1:a={a}[v][a0]", inputs.len()));
    if !any_audio {
        fc = fc.replace("[v][a0]", "[v]");
    }

    let (family, acodec) = container_defaults(&enc.format, enc.codec.as_deref());
    let venc = cpu_encoder(family)?;
    let tmp = TempGuard::file_for(out);
    args.extend(os(&["-filter_complex", &fc, "-map", "[v]"]));
    if any_audio {
        args.extend(os(&["-map", "[a0]", "-c:a", audio_encoder(acodec), "-b:a", "160k"]));
    }
    let a = Attempt { encoder: venc.into(), gpu: false };
    let (_, vargs) = video_codec_args(&a, enc, family);
    args.extend(vargs.iter().map(OsString::from));
    args.extend(container_args(&enc.format).iter().map(OsString::from));
    args.push(tmp.path.clone().into());
    ctx.set_engine(format!("CPU ({venc})"));
    ffmpeg::run(ctx, args, Some(total), 0.0, 1.0)?;
    tmp.commit(out)
}

fn verify(path: &Path, need_video: bool) -> Option<Probe> {
    let p = ffmpeg::probe(path).ok()?;
    let ok = p.duration.unwrap_or(0.0) > 0.0 && (!need_video || p.has_video) && (need_video || p.has_audio);
    ok.then_some(p)
}

/// Best-effort repair: first a tolerant remux (keeps quality), then a
/// tolerant re-encode. The result is verified; we never claim success
/// unless a playable stream with a duration was recovered.
fn repair(ctx: &Ctx, input_path: &Path, out: &Path) -> R<()> {
    let original = ffmpeg::probe(input_path).ok();
    let tolerant = os(&["-err_detect", "ignore_err", "-fflags", "+genpts+discardcorrupt+igndts"]);
    let tmp = TempGuard::file_for(out);

    ctx.stage(UiMsg::new("stage.repairRemux"));
    ctx.set_engine("Stream copy");
    let mut args = tolerant.clone();
    args.extend(input(input_path));
    args.extend(os(&["-map", "0:v?", "-map", "0:a?", "-c", "copy", "-ignore_unknown"]));
    args.push(tmp.path.clone().into());
    let dur = original.as_ref().and_then(|p| p.duration);
    let first = ffmpeg::run(ctx, args, dur, 0.0, 0.5);
    ctx.check()?;
    let mut recovered = first.is_ok().then(|| verify(&tmp.path, true)).flatten();

    if recovered.is_none() {
        ctx.stage(UiMsg::new("stage.repairReencode"));
        let venc = cpu_encoder("h264")?;
        ctx.set_engine(format!("CPU ({venc})"));
        let mut args = tolerant;
        args.extend(input(input_path));
        args.extend(os(&["-map", "0:v:0?", "-map", "0:a:0?", "-c:v", venc, "-pix_fmt", "yuv420p", "-c:a", "aac"]));
        args.push(tmp.path.clone().into());
        match ffmpeg::run(ctx, args, dur, 0.5, 1.0) {
            Ok(()) => recovered = verify(&tmp.path, true),
            Err(e) if e.0.key == "errors.cancelled" => return Err(e),
            Err(e) if e.0.vars.get("detail").is_some_and(|d| d.contains("moov atom not found")) => {
                return Err(ui!("errors.repairMoov"));
            }
            Err(_) => {}
        }
    }
    let Some(p) = recovered else {
        return Err(ui!("errors.repairFailed"));
    };
    tmp.commit(out)?;
    let secs = format!("{:.1}", p.duration.unwrap_or(0.0));
    ctx.stage(match dur {
        Some(d) => UiMsg::new("result.repairedOf").var("seconds", secs).var("original", format!("{d:.1}")),
        None => UiMsg::new("result.repaired").var("seconds", secs),
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trim_ranges() {
        assert_eq!(trim_range("10", "20", Some(60.0)).unwrap(), (10.0, Some(10.0)));
        assert_eq!(trim_range("", "", Some(60.0)).unwrap(), (0.0, None));
        assert!(trim_range("20", "10", None).is_err());
        assert!(trim_range("70", "", Some(60.0)).is_err());
    }

    #[test]
    fn scale() {
        assert!(scale_filter(None, None).is_none());
        assert!(scale_filter(Some(640), None).unwrap().contains("min(iw\\,640)"));
    }
}
