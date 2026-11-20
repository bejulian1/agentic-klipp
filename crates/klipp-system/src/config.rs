//! Innstillinger. Lagres som enkel `nøkkel=verdi`-tekst under XDG config, slik
//! at en manglende eller ødelagt fil aldri kan stoppe appen.

use crate::hotkeys::{self, Action};
use crate::i18n::{Language, tr, trc};
use gtk::glib;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Øker når standardvalg byttes ut, se [`Config::migrate`].
const SCHEMA: u32 = 2;
/// Standardtasten i v1.0.0.
const OLD_CAPTURE_KEY: &str = "Super+Shift+K";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Area,
    Freehand,
    Window,
    Screen,
}

impl Mode {
    pub const ALL: [Mode; 4] = [Mode::Area, Mode::Freehand, Mode::Window, Mode::Screen];

    pub fn id(self) -> &'static str {
        match self {
            Mode::Area => "area",
            Mode::Freehand => "free",
            Mode::Window => "window",
            Mode::Screen => "full",
        }
    }

    pub fn from_id(s: &str) -> Option<Mode> {
        Mode::ALL.into_iter().find(|m| m.id() == s)
    }
}

/// Hvem som tar bildet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CaptureMethod {
    /// Direkte gjennom utvidelsen. Blits og lyd er valgfritt.
    #[default]
    Silent,
    /// GNOMEs vanlige vei, som alltid har blits og lukkerlyd.
    Gnome,
}

impl CaptureMethod {
    pub const ALL: [CaptureMethod; 2] = [CaptureMethod::Silent, CaptureMethod::Gnome];

    pub fn id(self) -> &'static str {
        match self {
            CaptureMethod::Silent => "silent",
            CaptureMethod::Gnome => "gnome",
        }
    }

    pub fn from_id(s: &str) -> Option<CaptureMethod> {
        CaptureMethod::ALL.into_iter().find(|m| m.id() == s)
    }

    pub fn label(self) -> &'static str {
        match self {
            CaptureMethod::Silent => tr("Stille"),
            CaptureMethod::Gnome => "GNOME",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            CaptureMethod::Silent => tr("Raskest. Du velger selv blits og lyd under"),
            CaptureMethod::Gnome => tr("GNOMEs vanlige vei, alltid med blits og lukkerlyd"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

impl Theme {
    pub const ALL: [Theme; 3] = [Theme::System, Theme::Light, Theme::Dark];

    pub fn id(self) -> &'static str {
        match self {
            Theme::System => "system",
            Theme::Light => "light",
            Theme::Dark => "dark",
        }
    }

    pub fn from_id(s: &str) -> Option<Theme> {
        Theme::ALL.into_iter().find(|t| t.id() == s)
    }

    pub fn label(self) -> &'static str {
        match self {
            Theme::System => trc("theme", "System"),
            Theme::Light => trc("theme", "Lys"),
            Theme::Dark => trc("theme", "Mørk"),
        }
    }
}

pub fn default_save_dir() -> PathBuf {
    glib::user_special_dir(glib::UserDirectory::Pictures)
        .unwrap_or_else(|| glib::home_dir().join("Pictures"))
        .join("Klipp")
}

