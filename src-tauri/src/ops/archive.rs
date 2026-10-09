//! Safe archive extraction (ZIP and 7z natively, RAR via an installed 7-Zip).
//!
//! Protections: every entry path is sanitized (no absolute paths, drive
//! letters, `..`, reserved names), symlinks are never created, total
//! uncompressed size and entry count are capped (zip-bomb guard), and
//! extraction happens in a temporary folder that is only moved into place
//! once everything succeeded, so an existing folder is never partially
//! overwritten.

use crate::deps::{self, Tool};
use crate::error::R;
use crate::ffmpeg::run_process;
use crate::fsutil::{ext_lower, safe_relative_path, TempGuard};
use crate::jobs::Ctx;
use crate::ui;
use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};

pub const MAX_TOTAL_BYTES: u64 = 64 * 1024 * 1024 * 1024;
pub const MAX_ENTRIES: usize = 200_000;
/// Reject entries that inflate more than this ratio (classic zip bombs).
const MAX_RATIO: u64 = 1000;

pub fn run(ctx: &Ctx, input: &Path, out_dir: &Path) -> R<Vec<PathBuf>> {
    let tmp = TempGuard::dir_for(out_dir)?;
    match ext_lower(input).as_str() {
        "zip" => {
            ctx.set_engine("Native (zip)");
            extract_zip(ctx, input, &tmp.path)?
        }
        "7z" => {
            ctx.set_engine("Native (7z)");
            extract_7z(ctx, input, &tmp.path)?
        }
        "rar" => extract_with_7zip(ctx, input, &tmp.path)?,
        other => return Err(ui!("errors.unsupportedType", "path" => input.display(), "ext" => other)),
    }
    tmp.commit(out_dir)?;
    Ok(vec![out_dir.to_path_buf()])
}

fn extract_zip(ctx: &Ctx, input: &Path, dest: &Path) -> R<()> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(input)?)
        .map_err(|e| ui!("errors.archiveCorrupt", "detail" => e))?;
    if zip.len() > MAX_ENTRIES {
        return Err(ui!("errors.archiveTooManyEntries"));
    }
    let declared: u64 = (0..zip.len()).filter_map(|i| zip.by_index_raw(i).ok().map(|f| f.size())).sum();
    if declared > MAX_TOTAL_BYTES {
        return Err(ui!("errors.archiveTooLarge"));
    }
    let mut written: u64 = 0;
    let n = zip.len();
    for i in 0..n {
        ctx.check()?;
        let mut entry = match zip.by_index(i) {
            Ok(e) => e,
            Err(zip::result::ZipError::UnsupportedArchive(m)) if m.contains("Password") => {
                return Err(ui!("errors.archiveEncrypted"))
            }
            Err(e) => return Err(ui!("errors.archiveCorrupt", "detail" => e)),
        };
        if entry.encrypted() {
            return Err(ui!("errors.archiveEncrypted"));
        }
        let name = entry.name().map(|n| n.to_string()).unwrap_or_default();
        let Some(rel) = safe_relative_path(&name) else {
            return Err(ui!("errors.archiveUnsafePath", "entry" => name));
        };
        if entry.is_symlink() {
            continue; // never materialize links
        }
        let target = dest.join(&rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&target)?;
            continue;
        }
        if let Some(p) = target.parent() {
            std::fs::create_dir_all(p)?;
        }
        if entry.compressed_size() > 0 && entry.size() / entry.compressed_size().max(1) > MAX_RATIO {
            return Err(ui!("errors.archiveBomb", "entry" => &name));
        }
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(&target)?;
        let limit = MAX_TOTAL_BYTES - written;
        let copied = std::io::copy(&mut (&mut entry).take(limit + 1), &mut f)?;
        written += copied;
        if written > MAX_TOTAL_BYTES {
            return Err(ui!("errors.archiveTooLarge"));
        }
        ctx.progress((i + 1) as f64 / n as f64);
    }
    Ok(())
}

fn extract_7z(ctx: &Ctx, input: &Path, dest: &Path) -> R<()> {
    let mut written: u64 = 0;
    let mut count = 0usize;
    let mut failure: Option<crate::error::UiError> = None;
    let res = sevenz_rust2::decompress_file_with_extract_fn(input, dest, |entry, reader, _suggested| {
        let fail = |e: crate::error::UiError, slot: &mut Option<crate::error::UiError>| {
            *slot = Some(e);
            Err(std::io::Error::other("aborted").into())
        };
        if ctx.is_cancelled() {
            return fail(ui!("errors.cancelled"), &mut failure);
        }
        count += 1;
        if count > MAX_ENTRIES {
            return fail(ui!("errors.archiveTooManyEntries"), &mut failure);
        }
        let name = entry.name();
        let Some(rel) = safe_relative_path(name) else {
            return fail(ui!("errors.archiveUnsafePath", "entry" => name), &mut failure);
        };
        let target = dest.join(rel);
        if entry.is_directory() {
            std::fs::create_dir_all(&target)?;
            return Ok(true);
        }
        if let Some(p) = target.parent() {
            std::fs::create_dir_all(p)?;
        }
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(&target)?;
        let copied = std::io::copy(&mut reader.take(MAX_TOTAL_BYTES - written + 1), &mut f)?;
        written += copied;
        if written > MAX_TOTAL_BYTES {
            return fail(ui!("errors.archiveTooLarge"), &mut failure);
        }
        Ok(true)
    });
    if let Some(e) = failure {
        return Err(e);
    }
    res.map_err(|e| {
        let s = e.to_string();
        if s.to_lowercase().contains("password") {
            ui!("errors.archiveEncrypted")
        } else {
            ui!("errors.archiveCorrupt", "detail" => s)
        }
    })?;
    ctx.progress(1.0);
    Ok(())
}

