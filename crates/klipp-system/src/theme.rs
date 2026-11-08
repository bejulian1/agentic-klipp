//! Tema via AdwStyleManager.

use crate::config::Theme;

/// Krever at libadwaita er initialisert, som den er når `adw::Application` har startet.
pub fn apply_theme(theme: Theme) {
    adw::StyleManager::default().set_color_scheme(match theme {
        Theme::System => adw::ColorScheme::Default,
        Theme::Light => adw::ColorScheme::ForceLight,
        Theme::Dark => adw::ColorScheme::ForceDark,
    });
}
