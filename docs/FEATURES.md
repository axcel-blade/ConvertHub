# Feature checklist and matrix

This checklist was written before implementation from the project brief and
the official FormatFactory product page
(<https://pcfreetime.com/formatfactory/index.php?language=en>). The page lists
the same capability groups as the brief: video converter/clipper/joiner/
splitter/muxer/crop/delogo, DVD ripping, screen recording, online video
download, audio converter/clipper/joiner/splitter/mixer, music CD ripping,
image conversion (WebP, HEIC) with zoom/rotate/flip/tags, PDF joiner and PDF to
TXT/DOC/Excel/image, ZIP/RAR/7z extraction, iPhone/iPod formats, file-size
reduction, damaged-file repair, and multi-language support. Nothing from that
list is silently left out: every item is in the table below with its status.

## Status legend

| Status | Meaning |
| --- | --- |
| **Implemented** | Working code path behind a real UI control. |
| **Implemented, needs tool** | Working code path, but it only runs when the listed external tool is installed. Otherwise ConvertHub shows a "tool missing" message with setup steps. |
| **Partial** | Works with documented limitations. |
| **Unavailable** | Not implemented on that platform. The UI says so and explains why. |

## Verification legend

"Verified" means it actually ran on the machine named below. Anything not
listed as verified has **not** been tested yet, even if it compiles.

- **U** = covered by automated unit/integration tests (`cargo test`), run on Windows 11 x64.
- **B** = the code compiles in the Windows 11 x64 release build.
- **E** = ran end to end through the real job pipeline with FFmpeg 9.0.2 (gyan.dev full build) on Windows 11 x64 with an NVIDIA RTX 5060 Laptop GPU and Intel UHD Graphics (`cargo test -- --ignored`, see `src-tauri/src/ops/e2e_tests.rs`). Fixtures are generated test media, not real-world files.
- **—** = not run yet. macOS and Linux have not been built or run.

All optional tools were installed for the end-to-end pass: FFmpeg 9.0.2,
Poppler 25.07.0, LibreOffice, 7-Zip, ImageMagick 7.1.2 (Q16-HDRI), ExifTool and
yt-dlp 2026.06.09. LibreOffice and 7-Zip were not on `PATH`, so ConvertHub found
them through its default-install-folder fallback.

## Matrix

| # | Feature | Status | Dependencies | Formats | Verified |
| --- | --- | --- | --- | --- | --- |
| 1.1 | Video convert | Implemented, needs tool | FFmpeg | In: MP4 M4V MKV MOV AVI WMV FLV WebM MPG TS M2TS 3GP VOB OGV ASF RM/RMVB F4V GIF. Out: MP4 MKV MOV AVI WebM WMV FLV MPG 3GP M4V TS GIF | E (all 12 output formats) |
| 1.2 | Device presets (iPhone, iPod, iPad, Android, web) | Implemented, needs tool | FFmpeg | MP4/M4V H.264 or HEVC, WebM VP9 | E (iPod classic ≤320×240, iPhone H.264) |
| 1.3 | Clip / trim (fast stream copy or precise re-encode) | Implemented, needs tool | FFmpeg | Same as input | E (fast and precise; precise within 0.2 s) |
| 1.4 | Join | Implemented, needs tool | FFmpeg | Any video in, chosen format out (scaled/letterboxed to the first video; 30 fps; silent audio added where an input has none) | E (640×360 + 320×240 without audio → 8 s with audio) |
| 1.5 | Split (fixed-length parts) | Implemented, needs tool | FFmpeg | Same as input; fast mode splits at keyframes | E |
| 1.6 | Mux (replace soundtrack) | Implemented, needs tool | FFmpeg | MP4/MOV/MKV out (others become MKV) | E |
| 1.7 | Crop | Implemented, needs tool | FFmpeg | Same as input | E (exact size; out-of-frame rejected) |
| 1.8 | Reduce size (CRF, bitrate, max resolution, target size) | Implemented, needs tool | FFmpeg | All video outputs | E (target size, max width; see note) |
| 1.9 | Repair damaged video | Partial (best effort) | FFmpeg | Output MKV. Tolerant remux, then tolerant re-encode, then a check that the output really plays. Reports failure clearly; MP4 files missing their index ("moov atom") are reported as unrecoverable | E (truncated MKV recovered 2.9 s; truncated MP4 → 'cannot repair' message; random bytes → failure message) |
| 1.10 | Logo removal | Implemented, needs tool | FFmpeg (`delogo`) | Same as input. Requires ticking an ownership/authorization box, which the backend also checks | E (refused without authorization) |
| 1.11 | Video tags (title, artist, album, year, comment) | Implemented, needs tool | FFmpeg | Same as input, stream copy | E (title tag read back) |
| 2.1 | Audio convert (also extracts audio from video) | Implemented, needs tool | FFmpeg | In: MP3 M4A AAC WAV FLAC OGG OPUS WMA AIFF AC3 AMR APE MKA MP2 WV CAF + video files. Out: MP3 M4A AAC OGG OPUS FLAC WAV WMA AIFF AC3 | E (all 10 outputs; audio extracted from video) |
| 2.2 | Audio clip | Implemented, needs tool | FFmpeg | Same as input | E |
| 2.3 | Audio join | Implemented, needs tool | FFmpeg | Chosen format | E |
| 2.4 | Audio split | Implemented, needs tool | FFmpeg | Same as input | E |
| 2.5 | Audio mix | Implemented, needs tool | FFmpeg (`amix`) | Chosen format | E |
| 2.6 | Reduce size / bitrate, sample-rate, channel controls | Implemented, needs tool | FFmpeg | Lossy outputs | E (bitrate, sample rate, mono) |
| 2.7 | Repair damaged audio | Partial (best effort) | FFmpeg | Same format, or M4A | E (corrupted MP3) |
| 2.8 | Audio tags | Implemented, needs tool | FFmpeg | Same as input | E (same code path as 1.11) |
| 3.1 | Image convert | Implemented | none (Rust `image` crate) | In: PNG JPG WebP BMP GIF TIFF ICO TGA PNM QOI. Out: PNG JPG WebP (lossless) BMP GIF TIFF ICO TGA QOI | U (7 output formats) |
| 3.2 | Lossy WebP | Implemented, needs tool | FFmpeg with libwebp | WebP | E |
| 3.3 | HEIC / HEIF / AVIF | Implemented, needs tool | ImageMagick with libheif (read/write), or a recent FFmpeg (read only) | HEIC HEIF AVIF | E (PNG→HEIC/AVIF→PNG round trip with ImageMagick) |
| 3.4 | Resize ("zoom"), rotate 90/180/270, flip | Implemented | none | All image outputs | U |
| 3.5 | View tags (EXIF) | Implemented | none (`kamadak-exif`) | JPEG, TIFF, HEIF, PNG, WebP | E (Artist tag read natively) |
| 3.6 | Write tags (title, artist, copyright, description) | Implemented, needs tool | ExifTool | Formats ExifTool supports | E (title, artist, copyright, description written and read back) |
| 3.7 | Keep or strip metadata on convert | Implemented (stripping is native; keeping needs ExifTool) | ExifTool (optional) | All | E (default strips Artist and GPS; keep copies them) |
| 4.1 | Join PDFs | Implemented | none (`lopdf`) | PDF | U (2-file merge, page count) |
| 4.2 | PDF to TXT | Implemented | none (`pdf-extract`); Poppler `pdftotext` used if installed for better layout | TXT | U (native), E (Poppler) |
| 4.3 | PDF to DOCX | Implemented (text-only without LibreOffice) | LibreOffice (optional, keeps layout) | DOCX | U (native), E (LibreOffice) |
| 4.4 | PDF to DOC | Implemented, needs tool | LibreOffice | DOC | E (valid OLE .doc) |
| 4.5 | PDF to XLSX | Partial: rows and columns rebuilt from word positions (Poppler) or text spacing (native) | none (`rust_xlsxwriter`) | XLSX, one sheet per page | U (native), E (Poppler word boxes: a 3-column table round-trips exactly) |
| 4.6 | PDF to XLS | Implemented, needs tool | LibreOffice | XLS | E (valid OLE .xls) |
| 4.7 | PDF to HTML/HTM | Implemented | none (text per page); Poppler `pdftohtml` if installed | HTML, HTM | U (native), E (pdftohtml; .html and .htm) |
| 4.8 | Extract images (optionally as JPG) | Implemented | none (JPEG, JPEG 2000, 8-bit RGB/gray); Poppler `pdfimages` if installed (all types) | JPG PNG JP2 and others with Poppler | E (pdfimages; 2 pages → 2 JPGs) |
| 4.9 | OCR for scanned PDFs | **Unavailable** | Not implemented. Scanned PDFs return the "no text found" message | — | E (message confirmed on an image-only PDF) |
| 5.1 | DVD to video | Implemented, needs tool; unencrypted discs only | FFmpeg, optical drive or VIDEO_TS folder | All video outputs and presets | E (generated unencrypted VIDEO_TS; authorization enforced; unreadable VOB → copy-protection message) |
| 5.2 | Blu-ray to video | Implemented, needs tool; unencrypted discs only | FFmpeg, optical drive or BDMV folder | All video outputs and presets | E (generated BDMV/STREAM) |
| 5.3 | Music CD to audio, macOS | Implemented, needs tool | FFmpeg (macOS exposes tracks as AIFF) | All audio outputs | E (logic only: simulated AIFF track folder on Windows) |
| 5.3 | Music CD to audio, Linux | Implemented, needs tool | cdparanoia + FFmpeg | All audio outputs | — |
| 5.3 | Music CD to audio, Windows | **Unavailable** | Windows shows CD tracks as `.cda` stubs, which FFmpeg cannot read. Needs a native CDDA backend (planned) | — | — |
| 5.4 | DRM / copy-protection bypass | **Never supported, by design** | — | — | — |
| 6.1 | Direct media URL download | Implemented | Network | Video and audio files | E (local HTTP server: download, HTML rejected, YouTube blocked) |
| 6.2 | Site downloads (Internet Archive) | Implemented, needs tool | yt-dlp, network | Whatever the site serves | E (archive.org public-domain item downloaded through yt-dlp; ack enforced) |
| 6.3 | YouTube, Youku, Vimeo, DRM streaming services | **Unavailable (disabled on purpose)** | Provider terms do not permit it. See [DOWNLOAD_SOURCES.md](DOWNLOAD_SOURCES.md) | — | U (blocked) |
| 7.1 | Screen recording, Windows (display or window) | Implemented, needs tool | FFmpeg `gdigrab` | MP4, MKV | E (display capture 640×480; window capture not run) |
| 7.2 | Screen recording, macOS (display) | Implemented, needs tool | FFmpeg `avfoundation`, Screen Recording permission | MP4, MKV | — |
| 7.2b | Screen recording, macOS (single window) | **Unavailable** | FFmpeg's avfoundation cannot capture single windows | — | — |
| 7.3 | Screen recording, Linux X11 (display or window) | Implemented, needs tool | FFmpeg `x11grab`; `wmctrl` for the window list | MP4, MKV | — |
| 7.3b | Screen recording, Linux Wayland | **Unavailable** | Needs the PipeWire portal (planned) | — | — |
| 8.1 | ZIP extraction | Implemented | none (`zip`) | ZIP (deflate, bzip2, zstd, LZMA) | U (extract, traversal rejected, cleanup) |
| 8.2 | 7z extraction | Implemented | none (`sevenz-rust2`) | 7z | E (real 7-Zip archive; encrypted 7z refused with a clear message) |
| 8.3 | RAR extraction | Implemented, needs tool | 7-Zip (`7z`/`7zz`) | RAR4/RAR5 | E (spec-built RAR5 fixtures: extraction, and a `../` entry refused before extraction) |
| 8.4 | Path traversal, unsafe names, symlinks, archive bombs, overwrite protection | Implemented | — | — | U |
| 9.1 | FFmpeg codec, hwaccel and GPU detection with a real test encode | Implemented, needs tool | FFmpeg | NVENC, QSV, AMF, VideoToolbox, VA-API (H.264/HEVC) | E (NVENC + QSV verified; AMF/VA-API correctly rejected) |
| 9.2 | Auto / CPU / GPU choice, automatic CPU fallback, and the engine actually used shown per job | Implemented | FFmpeg | H.264/HEVC outputs | E (GPU mode → h264_nvenc; Auto + HEVC → hevc_nvenc; CPU mode → libx264) |
| 10.1 | Localization framework, English, language picker, contribution link | Implemented | — | — | U (`npm run check:i18n`) |
| 11.1 | Queue: batch, per-file settings, progress, cancel, retry, open folder | Implemented | — | — | B |
| 11.2 | Recent jobs history, format guide, dependency/GPU status page | Implemented | — | — | B |
| 11.3 | Overwrite warning (UI) and enforcement (backend) | Implemented | — | — | U, E |
| 11.4 | Backend validation of type (extension and magic bytes), size and existence | Implemented | — | — | U |
| 11.5 | Temp-file cleanup on success, failure and cancel | Implemented | — | — | U, E (cancel mid-encode leaves no files) |

## Platforms verified

| Platform | Release build | Unit tests | App launched | Installer built |
| --- | --- | --- | --- | --- |
| Windows 11 x64 | Yes (`--no-bundle`) | Yes (unit + FFmpeg end-to-end) | Yes (launch smoke test only) | No |
| macOS | No | No | No | No |
| Linux | No | No | No | No |

Packaging configuration exists for all three platforms (see [BUILDING.md](BUILDING.md)
and `.github/workflows/build.yml`), but only the Windows build has been run.

## Notes from the end-to-end pass

- **Target size is an upper bound.** A 0.5 MB target on a very simple 5 s test clip produced 0.21 MB, because the encoder needs fewer bits than the cap allows. Real footage lands closer to the target. The UI already describes the target as approximate.
- **Raw `.aac` duration:** players and ffprobe estimate the length of ADTS AAC files from bitrate (4.29 s shown for a 4.04 s file). The audio is correct. Use M4A when exact duration metadata matters.
- **PDF → XLSX fix:** the first run found that LibreOffice-generated tables put columns about 12 pt apart, which `pdftotext -layout` turns into a single space, so two columns were merged. With Poppler installed, cells are now built from per-word bounding boxes and split on gaps wider than 0.6× the text height. The native fallback (no Poppler) still splits on runs of spaces and has the same limitation.
- **RAR fixtures** are generated by `scripts/make-rar-fixtures.py` from the public RAR5 format spec (stored entries). 7-Zip validates them. Compressed RAR4/RAR5 and password-protected RAR archives were not tested, because no free tool can create them.
