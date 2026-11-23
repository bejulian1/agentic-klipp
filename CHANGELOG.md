# Changelog

## 2.0.1 — 2026-11-23

- The window opens in the middle of the screen again when you reopen it. After the minimize button had been used once, reopening from the app overview put the window in the top-left corner, and reopening from the system tray did not show it at all.
- Settings and the drawing window no longer come back minimized after a screenshot just because they had been minimized once before.
- When the Agentic Screenshot extension is turned on but GNOME has not loaded it yet, the app tells you once that GNOME took the screenshot with flash and sound, and that silent capture works after you log out and back in. Settings says the same under Capture Method, and Flash and Shutter Sound are grayed out until the extension is running.
- The shortcut no longer takes the first screenshot with flash and sound when it has to start the app first.
- Ctrl+A, Delete, and the other shortcuts in the main window do nothing while Settings or the drawing window is open.
- The trash button on an image is hidden from screen readers too while you select several images.
- A freehand capture that is only a line is rejected as too small, instead of saving an empty image.
- The drawing window fits on small screens and in half a screen. The toolbar scrolls sideways when there is not room for all of it.
- Saving in the drawing window keeps the extended permissions (ACL) of the file.

## 2.0.0 — 2026-11-23

- Agentic Klipp is now called Agentic Screenshot. The binary is `agentic-screenshot`, the app ID is `io.github.AgenticScreenshot`, and the Shell extension is `agentic-screenshot@bejulian1.github.io`.
- The first time the new version starts, settings, the shelf, and the cache move automatically from `~/.config/klipp`, `~/.local/state/klipp`, and `~/.cache/klipp` to folders named `agentic-screenshot`. Autostart and custom GNOME shortcuts that launched `klipp` are pointed at the new binary, and the new extension is turned on if the old one was on.
- Screenshots are never moved. If you used the default `Pictures/Klipp` folder, new screenshots keep going there after the upgrade. New installations save to `Pictures/Agentic Screenshot`, with file names that start with `screenshot-`.
- install.sh removes the old binary, .desktop file, icons, autostart entry, and extension before installing the new ones.
- Log out and back in once after upgrading so GNOME Shell loads the renamed extension. Until then, GNOME captures with a flash.
- Without the extension, GNOME asks once more for screenshot access and for the keyboard shortcut, because the app has a new app ID.
- The Debian package is named `agentic-screenshot` and replaces `agentic-klipp`.
- The version history in the app is shown in English for every language except Norwegian Bokmål, which still gets the Norwegian one.
- Restart in Settings no longer leaves the app closed when the old process is slow to be cleaned up. The new instance starts anyway after five seconds.

## 1.3.0 — 2026-11-20

- Klipp is now available in ten languages: Norwegian Bokmål, English, Swedish, Danish, German, French, Spanish, Italian, Brazilian Portuguese, and Dutch.
- New language option under Settings → Appearance. The default is the system language, and Klipp uses English when the system language is not supported.
- The Restart button applies the new language right away.
- The app name and the quick actions in the app overview are translated.
- The option buttons in Settings always show the full text.
- The screenshot shelf no longer loses history during rapid changes, right after startup, or when the pictures folder is disconnected. A corrupted shelf file is kept as shelf.tsv.ugyldig.
- Freehand captures are fully transparent outside the shape, and Copy in the drawing window also works after you have switched apps.
- The Debian package finds the Klipp extension, and Alt+K works after installing from the package.
- The source code is open on GitHub with an English README, contribution guidelines, and automated testing.
- The README shows new screenshots in English.

## 1.2.1 — 2026-11-14

- The text in the version history cannot be selected or receive text focus.
- Removed the blue text selection that appeared automatically at the top when opening.
- Removed the introduction text above the versions.
- The newest version is shown at the top, the oldest at the bottom.
- The README shows the main window, the top of Settings, and annotation mode.

## 1.2.0 — 2026-11-14

- New clickable Version History under Settings → About Klipp.
- Separate scrollable window with the versions from oldest to newest, and the current version and build number.
- The same built-in version history in the app and on GitHub, also available offline.
- Shared version 1.2.0 for the app and the internal libraries.
- The build number can be pinned at release; local builds use the time in Europe/Oslo.
- Updated README with three screenshots and credits for Claude and Codex.
- Release packages with a Linux binary, installation archive, Debian package, and SHA-256 checksums.

## 1.1.1 — 2026-11-08

First tagged and published GitHub version. Also summarizes the development in the 1.1 series before publication.

- Screenshots of an area, freehand, a window, and the entire screen, with Alt+K as the default.
- GNOME Shell extension for silent capture and a global shortcut.
- Shelf with multi-select, bulk copy, and trash.
- Drawing tools including arrow, line, ellipse, highlighter, text, redaction, and eraser.
- Eight colors, a custom color, and five sizes in the drawing window.
- Minimize button and the Code Pro signature under About Klipp.
- Installation script that checks dependencies, builds, installs, and launches the app.

## 1.0.0 — Early local version

First local version, referenced in the configuration migration. The exact date and full list of changes were not preserved in the Git history.

- The global shortcut was Super+Shift+K.
- The shelf was wide by default, and capturing had a separate confirmation step.
