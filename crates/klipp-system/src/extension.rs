//! Shell-utvidelsen som gir opptak uten blits og lukkerlyd. GNOME oppdager nye
//! utvidelser bare ved innlogging, så en utvidelse kan være slått på uten å kjøre.

use gtk::{gio, gio::prelude::*, glib};

const UUID: &str = "agentic-klipp@klipp.local";
const SCHEMA: &str = "org.gnome.shell";
const ENABLED: &str = "enabled-extensions";
const DISABLED: &str = "disabled-extensions";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExtensionState {
    Active,
    NeedsLogin,
    Off,
    NotInstalled,
}

impl ExtensionState {
    fn of(active: bool, installed: bool, enabled: bool) -> Self {
        match (active, installed, enabled) {
            (true, _, _) => Self::Active,
            (_, false, _) => Self::NotInstalled,
            (_, _, true) => Self::NeedsLogin,
            _ => Self::Off,
        }
    }

    pub(crate) fn text(self) -> &'static str {
        match self {
            Self::Active => "På: stille opptak er klart, og Alt+K virker alltid",
            Self::NeedsLogin => "Logg ut og inn én gang for å fullføre",
            Self::Off => "Av: kreves for stille opptak. Uten den blinker GNOME ved hvert bilde",
            Self::NotInstalled => "Utvidelsen mangler. Kjør install.sh",
        }
    }
}

fn shell_settings() -> Option<gio::Settings> {
    gio::SettingsSchemaSource::default()?.lookup(SCHEMA, true)?;
    Some(gio::Settings::new(SCHEMA))
}

/// `active` er om utvidelsen svarer på bussen akkurat nå.
pub(crate) fn extension_state(active: bool) -> ExtensionState {
    let installed = glib::user_data_dir()
        .join("gnome-shell/extensions")
        .join(UUID)
        .is_dir();
    let enabled = shell_settings().is_some_and(|s| s.strv(ENABLED).iter().any(|u| u == UUID));
    ExtensionState::of(active, installed, enabled)
}

/// Slår på utvidelsen fra neste innlogging. Kalles bare når brukeren ber om det.
pub(crate) fn enable_extension() -> bool {
    let Some(settings) = shell_settings() else {
        return false;
    };
    // Hver skriving får GNOME til å slå på utvidelsen. To rett etter hverandre ville
    // startet den to ganger samtidig, og den andre feiler.
    let disabled: Vec<String> = settings
        .strv(DISABLED)
        .iter()
        .map(|u| u.to_string())
        .collect();
    let mut enabled: Vec<String> = settings
        .strv(ENABLED)
        .iter()
        .map(|u| u.to_string())
        .collect();
    let was_disabled = disabled.iter().any(|u| u == UUID);
    let was_enabled = enabled.iter().any(|u| u == UUID);
    let mut saved = true;
    if was_disabled {
        let rest: Vec<String> = disabled.into_iter().filter(|u| u != UUID).collect();
        saved = settings.set_strv(DISABLED, rest).is_ok();
    }
    if !was_enabled {
        enabled.push(UUID.to_string());
        saved = saved && settings.set_strv(ENABLED, enabled).is_ok();
    }
    gio::Settings::sync();
    saved
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_follows_what_gnome_has_loaded() {
        let of = ExtensionState::of;
        assert_eq!(of(true, true, true), ExtensionState::Active);
        assert_eq!(of(false, true, true), ExtensionState::NeedsLogin);
        assert_eq!(of(false, true, false), ExtensionState::Off);
        assert_eq!(of(false, false, true), ExtensionState::NotInstalled);
    }
}
