//! Image conversion and editing. Common formats are handled natively in Rust;
//! HEIC/HEIF/AVIF and lossy WebP use ImageMagick or FFmpeg when installed.

use crate::deps::{self, Tool};
use crate::error::{R, UiMsg};
use crate::ffmpeg::{self, os};
use crate::fsutil::{ext_lower, TempGuard};
use crate::jobs::{Ctx, Operation as Op};
use crate::ui;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader};
use serde::Serialize;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

pub fn out_ext(format: &str) -> String {
    match format {
        "jpeg" => "jpg".into(),
        "tif" => "tiff".into(),
        f => f.to_string(),
    }
}

fn needs_external_decode(ext: &str) -> bool {
    matches!(ext, "heic" | "heif" | "avif")
}

/// Decode via ImageMagick or FFmpeg into a temporary PNG.
fn external_decode(ctx: &Ctx, path: &Path) -> R<DynamicImage> {
    let tmpdir = TempGuard::in_system_temp()?;
    let png = tmpdir.path.join("decoded.png");
    if let Some(magick) = deps::find(Tool::Magick) {
        let mut src = path.as_os_str().to_owned();
        src.push("[0]");
        ffmpeg::run_process(ctx, &magick, &[src, png.clone().into()], |_| {}, Some(std::time::Duration::from_secs(300)))?;
    } else if deps::find(Tool::Ffmpeg).is_some() {
        let mut args = vec!["-i".into(), path.into()];
        args.extend(os(&["-frames:v", "1"]));
        args.push(png.clone().into());
        ffmpeg::run(ctx, args, None, 0.0, 0.3)
            .map_err(|_| ui!("errors.heicDecode", "ext" => ext_lower(path)))?;
    } else {
        return Err(ui!("errors.heicDecode", "ext" => ext_lower(path)));
    }
    Ok(image::open(&png)?)
}

/// Decode and apply the EXIF orientation so output is visually upright.
pub fn decode(ctx: &Ctx, path: &Path) -> R<DynamicImage> {
    if needs_external_decode(&ext_lower(path)) {
        return external_decode(ctx, path);
    }
    let native = (|| -> Result<DynamicImage, image::ImageError> {
        let mut dec = ImageReader::open(path)?.with_guessed_format()?.into_decoder()?;
        let orientation = dec.orientation()?;
        let mut img = DynamicImage::from_decoder(dec)?;
        img.apply_orientation(orientation);
        Ok(img)
    })();
    match native {
        Ok(img) => Ok(img),
        // Fall back to external tools for exotic variants (e.g. CMYK TIFF).
        Err(e) => external_decode(ctx, path).map_err(|_| ui!("errors.imageDecode", "detail" => e)),
    }
}

fn transform(
    mut img: DynamicImage,
    width: Option<u32>,
    height: Option<u32>,
    percent: Option<f64>,
    rotate: Option<i32>,
    flip_h: bool,
    flip_v: bool,
) -> R<DynamicImage> {
    use image::imageops::FilterType::Lanczos3;
    let (w0, h0) = (img.width(), img.height());
    let target = match (width.filter(|v| *v > 0), height.filter(|v| *v > 0), percent.filter(|p| *p > 0.0)) {
        (Some(w), Some(h), _) => Some((w, h)),
        (Some(w), None, _) => Some((w, ((h0 as f64) * w as f64 / w0 as f64).round().max(1.0) as u32)),
        (None, Some(h), _) => Some((((w0 as f64) * h as f64 / h0 as f64).round().max(1.0) as u32, h)),
        (None, None, Some(p)) => {
            let f = p / 100.0;
            Some((((w0 as f64) * f).round().max(1.0) as u32, ((h0 as f64) * f).round().max(1.0) as u32))
        }
        _ => None,
    };
    if let Some((w, h)) = target {
        if w > 30_000 || h > 30_000 {
            return Err(ui!("errors.imageTooLarge"));
        }
        // `resize` keeps the aspect ratio inside the w x h box.
        img = img.resize(w, h, Lanczos3);
    }
    img = match rotate.unwrap_or(0).rem_euclid(360) {
        0 => img,
        90 => img.rotate90(),
        180 => img.rotate180(),
        270 => img.rotate270(),
        _ => return Err(ui!("errors.rotateAngle")),
    };
    if flip_h {
        img = img.fliph();
    }
    if flip_v {
        img = img.flipv();
    }
    Ok(img)
}

