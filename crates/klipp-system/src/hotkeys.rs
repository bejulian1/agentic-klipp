//! Hurtigtaster: ren logikk for kombinasjoner, opptak og kollisjoner.
//!
//! En kombinasjon lagres og vises som tekst, f.eks. `Alt+K`, i samme
//! form som prototypen. GTK og portalen får den oversatt med [`accel_for`].

use crate::config::{Config, Mode};
use gtk::gdk;
use gtk::glib::translate::FromGlib;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    Capture,
    ModeArea,
    ModeFreehand,
    ModeWindow,
    ModeScreen,
    ToggleShelf,
}

impl Action {
    pub const ALL: [Action; 6] = [
        Action::Capture,
        Action::ModeArea,
        Action::ModeFreehand,
        Action::ModeWindow,
        Action::ModeScreen,
        Action::ToggleShelf,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Action::Capture => "global",
            Action::ModeArea => "area",
            Action::ModeFreehand => "free",
            Action::ModeWindow => "window",
            Action::ModeScreen => "full",
            Action::ToggleShelf => "shelf",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Action::Capture => "Ta skjermbilde",
            Action::ModeArea => "Område",
            Action::ModeFreehand => "Frihånd",
            Action::ModeWindow => "Vindu",
            Action::ModeScreen => "Hele skjermen",
            Action::ToggleShelf => "Vis/skjul hylla",
        }
    }

    pub fn default_key(self) -> &'static str {
        match self {
            Action::Capture => "Alt+K",
            Action::ModeArea => "1",
            Action::ModeFreehand => "2",
            Action::ModeWindow => "3",
            Action::ModeScreen => "4",
            Action::ToggleShelf => "Ctrl+H",
        }
    }

    pub fn mode(self) -> Option<Mode> {
        match self {
            Action::ModeArea => Some(Mode::Area),
            Action::ModeFreehand => Some(Mode::Freehand),
            Action::ModeWindow => Some(Mode::Window),
            Action::ModeScreen => Some(Mode::Screen),
            _ => None,
        }
    }

    pub fn for_mode(mode: Mode) -> Action {
        match mode {
            Mode::Area => Action::ModeArea,
            Mode::Freehand => Action::ModeFreehand,
            Mode::Window => Action::ModeWindow,
            Mode::Screen => Action::ModeScreen,
        }
    }
}

/// Reservert av Klipp og kan ikke bindes. Prototypens `Ctrl+Alt+K` er utelatt
/// med vilje: den hørte bare til prototype-stripa.
pub const RESERVED: [&str; 4] = ["Enter", "Tab", "Space", "Ctrl+Z"];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Mods {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
}

impl Mods {
    pub fn none(self) -> bool {
        self == Mods::default()
    }

    pub fn from_gdk(state: gdk::ModifierType) -> Self {
        Self {
            ctrl: state.contains(gdk::ModifierType::CONTROL_MASK),
            alt: state.contains(gdk::ModifierType::ALT_MASK),
            shift: state.contains(gdk::ModifierType::SHIFT_MASK),
            meta: state.contains(gdk::ModifierType::SUPER_MASK),
        }
    }
}

/// Tastekombinasjon som tekst, samme form som vises i innstillingene.
pub fn combo_text(mods: Mods, key: &str) -> String {
    let mut parts: Vec<&str> = Vec::with_capacity(5);
    if mods.ctrl {
        parts.push("Ctrl");
    }
    if mods.alt {
        parts.push("Alt");
    }
    if mods.shift {
        parts.push("Shift");
    }
    if mods.meta {
        parts.push("Super");
    }
    parts.push(key);
    parts.join("+")
}

/// Deler `Ctrl+Shift+K` i modifikatorer og tast. Tasten kan selv være `+`.
pub fn parse_combo(combo: &str) -> Option<(Mods, &str)> {
    let mut mods = Mods::default();
    let mut rest = combo;
    loop {
        let (flag, tail) = if let Some(t) = rest.strip_prefix("Ctrl+") {
            (&mut mods.ctrl, t)
        } else if let Some(t) = rest.strip_prefix("Alt+") {
            (&mut mods.alt, t)
        } else if let Some(t) = rest.strip_prefix("Shift+") {
            (&mut mods.shift, t)
        } else if let Some(t) = rest.strip_prefix("Super+") {
            (&mut mods.meta, t)
        } else {
            break;
        };
        *flag = true;
        rest = tail;
    }
    (!rest.is_empty()).then_some((mods, rest))
}

