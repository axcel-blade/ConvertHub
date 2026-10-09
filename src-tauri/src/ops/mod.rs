//! Operation dispatch: input validation, output planning (with overwrite
//! protection), then the category-specific implementation.

pub mod archive;
pub mod audio;
pub mod disc;
pub mod download;
pub mod image;
pub mod pdf;
pub mod video;

#[cfg(test)]
mod e2e_tests;
#[cfg(test)]
mod e2e_tools;

use crate::error::R;
use crate::fsutil::{ext_lower, file_stem, plan_output, sanitize_component};
use crate::jobs::{Ctx, JobRequest, Operation as Op};
use crate::ui;
use crate::validate::{check_file, Category};
use std::path::{Path, PathBuf};

pub enum Target {
    File(PathBuf),
    Dir(PathBuf),
    /// Name is only known while running (downloads).
    Runtime(PathBuf),
}

fn input_rule(op: &Op) -> (Option<Category>, usize, usize) {
    // (category, min inputs, max inputs); category None = directory input.
    match op {
        Op::VideoJoin { .. } => (Some(Category::Video), 2, 200),
        Op::AudioJoin { .. } | Op::AudioMix { .. } => (Some(Category::Media), 2, 200),
        Op::PdfMerge => (Some(Category::Pdf), 2, 500),
        Op::VideoConvert { .. }
        | Op::VideoTrim { .. }
        | Op::VideoSplit { .. }
        | Op::VideoMux { .. }
        | Op::VideoCrop { .. }
        | Op::VideoDelogo { .. }
        | Op::VideoRepair => (Some(Category::Video), 1, 1),
        Op::AudioConvert { .. } | Op::AudioTrim { .. } | Op::AudioSplit { .. } | Op::AudioRepair => {
            (Some(Category::Media), 1, 1)
        }
        Op::MediaTags { .. } => (Some(Category::Media), 1, 1),
        Op::ImageConvert { .. } | Op::ImageTags { .. } => (Some(Category::Image), 1, 1),
        Op::PdfConvert { .. } | Op::PdfImages { .. } => (Some(Category::Pdf), 1, 1),
        Op::ArchiveExtract => (Some(Category::Archive), 1, 1),
        Op::DvdRip { .. } | Op::BlurayRip { .. } | Op::CdRip { .. } => (None, 1, 1),
        Op::Download { .. } => (None, 0, 0),
    }
}

fn validate_inputs(req: &JobRequest) -> R<()> {
    let (cat, min, max) = input_rule(&req.operation);
    let n = req.inputs.len();
    if n < min || n > max {
        return Err(ui!("errors.inputCount", "min" => min, "max" => max, "got" => n));
    }
    for i in &req.inputs {
        let p = Path::new(i);
        match cat {
            Some(c) => {
                check_file(p, c)?;
            }
            None => {
                if !p.is_dir() {
                    return Err(ui!("errors.notAFolder", "path" => p.display()));
                }
            }
        }
    }
    if let Op::VideoMux { audio } = &req.operation {
        check_file(Path::new(audio), Category::Media)?;
    }
    Ok(())
}

fn ext_of_input(req: &JobRequest) -> String {
    req.inputs.first().map(|i| ext_lower(Path::new(i))).unwrap_or_default()
}

