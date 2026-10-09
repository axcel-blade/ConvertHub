//! Output planning, safe temporary files, and path sanitizing.
//!
//! Every job writes into a hidden temporary file/folder next to the final
//! output and only renames it into place on success. [`TempGuard`] deletes the
//! temporary on drop, so failed and cancelled jobs never leave partial files.

use crate::error::R;
use crate::ui;
use std::path::{Component, Path, PathBuf};

pub fn file_stem(p: &Path) -> String {
    let s = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let s = sanitize_component(&s);
    if s.is_empty() { "output".into() } else { s }
}

pub fn ext_lower(p: &Path) -> String {
    p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

/// Resolve the final output path, refusing to clobber unless allowed.
pub fn plan_output(dir: &Path, stem: &str, ext: &str, overwrite: bool) -> R<PathBuf> {
    if !dir.is_dir() {
        return Err(ui!("errors.outputDirMissing", "path" => dir.display()));
    }
    let name = if ext.is_empty() { stem.to_string() } else { format!("{stem}.{ext}") };
    let path = dir.join(sanitize_component(&name));
    if path.exists() && !overwrite {
        return Err(ui!("errors.outputExists", "path" => path.display()));
    }
    Ok(path)
}

pub struct TempGuard {
    pub path: PathBuf,
    committed: bool,
}

impl TempGuard {
    /// A temporary sibling of `final_path` with the same extension (so tools
    /// like FFmpeg infer the right container).
    pub fn file_for(final_path: &Path) -> Self {
        let dir = final_path.parent().unwrap_or(Path::new("."));
        let ext = ext_lower(final_path);
        let id = uuid::Uuid::new_v4().simple().to_string();
        let name = if ext.is_empty() {
            format!(".converthub-{id}.part")
        } else {
            format!(".converthub-{id}.part.{ext}")
        };
        Self { path: dir.join(name), committed: false }
    }

    pub fn dir_for(final_dir: &Path) -> R<Self> {
        let parent = final_dir.parent().unwrap_or(Path::new("."));
        let id = uuid::Uuid::new_v4().simple().to_string();
        let path = parent.join(format!(".converthub-{id}.partdir"));
        std::fs::create_dir_all(&path)?;
        Ok(Self { path, committed: false })
    }

    pub fn in_system_temp() -> R<Self> {
        let id = uuid::Uuid::new_v4().simple().to_string();
        let path = std::env::temp_dir().join(format!("converthub-{id}"));
        std::fs::create_dir_all(&path)?;
        Ok(Self { path, committed: false })
    }

    /// Move the temporary into place (replacing an existing target, which the
    /// caller has already confirmed via `overwrite`).
    pub fn commit(mut self, final_path: &Path) -> R<()> {
        if final_path.exists() {
            if final_path.is_dir() {
                std::fs::remove_dir_all(final_path)?;
            } else {
                std::fs::remove_file(final_path)?;
            }
        }
        std::fs::rename(&self.path, final_path)?;
        self.committed = true;
        Ok(())
    }
}

impl Drop for TempGuard {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        if self.path.is_dir() {
            let _ = std::fs::remove_dir_all(&self.path);
        } else if self.path.exists() {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Make a single filename component safe on every supported OS.
pub fn sanitize_component(name: &str) -> String {
    let mut s: String = name
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    s = s.trim_end_matches(['.', ' ']).trim_start().to_string();
    if s == "." || s == ".." {
        s = "_".into();
    }
    let base = s.split('.').next().unwrap_or("").to_ascii_uppercase();
    if RESERVED.contains(&base.as_str()) {
        s = format!("_{s}");
    }
    if s.len() > 200 {
        let mut cut = 200;
        while !s.is_char_boundary(cut) {
            cut -= 1;
        }
        s.truncate(cut);
    }
    s
}

/// Turn an archive entry name into a safe relative path, or `None` if the
/// entry is absolute or tries to escape the extraction root.
pub fn safe_relative_path(entry: &str) -> Option<PathBuf> {
    let normalized = entry.replace('\\', "/");
    if normalized.starts_with('/') {
        return None;
    }
    let mut out = PathBuf::new();
    for part in normalized.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." || part.contains(':') {
            return None;
        }
        let clean = sanitize_component(part);
        if clean.is_empty() {
            return None;
        }
        out.push(clean);
    }
    // Defense in depth: the result must consist only of normal components.
    if out.as_os_str().is_empty() || out.components().any(|c| !matches!(c, Component::Normal(_))) {
        return None;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_traversal() {
        assert!(safe_relative_path("../evil.txt").is_none());
        assert!(safe_relative_path("a/../../evil.txt").is_none());
        assert!(safe_relative_path("/etc/passwd").is_none());
        assert!(safe_relative_path("C:/Windows/x.dll").is_none());
        assert!(safe_relative_path("..\\..\\x").is_none());
        assert!(safe_relative_path("").is_none());
    }

    #[test]
    fn keeps_normal_paths() {
        assert_eq!(safe_relative_path("a/b/c.txt").unwrap(), PathBuf::from("a").join("b").join("c.txt"));
        assert_eq!(safe_relative_path("./a.txt").unwrap(), PathBuf::from("a.txt"));
    }

    #[test]
    fn sanitizes_names() {
        assert_eq!(sanitize_component("CON.txt"), "_CON.txt");
        assert_eq!(sanitize_component("a<b>c?.mp4"), "a_b_c_.mp4");
        assert_eq!(sanitize_component("name. "), "name");
    }

    #[test]
    fn temp_guard_cleans_up() {
        let dir = tempfile::tempdir().unwrap();
        let final_path = dir.path().join("out.txt");
        let tmp_path;
        {
            let g = TempGuard::file_for(&final_path);
            std::fs::write(&g.path, b"x").unwrap();
            tmp_path = g.path.clone();
        }
        assert!(!tmp_path.exists());
        let g = TempGuard::file_for(&final_path);
        std::fs::write(&g.path, b"x").unwrap();
        g.commit(&final_path).unwrap();
        assert!(final_path.exists());
    }

    #[test]
    fn plan_output_refuses_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.mp4"), b"x").unwrap();
        assert!(plan_output(dir.path(), "a", "mp4", false).is_err());
        assert!(plan_output(dir.path(), "a", "mp4", true).is_ok());
    }
}