/// Samme tastekombinasjon, uansett rekkefølgen på modifikatorene. Standarden
/// heter `Super+Shift+K`, mens et opptak av samme tast gir `Shift+Super+K`.
pub fn same_combo(a: &str, b: &str) -> bool {
    parse_combo(a).is_some() && parse_combo(a) == parse_combo(b)
}

/// Taster med et eget navn i prototypen, med GDK-navnet som hører til.
const NAMED: [(&str, &str); 10] = [
    ("Space", "space"),
    ("Enter", "Return"),
    ("Tab", "Tab"),
    ("Backspace", "BackSpace"),
    ("PageUp", "Page_Up"),
    ("PageDown", "Page_Down"),
    ("ArrowUp", "Up"),
    ("ArrowDown", "Down"),
    ("ArrowLeft", "Left"),
    ("ArrowRight", "Right"),
];

/// Navnet en GDK-tast vises med. Rene modifikatorer gir `None`.
pub fn key_label(key: gdk::Key) -> Option<String> {
    const MODS: [&str; 18] = [
        "Control_L",
        "Control_R",
        "Shift_L",
        "Shift_R",
        "Alt_L",
        "Alt_R",
        "Meta_L",
        "Meta_R",
        "Super_L",
        "Super_R",
        "Hyper_L",
        "Hyper_R",
        "ISO_Level3_Shift",
        "ISO_Level5_Shift",
        "Mode_switch",
        "Caps_Lock",
        "Num_Lock",
        "Scroll_Lock",
    ];
    let name = key.name()?;
    let name = name.as_str();
    if MODS.contains(&name) || name.starts_with("dead_") || name == "VoidSymbol" {
        return None;
    }
    let name = match name {
        "KP_Enter" | "ISO_Enter" => "Return",
        "ISO_Left_Tab" => "Tab",
        n => n,
    };
    if let Some((label, _)) = NAMED.iter().find(|(_, gdk_name)| *gdk_name == name) {
        return Some((*label).to_string());
    }
    match key.to_unicode() {
        Some(c) if !c.is_control() => {
            let mut upper = c.to_uppercase();
            match (upper.next(), upper.next()) {
                (Some(u), None) => Some(u.to_string()),
                _ => Some(c.to_string()),
            }
        }
        _ => Some(name.to_string()),
    }
}

/// GDK-navnet til en tast slik den står i en kombinasjon.
fn key_name(label: &str) -> Option<String> {
    if let Some((_, gdk_name)) = NAMED.iter().find(|(l, _)| *l == label) {
        return Some((*gdk_name).to_string());
    }
    let mut chars = label.chars();
    let name = match (chars.next(), chars.next()) {
        (Some(c), None) => {
            let lower = c.to_lowercase().next().unwrap_or(c);
            let keyval = gdk::unicode_to_keyval(lower as u32);
            // SAFETY: Enhver u32 er en gyldig keyval, ukjente verdier har bare ikke navn.
            unsafe { gdk::Key::from_glib(keyval) }.name()?
        }
        _ => gdk::Key::from_name(label)?.name()?,
    };
    Some(name.to_string())
}

/// `Ctrl+H` til GTK-format, `<Primary>h`. Rekkefølgen følger GTK selv, og det
/// samme formatet er det GNOMEs dialog leser `preferred_trigger` med.
pub fn accel_for(combo: &str) -> Option<String> {
    let (mods, key) = parse_combo(combo)?;
    let mut out = String::new();
    if mods.ctrl {
        out.push_str("<Primary>");
    }
    if mods.alt {
        out.push_str("<Alt>");
    }
    if mods.shift {
        out.push_str("<Shift>");
    }
    if mods.meta {
        out.push_str("<Super>");
    }
    out.push_str(&key_name(key)?);
    Some(out)
}

/// Hva et tastetrykk betyr mens en rad lytter etter en ny snarvei.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyInput {
    /// Esc alene.
    Cancel,
    /// Backspace alene.
    Clear,
    Combo(String),
    /// Bare en modifikator så langt.
    Ignore,
}

