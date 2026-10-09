# Security Policy

## Supported versions

| Version | Supported |
| --- | --- |
| 0.1.x | Yes |
| < 0.1 | No |

Only the latest release on `main` receives security fixes. Fixes ship through
`hotfix/*` branches.

## Reporting a vulnerability

**Please do not open a public issue for security problems.**

Report vulnerabilities privately via
[GitHub Security Advisories](https://github.com/axcel-blade/ConvertHub/security/advisories/new).
Include:

- A description of the issue and its impact
- Steps to reproduce, or a proof of concept
- Affected version(s) and operating system
- Any suggested fix

You can expect an acknowledgement within 7 days and a status update after
triage. Reporters are credited in the release notes unless they prefer
otherwise.

## Areas of particular interest

- Archive extraction (path traversal, zip bombs)
- Untrusted media, PDF and image files passed to external tools
- The Download feature and URL handling
- Tauri IPC commands and filesystem permissions
- Command construction for FFmpeg and other external tools
