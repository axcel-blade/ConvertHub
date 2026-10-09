//! Backend input validation. The frontend filters files too, but every job
//! is re-checked here: existence, type (extension + content sniffing), and size.

use crate::error::R;
use crate::fsutil::ext_lower;
use crate::ui;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Video,
    Audio,
    Image,
    Pdf,
    Archive,
    /// Video *or* audio (joins, mixes and tag editing accept both).
    Media,
}

pub const VIDEO_IN: &[&str] = &[
    "mp4", "m4v", "mkv", "mov", "avi", "wmv", "flv", "webm", "mpg", "mpeg", "ts", "m2ts", "mts",
    "3gp", "3g2", "vob", "ogv", "asf", "rm", "rmvb", "f4v", "divx", "gif",
];
pub const AUDIO_IN: &[&str] = &[
    "mp3", "m4a", "aac", "wav", "flac", "ogg", "oga", "opus", "wma", "aiff", "aif", "ac3", "amr",
    "ape", "mka", "mp2", "wv", "caf",
];
pub const IMAGE_IN: &[&str] = &[
    "png", "jpg", "jpeg", "jfif", "webp", "bmp", "gif", "tif", "tiff", "ico", "tga", "pnm", "pbm",
    "pgm", "ppm", "qoi", "heic", "heif", "avif",
];
pub const ARCHIVE_IN: &[&str] = &["zip", "7z", "rar"];

pub const VIDEO_OUT: &[&str] =
    &["mp4", "mkv", "mov", "avi", "webm", "wmv", "flv", "mpg", "3gp", "m4v", "ts", "gif"];
pub const AUDIO_OUT: &[&str] =
    &["mp3", "m4a", "aac", "ogg", "opus", "flac", "wav", "wma", "aiff", "ac3"];
pub const IMAGE_OUT: &[&str] =
    &["png", "jpg", "webp", "bmp", "gif", "tiff", "ico", "tga", "qoi", "heic", "avif"];

/// Default per-category size ceilings (bytes). Generous, but they stop
/// accidental multi-terabyte selections and decompression-bomb style inputs.
pub fn max_size(cat: Category) -> u64 {
    const GB: u64 = 1024 * 1024 * 1024;
    match cat {
        Category::Video | Category::Media => 200 * GB,
        Category::Audio => 20 * GB,
        Category::Image => 2 * GB,
        Category::Pdf => 4 * GB,
        Category::Archive => 100 * GB,
    }
}

fn allowed(cat: Category, ext: &str) -> bool {
    match cat {
        Category::Video => VIDEO_IN.contains(&ext),
        Category::Audio => AUDIO_IN.contains(&ext),
        Category::Media => VIDEO_IN.contains(&ext) || AUDIO_IN.contains(&ext),
        Category::Image => IMAGE_IN.contains(&ext),
        Category::Pdf => ext == "pdf",
        Category::Archive => ARCHIVE_IN.contains(&ext),
    }
}

#[derive(Debug, Serialize)]
pub struct InputCheck {
    pub path: String,
    pub ok: bool,
    pub size: u64,
    pub error: Option<crate::error::UiMsg>,
}

pub fn check_file(path: &Path, cat: Category) -> R<u64> {
    let meta = std::fs::metadata(path).map_err(|_| ui!("errors.inputMissing", "path" => path.display()))?;
    if !meta.is_file() {
        return Err(ui!("errors.notAFile", "path" => path.display()));
    }
    let ext = ext_lower(path);
    if !allowed(cat, &ext) {
        return Err(ui!("errors.unsupportedType", "path" => path.display(), "ext" => ext));
    }
    if meta.len() == 0 {
        return Err(ui!("errors.emptyFile", "path" => path.display()));
    }
    if meta.len() > max_size(cat) {
        return Err(ui!("errors.tooLarge", "path" => path.display(), "limitGb" => max_size(cat) / (1024 * 1024 * 1024)));
    }
    sniff(path, cat)?;
    Ok(meta.len())
}

/// Content sniffing: reject files whose magic bytes clearly identify a
/// different, unrelated type (e.g. an executable renamed to .mp4). Unknown
/// signatures are allowed through because many media containers have none
/// that `infer` recognises; the processing tool reports those failures.
fn sniff(path: &Path, cat: Category) -> R<()> {
    use std::io::Read;
    let mut buf = [0u8; 8192];
    let n = std::fs::File::open(path)?.read(&mut buf)?;
    let Some(kind) = infer::get(&buf[..n]) else { return Ok(()) };
    let mt = kind.matcher_type();
    use infer::MatcherType as M;
    let ok = match cat {
        Category::Video | Category::Media | Category::Audio => {
            matches!(mt, M::Video | M::Audio | M::Image) // gif/webm etc. overlap
        }
        Category::Image => matches!(mt, M::Image | M::Video), // heic sniffs as video on some versions
        Category::Pdf => kind.mime_type() == "application/pdf",
        Category::Archive => matches!(mt, M::Archive),
    };
    if ok {
        Ok(())
    } else {
        Err(ui!("errors.contentMismatch", "path" => path.display(), "detected" => kind.mime_type()))
    }
}

pub fn check_many(paths: &[String], cat: Category) -> Vec<InputCheck> {
    paths
        .iter()
        .map(|p| match check_file(Path::new(p), cat) {
            Ok(size) => InputCheck { path: p.clone(), ok: true, size, error: None },
            Err(e) => InputCheck { path: p.clone(), ok: false, size: 0, error: Some(e.0) },
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_renamed_executable() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("movie.mp4");
        let mut data = b"MZ\x90\x00\x03\x00\x00\x00".to_vec();
        data.resize(512, 0);
        // Minimal PE: infer needs "PE\0\0" at the e_lfanew offset.
        data[0x3c] = 0x80;
        data[0x80..0x84].copy_from_slice(b"PE\0\0");
        std::fs::write(&p, data).unwrap();
        assert!(check_file(&p, Category::Video).is_err());
    }

    #[test]
    fn rejects_wrong_extension_and_empty() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.exe");
        std::fs::write(&p, b"hello").unwrap();
        assert!(check_file(&p, Category::Video).is_err());
        let e = dir.path().join("e.mp3");
        std::fs::write(&e, b"").unwrap();
        assert!(check_file(&e, Category::Audio).is_err());
    }

    #[test]
    fn accepts_png() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.png");
        image::RgbImage::new(4, 4).save(&p).unwrap();
        assert!(check_file(&p, Category::Image).is_ok());
    }
}
