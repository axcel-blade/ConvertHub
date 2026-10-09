# Supported download sources

The Download feature only fetches content from sources where downloading is
allowed. The list lives in code at `src-tauri/src/ops/download.rs` (`SOURCES`)
and is shown in the app. Every download also needs the user to confirm they
have the right to download that content.

| Source | Status | How | Limitations |
| --- | --- | --- | --- |
| Direct media links (http/https) | Supported | Native HTTP client | The URL must point to a video or audio file; HTML pages are rejected. No logins, cookies, referer tricks or paywall bypassing. Maximum size is 50 GB. |
| Internet Archive (`archive.org`) | Supported | yt-dlp (must be installed) | Public items only. Respect each item's license. |
| Wikimedia Commons (`upload.wikimedia.org`) | Supported | Native (direct file links) | Respect the file's license and attribution terms. |
| YouTube (`youtube.com`, `youtu.be`) | **Not supported** | — | The YouTube Terms of Service prohibit downloading except through features YouTube provides. |
| Youku (`youku.com`) | **Not supported** | — | The terms do not allow third-party downloading, and content is often region-restricted. |
| Vimeo (`vimeo.com`) | **Not supported** | — | Vimeo allows downloads only through the owner-enabled download button on its site. |
| Netflix, Disney+, Prime Video and similar | **Never supported** | — | DRM-protected. ConvertHub never bypasses DRM. |

Unknown hosts are treated as **direct links**: the server must answer with a
media file. ConvertHub does not run site extractors for unlisted sites.

## Adding a source

1. Confirm that the provider's current terms permit downloading.
2. Add an entry to `SOURCES` with its hostnames, `enabled: true`, and a new
   `download.notes.<id>` string in `src/locales/en.json`.
3. Route it in `run()` (yt-dlp, or the native direct downloader).
4. Update this file and add a classification test.

Re-check these terms periodically. If a provider's rules change, update or
disable the entry.
