# Security Policy

## Supported versions

Only the latest release of Agentic Screenshot is supported with security updates.

| Version | Supported |
| --- | --- |
| Latest release | Yes |
| Older releases | No |

## Reporting a vulnerability

Please **do not** report security vulnerabilities in public GitHub issues.

Instead, report them privately using GitHub's private vulnerability reporting (**Security** tab → **Report a vulnerability**):

https://github.com/bejulian1/agentic-screenshot/security/advisories/new

Include as much detail as you can (steps to reproduce, affected version, and potential impact) so the issue can be investigated and fixed promptly.

## Scope

Areas particularly relevant to security in Agentic Screenshot include:

- **GNOME Shell extension D-Bus interface:** `shell-extension/` exposes a session D-Bus service that restricts callers to `io.github.AgenticScreenshot` (via GNOME's `DBusSenderChecker`) and restricts capture output paths to `$XDG_RUNTIME_DIR`. Any bypass of caller verification or path restriction is in scope.
- **Screenshot files and storage:** Handling of temporary files, saved PNGs, and file paths.
- **Clipboard handling:** Reading and writing image data, file URIs, and paths to the Wayland/GTK clipboard.
