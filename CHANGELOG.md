# Changelog

All notable changes to this project are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- Unfinished jobs are saved to disk and offered for resuming after a restart or crash.
- "Retry failed" for the whole queue, and "Run again" for entries in Recent jobs.
- Setting for how many jobs run at the same time (1–8).
- Last-used settings are remembered per tool, and settings can be saved as named "My presets".
- Taskbar/dock progress, and a system notification when the queue finishes while the window is in the background.
- Keyboard shortcuts (Ctrl+O, Ctrl+Enter, Ctrl+Shift+S, Alt+1–8, Alt+Q/R/S, F1 for the list) and a skip-to-content link.
- Optional signed auto-updates (enabled when a signing key is configured; see docs/BUILDING.md).
- Frontend unit tests (Vitest), Biome linting, `cargo fmt`/`clippy` checks, and CI on Windows, macOS and Linux.
- `npm run version:set <x.y.z>` updates every version field; CI checks they agree.

### Changed
- The frontend is split into modules (`state.ts`, `ui.ts`, `prefs.ts`, `shortcuts.ts`, `views/*`).
- `tauri.conf.json` reads its version from `package.json`.

### Fixed
- A panic inside one job can no longer poison the queue lock and stop every later job.

### Security
- Media info (`probe_media`) now validates the file like job inputs do.
- Direct downloads only follow redirects to URLs that pass the same source checks, so a link cannot redirect onto a disabled source.

## [0.1.1] - 2026-10-09

### Fixed

- Windows: opening the System & tools page no longer pops up a LibreOffice
  console asking to "Press Enter to continue...". The LibreOffice version is
  now read from `programersion.ini` instead of running `soffice --version`.
- Tool version checks now run with an empty stdin, so no tool can wait for
  keyboard input.

### Added

- Community docs: CONTRIBUTING, CODE_OF_CONDUCT, SECURITY, SUPPORT,
  plus issue, pull request and discussion templates.
- `ci` GitHub Actions workflow running frontend checks and Rust tests on
  every Git Flow branch and pull request.

## [0.1.0]

### Added

- Initial ConvertHub desktop app (Tauri 2, Rust + TypeScript).
- Video, Audio, Image, PDF & Documents, DVD / Blu-ray / CD, Download,
  Screen Recorder and Archive sections.
- Batch queue, recent-jobs history, format guide, System & tools page and
  language picker.

[Unreleased]: https://github.com/axcel-blade/ConvertHub/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/axcel-blade/ConvertHub/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/axcel-blade/ConvertHub/releases/tag/v0.1.0