/// Compute where the job will write, without touching the filesystem.
pub fn target(req: &JobRequest) -> R<Target> {
    let dir = PathBuf::from(&req.output_dir);
    let first = req.inputs.first().map(PathBuf::from).unwrap_or_default();
    let stem = match req.output_name.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(n) => sanitize_component(n),
        None => file_stem(&first),
    };
    let same = ext_of_input(req);
    let file = |suffix: &str, ext: &str| -> Target {
        let s = if req.output_name.is_some() { stem.clone() } else { format!("{stem}{suffix}") };
        let mut p = dir.join(sanitize_component(&format!("{s}.{ext}")));
        // Never write onto the input itself.
        if req.inputs.iter().any(|i| Path::new(i) == p) {
            p = dir.join(sanitize_component(&format!("{s}_converted.{ext}")));
        }
        Target::File(p)
    };
    let folder = |suffix: &str| -> Target {
        let s = if req.output_name.is_some() { stem.clone() } else { format!("{stem}{suffix}") };
        Target::Dir(dir.join(sanitize_component(&s)))
    };
    Ok(match &req.operation {
        Op::VideoConvert { format, preset, .. } => {
            let f = preset.as_deref().and_then(crate::presets::get).map(|p| p.format).unwrap_or(format);
            file("", f)
        }
        Op::VideoTrim { .. } => file("_clip", &same),
        Op::VideoSplit { .. } | Op::AudioSplit { .. } => folder("_parts"),
        Op::VideoJoin { format } | Op::AudioJoin { format } => file("_joined", format),
        Op::VideoMux { .. } => file("_muxed", if same == "mp4" || same == "mov" || same == "mkv" { &same } else { "mkv" }),
        Op::VideoCrop { .. } => file("_crop", &same),
        Op::VideoDelogo { .. } => file("_delogo", &same),
        Op::VideoRepair | Op::AudioRepair => file("_repaired", &video::repair_ext(&same)),
        Op::AudioConvert { format, .. } => file("", format),
        Op::AudioTrim { .. } => file("_clip", &same),
        Op::AudioMix { format } => file("_mix", format),
        Op::MediaTags { .. } => file("_tagged", &same),
        Op::ImageConvert { format, .. } => file("", &image::out_ext(format)),
        Op::ImageTags { .. } => file("_tagged", &same),
        Op::PdfMerge => file("_merged", "pdf"),
        Op::PdfConvert { target } => file("", target),
        Op::PdfImages { .. } => folder("_images"),
        Op::ArchiveExtract => folder(""),
        Op::DvdRip { format, preset, .. } | Op::BlurayRip { format, preset, .. } => {
            let f = preset.as_deref().and_then(crate::presets::get).map(|p| p.format).unwrap_or(format);
            file("", f)
        }
        Op::CdRip { .. } => folder("_tracks"),
        Op::Download { .. } => Target::Runtime(dir),
    })
}

/// Existing paths the job would overwrite (shown to the user before queuing).
pub fn conflicts(req: &JobRequest) -> Vec<String> {
    match target(req) {
        Ok(Target::File(p)) | Ok(Target::Dir(p)) if p.exists() => vec![p.display().to_string()],
        _ => vec![],
    }
}

pub fn run(ctx: &Ctx, req: &JobRequest) -> R<Vec<PathBuf>> {
    validate_inputs(req)?;
    let target = target(req)?;
    let out_dir = PathBuf::from(&req.output_dir);
    // Re-check existence and overwrite permission on the backend.
    let final_path = match &target {
        Target::File(p) | Target::Dir(p) => {
            let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            plan_output(&out_dir, &name, "", req.overwrite)?
        }
        Target::Runtime(d) => {
            if !d.is_dir() {
                return Err(ui!("errors.outputDirMissing", "path" => d.display()));
            }
            d.clone()
        }
    };
    let inputs: Vec<PathBuf> = req.inputs.iter().map(PathBuf::from).collect();
    ctx.progress(0.0);
    match &req.operation {
        Op::VideoConvert { .. }
        | Op::VideoTrim { .. }
        | Op::VideoSplit { .. }
        | Op::VideoJoin { .. }
        | Op::VideoMux { .. }
        | Op::VideoCrop { .. }
        | Op::VideoDelogo { .. }
        | Op::VideoRepair
        | Op::MediaTags { .. } => video::run(ctx, &req.operation, &inputs, &final_path),
        Op::AudioConvert { .. }
        | Op::AudioTrim { .. }
        | Op::AudioSplit { .. }
        | Op::AudioJoin { .. }
        | Op::AudioMix { .. }
        | Op::AudioRepair => audio::run(ctx, &req.operation, &inputs, &final_path),
        Op::ImageConvert { .. } | Op::ImageTags { .. } => image::run(ctx, &req.operation, &inputs[0], &final_path),
        Op::PdfMerge | Op::PdfConvert { .. } | Op::PdfImages { .. } => {
            pdf::run(ctx, &req.operation, &inputs, &final_path)
        }
        Op::DvdRip { .. } | Op::BlurayRip { .. } | Op::CdRip { .. } => {
            disc::run(ctx, &req.operation, &inputs[0], &final_path)
        }
        Op::Download { url, acknowledged } => download::run(ctx, url, *acknowledged, &final_path, req.overwrite),
        Op::ArchiveExtract => archive::run(ctx, &inputs[0], &final_path),
    }
}