/// RAR (and other 7-Zip-readable formats) via an external 7-Zip. Entry names
/// are listed and validated *before* extracting; afterwards the tree is
/// re-checked for links or anything resolving outside the destination.
fn extract_with_7zip(ctx: &Ctx, input: &Path, dest: &Path) -> R<()> {
    let sz = deps::find(Tool::SevenZip).ok_or_else(|| ui!("errors.rarNeeds7zip"))?;
    ctx.set_engine("7-Zip");
    let mut paths = vec![];
    let mut total: u64 = 0;
    let list_args: Vec<OsString> = vec!["l".into(), "-slt".into(), "-ba".into(), "-p".into(), input.into()];
    run_process(
        ctx,
        &sz,
        &list_args,
        |line| {
            if let Some(p) = line.strip_prefix("Path = ") {
                paths.push(p.to_string());
            } else if let Some(s) = line.strip_prefix("Size = ") {
                total += s.trim().parse::<u64>().unwrap_or(0);
            }
        },
        Some(std::time::Duration::from_secs(300)),
    )
    .map_err(|e| match e.0.vars.get("detail") {
        Some(d) if d.contains("password") || d.contains("Wrong password") => ui!("errors.archiveEncrypted"),
        _ => e,
    })?;
    if paths.len() > MAX_ENTRIES {
        return Err(ui!("errors.archiveTooManyEntries"));
    }
    if total > MAX_TOTAL_BYTES {
        return Err(ui!("errors.archiveTooLarge"));
    }
    for p in &paths {
        if safe_relative_path(p).is_none() {
            return Err(ui!("errors.archiveUnsafePath", "entry" => p));
        }
    }
    ctx.progress(0.1);
    let mut out_arg = OsString::from("-o");
    out_arg.push(dest);
    // -snl- : never store/restore symlinks as links; -p : empty password (no prompt).
    let args: Vec<OsString> =
        vec!["x".into(), "-y".into(), "-snl-".into(), "-p".into(), "-bsp1".into(), out_arg, input.into()];
    run_process(
        ctx,
        &sz,
        &args,
        |line| {
            if let Some(pct) = line.trim().split('%').next().and_then(|s| s.trim().parse::<f64>().ok()) {
                ctx.progress(0.1 + 0.85 * pct / 100.0);
            }
        },
        None,
    )?;
    verify_tree(dest)
}

/// Remove symlinks and make sure every file resolves inside `root`.
pub fn verify_tree(root: &Path) -> R<()> {
    let root_c = root.canonicalize()?;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(&dir)? {
            let e = e?;
            let ft = e.file_type()?;
            let p = e.path();
            if ft.is_symlink() {
                let _ = std::fs::remove_file(&p).or_else(|_| std::fs::remove_dir(&p));
                continue;
            }
            if !p.canonicalize()?.starts_with(&root_c) {
                return Err(ui!("errors.archiveUnsafePath", "entry" => p.display()));
            }
            if ft.is_dir() {
                stack.push(p);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let f = std::fs::File::create(path).unwrap();
        let mut z = zip::ZipWriter::new(f);
        for (name, data) in entries {
            z.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            z.write_all(data).unwrap();
        }
        z.finish().unwrap();
    }

    #[test]
    fn zip_traversal_is_rejected_and_nothing_written() {
        let dir = tempfile::tempdir().unwrap();
        let zp = dir.path().join("evil.zip");
        make_zip(&zp, &[("ok.txt", b"fine"), ("../escape.txt", b"bad")]);
        let out = dir.path().join("evil");
        let ctx = Ctx::for_test();
        let err = run(&ctx, &zp, &out).unwrap_err();
        assert_eq!(err.0.key, "errors.archiveUnsafePath");
        assert!(!dir.path().join("escape.txt").exists());
        assert!(!out.exists(), "partial output must be cleaned up");
        // No temp folders left behind either.
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn zip_extracts() {
        let dir = tempfile::tempdir().unwrap();
        let zp = dir.path().join("good.zip");
        make_zip(&zp, &[("a/b.txt", b"hello"), ("c.txt", b"world")]);
        let out = dir.path().join("good");
        run(&Ctx::for_test(), &zp, &out).unwrap();
        assert_eq!(std::fs::read(out.join("a").join("b.txt")).unwrap(), b"hello");
    }

    #[test]
    fn verify_tree_accepts_normal() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("a/b")).unwrap();
        std::fs::write(dir.path().join("a/b/c.txt"), b"x").unwrap();
        assert!(verify_tree(dir.path()).is_ok());
    }
}
