//! End-to-end tests that run real FFmpeg through the full job pipeline
//! (validation -> output planning -> processing -> temp commit).
//! They need FFmpeg on PATH and are ignored by default:
//!
//!     cargo test -- --ignored --test-threads=4

use crate::deps::{self, Tool};
use crate::error::R;
use crate::ffmpeg::{self, Probe};
use crate::gpu::{self, Accel};
use crate::jobs::{Ctx, JobRequest, Operation as Op};
use std::path::{Path, PathBuf};
use std::process::Command;

pub(super) fn ff(args: &[&str]) {
    let st = Command::new(deps::require(Tool::Ffmpeg).expect("ffmpeg on PATH"))
        .args(["-hide_banner", "-loglevel", "error", "-y"])
        .args(args)
        .status()
        .unwrap();
    assert!(st.success(), "fixture ffmpeg failed: {args:?}");
}

pub(super) fn s(p: &Path) -> String {
    p.display().to_string()
}

/// 640x360, 5 s, H.264 + AAC, moov at the end (no faststart).
fn video(dir: &Path) -> PathBuf {
    let p = dir.join("clip.mp4");
    ff(&["-f", "lavfi", "-i", "testsrc=size=640x360:rate=25:duration=5", "-f", "lavfi", "-i", "sine=frequency=440:duration=5",
        "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest", &s(&p)]);
    p
}

fn silent_video(dir: &Path) -> PathBuf {
    let p = dir.join("silent.mkv");
    ff(&["-f", "lavfi", "-i", "testsrc2=size=320x240:rate=30:duration=3", "-c:v", "mpeg4", &s(&p)]);
    p
}

fn tone(dir: &Path, name: &str, secs: u32, freq: u32) -> PathBuf {
    let p = dir.join(name);
    ff(&["-f", "lavfi", "-i", &format!("sine=frequency={freq}:duration={secs}"), &s(&p)]);
    p
}

pub(super) fn job(inputs: &[&Path], out: &Path, op: Op) -> (R<Vec<PathBuf>>, Option<String>) {
    let ctx = Ctx::for_test();
    let req = JobRequest {
        inputs: inputs.iter().map(|p| s(p)).collect(),
        output_dir: s(out),
        output_name: None,
        overwrite: false,
        operation: op,
    };
    let r = super::run(&ctx, &req);
    let engine = ctx.engine_seen.lock().unwrap().clone();
    (r, engine)
}

pub(super) fn ok(inputs: &[&Path], out: &Path, op: Op) -> (PathBuf, Option<String>) {
    let (r, e) = job(inputs, out, op.clone());
    let outs = r.unwrap_or_else(|err| panic!("{op:?} failed: {err}"));
    (outs[0].clone(), e)
}

pub(super) fn probe(p: &Path) -> Probe {
    ffmpeg::probe(p).unwrap()
}

pub(super) fn decoded_secs(p: &Path) -> f64 {
    let out = Command::new(deps::find(Tool::Ffmpeg).unwrap()).arg("-i").arg(p).args(["-f", "null", "-"]).output().unwrap();
    let text = String::from_utf8_lossy(&out.stderr);
    let t = text.rsplit("time=").next().unwrap_or("").split_whitespace().next().unwrap_or("");
    ffmpeg::parse_time(t).unwrap_or(0.0)
}

pub(super) fn dur(p: &Path) -> f64 {
    probe(p).duration.unwrap_or(0.0)
}

pub(super) fn no_temp_files(dir: &Path) {
    let leftovers: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().contains(".converthub-"))
        .collect();
    assert!(leftovers.is_empty(), "temp files left behind: {leftovers:?}");
}

fn convert(format: &str, accel: Accel) -> Op {
    Op::VideoConvert {
        format: format.into(), preset: None, codec: None, accel, crf: None, video_bitrate_k: None,
        audio_bitrate_k: None, max_width: None, max_height: None, target_size_mb: None,
    }
}

