# Contributing to Agentic Klipp

Thanks for your interest in contributing to Agentic Klipp! Contributions of all kinds are welcome:

- **Bug reports** — clear reproduction steps help a lot.
- **Features and fixes** — code improvements, polish, and bug fixes.
- **Translations / i18n** — the UI is currently Norwegian (bokmål) only; help with internationalization is very welcome.
- **Documentation** — clearer guides, troubleshooting tips, and examples.

## Reporting bugs

When opening a bug report, please include:

- Your Ubuntu and GNOME version
- Session type (Wayland or X11)
- Whether the Klipp GNOME Shell extension is enabled (`gnome-extensions list --enabled | grep klipp`)
- Output of `klipp --version`
- Steps to reproduce the issue (and screenshots if relevant)

## Development setup

Agentic Klipp targets **Ubuntu 26.04** with **GNOME 50** (GTK 4.22 and libadwaita 1.9). Install the build dependencies with:

```sh
sudo apt install cargo rustc mold pkgconf libgtk-4-dev libadwaita-1-dev
```

### Build, test, and lint

```sh
cargo build
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
```

> **Tips:**
> - On low-memory machines, limit parallel jobs with `CARGO_BUILD_JOBS=2`.
> - Release builds (`cargo build --release`) enable full LTO and take several minutes.

### Running component examples

You can run standalone examples for parts of `klipp-system`. Pass a throwaway `XDG_CONFIG_HOME` so your real settings are untouched:

```sh
XDG_CONFIG_HOME=$(mktemp -d) cargo run -p klipp-system --example settings -- --hotkey
cargo run -p klipp-system --example tray
cargo run -p klipp-system --example hotkey
```

### Testing the GNOME Shell extension

On Wayland, GNOME Shell only reloads extensions at login. After modifying files under `shell-extension/` and reinstalling them (e.g., via `./install.sh`), log out and log back in to test your changes.

## Project layout

| Directory | Contents |
| --- | --- |
| `src/` | The app itself: window, shelf, capture, clipboard, and storage |
| `crates/klipp-annotate/` | Annotation window and drawing tools |
| `crates/klipp-system/` | Settings, hotkeys, system tray, and configuration |
| `shell-extension/` | GNOME Shell extension for silent capture and Alt+K |
| `data/` | Icons, CSS, and GResource |
| `prototype/` | HTML prototype used as the visual reference |

## UI strings, changelog, and versioning

- **UI strings:** The app's UI is currently in Norwegian (bokmål). Please keep new UI strings in Norwegian consistent with existing phrasing — or write them in English and note it in your PR so the maintainer can translate and adjust them.
- **Changelog:** For user-visible changes, add an entry to [CHANGELOG.md](CHANGELOG.md) (in Norwegian, or in English for the maintainer to translate) and update the embedded section in `README.md` by running:
  ```sh
  python3 scripts/update-readme-history.py
  ```
- **Versioning:** Bumping the version across all three `Cargo.toml` files is handled by the maintainer at release time. Please don't bump versions in feature or bug-fix PRs.

## Pull request guidelines

- Keep pull requests small, focused, and easy to review.
- Describe what changed and how you tested it.
- Include screenshots or recordings for UI changes.
- Make sure `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` pass so CI stays green.

### AI-assisted contributions

AI-assisted contributions are welcome — Agentic Klipp itself is built with AI coding agents. However, you are responsible for what you submit: please make sure you have read, built, and tested every change yourself before opening a PR.

## License

By contributing to Agentic Klipp, you agree that your contributions will be licensed under the [MIT License](LICENSE).