fn flatten_on_white(img: &DynamicImage) -> image::RgbImage {
    let rgba = img.to_rgba8();
    let mut out = image::RgbImage::new(rgba.width(), rgba.height());
    for (o, p) in out.pixels_mut().zip(rgba.pixels()) {
        let a = p[3] as u32;
        for c in 0..3 {
            o[c] = ((p[c] as u32 * a + 255 * (255 - a)) / 255) as u8;
        }
    }
    out
}

fn encode(ctx: &Ctx, img: &DynamicImage, out: &Path, format: &str, quality: Option<u8>) -> R<String> {
    let q = quality.unwrap_or(90).clamp(1, 100);
    match format {
        "jpg" => {
            let f = BufWriter::new(std::fs::File::create(out)?);
            let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(f, q);
            flatten_on_white(img).write_with_encoder(enc)?;
            Ok("Native (Rust)".into())
        }
        "webp" if q < 100 && ffmpeg::has_encoder("libwebp") => {
            let tmpdir = TempGuard::in_system_temp()?;
            let png = tmpdir.path.join("in.png");
            img.save_with_format(&png, ImageFormat::Png)?;
            let mut args = vec!["-i".into(), png.into()];
            args.extend(os(&["-c:v", "libwebp", "-quality", &q.to_string()]));
            args.push(out.into());
            ffmpeg::run(ctx, args, None, 0.5, 1.0)?;
            Ok("FFmpeg (libwebp)".into())
        }
        "webp" => {
            let f = BufWriter::new(std::fs::File::create(out)?);
            img.to_rgba8().write_with_encoder(image::codecs::webp::WebPEncoder::new_lossless(f))?;
            Ok("Native (lossless WebP)".into())
        }
        "heic" | "avif" => {
            let magick = deps::find(Tool::Magick).ok_or_else(|| ui!("errors.heicEncode", "ext" => format))?;
            let tmpdir = TempGuard::in_system_temp()?;
            let png = tmpdir.path.join("in.png");
            img.save_with_format(&png, ImageFormat::Png)?;
            let args = vec![png.into(), "-quality".into(), q.to_string().into(), out.into()];
            ffmpeg::run_process(ctx, &magick, &args, |_| {}, Some(std::time::Duration::from_secs(600)))
                .map_err(|_| ui!("errors.heicEncode", "ext" => format))?;
            Ok("ImageMagick".into())
        }
        "ico" => {
            let img = if img.width() > 256 || img.height() > 256 {
                img.resize(256, 256, image::imageops::FilterType::Lanczos3)
            } else {
                img.clone()
            };
            img.to_rgba8().save_with_format(out, ImageFormat::Ico)?;
            Ok("Native (Rust)".into())
        }
        "gif" | "tga" | "qoi" | "png" | "tiff" => {
            let fmt = ImageFormat::from_extension(format).ok_or_else(|| ui!("errors.unsupportedOutput", "format" => format))?;
            img.to_rgba8().save_with_format(out, fmt)?;
            Ok("Native (Rust)".into())
        }
        "bmp" => {
            img.to_rgb8().save_with_format(out, ImageFormat::Bmp)?;
            Ok("Native (Rust)".into())
        }
        other => Err(ui!("errors.unsupportedOutput", "format" => other)),
    }
}

