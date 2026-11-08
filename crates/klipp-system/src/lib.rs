//! Oppsett og systemintegrasjon i Agentic Klipp: config, tema, innstillinger,
//! tray og global hurtigtast.
//!
//! Alt GTK-relatert her skal kalles fra GTK-hovedtråden.

mod config;
mod extension;
pub mod hotkeys;
mod portal;
mod settings;
mod theme;
mod tray;

/// App-ID. Må være lik navnet på den installerte `.desktop`-filen, ellers nekter
/// portalen å kjenne igjen Klipp.
pub const APP_ID: &str = "io.github.AgenticKlipp";

pub use config::{CaptureMethod, Config, Mode, Theme, default_save_dir, pretty_dir};
pub use hotkeys::Action;
pub use portal::{HotkeyStatus, hotkey_handed_over, start_hotkey};
pub use settings::{About, SettingsChange, open_settings};
pub use theme::apply_theme;
pub use tray::{Tray, TrayEvent, tray_online, tray_set_window_visible};