/// Vis stien som `~/Bilder/Klipp/` når den ligger under hjemmemappen.
pub fn pretty_dir(dir: &Path) -> String {
    match dir.strip_prefix(glib::home_dir()) {
        Ok(rel) => format!("~/{}/", rel.display()),
        Err(_) => format!("{}/", dir.display()),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub theme: Theme,
    pub language: Language,
    pub instant_capture: bool,
    pub auto_copy: bool,
    pub capture_method: CaptureMethod,
    /// Blits over området når bildet er tatt. Gjelder stille opptak.
    pub flash: bool,
    /// Lukkerlyd når bildet er tatt. Gjelder stille opptak.
    pub sound: bool,
    pub compact: bool,
    pub last_mode: Mode,
    /// Egen mappe for nye bilder. `None` er standardmappen.
    pub custom_save_dir: Option<PathBuf>,
    /// Varselet om at Klipp kjører i bakgrunnen er vist.
    pub close_hint_shown: bool,
    /// Tasten brukeren avviste i GNOMEs dialog. Klipp spør ikke på nytt om den,
    /// ellers ville dialogen dukket opp ved hver innlogging med autostart.
    pub(crate) hotkey_declined: Option<String>,
    keys: [Option<String>; Action::ALL.len()],
}

impl Default for Config {
    fn default() -> Self {
        Self {
            theme: Theme::System,
            language: Language::System,
            instant_capture: true,
            auto_copy: true,
            capture_method: CaptureMethod::Silent,
            flash: false,
            sound: false,
            compact: true,
            last_mode: Mode::Area,
            custom_save_dir: None,
            close_hint_shown: false,
            hotkey_declined: None,
            keys: Action::ALL.map(|a| Some(a.default_key().to_string())),
        }
    }
}

fn index(a: Action) -> usize {
    Action::ALL.iter().position(|x| *x == a).unwrap_or(0)
}

impl Config {
    /// Leser config fra XDG config, eller standardverdier hvis filen mangler eller er ugyldig.
    pub fn load() -> Self {
        Self::load_from(&Self::path())
    }

    pub fn save(&self) -> Result<(), String> {
        let path = Self::path();
        self.save_to(&path)
            .map_err(|e| format!("Kunne ikke lagre {}: {e}", path.display()))
    }

    pub fn path() -> PathBuf {
        gtk::glib::user_config_dir().join("klipp").join("config")
    }

    pub fn load_from(path: &Path) -> Self {
        fs::read_to_string(path)
            .map(|t| Self::parse(&t))
            .unwrap_or_default()
    }

    pub fn save_to(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        write_atomic(path, self.serialize().as_bytes())
    }

    pub fn save_dir(&self) -> PathBuf {
        self.custom_save_dir
            .clone()
            .unwrap_or_else(default_save_dir)
    }

    /// Hurtigtast i GTK-format (f.eks. `<Primary>h`), eller `None` hvis den er fjernet.
    pub fn accel(&self, action: Action) -> Option<String> {
        self.key(action).and_then(hotkeys::accel_for)
    }

    /// Hurtigtast slik den vises og lagres (f.eks. `Ctrl+H`).
    pub fn key(&self, action: Action) -> Option<&str> {
        self.keys[index(action)].as_deref()
    }

    pub fn set_key(&mut self, action: Action, combo: Option<String>) {
        self.keys[index(action)] = combo;
    }

    pub fn reset_keys(&mut self) {
        self.keys = Action::ALL.map(|a| Some(a.default_key().to_string()));
    }

    pub fn keys_are_default(&self) -> bool {
        Action::ALL.into_iter().all(|a| {
            self.key(a)
                .is_some_and(|k| hotkeys::same_combo(k, a.default_key()))
        })
    }

    /// Ukjente linjer og ugyldige verdier hoppes over.
    pub fn parse(text: &str) -> Self {
        let mut c = Self::default();
        let mut schema = 1;
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let (k, v) = (k.trim(), v.trim());
            match k {
                "schema" => schema = v.parse().unwrap_or(schema),
                "mode" => {
                    if let Some(m) = Mode::from_id(v) {
                        c.last_mode = m;
                    }
                }
                "theme" => {
                    if let Some(t) = Theme::from_id(v) {
                        c.theme = t;
                    }
                }
                "language" => {
                    if let Some(language) = Language::from_id(v) {
                        c.language = language;
                    }
                }
                "compact" => parse_bool(v, &mut c.compact),
                "instant" => parse_bool(v, &mut c.instant_capture),
                "autocopy" => parse_bool(v, &mut c.auto_copy),
                "capture_method" => {
                    if let Some(m) = CaptureMethod::from_id(v) {
                        c.capture_method = m;
                    }
                }
                "flash" => parse_bool(v, &mut c.flash),
                "sound" => parse_bool(v, &mut c.sound),
                "save_dir" if Path::new(v).is_absolute() => {
                    c.custom_save_dir = Some(PathBuf::from(v));
                }
                "close_hint_shown" => parse_bool(v, &mut c.close_hint_shown),
                "hotkey_declined" => c.hotkey_declined = (!v.is_empty()).then(|| v.to_string()),
                _ => {
                    let action = k
                        .strip_prefix("key.")
                        .and_then(|id| Action::ALL.into_iter().find(|a| a.id() == id));
                    if let Some(a) = action {
                        if v.is_empty() {
                            c.set_key(a, None);
                        } else if hotkeys::accel_for(v).is_some() {
                            c.set_key(a, Some(v.to_string()));
                        }
                    }
                }
            }
        }
        if schema < SCHEMA {
            c.migrate();
        }
        c
    }

    /// Filer fra v1.0.0 har valg som ikke lenger er standard: bred hylle, eget steg
    /// for å ta bildet, og en tast mange tastatur ikke har. Avslag på tasten glemmes,
    /// for de kom ofte fra en dialog som aldri nådde brukeren.
    fn migrate(&mut self) {
        self.compact = true;
        self.instant_capture = true;
        self.hotkey_declined = None;
        let capture = Action::Capture;
        if self
            .key(capture)
            .is_some_and(|k| hotkeys::same_combo(k, OLD_CAPTURE_KEY))
        {
            self.set_key(capture, Some(capture.default_key().to_string()));
        }
    }

    pub fn serialize(&self) -> String {
        let mut s = format!(
            "schema={SCHEMA}\nmode={}\ncompact={}\ntheme={}\nlanguage={}\ninstant={}\nautocopy={}\ncapture_method={}\nflash={}\nsound={}\nclose_hint_shown={}\n",
            self.last_mode.id(),
            self.compact,
            self.theme.id(),
            self.language.id(),
            self.instant_capture,
            self.auto_copy,
            self.capture_method.id(),
            self.flash,
            self.sound,
            self.close_hint_shown
        );
        if let Some(dir) = self.custom_save_dir.as_ref().and_then(|d| d.to_str()) {
            s.push_str(&format!("save_dir={dir}\n"));
        }
        if let Some(combo) = &self.hotkey_declined {
            s.push_str(&format!("hotkey_declined={combo}\n"));
        }
        for a in Action::ALL {
            s.push_str(&format!("key.{}={}\n", a.id(), self.key(a).unwrap_or("")));
        }
        s
    }
}

