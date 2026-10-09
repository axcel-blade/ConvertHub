# Test checklist

## Automated

```bash
npm test          # typecheck + translation keys + cargo test
npm run test:e2e  # end-to-end tests; need FFmpeg on PATH (GPU and screen-recording tests included)
# Optional: python scripts/make-rar-fixtures.py <dir>, then set CONVERTHUB_E2E_RAR_DIR=<dir>
# Optional (network): set CONVERTHUB_E2E_ARCHIVE_URL=https://archive.org/details/<small public-domain item>
# Tool tests print SKIP when their tool is not installed.
```

`cargo test` covers path sanitizing and traversal rejection, temp-file
cleanup, overwrite refusal, file validation (extension, magic bytes, empty
files), native image conversion to 7 formats with resize/rotate, PDF merge and
PDF to TXT/XLSX/DOCX (native paths), ZIP extraction (with an unsafe archive
rejected and nothing left on disk), DVD main-title detection, download URL
classification (blocked providers, look-alike hosts), filename safety, time
parsing, and FFmpeg encoder-list parsing.

## Manual checklist (per platform)

Mark each item with the platform and date when verified. Run with FFmpeg
installed unless the item says otherwise.

### General
- [ ] App starts; window can be resized; the minimum size is respected
- [ ] Every sidebar section opens; Tab/Shift+Tab reach every control; focus outline is visible
- [ ] Drag files from the file manager onto the window: they are added to the current tool
- [ ] Unsupported or renamed files (e.g. `.exe` renamed to `.mp4`) are rejected with a message
- [ ] Language picker lists English; "Help translate" opens the browser
- [ ] System & tools lists every tool with found/missing and install hints
- [ ] Without FFmpeg: video/audio pages show the "FFmpeg not found" banner; jobs fail with the setup message

### Queue
- [ ] Batch of 5 files queues 5 jobs; progress bars move; status changes to Done
- [ ] Cancel while running: status Cancelled, no partial or `.converthub-*` files left
- [ ] Retry a failed job works
- [ ] "Open folder" reveals the output
- [ ] Output exists already: overwrite prompt appears; "Cancel" queues nothing; "Replace" replaces it
- [ ] Recent jobs survive an app restart

### Video
- [ ] MP4 to MKV/WebM/AVI/GIF; iPhone and iPod presets play on the device or in QuickTime
- [ ] Target size 10 MB produces roughly 10 MB
- [ ] GPU: with a supported GPU, Engine shows `GPU (h264_nvenc)` (or QSV/AMF/VideoToolbox/VA-API); with none, only CPU is offered
- [ ] Force a GPU failure (e.g. unsupported resolution) in Auto: the job falls back to CPU and the Engine says so
- [ ] Clip fast and precise; split 60 s; join two different-size videos; mux an MP3; crop; delogo (checkbox required)
- [ ] Repair: truncated MKV recovers partially with a message; MP4 without moov shows the "cannot be repaired" message; random bytes report failure

### Audio
- [ ] Convert to every output format; bitrate/sample-rate/channels honored
- [ ] Clip, split, join, mix; tags visible in a player
- [ ] Repair a truncated MP3

### Image
- [ ] PNG/JPG/WebP/BMP/GIF/TIFF/ICO conversions; resize by width, by %, rotate, flip
- [ ] EXIF-rotated phone photo comes out upright
- [ ] HEIC input/output with ImageMagick; clear message without it
- [ ] "i" shows EXIF; Edit tags writes tags (ExifTool)

### PDF
- [ ] Merge 3 PDFs in a custom order
- [ ] TXT/DOCX/XLSX/HTML without Poppler/LibreOffice; DOC/XLS show the LibreOffice message
- [ ] Same with Poppler and LibreOffice installed (better layout)
- [ ] Windows: opening System & tools with LibreOffice installed shows its version and no console window
- [ ] Scanned PDF reports "no text found (OCR not supported)"; encrypted PDF reports encryption
- [ ] Extract images, with and without "convert to JPG"

### Disc
- [ ] DVD folder (unencrypted) rips the main title; authorization box required
- [ ] Encrypted commercial disc fails with the copy-protection message (no bypass)
- [ ] Blu-ray BDMV folder (unencrypted)
- [ ] Audio CD: macOS mounted CD; Linux with cdparanoia; Windows shows "not supported"

### Download
- [ ] Direct MP4 URL downloads with progress; HTML page URL rejected
- [ ] YouTube/Youku/Vimeo URLs rejected as unsupported
- [ ] archive.org item with yt-dlp installed
- [ ] Acknowledgement checkbox required

### Screen recorder
- [ ] Windows: record a display and a window; red banner and "● Recording" title; Stop saves a playable file
- [ ] macOS: first run triggers the permission prompt; denial shows the permission message
- [ ] Linux X11 display/window; Wayland shows "not available"

### Archive
- [ ] ZIP, 7z, RAR (with 7-Zip) extract to a new folder
- [ ] Archive with `../` entry is rejected and nothing is written
- [ ] Password-protected archive gives a clear message
- [ ] Extracting again over an existing folder asks before replacing

## Verification log

| Date | Platform | What | Result |
| --- | --- | --- | --- |
| 2026-10-09 | Windows 11 Pro x64 (26300), Rust 1.98, Node 26 | `cargo test` (24 tests) | Pass |
| 2026-10-09 | Windows 11 Pro x64 | `npm run typecheck`, `npm run check:i18n`, `vite build` | Pass |
| 2026-10-09 | Windows 11 Pro x64 | `tauri build --no-bundle` release build | Pass |
| 2026-10-09 | Windows 11 Pro x64 | Release exe launched; main window "ConvertHub" opened and responded; closed after 8 s | Pass |
| 2026-10-09 | Browser (Vite dev, no backend) | All 13 views and 28 operation tabs rendered with no script errors or missing translation keys | Pass |
| 2026-10-09 | Windows 11 Pro x64, FFmpeg 9.0.2 full, RTX 5060 + Intel UHD | `cargo test -- --ignored` (11 end-to-end tests: video formats, presets/compression, editing, repair, audio, lossy WebP, DVD/Blu-ray/CD, download, cancel, GPU, screen recording) | Pass |
| 2026-10-09 | Windows 11 Pro x64 + Poppler 25.07, LibreOffice, 7-Zip, ImageMagick 7.1.2, ExifTool, yt-dlp 2026.06.09 | 8 tool end-to-end tests (`e2e_tools.rs`): PDF→TXT/HTML/HTM/XLSX/DOCX/DOC/XLS, PDF image extraction, scanned-PDF message, 7z + encrypted 7z, RAR5 + traversal RAR, HEIC/AVIF round trip, EXIF strip/keep/write, archive.org download | Pass (after the PDF→XLSX column fix) |
| 2026-10-09 | Windows 11 Pro x64 | Full run: `cargo test --lib -- --include-ignored` | 43/43 pass |
| — | Windows | Manual GUI walkthrough of the checklist above | **Not run** |
| — | macOS, Linux | Build, tests, launch | **Not run** |
| — | Windows | NSIS/MSI installers | **Not run** (`--no-bundle` was used) |