#[test]
#[ignore]
fn video_convert_formats() {
    let d = tempfile::tempdir().unwrap();
    let v = video(d.path());
    let out = d.path().join("out");
    std::fs::create_dir(&out).unwrap();
    for fmt in ["mkv", "webm", "avi", "mov", "wmv", "flv", "mpg", "3gp", "m4v", "ts", "gif"] {
        let (o, engine) = ok(&[&v], &out, convert(fmt, Accel::Cpu));
        assert_eq!(o.extension().unwrap(), fmt);
        let p = probe(&o);
        assert!(p.has_video, "{fmt}");
        assert!(p.duration.unwrap_or(0.0) > 4.0, "{fmt} duration {:?}", p.duration);
        println!("{fmt}: {engine:?} {:?}/{:?}", p.video_codec, p.audio_codec);
    }
    no_temp_files(&out);
}

#[test]
#[ignore]
fn gpu_detection_and_encode() {
    let hw = gpu::info(true);
    println!("hwaccels={:?}\ncompiled={:?}\nverified={:?}", hw.hwaccels, hw.compiled, hw.verified);
    let d = tempfile::tempdir().unwrap();
    let v = video(d.path());
    if hw.verified.is_empty() {
        let (r, _) = job(&[&v], d.path(), convert("mkv", Accel::Gpu));
        assert_eq!(r.unwrap_err().0.key, "errors.gpuUnavailable");
        return;
    }
    let (o, engine) = ok(&[&v], d.path(), convert("mkv", Accel::Gpu));
    let engine = engine.unwrap();
    assert!(engine.starts_with("GPU ("), "{engine}");
    assert_eq!(probe(&o).video_codec.as_deref(), Some("h264"));
    println!("GPU h264 engine: {engine}");
    // Auto must pick the GPU too, and HEVC must work when verified.
    let (_, auto) = ok(&[&v], d.path(), Op::VideoConvert {
        format: "mp4".into(), preset: None, codec: Some("hevc".into()), accel: Accel::Auto, crf: Some(28),
        video_bitrate_k: None, audio_bitrate_k: None, max_width: None, max_height: None, target_size_mb: None,
    });
    println!("Auto hevc engine: {auto:?}");
    assert!(auto.unwrap().starts_with("GPU ("));
}

#[test]
#[ignore]
fn presets_and_compression() {
    let d = tempfile::tempdir().unwrap();
    let v = video(d.path());
    let (o, _) = ok(&[&v], d.path(), Op::VideoConvert {
        format: "mp4".into(), preset: Some("ipod_classic".into()), codec: None, accel: Accel::Cpu, crf: None,
        video_bitrate_k: None, audio_bitrate_k: None, max_width: None, max_height: None, target_size_mb: None,
    });
    let p = probe(&o);
    assert_eq!(o.extension().unwrap(), "m4v");
    assert!(p.width.unwrap() <= 320 && p.height.unwrap() <= 240, "{p:?}");

    let (o, _) = ok(&[&v], d.path(), Op::VideoConvert {
        format: "mp4".into(), preset: Some("iphone_modern".into()), codec: None, accel: Accel::Cpu, crf: None,
        video_bitrate_k: None, audio_bitrate_k: None, max_width: None, max_height: None, target_size_mb: None,
    });
    assert_eq!(probe(&o).video_codec.as_deref(), Some("h264"));

    // Target size: 0.5 MB for 5 s.
    let out = d.path().join("small");
    std::fs::create_dir(&out).unwrap();
    let (o, _) = ok(&[&v], &out, Op::VideoConvert {
        format: "mp4".into(), preset: None, codec: None, accel: Accel::Cpu, crf: None, video_bitrate_k: None,
        audio_bitrate_k: Some(64), max_width: Some(320), max_height: None, target_size_mb: Some(0.5),
    });
    let mb = std::fs::metadata(&o).unwrap().len() as f64 / 1048576.0;
    println!("target 0.5 MB -> {mb:.3} MB");
    assert!(mb < 0.75, "{mb}");
    assert_eq!(probe(&o).width, Some(320));
}

