# Changelog

All notable changes to this project are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

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