/// `digit` er sifferet på tastens fysiske plass, som i prototypen, slik at
/// Shift+1 blir `Shift+1` og ikke `Shift+!` uavhengig av tastaturoppsett.
pub fn classify_key(key: gdk::Key, digit: Option<char>, mods: Mods) -> KeyInput {
    if mods.none() && key == gdk::Key::Escape {
        return KeyInput::Cancel;
    }
    if mods.none() && key == gdk::Key::BackSpace {
        return KeyInput::Clear;
    }
    let label = digit.map(String::from).or_else(|| key_label(key));
    match label {
        Some(label) => KeyInput::Combo(combo_text(mods, &label)),
        None => KeyInput::Ignore,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindError {
    Reserved(String),
    InUse { combo: String, by: Action },
}

impl BindError {
    pub fn message(&self) -> String {
        match self {
            BindError::Reserved(c) => format!("«{c}» er reservert av Klipp"),
            BindError::InUse { combo, by } => {
                format!("«{combo}» brukes allerede av {}", by.label())
            }
        }
    }
}

/// Sjekk om `combo` kan bindes til `action`.
pub fn check_binding(cfg: &Config, action: Action, combo: &str) -> Result<(), BindError> {
    if RESERVED.iter().any(|r| same_combo(r, combo)) {
        return Err(BindError::Reserved(combo.to_string()));
    }
    let other = Action::ALL
        .into_iter()
        .find(|a| *a != action && cfg.key(*a).is_some_and(|k| same_combo(k, combo)));
    match other {
        Some(by) => Err(BindError::InUse {
            combo: combo.to_string(),
            by,
        }),
        None => Ok(()),
    }
}

/// Handlingen en tast utløser inne i Klipp. Den globale hører ikke hjemme her.
pub fn action_for(cfg: &Config, combo: &str) -> Option<Action> {
    Action::ALL
        .into_iter()
        .find(|a| *a != Action::Capture && cfg.key(*a).is_some_and(|k| same_combo(k, combo)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_MODS: Mods = Mods {
        ctrl: true,
        alt: true,
        shift: true,
        meta: true,
    };

    /// `gtk::accelerator_parse` krever en skjerm, så testene leser tasten selv.
    fn accel_key(accel: &str) -> Option<gdk::Key> {
        gdk::Key::from_name(accel.rsplit('>').next()?)
    }

    #[test]
    fn combo_order_matches_prototype() {
        assert_eq!(combo_text(ALL_MODS, "K"), "Ctrl+Alt+Shift+Super+K");
        assert_eq!(combo_text(Mods::default(), "1"), "1");
    }

    #[test]
    fn combos_parse_back() {
        assert_eq!(parse_combo("Ctrl+Alt+Shift+Super+K"), Some((ALL_MODS, "K")));
        assert_eq!(parse_combo("1"), Some((Mods::default(), "1")));
        let ctrl = Mods {
            ctrl: true,
            ..Mods::default()
        };
        assert_eq!(parse_combo("Ctrl++"), Some((ctrl, "+")));
        assert_eq!(parse_combo("Ctrl+"), None);
        assert_eq!(parse_combo(""), None);
    }

    #[test]
    fn key_labels_are_normalized() {
        assert_eq!(key_label(gdk::Key::a).as_deref(), Some("A"));
        assert_eq!(key_label(gdk::Key::K).as_deref(), Some("K"));
        assert_eq!(key_label(gdk::Key::space).as_deref(), Some("Space"));
        assert_eq!(key_label(gdk::Key::Return).as_deref(), Some("Enter"));
        assert_eq!(key_label(gdk::Key::KP_Enter).as_deref(), Some("Enter"));
        assert_eq!(key_label(gdk::Key::ISO_Left_Tab).as_deref(), Some("Tab"));
        assert_eq!(key_label(gdk::Key::F5).as_deref(), Some("F5"));
        assert_eq!(key_label(gdk::Key::Up).as_deref(), Some("ArrowUp"));
        assert_eq!(key_label(gdk::Key::Page_Down).as_deref(), Some("PageDown"));
        assert_eq!(key_label(gdk::Key::Delete).as_deref(), Some("Delete"));
        assert_eq!(key_label(gdk::Key::Escape).as_deref(), Some("Escape"));
        assert_eq!(key_label(gdk::Key::plus).as_deref(), Some("+"));
        assert_eq!(key_label(gdk::Key::aring).as_deref(), Some("Å"));
        assert_eq!(key_label(gdk::Key::Shift_L), None);
        assert_eq!(key_label(gdk::Key::Super_L), None);
        assert_eq!(key_label(gdk::Key::dead_acute), None);
    }

    #[test]
    fn accels_name_real_keys() {
        for combo in [
            "Super+Shift+K",
            "Ctrl+H",
            "1",
            "Ctrl+Alt+F5",
            "Space",
            "Ctrl+ArrowUp",
            "PageDown",
            "Ctrl++",
            "Alt+-",
            "Å",
            "Shift+Backspace",
        ] {
            let accel = accel_for(combo).unwrap_or_else(|| panic!("{combo} ga ikke accel"));
            assert!(accel_key(&accel).is_some(), "{combo} ga {accel}");
        }
        assert_eq!(
            accel_for("Ctrl+Alt+F5").as_deref(),
            Some("<Primary><Alt>F5")
        );
        assert_eq!(accel_for("Å").as_deref(), Some("aring"));
        assert_eq!(accel_for("Ctrl++").as_deref(), Some("<Primary>plus"));
        assert_eq!(accel_for("Ukjenttast"), None);
    }

    #[test]
    fn labels_round_trip_through_accels() {
        for key in [gdk::Key::q, gdk::Key::F12, gdk::Key::Home, gdk::Key::ae] {
            let label = key_label(key).unwrap();
            assert_eq!(accel_key(&accel_for(&label).unwrap()), Some(key));
        }
    }

    #[test]
    fn recording_understands_esc_and_backspace() {
        let plain = Mods::default();
        assert_eq!(
            classify_key(gdk::Key::Escape, None, plain),
            KeyInput::Cancel
        );
        assert_eq!(
            classify_key(gdk::Key::BackSpace, None, plain),
            KeyInput::Clear
        );
        assert_eq!(
            classify_key(gdk::Key::Control_L, None, plain),
            KeyInput::Ignore
        );
    }

    #[test]
    fn recording_gives_combos() {
        let ctrl = Mods {
            ctrl: true,
            ..Mods::default()
        };
        let shift = Mods {
            shift: true,
            ..Mods::default()
        };
        assert_eq!(
            classify_key(gdk::Key::h, None, ctrl),
            KeyInput::Combo("Ctrl+H".into())
        );
        assert_eq!(
            classify_key(gdk::Key::Escape, None, ctrl),
            KeyInput::Combo("Ctrl+Escape".into())
        );
        assert_eq!(
            classify_key(gdk::Key::BackSpace, None, shift),
            KeyInput::Combo("Shift+Backspace".into())
        );
        assert_eq!(
            classify_key(gdk::Key::exclam, Some('1'), shift),
            KeyInput::Combo("Shift+1".into())
        );
    }

    #[test]
    fn reserved_keys_are_refused() {
        let c = Config::default();
        assert_eq!(
            check_binding(&c, Action::ModeArea, "Ctrl+Z"),
            Err(BindError::Reserved("Ctrl+Z".into()))
        );
        assert_eq!(
            check_binding(&c, Action::ModeArea, "Ctrl+Z")
                .unwrap_err()
                .message(),
            "«Ctrl+Z» er reservert av Klipp"
        );
        for combo in ["Enter", "Tab", "Space"] {
            assert!(check_binding(&c, Action::ToggleShelf, combo).is_err());
        }
    }

    #[test]
    fn modifier_order_does_not_matter() {
        assert!(same_combo("Super+Shift+K", "Shift+Super+K"));
        assert!(!same_combo("Super+K", "Shift+Super+K"));
        let mut c = Config::default();
        c.set_key(Action::Capture, Some("Super+Shift+K".into()));
        assert_eq!(
            check_binding(&c, Action::ModeArea, "Shift+Super+K"),
            Err(BindError::InUse {
                combo: "Shift+Super+K".into(),
                by: Action::Capture
            })
        );
        c.set_key(Action::Capture, Some("Alt+K".into()));
        assert!(c.keys_are_default());
    }

    #[test]
    fn prototype_only_reservation_is_gone() {
        let c = Config::default();
        assert_eq!(check_binding(&c, Action::Capture, "Ctrl+Alt+K"), Ok(()));
    }

    #[test]
    fn collision_names_the_other_action() {
        let c = Config::default();
        let e = check_binding(&c, Action::ModeScreen, "1").unwrap_err();
        assert_eq!(
            e,
            BindError::InUse {
                combo: "1".into(),
                by: Action::ModeArea
            }
        );
        assert_eq!(e.message(), "«1» brukes allerede av Område");
    }

    #[test]
    fn rebinding_own_key_is_fine() {
        let c = Config::default();
        assert_eq!(check_binding(&c, Action::ModeArea, "1"), Ok(()));
        assert_eq!(check_binding(&c, Action::ModeArea, "Q"), Ok(()));
    }

    #[test]
    fn local_actions_ignore_the_global_key() {
        let c = Config::default();
        assert_eq!(action_for(&c, "Super+Shift+K"), None);
        assert_eq!(action_for(&c, "Ctrl+H"), Some(Action::ToggleShelf));
        assert_eq!(action_for(&c, "3"), Some(Action::ModeWindow));
    }

    #[test]
    fn modes_map_to_actions_both_ways() {
        for mode in Mode::ALL {
            assert_eq!(Action::for_mode(mode).mode(), Some(mode));
        }
        assert_eq!(Action::Capture.mode(), None);
    }
}