#[test]
#[ignore]
fn video_editing() {
    let d = tempfile::tempdir().unwrap();
    let v = video(d.path());
    let sv = silent_video(d.path());
    let a = tone(d.path(), "music.mp3", 3, 660);

    let (o, e) = ok(&[&v], d.path(), Op::VideoTrim { start: "1".into(), end: "3".into(), precise: false });
    assert_eq!(e.as_deref(), Some("Stream copy"));
    assert!(dur(&o) > 1.0 && dur(&o) < 3.5, "fast trim {}", dur(&o));
    std::fs::remove_file(&o).unwrap();
    let (o, _) = ok(&[&v], d.path(), Op::VideoTrim { start: "00:00:01".into(), end: "3".into(), precise: true });
    assert!((dur(&o) - 2.0).abs() < 0.2, "precise trim {}", dur(&o));

    let (dir, _) = ok(&[&v], d.path(), Op::VideoSplit { segment_seconds: 2.0, precise: true });
    let parts = std::fs::read_dir(&dir).unwrap().count();
    assert!((2..=3).contains(&parts), "parts {parts}");

    let (o, _) = ok(&[&v, &sv], d.path(), Op::VideoJoin { format: "mp4".into() });
    let p = probe(&o);
    assert!((dur(&o) - 8.0).abs() < 0.5, "join {}", dur(&o));
    assert_eq!((p.width, p.height), (Some(640), Some(360)));
    assert!(p.has_audio);

    let (o, _) = ok(&[&sv], d.path(), Op::VideoMux { audio: s(&a) });
    assert!(probe(&o).has_audio);

    let (o, _) = ok(&[&v], d.path(), Op::VideoCrop { x: 100, y: 50, w: 200, h: 120 });
    assert_eq!((probe(&o).width, probe(&o).height), (Some(200), Some(120)));
    let (r, _) = job(&[&v], &d.path().join("x"), Op::VideoCrop { x: 600, y: 0, w: 200, h: 100 });
    assert!(r.is_err());

    let (r, _) = job(&[&v], d.path(), Op::VideoDelogo { x: 10, y: 10, w: 80, h: 40, authorized: false });
    assert_eq!(r.unwrap_err().0.key, "errors.delogoAuthorization");
    let (o, _) = ok(&[&v], d.path(), Op::VideoDelogo { x: 10, y: 10, w: 80, h: 40, authorized: true });
    assert!(dur(&o) > 4.0);

    let (o, _) = ok(&[&v], d.path(), Op::MediaTags {
        title: "Test Title".into(), artist: "Me".into(), album: String::new(), year: "2026".into(), comment: String::new(),
    });
    let tags = Command::new(deps::find(Tool::Ffprobe).unwrap())
        .args(["-v", "error", "-show_entries", "format_tags=title", "-of", "default=nw=1:nk=1"]).arg(&o).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&tags.stdout).trim(), "Test Title");

    // Existing output is never overwritten without permission.
    let (r, _) = job(&[&v], d.path(), Op::VideoCrop { x: 100, y: 50, w: 200, h: 120 });
    assert_eq!(r.unwrap_err().0.key, "errors.outputExists");
    no_temp_files(d.path());
}

