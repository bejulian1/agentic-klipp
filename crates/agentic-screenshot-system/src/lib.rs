//! Oppsett og systemintegrasjon i Agentic Screenshot: config, tema, innstillinger,
//! tray og global hurtigtast.
//!
//! Alt GTK-relatert her skal kalles fra GTK-hovedtråden.

mod config;
mod extension;
mod history;
pub mod hotkeys;
pub mod i18n;
mod legacy;
mod portal;
mod settings;
mod theme;
mod tray;

/// App-ID. Må være lik navnet på den installerte `.desktop`-filen, ellers nekter
/// portalen å kjenne igjen appen.
pub const APP_ID: &str = "io.github.AgenticScreenshot";

/// Mappenavnet under XDG config, state og cache.
pub const DIR_NAME: &str = "agentic-screenshot";

/// XDG state. Bindingene har ikke `g_get_user_state_dir` på vår GLib-funksjon.
pub fn user_state_dir() -> std::path::PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| gtk::glib::home_dir().join(".local").join("state"))
}

pub use config::{CaptureMethod, Config, Mode, Theme, default_save_dir, pretty_dir};
pub use extension::silent_capture_awaits_login;
pub use hotkeys::Action;
pub use legacy::migrate_legacy;
pub use portal::{HotkeyStatus, hotkey_handed_over, start_hotkey};
pub use settings::{About, SettingsChange, open_settings};
pub use theme::apply_theme;
pub use tray::{Tray, TrayEvent, tray_online, tray_set_window_visible};
