//! Detection of external helper tools. ConvertHub does not assume anything is
//! installed: each feature checks for the tools it needs and, when missing,
//! reports a translated setup hint instead of failing obscurely.
//!
//! Lookup order: `CONVERTHUB_<TOOL>` env var -> bundled `bin/` resource
//! folder (for packagers who ship license-compatible builds) -> `PATH`.

use serde::Serialize;
use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Tool {
    Ffmpeg,
    Ffprobe,
    YtDlp,
    SevenZip,
    Pdftotext,
    Pdftohtml,
    Pdfimages,
    Soffice,
    Magick,
    Exiftool,
    Cdparanoia,
}

pub const ALL: &[Tool] = &[
    Tool::Ffmpeg,
    Tool::Ffprobe,
    Tool::YtDlp,
    Tool::SevenZip,
    Tool::Pdftotext,
    Tool::Pdftohtml,
    Tool::Pdfimages,
    Tool::Soffice,
    Tool::Magick,
    Tool::Exiftool,
    Tool::Cdparanoia,
];

impl Tool {
    fn candidates(self) -> &'static [&'static str] {
        match self {
            Tool::Ffmpeg => &["ffmpeg"],
            Tool::Ffprobe => &["ffprobe"],
            Tool::YtDlp => &["yt-dlp"],
            Tool::SevenZip => &["7z", "7zz", "7za"],
            Tool::Pdftotext => &["pdftotext"],
            Tool::Pdftohtml => &["pdftohtml"],
            Tool::Pdfimages => &["pdfimages"],
            Tool::Soffice => &["soffice", "libreoffice"],
            Tool::Magick => &["magick"],
            Tool::Exiftool => &["exiftool"],
            Tool::Cdparanoia => &["cdparanoia"],
        }
    }
    pub fn id(self) -> &'static str {
        match self {
            Tool::Ffmpeg => "ffmpeg",
            Tool::Ffprobe => "ffprobe",
            Tool::YtDlp => "yt-dlp",
            Tool::SevenZip => "7z",
            Tool::Pdftotext => "pdftotext",
            Tool::Pdftohtml => "pdftohtml",
            Tool::Pdfimages => "pdfimages",
            Tool::Soffice => "soffice",
            Tool::Magick => "magick",
            Tool::Exiftool => "exiftool",
            Tool::Cdparanoia => "cdparanoia",
        }
    }
    fn version_args(self) -> &'static [&'static str] {
        match self {
            Tool::Ffmpeg | Tool::Ffprobe => &["-hide_banner", "-version"],
            Tool::Pdftotext | Tool::Pdftohtml | Tool::Pdfimages => &["-v"],
            Tool::SevenZip => &[],
            Tool::Exiftool => &["-ver"],
            Tool::Cdparanoia => &["--version"],
            _ => &["--version"],
        }
    }
}

static RESOURCE_BIN: OnceLock<Option<PathBuf>> = OnceLock::new();

pub fn set_resource_bin(dir: Option<PathBuf>) {
    let _ = RESOURCE_BIN.set(dir);
}

pub fn find(tool: Tool) -> Option<PathBuf> {
    let env_key = format!("CONVERTHUB_{}", tool.id().to_uppercase().replace('-', "_"));
    if let Ok(p) = std::env::var(env_key) {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    if let Some(Some(dir)) = RESOURCE_BIN.get() {
        for c in tool.candidates() {
            let p = dir.join(if cfg!(windows) { format!("{c}.exe") } else { c.to_string() });
            if p.is_file() {
                return Some(p);
            }
        }
    }
    for c in tool.candidates() {
        if let Ok(p) = which::which(c) {
            return Some(p);
        }
    }
    // Common install locations that are often missing from PATH.
    let extra: &[&str] = match tool {
        Tool::SevenZip if cfg!(windows) => &[r"C:\Program Files\7-Zip\7z.exe"],
        Tool::Soffice if cfg!(windows) => &[r"C:\Program Files\LibreOffice\program\soffice.exe"],
        Tool::Soffice if cfg!(target_os = "macos") => &["/Applications/LibreOffice.app/Contents/MacOS/soffice"],
        _ => &[],
    };
    extra.iter().map(PathBuf::from).find(|p| p.is_file())
}

/// A `Command` that does not flash a console window on Windows.
pub fn command(program: &std::path::Path) -> Command {
    #[allow(unused_mut)]
    let mut c = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        c.creation_flags(CREATE_NO_WINDOW);
    }
    c
}

pub fn require(tool: Tool) -> crate::error::R<PathBuf> {
    find(tool).ok_or_else(|| crate::ui!("errors.toolMissing", "tool" => tool.id()))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DepStatus {
    pub tool: &'static str,
    pub found: bool,
    pub path: Option<String>,
    pub version: Option<String>,
}

pub fn status_all() -> Vec<DepStatus> {
    ALL.iter()
        .map(|&t| {
            let path = find(t);
            let version = path.as_ref().and_then(|p| {
                let out = command(p).args(t.version_args()).output().ok()?;
                let text = if out.stdout.is_empty() { out.stderr } else { out.stdout };
                String::from_utf8_lossy(&text)
                    .lines()
                    .map(str::trim)
                    .find(|l| !l.is_empty())
                    .map(|l| l.chars().take(120).collect())
            });
            DepStatus {
                tool: t.id(),
                found: path.is_some(),
                path: path.map(|p| p.display().to_string()),
                version,
            }
        })
        .collect()
}