pub fn run(ctx: &Ctx, op: &Op, input: &Path, out: &Path) -> R<Vec<PathBuf>> {
    match op {
        Op::ImageConvert { format, quality, width, height, percent, rotate, flip_h, flip_v, keep_metadata } => {
            ctx.stage(UiMsg::new("stage.decoding"));
            let img = decode(ctx, input)?;
            ctx.progress(0.4);
            ctx.check()?;
            let img = transform(img, *width, *height, *percent, *rotate, *flip_h, *flip_v)?;
            ctx.progress(0.6);
            let tmp = TempGuard::file_for(out);
            ctx.stage(UiMsg::new("stage.encoding"));
            let engine = encode(ctx, &img, &tmp.path, &out_ext(format), *quality)?;
            ctx.set_engine(engine);
            if *keep_metadata {
                match deps::find(Tool::Exiftool) {
                    Some(et) => {
                        let args = vec![
                            "-overwrite_original".into(),
                            "-TagsFromFile".into(),
                            input.into(),
                            "-all:all".into(),
                            "-Orientation#=1".into(),
                            tmp.path.clone().into(),
                        ];
                        // Metadata copy is best effort; the conversion itself succeeded.
                        if ffmpeg::run_process(ctx, &et, &args, |_| {}, Some(std::time::Duration::from_secs(60))).is_err() {
                            ctx.stage(UiMsg::new("result.metadataNotKept"));
                        }
                    }
                    None => ctx.stage(UiMsg::new("result.metadataNotKept")),
                }
            }
            tmp.commit(out)?;
        }
        Op::ImageTags { title, artist, copyright, comment } => {
            let et = deps::require(Tool::Exiftool)?;
            let tmp = TempGuard::file_for(out);
            std::fs::copy(input, &tmp.path)?;
            let mut args: Vec<std::ffi::OsString> = vec!["-overwrite_original".into()];
            for (tag, v) in [("Title", title), ("Artist", artist), ("Copyright", copyright), ("ImageDescription", comment)] {
                if !v.trim().is_empty() {
                    args.push(format!("-{tag}={}", v.trim()).into());
                }
            }
            if args.len() == 1 {
                return Err(ui!("errors.noTags"));
            }
            args.push(tmp.path.clone().into());
            ctx.set_engine("ExifTool");
            ffmpeg::run_process(ctx, &et, &args, |_| {}, Some(std::time::Duration::from_secs(60)))?;
            tmp.commit(out)?;
        }
        _ => unreachable!(),
    }
    ctx.progress(1.0);
    Ok(vec![out.to_path_buf()])
}

#[derive(Serialize)]
pub struct ImageInfo {
    pub width: u32,
    pub height: u32,
    pub format: String,
    pub tags: Vec<(String, String)>,
}

/// Read dimensions and EXIF tags for display (no processing tools needed
/// for common formats).
pub fn info(path: &Path) -> R<ImageInfo> {
    let reader = ImageReader::open(path)?.with_guessed_format()?;
    let format = reader.format().map(|f| format!("{f:?}")).unwrap_or_else(|| ext_lower(path).to_uppercase());
    let (width, height) = reader.into_dimensions().unwrap_or((0, 0));
    let mut tags = vec![];
    if let Ok(f) = std::fs::File::open(path) {
        if let Ok(exif) = exif::Reader::new().read_from_container(&mut std::io::BufReader::new(f)) {
            for field in exif.fields().take(200) {
                if field.tag == exif::Tag::MakerNote {
                    continue;
                }
                let v = field.display_value().with_unit(&exif).to_string();
                tags.push((field.tag.to_string(), v.chars().take(200).collect()));
            }
        }
    }
    Ok(ImageInfo { width, height, format, tags })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transform_resize_rotate_flip() {
        let img = DynamicImage::ImageRgb8(image::RgbImage::new(100, 50));
        let r = transform(img.clone(), Some(50), None, None, None, false, false).unwrap();
        assert_eq!((r.width(), r.height()), (50, 25));
        let r = transform(img.clone(), None, None, Some(200.0), Some(90), true, true).unwrap();
        assert_eq!((r.width(), r.height()), (100, 200));
        assert!(transform(img, None, None, None, Some(45), false, false).is_err());
    }

    #[test]
    fn convert_png_to_jpg_and_webp() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("in.png");
        image::RgbaImage::from_pixel(64, 32, image::Rgba([10, 200, 30, 255])).save(&src).unwrap();
        let ctx = Ctx::for_test();
        for fmt in ["jpg", "webp", "bmp", "gif", "tiff", "ico", "qoi"] {
            let out = d.path().join(format!("out.{fmt}"));
            let op = Op::ImageConvert {
                format: fmt.into(), quality: Some(100), width: Some(32), height: None, percent: None,
                rotate: Some(90), flip_h: false, flip_v: false, keep_metadata: false,
            };
            run(&ctx, &op, &src, &out).unwrap();
            let img = image::open(&out).unwrap();
            assert_eq!((img.width(), img.height()), (16, 32), "{fmt}");
        }
    }

    #[test]
    fn flatten() {
        let mut rgba = image::RgbaImage::new(1, 1);
        rgba.put_pixel(0, 0, image::Rgba([0, 0, 0, 0]));
        let out = flatten_on_white(&DynamicImage::ImageRgba8(rgba));
        assert_eq!(out.get_pixel(0, 0).0, [255, 255, 255]);
    }
}
