//! Lokale hurtigtaster: oversetter tastetrykk til handlingene i `Config`.

use gtk::gdk;
use gtk::prelude::*;
use klipp_system::i18n::tr_fmt;
use klipp_system::{Action, Config};

/// Handlingene som virker inne i Klipp. Den globale hører ikke hjemme her.
const LOCAL: [Action; 5] = [
    Action::ModeArea,
    Action::ModeFreehand,
    Action::ModeWindow,
    Action::ModeScreen,
    Action::ToggleShelf,
];

fn relevant(state: gdk::ModifierType) -> gdk::ModifierType {
    state
        & (gdk::ModifierType::CONTROL_MASK
            | gdk::ModifierType::ALT_MASK
            | gdk::ModifierType::SHIFT_MASK
            | gdk::ModifierType::SUPER_MASK
            | gdk::ModifierType::META_MASK)
}

/// Finn handlingen for et tastetrykk. Tasten leses uten modifikatorer, slik at
/// sifferraden virker på nordisk tastatur også med Shift.
pub fn action_for(
    cfg: &Config,
    display: &gdk::Display,
    keyval: gdk::Key,
    keycode: u32,
    group: u32,
    state: gdk::ModifierType,
) -> Option<Action> {
    let base = display
        .translate_key(keycode, gdk::ModifierType::empty(), group as i32)
        .map_or(keyval, |t| t.0);
    let want = relevant(state);
    LOCAL.into_iter().find(|a| {
        let Some(accel) = cfg.accel(*a) else {
            return false;
        };
        let Some((key, mods)) = gtk::accelerator_parse(&accel) else {
            return false;
        };
        relevant(mods) == want && (key.to_lower() == base.to_lower() || key == keyval)
    })
}

/// Teksten en hurtigtast vises med, for eksempel `Ctrl+H`. `None` når den er fjernet.
pub fn label(cfg: &Config, action: Action) -> Option<String> {
    cfg.key(action).map(str::to_string)
}

pub fn hint(cfg: &Config, action: Action, label: &str) -> String {
    if let Some(key) = cfg.key(action) {
        // Oversettere: {label} is the action name and {key} is its configured shortcut.
        tr_fmt("{label} ({key})", &[("label", label), ("key", key)])
    } else {
        label.to_owned()
    }
}

/// Filtrerer bort tasterepetisjon. GTK skiller ikke repetisjon fra nytt trykk.
#[derive(Default)]
pub struct RepeatFilter(std::cell::Cell<Option<u32>>);

impl RepeatFilter {
    /// Sann første gang en tast trykkes, usann mens den holdes nede.
    pub fn press(&self, keycode: u32) -> bool {
        self.0.replace(Some(keycode)) != Some(keycode)
    }
    pub fn release(&self) {
        self.0.set(None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeat_filter_lets_first_press_through() {
        let f = RepeatFilter::default();
        assert!(f.press(10));
        assert!(!f.press(10));
        assert!(f.press(11));
        f.release();
        assert!(f.press(11));
    }
}
