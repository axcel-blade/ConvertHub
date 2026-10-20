# Building and running ConvertHub

ConvertHub is a Tauri 2 app. The backend is Rust (`src-tauri/`) and the
frontend is HTML/CSS/TypeScript bundled by Vite (`src/`). There is no Node.js
backend; Node is only needed at build time.

## Prerequisites (all platforms)

- Rust (stable, 1.77 or newer): <https://rustup.rs>
- Node.js 20 or newer, with npm
- Platform prerequisites for Tauri: <https://tauri.app/start/prerequisites/>

Runtime tools such as FFmpeg are **not** needed to build. They are detected
at runtime (see [DEPENDENCIES.md](DEPENDENCIES.md)).

```bash
npm install
```

### Windows

- Microsoft C++ Build Tools ("Desktop development with C++")
- WebView2 runtime. It ships with Windows 10 (1803+) and 11, and the
  installer bootstraps it if it is missing.

### macOS

- Xcode Command Line Tools: `xcode-select --install`
- macOS 11 or newer (`tauri.macos.conf.json` sets the minimum)

### Linux (Debian/Ubuntu)

```bash
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file \
  libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev
```

Fedora: `sudo dnf install webkit2gtk4.1-devel openssl-devel curl wget file libappindicator-gtk3-devel librsvg2-devel`
and `sudo dnf group install "c-development"`.

## Run in development

```bash
npm run tauri dev
```

This starts Vite on port 1420 and opens the app with hot reload.

## Test

```bash
npm test
```

This runs the Biome lint, the TypeScript type check, the translation key check,
the version consistency check, the Vitest frontend unit tests, and the Rust
unit/integration tests. Before pushing Rust changes, also run these in
`src-tauri/`:

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
```

## Release build and installers

```bash
npm run tauri build
```

Bundles are written to `src-tauri/target/release/bundle/`:

| Platform | Outputs | Config |
| --- | --- | --- |
| Windows | NSIS `.exe` installer (per-user) and `.msi` | `src-tauri/tauri.windows.conf.json` |
| macOS | `.app` and `.dmg` | `src-tauri/tauri.macos.conf.json` |
| Linux | `.deb`, `.rpm`, `.AppImage` | `src-tauri/tauri.linux.conf.json` |

To build only some formats: `npm run tauri build -- --bundles nsis` (or `msi`,
`dmg`, `app`, `deb`, `rpm`, `appimage`). To build only the executable:
`npm run tauri build -- --no-bundle`.

Code signing is not configured. On Windows set up an Authenticode
certificate, and on macOS a Developer ID plus notarization, following the
Tauri distribution guides, before publishing builds.

## Auto-updates (optional)

The updater is built in but stays disabled until a signing key exists, so
unsigned builds never offer updates.

1. Generate a key pair: `npm run tauri signer generate -- -w ~/.tauri/converthub.key`.
2. In the GitHub repository, add the secrets `TAURI_SIGNING_PRIVATE_KEY` and
   `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`, and the variable `TAURI_UPDATER_PUBKEY`
   (the public key text).
3. `build.yml` then builds signed updater artifacts (`*.sig`). Publish them with a
   `latest.json` on the GitHub release; the app checks
   `releases/latest/download/latest.json` from Settings → Updates.

## Versioning

`package.json` holds the version. Run `npm run version:set 0.2.0` to update
`Cargo.toml` and `Cargo.lock` too; `npm run check:version` (also in CI) fails
when they disagree.

## Continuous integration

`.github/workflows/ci.yml` runs the frontend checks (lint, type check, i18n,
version, unit tests, build) and the Rust tests on Ubuntu, Windows and macOS,
plus `cargo fmt --check` and `cargo clippy` on Ubuntu.

`.github/workflows/build.yml` builds and tests on `windows-latest`,
`macos-latest` and `ubuntu-22.04`, and uploads the installers as workflow
artifacts. When the updater signing secrets are set, it also produces signed
updater artifacts (see "Auto-updates" above). It has not been run yet.

## Shipping helper tools inside the app (optional)

ConvertHub looks for helper tools in this order:

1. The `CONVERTHUB_<TOOL>` environment variable, for example
   `CONVERTHUB_FFMPEG=/opt/ffmpeg/bin/ffmpeg`
2. A `bin/` folder in the app's resource directory
3. `PATH`, plus common install folders (7-Zip and LibreOffice on Windows/macOS)

To bundle tools, put the binaries in `src-tauri/bin/` and add
`"resources": { "bin/": "bin/" }` to the `bundle` section of
`tauri.conf.json`. Do this **only** after checking the license obligations in
[DEPENDENCIES.md](DEPENDENCIES.md). The default build bundles no third-party
executables.