#[test]
#[ignore]
fn repair_paths() {
    let d = tempfile::tempdir().unwrap();
    let v = video(d.path());
    // Truncated MKV: partially recoverable.
    let mkv = d.path().join("broken.mkv");
    ff(&["-i", &s(&v), "-c", "copy", &s(&mkv)]);
    let bytes = std::fs::read(&mkv).unwrap();
    std::fs::write(&mkv, &bytes[..bytes.len() * 6 / 10]).unwrap();
    let (o, _) = ok(&[&mkv], d.path(), Op::VideoRepair);
    println!("recovered {:.2}s from truncated mkv", dur(&o));
    assert!(dur(&o) > 0.5);

    // Truncated MP4 with moov at the end: not recoverable, explained.
    let mp4 = d.path().join("cut.mp4");
    let bytes = std::fs::read(&v).unwrap();
    std::fs::write(&mp4, &bytes[..bytes.len() / 2]).unwrap();
    let (r, _) = job(&[&mp4], d.path(), Op::VideoRepair);
    let key = r.unwrap_err().0.key;
    assert!(key == "errors.repairMoov" || key == "errors.repairFailed", "{key}");
    println!("truncated mp4 -> {key}");

    // Random bytes.
    let junk = d.path().join("junk.avi");
    std::fs::write(&junk, (0..200_000u32).map(|i| (i * 7919 % 251) as u8).collect::<Vec<_>>()).unwrap();
    let (r, _) = job(&[&junk], d.path(), Op::VideoRepair);
    assert!(r.is_err());
    println!("junk -> {}", r.unwrap_err());

    // Damaged MP3.
    let mp3 = tone(d.path(), "song.mp3", 6, 500);
    let mut b = std::fs::read(&mp3).unwrap();
    let n = b.len();
    for x in &mut b[n / 3..n / 3 + 4000] {
        *x = 0x55;
    }
    std::fs::write(&mp3, &b).unwrap();
    let (o, _) = ok(&[&mp3], d.path(), Op::AudioRepair);
    println!("audio repair -> {:.2}s", dur(&o));
    assert!(dur(&o) > 3.0);
    no_temp_files(d.path());
}

#[test]
#[ignore]
fn audio_tools() {
    let d = tempfile::tempdir().unwrap();
    let a = tone(d.path(), "a.wav", 4, 440);
    let b = tone(d.path(), "b.flac", 3, 880);
    let v = video(d.path());
    let out = d.path().join("out");
    std::fs::create_dir(&out).unwrap();
    for fmt in ["mp3", "m4a", "aac", "ogg", "opus", "flac", "wav", "wma", "aiff", "ac3"] {
        let (o, e) = ok(&[&a], &out, Op::AudioConvert { format: fmt.into(), bitrate_k: Some(128), sample_rate: Some(44100), channels: Some(1) });
        let p = probe(&o);
        // Raw ADTS AAC has no index; ffprobe estimates its duration from the
        // bitrate, so measure it by decoding instead.
        let d = if fmt == "aac" { decoded_secs(&o) } else { p.duration.unwrap() };
        assert!(p.has_audio && (d - 4.0).abs() < 0.2, "{fmt} {d} {p:?}");
        println!("{fmt}: {e:?} {:?}", p.audio_codec);
    }
    // Extract audio from a video file.
    let (o, _) = ok(&[&v], &out, Op::AudioConvert { format: "mp3".into(), bitrate_k: None, sample_rate: None, channels: None });
    assert!(!probe(&o).has_video);

    let (o, _) = ok(&[&a], d.path(), Op::AudioTrim { start: "0.5".into(), end: "2.5".into() });
    assert!((dur(&o) - 2.0).abs() < 0.1);
    let (dir, _) = ok(&[&a], d.path(), Op::AudioSplit { segment_seconds: 1.0 });
    assert!(std::fs::read_dir(dir).unwrap().count() >= 4);
    let (o, _) = ok(&[&a, &b], d.path(), Op::AudioJoin { format: "mp3".into() });
    assert!((dur(&o) - 7.0).abs() < 0.2, "join {}", dur(&o));
    let (o, _) = ok(&[&a, &b], d.path(), Op::AudioMix { format: "ogg".into() });
    assert!((dur(&o) - 4.0).abs() < 0.2, "mix {}", dur(&o));
    no_temp_files(d.path());
}

