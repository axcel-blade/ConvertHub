# Contributing to ConvertHub

Thanks for helping improve ConvertHub! This guide covers how we branch, build,
test and review changes.

## Code of conduct

By participating you agree to follow our [Code of Conduct](CODE_OF_CONDUCT.md).

## Getting set up

```bash
npm install
npm run tauri dev
```

You need Node.js, a Rust toolchain and the Tauri 2 prerequisites for your OS.
See [docs/BUILDING.md](docs/BUILDING.md). Install FFmpeg for video/audio work.

## Branching model (Git Flow)

| Branch | Purpose | Branches from | Merges into |
| --- | --- | --- | --- |
| `main` | Released, production-ready code. Every merge is a tagged release. | — | — |
| `develop` | Integration branch for the next release. | `main` | `release/*` |
| `feature/*` | New features and non-urgent fixes, e.g. `feature/pdf-split`. | `develop` | `develop` |
| `release/*` | Release stabilisation, e.g. `release/0.2.0`. Only fixes, docs and version bumps. | `develop` | `main` and `develop` |
| `hotfix/*` | Urgent fixes to a released version, e.g. `hotfix/0.1.1`. | `main` | `main` and `develop` |

Rules of thumb:

- Never commit directly to `main` or `develop`; open a pull request.
- Keep feature branches short-lived and merge `develop` into them regularly.
- Tag releases on `main` as `vX.Y.Z` and update [CHANGELOG.md](CHANGELOG.md).

## Making a change

1. Open or find an issue describing the change.
2. Create a branch from `develop`: `git checkout -b feature/short-description develop`.
3. Make your change, keeping commits focused.
4. Run the checks with `npm test` (TypeScript type check, i18n key check and Rust tests).
5. Update docs (`docs/`, `README.md`, `CHANGELOG.md`) when behaviour changes.
6. Open a pull request against `develop` and fill in the template.

## Continuous integration

Two GitHub Actions workflows run automatically:

| Workflow | Runs on | What it does |
| --- | --- | --- |
| [`ci.yml`](.github/workflows/ci.yml) | Pushes to `main`, `develop`, `feature/**`, `release/**`, `hotfix/**`; PRs into `main`, `develop`, `release/**` | Type check, i18n check and Vite build (frontend), plus `cargo test` (backend), on Ubuntu |
| [`build.yml`](.github/workflows/build.yml) | Pushes to `main` and `develop`; all PRs | Full checks and installers for Windows, macOS and Linux, uploaded as artifacts |

Pull requests must pass CI before merging. Running `npm test` locally covers
the same checks as `ci.yml`.

## Commit messages

We use [Conventional Commits](https://www.conventionalcommits.org/):
`feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`, `build:`, `ci:`.

## Translations

New UI strings must be added to every locale. See
[docs/TRANSLATING.md](docs/TRANSLATING.md); `npm run check:i18n` verifies this.

## Scope and legal limits

ConvertHub does not bypass DRM or copy protection, and downloads only from
permitted sources. Contributions that add such capabilities will not be accepted.

## Reporting bugs and security issues

Use the issue templates for bugs and feature requests. Report vulnerabilities
privately as described in [SECURITY.md](SECURITY.md).
