# Architecture

ConvertHub is a [Tauri 2](https://tauri.app/) desktop app. A Rust backend does
all the work; a small TypeScript frontend (no framework, bundled by Vite)
renders the UI inside the system webview. The two talk only through Tauri IPC
commands and events. There is no server and no network use except the
Download feature.

```
┌──────────────────────────── Webview (src/) ─────────────────────────────┐
│  main.ts + views/ ── sections, forms (from defs.ts), queue, history     │
│     │  api.ts  invoke("submit_jobs", …)          listen("job-updated")  │
└─────┼──────────────────────────────────────────────────────▲────────────┘
      │ IPC command                                           │ IPC event
┌─────▼───────────────────── Rust (src-tauri/src/) ───────────┼────────────┐
│  commands.rs ──► jobs.rs (JobManager: queue + worker pool) ─┘            │
│                     │                                                    │
│                     ▼                                                    │
│                 ops/mod.rs  validate → plan output → dispatch            │
│                     │                                                    │
│      ┌──────────┬───┴──────┬──────────┬──────────┬──────────┬─────────┐  │
│    video      audio      image       pdf        disc     download  archive│
│      │          │          │          │          │          │         │  │
│      └──── ffmpeg.rs (run_process) ───┴── native crates / external tools │
│            deps.rs (tool lookup) · gpu.rs · fsutil.rs · error.rs         │
└──────────────────────────────────────────────────────────────────────────┘
```

## Repository layout

```
src/                     Frontend (TypeScript)
  main.ts                App shell (sidebar, header) and startup
  state.ts               Shared UI state, render(), navigation helpers
  ui.ts                  Toasts, modal dialog, output-folder widgets
  prefs.ts               Last-used settings and user presets (localStorage)
  shortcuts.ts           Keyboard shortcuts and their help dialog
  views/tool.ts          File-based tools: file list, generated settings form, enqueue
  views/queue.ts         Queue table, job updates, taskbar progress, notifications
  views/pages.ts         Recent jobs, format guide, system, settings (workers, updates)
  views/capture.ts       Download and screen recorder
  __tests__/             Vitest unit tests
  defs.ts                Declarative operation definitions → generated forms
  api.ts                 Typed wrappers around every Rust command
  i18n.ts                Locale loading and rendering of {key, vars} messages
  dom.ts                 Small DOM helpers
  locales/en.json        Every user-facing string
  styles.css             Theme
src-tauri/
  src/lib.rs             App setup: plugins, managed state, command registration
  src/commands.rs        Thin #[tauri::command] layer
  src/jobs.rs            Operation enum, job queue, workers, events, history, pending jobs
  src/sync.rs            Poison-tolerant Mutex/Condvar helpers
  src/ops/               One module per section + dispatch (mod.rs) + e2e tests
  src/ffmpeg.rs          External-process runner (progress, cancel, timeout), probing
  src/gpu.rs             Hardware-encoder detection by test encode
  src/recorder.rs        Screen recording
  src/presets.rs         Device output profiles
  src/validate.rs        Input checks (extension, content sniffing, size)
  src/fsutil.rs          Output planning, temp files, path sanitizing
  src/deps.rs            External tool discovery
  src/error.rs           Translatable errors (UiMsg / UiError)
  capabilities/          Tauri permission set for the main window
  tauri*.conf.json       App config, with per-OS overrides
scripts/check-i18n.mjs   Verifies translation keys used in code exist
scripts/version.mjs      Checks / sets the version in package.json and the Cargo files
```

## Frontend

- **Forms are data.** [`defs.ts`](../src/defs.ts) lists every operation as an
  `OpDef` (section, input rules, fields with types, defaults, ranges and
  `showIf` conditions). `views/tool.ts` renders a form from the definition and
  builds a `JobRequest` (`inputs`, `outputDir`, `outputName`, `overwrite`,
  `operation`) from the values.
- **No business logic.** The frontend filters files for convenience, but the
  backend re-validates everything.
- **State comes from events.** The queue view updates from `job-updated` and
  `queue-state` events instead of polling.
- **No hard-coded text.** All strings come from `locales/*.json` via `i18n.ts`;
  `npm run check:i18n` fails the build if a key is missing.
- **One render function.** Views call `render()` from `state.ts`, which
  rebuilds the shell and restores keyboard focus. `main.ts` registers the
  shell with `setRenderer`, so views never import `main.ts`.
- **Local preferences.** Language, output folder, parallel-job count, the
  last-used settings per tool and user presets live in `localStorage`
  ([`prefs.ts`](../src/prefs.ts)). Authorization checkboxes and chosen file
  paths are never stored.

## Backend

### Startup ([`lib.rs`](../src-tauri/src/lib.rs))

1. Registers the dialog, opener, notification, process and updater plugins.
2. Points tool lookup at the bundled `bin/` resource folder.
3. Creates the `JobManager` with a default of `clamp(cores / 4, 1, 3)`
   parallel jobs (changeable in Settings, up to 8), plus `recent-jobs.json`
   and `pending-jobs.json` files in the app data directory.
4. Creates the `Recorder` state.
5. Starts GPU detection on a background thread so it's ready when needed.

### Commands ([`commands.rs`](../src-tauri/src/commands.rs))

A thin layer that forwards to the managed state. Groups:

| Group | Commands |
| --- | --- |
| System | `system_info`, `hardware_info`, `default_output_dir`, `path_exists` |
| Validation | `validate_inputs`, `check_conflicts` |
| Queue | `submit_jobs`, `start_queue`, `pause_queue`, `queue_state`, `list_jobs`, `cancel_job`, `retry_job`, `retry_failed`, `remove_job`, `clear_finished`, `get_workers`, `set_workers` |
| Restore | `restorable_jobs`, `restore_jobs` |
| History | `recent_jobs`, `clear_recent` |
| Updates | `updater_configured` (checking and installing use the updater plugin from the frontend) |
| Media info | `probe_media`, `image_info`, `list_presets`, `format_guide`, `download_sources` |
| Recorder | `capture_targets`, `start_recording`, `stop_recording`, `recording_state` |

### Jobs ([`jobs.rs`](../src-tauri/src/jobs.rs))

- `Operation` is a tagged enum (`{"op": "video_convert", …}`) with one variant
  per tool. The frontend sends the same shape.
- Each `Job` moves through `queued → running → done | failed | cancelled`.
  Failed and cancelled jobs can be retried.
- Eight worker threads are started; at most `max_active` of them (the
  parallel-jobs setting) run jobs at once while the queue is running.
  Each job gets a `Ctx` that carries its cancel flag and reports progress,
  stage messages and the engine in use. Progress events are rate-limited.
- Finished jobs are appended to the recent-jobs history.
- Every queue change rewrites `pending-jobs.json` with the queued and running
  jobs. On the next launch they are held back until the user resumes
  (`restore_jobs(true)`) or discards them.
- Locks use `sync::LockExt::lock_safe`, so a panic inside one job (already
  caught and reported as a failure) can't poison the queue for later jobs.

### Operations ([`ops/`](../src-tauri/src/ops/))

`ops::run` handles every job the same way:

1. **Validate inputs:** the input count and category rule for the operation,
   then [`validate.rs`](../src-tauri/src/validate.rs) checks existence,
   extension, magic bytes and size.
2. **Plan the output:** `target()` decides a file, a folder, or (for
   downloads) a name known only at run time, using suffixes like `_clip` or
   `_joined`. Existing outputs are refused unless overwrite is on;
   `check_conflicts` lets the UI ask first.
3. **Write to a temp path:** work happens in a hidden temp file or folder next
   to the output. A `TempGuard` deletes it on failure or cancel, so no partial
   files are left; on success it is renamed into place.
4. **Dispatch** to the section module (`video`, `audio`, `image`, `pdf`,
   `disc`, `download`, `archive`).

### External tools

- [`deps.rs`](../src-tauri/src/deps.rs) finds each tool by checking
  `CONVERTHUB_<TOOL>`, then the bundled `bin/` folder, then `PATH`. A missing
  tool produces a translated setup hint, not a crash.
- [`ffmpeg.rs`](../src-tauri/src/ffmpeg.rs) `run_process` runs every external
  tool (FFmpeg, yt-dlp, 7-Zip, Poppler, …). It streams stdout lines for
  progress parsing, kills the child process on cancel, and supports timeouts.
  Arguments are passed as a list, never through a shell.
- Native Rust crates cover images, EXIF, PDF merge and extraction, DOCX and
  XLSX output, ZIP/7z and HTTP downloads, so those features work without
  extra tools. See [DEPENDENCIES.md](DEPENDENCIES.md).

### GPU acceleration ([`gpu.rs`](../src-tauri/src/gpu.rs))

Having an encoder compiled into FFmpeg doesn't mean a working GPU and driver
exist, so each candidate (NVENC, QSV, AMF, VideoToolbox, VAAPI, …) is tried
with a tiny test encode. Only encoders that pass are offered. `Accel` is
`auto`, `cpu` or `gpu`; `auto` falls back to CPU.

### Screen recording ([`recorder.rs`](../src-tauri/src/recorder.rs))

Uses FFmpeg capture devices: `gdigrab` on Windows, `avfoundation` on macOS and
`x11grab` on Linux X11. Wayland is reported as unsupported. The OS enforces
capture permission.

### Errors and translation ([`error.rs`](../src-tauri/src/error.rs))

The backend never builds English sentences. Errors and progress stages are a
`UiMsg { key, vars }` (built with the `ui!` macro). It crosses IPC as is, and
the frontend renders it from the active locale.

## Security model

- **Least privilege:** [`capabilities/default.json`](../src-tauri/capabilities/default.json)
  allows only core window APIs (title, taskbar progress, attention request),
  file dialogs, "reveal in folder", opening one help URL, notifications,
  app restart and the updater.
- **Signed updates only:** the updater verifies downloads against the public
  key in the config. With no key configured, the update button is hidden.
- **Validate everything:** all input is re-validated in Rust, whatever the UI
  allowed, including paths passed to info commands such as `probe_media`.
- **Safe file handling:** output names are sanitized, existing files are never
  overwritten silently, and archive extraction guards against path traversal.
- **No shell:** external tools are run with argument lists.
- **Network:** only Download uses the network, and only for permitted sources
  ([DOWNLOAD_SOURCES.md](DOWNLOAD_SOURCES.md)). Redirects are re-checked
  against the same rules, so a direct link cannot redirect onto a disabled
  source.
- **Legal limits:** no DRM or copy-protection bypass. Logo removal and disc
  ripping need explicit authorization.

## Adding an operation

1. Add a variant to `Operation` in `jobs.rs`.
2. Add its input rule and output target in `ops/mod.rs`, and implement it in
   the section module.
3. Add an `OpDef` entry in `src/defs.ts`.
4. Add its strings to `src/locales/en.json` (and the other locales).
5. Add tests, then run `npm test`.

## Testing

- `npm test`: Biome lint, TypeScript type check, i18n key check, version
  check, Vitest unit tests (`src/__tests__/`) and Rust unit tests. CI runs the
  same checks plus `cargo fmt --check` and `cargo clippy`, with Rust tests on
  Windows, macOS and Linux ([`ci.yml`](../.github/workflows/ci.yml)).
- `npm run test:e2e`: end-to-end tests in `ops/e2e_tests.rs` that run real
  conversions with the installed tools. They are marked `#[ignore]`, so they
  only run with this command.
- Manual checklist: [TESTING.md](TESTING.md).
