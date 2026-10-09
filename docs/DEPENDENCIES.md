# Dependencies and licensing

ConvertHub's own code is MIT-licensed (see `LICENSE`).

## Linked libraries (compiled into the app)

All of these are permissively licensed (MIT and/or Apache-2.0, plus BSD/zlib
for some transitive crates), so they can be distributed in binaries.
`cargo tree` shows the full tree. Run `cargo about` or `cargo deny` before
shipping to generate the third-party notice file.

| Crate | Purpose |
| --- | --- |
| tauri, tauri-plugin-dialog, tauri-plugin-opener | Desktop shell, native dialogs, open folder / URL |
| image | Native image decoding and encoding |
| kamadak-exif | Reading EXIF tags |
| lopdf, pdf-extract | PDF merge, image extraction, text extraction |
| docx-rs, rust_xlsxwriter | Native DOCX/XLSX output |
| zip, sevenz-rust2 | ZIP and 7z extraction |
| reqwest (rustls) | Direct-URL downloads |
| infer | Content sniffing (magic bytes) |
| serde, serde_json, uuid, which | Infrastructure |

## External tools (detected at runtime, not bundled)

ConvertHub runs these as separate programs. Nothing is bundled by default, so
the user installs them and their licenses apply to those installations.

| Tool | Used for | License | Notes on redistribution |
| --- | --- | --- | --- |
| FFmpeg / ffprobe | All video/audio work, disc ripping, screen recording, GPU encoding | LGPL-2.1+; GPL-2+/GPL-3 when built with libx264, libx265 or other GPL parts; "nonfree" builds (e.g. with fdk-aac) **cannot be redistributed** | You may ship an LGPL build if you provide its source (or a written offer), keep it as a separate executable, and include its license. Shipping a GPL build makes the bundle subject to GPL terms. Some codecs (H.264/HEVC) may also need patent licenses in some countries. |
| yt-dlp | Internet Archive downloads | Unlicense | Redistributable |
| 7-Zip (`7z`, `7zz`) | RAR extraction | LGPL-2.1 + unRAR restriction | The unRAR code may not be used to create RAR archives; extraction is fine. |
| Poppler (`pdftotext`, `pdftohtml`, `pdfimages`) | Higher-fidelity PDF conversion | GPL-2/GPL-3 | Separate executable. Shipping it requires GPL compliance for those binaries. |
| LibreOffice (`soffice`) | PDF to DOC/DOCX with layout, XLS output | MPL-2.0 | Large; recommend users install it themselves |
| ImageMagick (`magick`) | HEIC/HEIF/AVIF | ImageMagick License (Apache-2.0 style) | Redistributable. HEIC support depends on libheif (LGPL) and its codecs. |
| ExifTool | Writing image tags, keeping metadata | Artistic/GPL (Perl) | Redistributable |
| cdparanoia | Linux audio-CD ripping | GPL-2 | Install through the distribution |
| wmctrl (Linux, optional) | Window list for window recording | GPL-2 | Install through the distribution |

**Recommendation:** distribute ConvertHub without third-party executables and
let the System & tools page guide users to install them. This is the default.
If you bundle FFmpeg, use an LGPL build from a trusted source, ship its license
and source offer, and do not use `--enable-nonfree` builds.

## Network use

Only the Download feature uses the network. ConvertHub has no telemetry, no
update checks, and no account system. The "Help translate" link opens the
reference page in the user's browser.
