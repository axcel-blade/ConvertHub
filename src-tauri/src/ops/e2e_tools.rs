//! End-to-end tests for the optional external tools: Poppler, LibreOffice,
//! 7-Zip, ImageMagick, ExifTool and yt-dlp. Ignored by default; each test
//! prints SKIP and returns if its tool is not installed.

use super::e2e_tests::{job, no_temp_files, ok, probe, s};
use crate::deps::{self, Tool};
use crate::jobs::Operation as Op;
use std::path::{Path, PathBuf};

fn need(tool: Tool) -> Option<PathBuf> {
    let p = deps::find(tool);
    if p.is_none() {
        println!("SKIP: {} not installed", tool.id());
    }
    p
}

fn run_tool(p: &Path, args: &[&str]) -> String {
    let out = deps::command(p).args(args).output().unwrap();
    assert!(out.status.success(), "{} {args:?}: {}", p.display(), String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn lo_profile(dir: &Path) -> String {
    format!("-env:UserInstallation=file:///{}", dir.join("lo-profile").display().to_string().replace('\\', "/"))
}

/// A text PDF containing a real table, produced by LibreOffice from a CSV.
fn table_pdf(dir: &Path, so: &Path) -> PathBuf {
    let csv = dir.join("table.csv");
    std::fs::write(&csv, "City,Population,Area\nOslo,709037,454\nBergen,291940,465\nTrondheim,212660,342\n").unwrap();
    run_tool(so, &[&lo_profile(dir), "--headless", "--convert-to", "pdf", "--outdir", &s(dir), &s(&csv)]);
    let pdf = dir.join("table.pdf");
    assert!(pdf.exists());
    pdf
}

#[test]
#[ignore]
fn pdf_with_poppler_and_libreoffice() {
    let (Some(_), Some(so)) = (need(Tool::Pdftotext), need(Tool::Soffice)) else { return };
    println!("LibreOffice found at {}", so.display());
    let d = tempfile::tempdir().unwrap();
    let pdf = table_pdf(d.path(), &so);
    let out = d.path().join("out");
    std::fs::create_dir(&out).unwrap();
    let conv = |target: &str| ok(&[&pdf], &out, Op::PdfConvert { target: target.into() });

    let (o, e) = conv("txt");
    assert_eq!(e.as_deref(), Some("Poppler (pdftotext)"));
    let txt = std::fs::read_to_string(&o).unwrap();
    assert!(txt.contains("Bergen") && txt.contains("291940"), "{txt}");

    let (o, e) = conv("html");
    assert_eq!(e.as_deref(), Some("Poppler (pdftohtml)"));
    assert!(std::fs::read_to_string(&o).unwrap().contains("Trondheim"));
    let (o, _) = conv("htm");
    assert_eq!(o.extension().unwrap(), "htm");

    // XLSX columns are rebuilt from pdftotext -layout spacing; verify by
    // exporting the workbook back to CSV with LibreOffice.
    let (o, _) = conv("xlsx");
    let back = d.path().join("back");
    std::fs::create_dir(&back).unwrap();
    run_tool(&so, &[&lo_profile(d.path()), "--headless", "--convert-to", "csv", "--outdir", &s(&back), &s(&o)]);
    let csv = std::fs::read_to_string(back.join("table.csv")).unwrap();
    println!("xlsx round trip:\n{csv}");
    assert!(csv.lines().any(|l| l.starts_with("Bergen,291940,465")), "{csv}");

    let (o, e) = conv("docx");
    assert_eq!(e.as_deref(), Some("LibreOffice"));
    assert_eq!(&std::fs::read(&o).unwrap()[..2], b"PK", "DOCX is a zip package");
    let ole = [0xD0u8, 0xCF, 0x11, 0xE0];
    let (o, _) = conv("doc");
    assert_eq!(&std::fs::read(&o).unwrap()[..4], &ole, "DOC is an OLE file");
    let (o, _) = conv("xls");
    assert_eq!(&std::fs::read(&o).unwrap()[..4], &ole, "XLS is an OLE file");
    no_temp_files(&out);
}

#[test]
#[ignore]
fn pdf_images_and_scanned() {
    let (Some(_), Some(magick)) = (need(Tool::Pdfimages), need(Tool::Magick)) else { return };
    let d = tempfile::tempdir().unwrap();
    let png = d.path().join("pic.png");
    image::RgbImage::from_fn(120, 80, |x, y| image::Rgb([x as u8, (y * 3) as u8, 200])).save(&png).unwrap();
    let jpg = d.path().join("photo.jpg");
    image::RgbImage::from_fn(90, 60, |x, _| image::Rgb([(x * 2) as u8, 50, 50])).save(&jpg).unwrap();
    // Image-only ("scanned") two-page PDF.
    let scanned = d.path().join("scanned.pdf");
    run_tool(&magick, &[&s(&png), &s(&jpg), &s(&scanned)]);

    let (dir, e) = ok(&[&scanned], d.path(), Op::PdfImages { as_jpg: true });
    assert_eq!(e.as_deref(), Some("Poppler (pdfimages)"));
    let files: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().path()).collect();
    println!("extracted: {files:?}");
    assert!(files.len() >= 2);
    assert!(files.iter().all(|f| f.extension().unwrap() == "jpg"));

    let (r, _) = job(&[&scanned], d.path(), Op::PdfConvert { target: "txt".into() });
    assert_eq!(r.unwrap_err().0.key, "errors.pdfNoText");
}

#[test]
#[ignore]
fn archives_with_7zip() {
    let Some(sz) = need(Tool::SevenZip) else { return };
    println!("7-Zip found at {}", sz.display());
    let d = tempfile::tempdir().unwrap();
    let src = d.path().join("src");
    std::fs::create_dir_all(src.join("sub")).unwrap();
    std::fs::write(src.join("a.txt"), "alpha").unwrap();
    std::fs::write(src.join("sub").join("b.txt"), "beta").unwrap();

    // Native extraction of a real 7-Zip archive.
    let a7 = d.path().join("pack.7z");
    run_tool(&sz, &["a", "-bd", &s(&a7), &format!("{}{}*", s(&src), std::path::MAIN_SEPARATOR)]);
    let (o, e) = ok(&[&a7], d.path(), Op::ArchiveExtract);
    assert_eq!(e.as_deref(), Some("Native (7z)"));
    assert_eq!(std::fs::read_to_string(o.join("sub").join("b.txt")).unwrap(), "beta");

    // Password-protected 7z: clear error, nothing written.
    let enc = d.path().join("secret.7z");
    run_tool(&sz, &["a", "-bd", "-pHunter2", "-mhe=on", &s(&enc), &s(&src.join("a.txt"))]);
    let (r, _) = job(&[&enc], d.path(), Op::ArchiveExtract);
    println!("encrypted 7z -> {}", r.as_ref().unwrap_err());
    assert!(r.is_err());
    assert!(!d.path().join("secret").exists());
    no_temp_files(d.path());
}

/// Free tools cannot create RAR files, so fixtures come from
/// `scripts/make-rar-fixtures.py` via
/// CONVERTHUB_E2E_RAR_DIR: good.rar, plus optional traversal.rar and
/// encrypted.rar.
#[test]
#[ignore]
fn rar_extraction() {
    let Some(_) = need(Tool::SevenZip) else { return };
    let Ok(dir) = std::env::var("CONVERTHUB_E2E_RAR_DIR") else {
        println!("SKIP: set CONVERTHUB_E2E_RAR_DIR to a folder containing good.rar");
        return;
    };
    let dir = PathBuf::from(dir);
    let d = tempfile::tempdir().unwrap();
    let copy = |name: &str| {
        let p = d.path().join(name);
        std::fs::copy(dir.join(name), &p).unwrap();
        p
    };
    let good = copy("good.rar");
    let (o, e) = ok(&[&good], d.path(), Op::ArchiveExtract);
    assert_eq!(e.as_deref(), Some("7-Zip"));
    let n = std::fs::read_dir(&o).unwrap().count();
    println!("good.rar -> {n} entries");
    assert!(n > 0);
    if dir.join("traversal.rar").exists() {
        let (r, _) = job(&[&copy("traversal.rar")], d.path(), Op::ArchiveExtract);
        assert_eq!(r.unwrap_err().0.key, "errors.archiveUnsafePath");
    }
    if dir.join("encrypted.rar").exists() {
        let (r, _) = job(&[&copy("encrypted.rar")], d.path(), Op::ArchiveExtract);
        println!("encrypted.rar -> {}", r.as_ref().unwrap_err());
        assert!(r.is_err());
    }
    no_temp_files(d.path());
}

fn img_op(format: &str) -> Op {
    Op::ImageConvert {
        format: format.into(), quality: Some(80), width: None, height: None, percent: None,
        rotate: None, flip_h: false, flip_v: false, keep_metadata: false,
    }
}

#[test]
#[ignore]
fn heic_and_avif_with_imagemagick() {
    let Some(_) = need(Tool::Magick) else { return };
    let d = tempfile::tempdir().unwrap();
    let png = d.path().join("in.png");
    image::RgbImage::from_fn(96, 64, |x, y| image::Rgb([(x * 2) as u8, (y * 3) as u8, 120])).save(&png).unwrap();
    for fmt in ["heic", "avif"] {
        let (o, e) = ok(&[&png], d.path(), img_op(fmt));
        assert_eq!(e.as_deref(), Some("ImageMagick"));
        // Back to PNG through the HEIC/AVIF decode path.
        let back = d.path().join(format!("back_{fmt}"));
        std::fs::create_dir(&back).unwrap();
        let (b, _) = ok(&[&o], &back, img_op("png"));
        assert_eq!(image::image_dimensions(&b).unwrap(), (96, 64), "{fmt}");
        println!("{fmt}: {} bytes, round trip ok", std::fs::metadata(&o).unwrap().len());
    }
}

#[test]
#[ignore]
fn exif_tags_with_exiftool() {
    let Some(et) = need(Tool::Exiftool) else { return };
    let d = tempfile::tempdir().unwrap();
    let jpg = d.path().join("photo.jpg");
    image::RgbImage::from_pixel(40, 20, image::Rgb([200, 30, 30])).save(&jpg).unwrap();
    run_tool(&et, &["-overwrite_original", "-Artist=Original Author", "-GPSLatitude=59.91", "-GPSLatitudeRef=N", &s(&jpg)]);

    // The native EXIF reader sees the tags.
    let info = super::image::info(&jpg).unwrap();
    assert!(info.tags.iter().any(|(k, v)| k == "Artist" && v.contains("Original Author")), "{:?}", info.tags);

    // Default conversion strips metadata, including GPS.
    let strip = d.path().join("strip");
    std::fs::create_dir(&strip).unwrap();
    let (o, _) = ok(&[&jpg], &strip, img_op("jpg"));
    let tags = run_tool(&et, &["-s", "-Artist", "-GPSLatitude", &s(&o)]);
    assert!(tags.trim().is_empty(), "metadata should be stripped: {tags}");

    // keep_metadata copies it.
    let keep = d.path().join("keep");
    std::fs::create_dir(&keep).unwrap();
    let (o, _) = ok(&[&jpg], &keep, Op::ImageConvert {
        format: "jpg".into(), quality: Some(90), width: Some(20), height: None, percent: None,
        rotate: None, flip_h: false, flip_v: false, keep_metadata: true,
    });
    let tags = run_tool(&et, &["-s", "-Artist", &s(&o)]);
    assert!(tags.contains("Original Author"), "{tags}");

    // Writing new tags.
    let (o, e) = ok(&[&jpg], d.path(), Op::ImageTags {
        title: "Sunset".into(), artist: "ConvertHub Test".into(), copyright: "CC0".into(), comment: "hello".into(),
    });
    assert_eq!(e.as_deref(), Some("ExifTool"));
    let tags = run_tool(&et, &["-s", "-Artist", "-Copyright", "-ImageDescription", "-Title", &s(&o)]);
    println!("{tags}");
    assert!(tags.contains("ConvertHub Test") && tags.contains("CC0") && tags.contains("hello") && tags.contains("Sunset"), "{tags}");
    let none = d.path().join("none");
    std::fs::create_dir(&none).unwrap();
    let (r, _) = job(&[&jpg], &none, Op::ImageTags {
        title: String::new(), artist: String::new(), copyright: String::new(), comment: String::new(),
    });
    assert_eq!(r.unwrap_err().0.key, "errors.noTags");
}

/// Network test against archive.org: set CONVERTHUB_E2E_ARCHIVE_URL to a
/// small public-domain item.
#[test]
#[ignore]
fn archive_org_with_ytdlp() {
    let Some(_) = need(Tool::YtDlp) else { return };
    let Ok(url) = std::env::var("CONVERTHUB_E2E_ARCHIVE_URL") else {
        println!("SKIP: set CONVERTHUB_E2E_ARCHIVE_URL");
        return;
    };
    let d = tempfile::tempdir().unwrap();
    let (r, _) = job(&[], d.path(), Op::Download { url: url.clone(), acknowledged: false });
    assert_eq!(r.unwrap_err().0.key, "errors.downloadAck");
    let (o, e) = ok(&[], d.path(), Op::Download { url, acknowledged: true });
    assert_eq!(e.as_deref(), Some("yt-dlp"));
    println!("downloaded {} ({} bytes)", o.display(), std::fs::metadata(&o).unwrap().len());
    assert!(probe(&o).duration.unwrap_or(0.0) > 0.0);
    no_temp_files(d.path());
}
