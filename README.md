# ConvertHub

<p align="center">
  <img src="src-tauri/icons/128x128.png" alt="ConvertHub logo" width="128">
</p>

<!-- Dynamic badges (live data from GitHub) -->
[![CI](https://img.shields.io/github/actions/workflow/status/axcel-blade/ConvertHub/ci.yml?branch=develop&label=CI&logo=githubactions&logoColor=white)](https://github.com/axcel-blade/ConvertHub/actions/workflows/ci.yml)
[![Build](https://img.shields.io/github/actions/workflow/status/axcel-blade/ConvertHub/build.yml?branch=develop&label=build&logo=tauri&logoColor=white)](https://github.com/axcel-blade/ConvertHub/actions/workflows/build.yml)
[![Latest release](https://img.shields.io/github/v/release/axcel-blade/ConvertHub?include_prereleases&sort=semver)](https://github.com/axcel-blade/ConvertHub/releases)
[![Open issues](https://img.shields.io/github/issues/axcel-blade/ConvertHub)](https://github.com/axcel-blade/ConvertHub/issues)
[![Last commit](https://img.shields.io/github/last-commit/axcel-blade/ConvertHub/develop)](https://github.com/axcel-blade/ConvertHub/commits/develop)
[![Stars](https://img.shields.io/github/stars/axcel-blade/ConvertHub?style=flat)](https://github.com/axcel-blade/ConvertHub/stargazers)

<!-- Static badges -->
[![License: MIT](https://img.shields.io/badge/license-MIT-c4f82a)](LICENSE)
[![Version](https://img.shields.io/badge/version-0.1.2-c4f82a)](CHANGELOG.md)
[![Tauri 2](https://img.shields.io/badge/Tauri-2-24C8DB?logo=tauri&logoColor=white)](https://tauri.app/)
[![Rust](https://img.shields.io/badge/Rust-stable-000000?logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![TypeScript](https://img.shields.io/badge/TypeScript-Vite-3178C6?logo=typescript&logoColor=white)](https://www.typescriptlang.org/)
[![Platforms](https://img.shields.io/badge/platforms-Windows%20%7C%20macOS%20%7C%20Linux-5cd6e6)](docs/BUILDING.md)
[![FFmpeg](https://img.shields.io/badge/media-FFmpeg-007808?logo=ffmpeg&logoColor=white)](https://ffmpeg.org/)
[![Local only](https://img.shields.io/badge/processing-100%25%20local-555)](#responsible-use)

A desktop multimedia conversion toolkit for Windows, macOS and Linux. Convert,
edit and repair video, audio, images and PDFs, rip unencrypted discs, record the
screen, download media from permitted sources, and extract archives.

Everything runs **locally**: no account, nothing uploaded. Only the Download
feature uses the network.

## Architecture

```
src/ (Frontend, TypeScript + Vite)            src-tauri/src/ (Backend, Rust + Tauri 2)
├── main.ts    — app shell, startup           ├── commands.rs — Tauri IPC commands
├── views/     — tool, queue, pages, capture  ├── jobs.rs     — queue, workers, events, history,
├── state.ts   — shared UI state, render()    │                 resumable pending jobs
├── ui.ts      — toasts, dialogs, widgets     ├── ops/        — video, audio, image, pdf,
├── prefs.ts   — remembered settings, presets │                 disc, download, archive
├── shortcuts.ts — keyboard shortcuts         ├── ffmpeg.rs   — process runner, progress, cancel
├── defs.ts    — declarative operation        ├── gpu.rs      — GPU encoder detection
│                definitions → forms          ├── recorder.rs — screen capture
├── api.ts     — typed IPC calls + events     ├── presets.rs  — device / format presets
├── dom.ts, i18n.ts — DOM + translations      ├── validate.rs, fsutil.rs — input + safe file handling
├── locales/   — all user-facing text         ├── sync.rs     — poison-tolerant locking
└── styles.css — theme                        └── deps.rs     — external tool discovery
```

The frontend renders forms from `defs.ts` and sends jobs to the backend over
Tauri IPC. The backend validates input, runs native Rust code or external tools
(FFmpeg, 7-Zip, Poppler, …) and streams progress events back to the queue.
See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for a detailed walkthrough.

## Features

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

- **Batch queue:** per-file settings, progress, cancel, retry (one or all failed) and open-folder actions, with an adjustable number of jobs running at once.
- **Resume after restart:** unfinished jobs are offered again the next time the app opens.
- **Remembered settings:** each tool starts from your last-used settings, and you can save named presets.
- **Recent jobs:** history of finished conversions, with "Run again".
- **Progress outside the app:** taskbar/dock progress and a notification when the queue finishes.
- **Keyboard shortcuts:** Ctrl+O, Ctrl+Enter, Ctrl+Shift+S, Alt+1–8, Alt+Q/R/S; press F1 for the list.
- **Auto-updates** from Settings (signed builds only).
- **GPU acceleration:** uses hardware encoders when a working one is detected.
- **System & tools page:** shows detected tools, GPU encoders and how to install missing ones.
- **Format guide** and a **language picker**.

See [docs/FEATURES.md](docs/FEATURES.md) for the full feature matrix.

## Requirements

### To run
- Windows 10/11, macOS or Linux
- [FFmpeg](https://ffmpeg.org/) for video, audio, disc and screen-recording features
- Optional: 7-Zip (RAR), Poppler and LibreOffice (better PDF conversion), ImageMagick (HEIC/AVIF), ExifTool, yt-dlp, cdparanoia (Linux CDs)

### To build
- Rust stable (1.77+)
- Node.js 20+ with npm
- [Tauri 2 prerequisites](https://tauri.app/start/prerequisites/) for your OS

## Getting Started

1. Install FFmpeg:
   - Windows: `winget install Gyan.FFmpeg`
   - macOS: `brew install ffmpeg`
   - Linux: `sudo apt install ffmpeg`
2. Install dependencies: `npm install`
3. Run the app in development mode: `npm run tauri dev`
4. Open **System & tools** to check which optional tools were found.

Having trouble? See [SUPPORT.md](SUPPORT.md).

## Development

```sh
npm test            # lint + type check + i18n + version check + unit tests + Rust tests
npm run test:unit   # frontend unit tests (Vitest) only
npm run lint        # Biome lint for TypeScript
npm run test:e2e    # end-to-end tests (needs external tools installed)
npm run version:set 0.2.0  # set the version in every file
npm run tauri build # build installers
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for the Git Flow branching model and PR
guidelines, and [docs/BUILDING.md](docs/BUILDING.md) for packaging.

## Documentation

- [Architecture](docs/ARCHITECTURE.md)
- [Feature checklist and matrix](docs/FEATURES.md)
- [Building and packaging](docs/BUILDING.md)
- [Dependencies and licensing](docs/DEPENDENCIES.md)
- [Supported download sources](docs/DOWNLOAD_SOURCES.md)
- [Translating](docs/TRANSLATING.md)
- [Test checklist and verification log](docs/TESTING.md)
- [Security policy](SECURITY.md) · [Code of Conduct](CODE_OF_CONDUCT.md)

## Responsible use

ConvertHub never bypasses DRM, copy protection or access controls. Logo
removal and disc ripping need the user to confirm they are authorized. Repair
is best-effort and reports when recovery fails. PDF conversion of complex
layouts, scanned pages and tables is approximate, and OCR is not included.

ConvertHub is an independent project and is not affiliated with FormatFactory.

## Versions

| Component | Version |
|-----------|---------|
| ConvertHub desktop app | **v0.1.0** |

See the [changelog](CHANGELOG.md).

## License

MIT. See [LICENSE](LICENSE). Third-party tools keep their own licenses; see
[DEPENDENCIES.md](docs/DEPENDENCIES.md).
