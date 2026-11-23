# Contributing to Agentic Screenshot

Thanks for your interest in contributing to Agentic Screenshot! Contributions of all kinds are welcome:

- **Bug reports** — clear reproduction steps help a lot.
- **Features and fixes** — code improvements, polish, and bug fixes.
- **Translations** — help translate the app into more languages.
- **Documentation** — clearer guides, troubleshooting tips, and examples.

## Reporting bugs

When opening a bug report, please include:

- Your Ubuntu and GNOME version
- Session type (Wayland or X11)
- Whether the Agentic Screenshot GNOME Shell extension is enabled (`gnome-extensions list --enabled | grep agentic-screenshot`)
- Output of `agentic-screenshot --version`
- Steps to reproduce the issue (and screenshots if relevant)

## Development setup

Agentic Screenshot targets **Ubuntu 26.04** with **GNOME 50** (GTK 4.22 and libadwaita 1.9). Install the build dependencies with:

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

You can run standalone examples for parts of `agentic-screenshot-system`. Pass a throwaway `XDG_CONFIG_HOME` so your real settings are untouched:

```sh
XDG_CONFIG_HOME=$(mktemp -d) cargo run -p agentic-screenshot-system --example settings -- --hotkey
cargo run -p agentic-screenshot-system --example tray
cargo run -p agentic-screenshot-system --example hotkey
```

### Testing the GNOME Shell extension

On Wayland, GNOME Shell only reloads extensions at login. After modifying files under `shell-extension/` and reinstalling them (e.g., via `./install.sh`), log out and log back in to test your changes.

## Project layout

| Directory | Contents |
| --- | --- |
| `src/` | The app itself: window, shelf, capture, clipboard, and storage |
| `crates/agentic-screenshot-annotate/` | Annotation window and drawing tools |
| `crates/agentic-screenshot-system/` | Settings, hotkeys, system tray, and configuration |
| `shell-extension/` | GNOME Shell extension for silent capture and Alt+K |
| `data/` | Icons, CSS, and GResource |
| `prototype/` | HTML prototype used as the visual reference |

## Translations

Norwegian Bokmål strings in Rust are the source messages. Use the functions in
`agentic_screenshot_system::i18n` for UI strings and add `// Oversettere:` comments where
translators need context. Run `python3 -I scripts/update-po.py` to regenerate
`po/agentic-screenshot.pot` and merge changes into the language catalogs.

Edit `po/<language>.po` in a text editor or Poedit. Keep the same named
placeholders, such as `{name}` and `{n}`, in each translation and its source
form. You can move them within the sentence. Blank entries fall back to English.

To add a language, add its code and endonym to `Language` in
`crates/agentic-screenshot-system/src/i18n.rs`, configure locale matching and plural rules,
then create and embed `po/<language>.po`. Add the corresponding PO header rule
in `scripts/update-po.py` and run the script again. Once the catalog is complete,
add the language to `COMPLETE_LANGUAGES` in the i18n tests.

## Changelog and versioning

- **Changelog:** For user-visible changes, add an entry to [CHANGELOG.md](CHANGELOG.md) (English) and to [CHANGELOG.nb.md](CHANGELOG.nb.md) (Norwegian Bokmål, or leave it for the maintainer to translate). Both files must list the same versions; the app shows the Norwegian one when its language is Norwegian Bokmål and the English one otherwise. Update the embedded section in `README.md` by running:
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

AI-assisted contributions are welcome — Agentic Screenshot itself is built with AI coding agents. However, you are responsible for what you submit: please make sure you have read, built, and tested every change yourself before opening a PR.

## License

By contributing to Agentic Screenshot, you agree that your contributions will be licensed under the [MIT License](LICENSE).