fn parse_bool(v: &str, out: &mut bool) {
    match v {
        "true" => *out = true,
        "false" => *out = false,
        _ => {}
    }
}

/// Skriv via en midlertidig fil, så en krasj aldri etterlater en halv fil.
/// Følger symlenker, slik at en config i dotfiles-oppsett forblir en lenke.
fn write_atomic(path: &Path, data: &[u8]) -> io::Result<()> {
    let target = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let mut tmp = target.as_os_str().to_owned();
    tmp.push(format!(".tmp{}", std::process::id()));
    let tmp = PathBuf::from(tmp);
    let written = fs::File::create(&tmp).and_then(|mut f| {
        f.write_all(data)?;
        f.sync_all()
    });
    written
        .and_then(|()| fs::rename(&tmp, &target))
        .inspect_err(|_| {
            let _ = fs::remove_file(&tmp);
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("klipp-cfg-{name}-{}", std::process::id()))
    }

    #[test]
    fn language_round_trips_and_defaults_to_system() {
        for language in Language::ALL {
            let config = Config {
                language,
                ..Config::default()
            };
            assert_eq!(Config::parse(&config.serialize()), config);
        }
        assert_eq!(Config::parse("").language, Language::System);
        assert_eq!(Config::parse("language=xx").language, Language::System);
    }

    #[test]
    fn defaults_match_prototype() {
        let c = Config::default();
        assert_eq!(c.last_mode, Mode::Area);
        assert_eq!(c.theme, Theme::System);
        assert!(c.compact && c.instant_capture && c.auto_copy);
        assert_eq!(c.capture_method, CaptureMethod::Silent);
        assert!(!c.flash && !c.sound);
        assert_eq!(c.key(Action::Capture), Some("Alt+K"));
        assert_eq!(c.key(Action::ModeArea), Some("1"));
        assert_eq!(c.key(Action::ModeFreehand), Some("2"));
        assert_eq!(c.key(Action::ModeWindow), Some("3"));
        assert_eq!(c.key(Action::ModeScreen), Some("4"));
        assert_eq!(c.key(Action::ToggleShelf), Some("Ctrl+H"));
        assert!(c.keys_are_default());
    }

    #[test]
    fn accel_is_in_gtk_format() {
        let c = Config::default();
        assert_eq!(c.accel(Action::ToggleShelf).as_deref(), Some("<Primary>h"));
        assert_eq!(c.accel(Action::Capture).as_deref(), Some("<Alt>k"));
        assert_eq!(c.accel(Action::ModeArea).as_deref(), Some("1"));
    }

    #[test]
    fn removed_key_has_no_accel() {
        let mut c = Config::default();
        c.set_key(Action::ModeArea, None);
        assert_eq!(c.accel(Action::ModeArea), None);
    }

    #[test]
    fn round_trip_keeps_everything() {
        let mut c = Config {
            last_mode: Mode::Freehand,
            compact: false,
            theme: Theme::Dark,
            instant_capture: false,
            auto_copy: false,
            capture_method: CaptureMethod::Gnome,
            flash: true,
            sound: true,
            close_hint_shown: true,
            hotkey_declined: Some("Ctrl+Alt+K".into()),
            custom_save_dir: Some("/mnt/data/skjerm bilder=nye".into()),
            ..Config::default()
        };
        c.set_key(Action::ModeArea, None);
        c.set_key(Action::ModeScreen, Some("Ctrl+4".into()));
        assert_eq!(Config::parse(&c.serialize()), c);
        assert!(!c.keys_are_default());
    }

    #[test]
    fn file_from_v1_0_0_gets_the_new_defaults() {
        let old = "compact=false\ninstant=false\nhotkey_declined=Alt+K\nkey.global=Super+Shift+K\n";
        let c = Config::parse(old);
        assert!(c.compact && c.instant_capture);
        assert_eq!(c.hotkey_declined, None);
        assert_eq!(c.key(Action::Capture), Some("Alt+K"));

        let own_key = Config::parse("key.global=Ctrl+Alt+P\n");
        assert_eq!(own_key.key(Action::Capture), Some("Ctrl+Alt+P"));
    }

    #[test]
    fn current_file_is_not_migrated() {
        let mut c = Config {
            compact: false,
            instant_capture: false,
            hotkey_declined: Some("Alt+K".into()),
            ..Config::default()
        };
        c.set_key(Action::Capture, Some("Super+Shift+K".into()));
        assert_eq!(Config::parse(&c.serialize()), c);
    }

    #[test]
    fn save_dir_falls_back_to_the_default() {
        let c = Config::default();
        assert_eq!(c.save_dir(), default_save_dir());
        let relative = Config::parse("schema=2\nsave_dir=../et/sted\n");
        assert_eq!(relative.custom_save_dir, None);
        let own = Config::parse("schema=2\nsave_dir=/mnt/data/bilder\n");
        assert_eq!(own.save_dir(), PathBuf::from("/mnt/data/bilder"));
    }

    #[test]
    fn garbage_falls_back_to_defaults() {
        let text = "mode=tull\ntheme=\ncompact=kanskje\nsøppel\nkey.ukjent=X\nkey.area=Ctrl+\ncapture_method=tull\nflash=kanskje\n";
        assert_eq!(Config::parse(text), Config::default());
    }

    #[test]
    fn valid_lines_survive_among_garbage() {
        let c = Config::parse("\u{0}\u{1}\n=\n===\ntheme=dark\nnøkkel uten verdi\n");
        assert_eq!(c.theme, Theme::Dark);
    }

    #[test]
    fn reset_restores_default_keys() {
        let mut c = Config::default();
        c.set_key(Action::Capture, None);
        c.reset_keys();
        assert!(c.keys_are_default());
    }

    #[test]
    fn saves_and_loads_from_disk() {
        let dir = temp_dir("disk");
        let path = dir.join("under").join("config");
        let c = Config {
            last_mode: Mode::Window,
            ..Config::default()
        };
        c.save_to(&path).unwrap();
        assert_eq!(Config::load_from(&path), c);
        let left: Vec<_> = fs::read_dir(path.parent().unwrap()).unwrap().collect();
        assert_eq!(left.len(), 1, "den midlertidige filen skal være borte");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn missing_or_binary_file_gives_defaults() {
        let dir = temp_dir("binary");
        fs::create_dir_all(&dir).unwrap();
        assert_eq!(
            Config::load_from(&dir.join("finnes-ikke")),
            Config::default()
        );
        let path = dir.join("config");
        fs::write(&path, [0xff, 0xfe, 0x00, 0x80]).unwrap();
        assert_eq!(Config::load_from(&path), Config::default());
        assert_eq!(Config::load_from(&dir), Config::default());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn failed_save_is_an_error_and_keeps_the_old_file() {
        let dir = temp_dir("readonly");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config");
        fs::write(&path, "theme=dark\n").unwrap();
        // En mappe der filen skal ligge gjør at skrivingen må feile.
        let blocked = path.join("config");
        assert!(Config::default().save_to(&blocked).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "theme=dark\n");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn save_follows_symlink() {
        let dir = temp_dir("symlink");
        fs::create_dir_all(&dir).unwrap();
        let real = dir.join("ekte");
        let link = dir.join("config");
        fs::write(&real, "").unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();
        Config::default().save_to(&link).unwrap();
        assert!(fs::symlink_metadata(&link).unwrap().is_symlink());
        assert_eq!(Config::load_from(&real), Config::default());
        let _ = fs::remove_dir_all(dir);
    }
}
