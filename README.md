# ConvertHub

ConvertHub is a desktop multimedia conversion toolkit for Windows, macOS and
Linux. It converts, edits and repairs video, audio, images and PDFs, rips
unencrypted DVDs, Blu-rays and music CDs, records the screen, downloads media
from permitted sources, and extracts archives.

Everything runs **locally**. There's no account and nothing is uploaded. Only
the Download feature uses the network.

Built with Tauri 2: a Rust backend plus an HTML/CSS/TypeScript frontend. Media
processing uses FFmpeg and uses GPU encoders when a working one is detected.

## Sections

| Section | Tools |
| --- | --- |
| Video | Convert / compress (device presets for iPhone, iPod, iPad, Android), clip, split, join, mux audio, crop, logo removal (authorized content only), repair, tags |
| Audio | Convert / compress, clip, split, join, mix, repair, tags |
| Image | Convert (WebP, and HEIC/AVIF with ImageMagick), resize, rotate, flip, view/strip/write metadata |
| PDF & Documents | Join PDFs; PDF to TXT, DOC/DOCX, XLS/XLSX, HTML/HTM; extract images |
| DVD / Blu-ray / CD | Rip unencrypted DVDs and Blu-rays to video, and music CDs to audio (no DRM bypass) |
| Download | Direct media links and permitted sites ([list](docs/DOWNLOAD_SOURCES.md)) |
| Screen Recorder | Record a display or window, with a visible recording indicator |
| Archive | Safe ZIP / 7z / RAR extraction |

There is also a batch queue with per-file settings, progress, cancel, retry and
open-folder actions, plus a recent-jobs history, a format guide, a
System & tools page showing detected tools and GPU encoders, and a language
picker.

## Quick start

```bash
npm install
npm run tauri dev
```

Install FFmpeg for video/audio features: `winget install Gyan.FFmpeg`,
`brew install ffmpeg`, or `sudo apt install ffmpeg`. The System & tools page
lists the other optional tools and how to install them.

## Documentation

- [Feature checklist and matrix](docs/FEATURES.md): status, dependencies, formats and verification for every feature
- [Building and packaging](docs/BUILDING.md) for Windows, macOS and Linux
- [Dependencies and licensing](docs/DEPENDENCIES.md)
- [Supported download sources](docs/DOWNLOAD_SOURCES.md)
- [Translating](docs/TRANSLATING.md)
- [Test checklist and verification log](docs/TESTING.md)

## Project layout

```
src/                  Frontend (TypeScript, no framework)
  defs.ts             Declarative operation definitions -> forms
  locales/en.json     All user-facing text
src-tauri/src/
  jobs.rs             Queue, workers, events, recent-jobs history
  ops/                video, audio, image, pdf, disc, download, archive
  ffmpeg.rs gpu.rs    Process runner with progress/cancel; GPU detection
  recorder.rs         Screen capture (gdigrab / avfoundation / x11grab)
  validate.rs fsutil.rs  Input validation, safe output/temp handling
  deps.rs             External tool discovery
```

## Responsible use

ConvertHub never bypasses DRM, copy protection or access controls. Logo
removal and disc ripping need the user to confirm they are authorized. Repair
is best-effort and reports when recovery fails. PDF conversion of complex
layouts, scanned pages and tables is approximate, and OCR is not included.

ConvertHub is an independent project and is not affiliated with FormatFactory.

## License

MIT. See [LICENSE](LICENSE). Third-party tools keep their own licenses; see
[DEPENDENCIES.md](docs/DEPENDENCIES.md).