#[test]
#[ignore]
fn image_via_ffmpeg() {
    let d = tempfile::tempdir().unwrap();
    let src = d.path().join("in.png");
    image::RgbImage::from_fn(64, 48, |x, y| image::Rgb([x as u8 * 4, y as u8 * 5, 90])).save(&src).unwrap();
    let (o, e) = ok(&[&src], d.path(), Op::ImageConvert {
        format: "webp".into(), quality: Some(70), width: None, height: None, percent: Some(50.0),
        rotate: None, flip_h: true, flip_v: false, keep_metadata: false,
    });
    assert_eq!(e.as_deref(), Some("FFmpeg (libwebp)"));
    assert_eq!(image::image_dimensions(&o).unwrap(), (32, 24));
    // HEIC output without ImageMagick: clear message.
    if deps::find(Tool::Magick).is_none() {
        let (r, _) = job(&[&src], d.path(), Op::ImageConvert {
            format: "heic".into(), quality: None, width: None, height: None, percent: None,
            rotate: None, flip_h: false, flip_v: false, keep_metadata: false,
        });
        assert_eq!(r.unwrap_err().0.key, "errors.heicEncode");
    }
}

#[test]
#[ignore]
fn disc_rips() {
    let d = tempfile::tempdir().unwrap();
    // Unencrypted DVD structure.
    let dvd = d.path().join("DVD");
    std::fs::create_dir_all(dvd.join("VIDEO_TS")).unwrap();
    ff(&["-f", "lavfi", "-i", "testsrc=size=720x480:rate=30:duration=4", "-f", "lavfi", "-i", "sine=duration=4",
        "-c:v", "mpeg2video", "-c:a", "mp2", "-f", "vob", &s(&dvd.join("VIDEO_TS/VTS_01_1.VOB"))]);
    ff(&["-f", "lavfi", "-i", "testsrc=size=720x480:rate=30:duration=3", "-c:v", "mpeg2video", "-f", "vob",
        &s(&dvd.join("VIDEO_TS/VTS_01_2.VOB"))]);
    let (r, _) = job(&[&dvd], d.path(), Op::DvdRip { format: "mp4".into(), preset: None, authorized: false });
    assert_eq!(r.unwrap_err().0.key, "errors.discAuthorization");
    let (o, _) = ok(&[&dvd], d.path(), Op::DvdRip { format: "mp4".into(), preset: None, authorized: true });
    println!("dvd rip {:.2}s", dur(&o));
    assert!(dur(&o) > 6.0);

    // Blu-ray folder structure (unencrypted M2TS).
    let bd = d.path().join("BD");
    std::fs::create_dir_all(bd.join("BDMV/STREAM")).unwrap();
    ff(&["-f", "lavfi", "-i", "testsrc=size=1280x720:rate=24:duration=3", "-c:v", "libx264", "-f", "mpegts",
        &s(&bd.join("BDMV/STREAM/00001.m2ts"))]);
    let (o, _) = ok(&[&bd], d.path(), Op::BlurayRip { format: "mkv".into(), preset: None, authorized: true });
    assert!(dur(&o) > 2.5);

    // Audio CD as macOS exposes it (AIFF tracks).
    let cd = d.path().join("CD");
    std::fs::create_dir(&cd).unwrap();
    for i in 1..=2 {
        ff(&["-f", "lavfi", "-i", &format!("sine=frequency={}:duration=2", 300 * i), &s(&cd.join(format!("{i} Track.aiff")))]);
    }
    let (dir, _) = ok(&[&cd], d.path(), Op::CdRip { format: "mp3".into(), bitrate_k: Some(192) });
    assert_eq!(std::fs::read_dir(dir).unwrap().count(), 2);

    // Encrypted/unreadable disc data: clear message, no bypass.
    let bad = d.path().join("BAD");
    std::fs::create_dir_all(bad.join("VIDEO_TS")).unwrap();
    std::fs::write(bad.join("VIDEO_TS/VTS_01_1.VOB"), vec![0x42u8; 300_000]).unwrap();
    let (r, _) = job(&[&bad], d.path(), Op::DvdRip { format: "mp4".into(), preset: None, authorized: true });
    assert_eq!(r.unwrap_err().0.key, "errors.discProtected");
    no_temp_files(d.path());
}

#[test]
#[ignore]
fn direct_download_local_server() {
    use std::io::{Read, Write};
    let d = tempfile::tempdir().unwrap();
    let v = video(d.path());
    let body = std::fs::read(&v).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for (i, stream) in listener.incoming().take(2).enumerate() {
            let mut st = stream.unwrap();
            let mut buf = [0u8; 2048];
            let _ = st.read(&mut buf);
            if i == 0 {
                let _ = write!(st, "HTTP/1.1 200 OK\r\nContent-Type: video/mp4\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                let _ = st.write_all(&body);
            } else {
                let html = b"<html>not media</html>";
                let _ = write!(st, "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", html.len());
                let _ = st.write_all(html);
            }
        }
    });
    let out = d.path().join("dl");
    std::fs::create_dir(&out).unwrap();
    let (o, e) = ok(&[], &out, Op::Download { url: format!("http://127.0.0.1:{port}/media/sample%20clip.mp4"), acknowledged: true });
    assert_eq!(e.as_deref(), Some("Native (HTTP)"));
    assert_eq!(o.file_name().unwrap(), "sample clip.mp4");
    assert!(dur(&o) > 4.0);
    let (r, _) = job(&[], &out, Op::Download { url: format!("http://127.0.0.1:{port}/page"), acknowledged: true });
    assert_eq!(r.unwrap_err().0.key, "errors.notMediaUrl");
    let (r, _) = job(&[], &out, Op::Download { url: "https://www.youtube.com/watch?v=x".into(), acknowledged: true });
    assert_eq!(r.unwrap_err().0.key, "errors.sourceDisabled");
    no_temp_files(&out);
}

#[test]
#[ignore]
fn cancel_cleans_up() {
    let d = tempfile::tempdir().unwrap();
    let long = d.path().join("long.mp4");
    ff(&["-f", "lavfi", "-i", "testsrc=size=1920x1080:rate=30:duration=60", "-c:v", "libx264", "-preset", "ultrafast", &s(&long)]);
    let ctx = std::sync::Arc::new(Ctx::for_test());
    let req = JobRequest {
        inputs: vec![s(&long)], output_dir: s(d.path()), output_name: None, overwrite: false,
        operation: Op::VideoConvert {
            format: "webm".into(), preset: None, codec: None, accel: Accel::Cpu, crf: None, video_bitrate_k: None,
            audio_bitrate_k: None, max_width: None, max_height: None, target_size_mb: None,
        },
    };
    let c2 = ctx.clone();
    let h = std::thread::spawn(move || super::run(&c2, &req));
    std::thread::sleep(std::time::Duration::from_secs(3));
    ctx.cancel_for_test();
    let r = h.join().unwrap();
    assert_eq!(r.unwrap_err().0.key, "errors.cancelled");
    assert!(!d.path().join("long.webm").exists());
    no_temp_files(d.path());
}

#[test]
#[ignore]
fn screen_recording() {
    use crate::recorder::{Display, RecordOptions, Recorder};
    let d = tempfile::tempdir().unwrap();
    let rec = Recorder::default();
    let disp = vec![Display { id: "0".into(), name: "test".into(), x: 0, y: 0, width: 640, height: 480 }];
    let path = rec
        .start(
            RecordOptions { kind: "display".into(), id: "0".into(), fps: 15, format: "mp4".into(), output_dir: s(d.path()), file_name: "rec".into(), overwrite: false },
            disp,
        )
        .unwrap();
    assert!(rec.state().recording);
    std::thread::sleep(std::time::Duration::from_secs(3));
    let saved = rec.stop().unwrap();
    assert_eq!(saved, path);
    let p = probe(Path::new(&saved));
    println!("recording {:.2}s {:?}x{:?}", p.duration.unwrap_or(0.0), p.width, p.height);
    assert!(p.duration.unwrap_or(0.0) > 1.5);
    assert_eq!((p.width, p.height), (Some(640), Some(480)));
    no_temp_files(d.path());
}
