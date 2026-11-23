//! Flytting fra 1.x, da appen het Agentic Klipp. Første gang 2.0 starter, flyttes
//! innstillingene, hylla og thumbnails til de nye mappene, og det brukeren satte
//! opp for den gamle appen peker videre på den nye. Bildene flyttes aldri.

use crate::config::{self, Config};
use crate::{APP_ID, DIR_NAME, extension};
use gtk::gio::prelude::*;
use gtk::{gio, glib};
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Navnene fra 1.x.
const OLD_DIR_NAME: &str = "klipp";
const OLD_PICTURES_NAME: &str = "Klipp";
const OLD_APP_ID: &str = "io.github.AgenticKlipp";
const OLD_OBJECT_PATH: &str = "/io/github/AgenticKlipp";
const OLD_BINARY: &str = "klipp";
const OLD_EXTENSION: &str = "agentic-klipp@klipp.local";
const OLD_NAME: &str = "Agentic Klipp";
const NAME: &str = "Agentic Screenshot";

const MEDIA_KEYS: &str = "org.gnome.settings-daemon.plugins.media-keys";
const CUSTOM_SHORTCUT: &str = "org.gnome.settings-daemon.plugins.media-keys.custom-keybinding";

/// Hvor filene lå i 1.x, og hvor de skal ligge nå.
struct Layout {
    /// Config, state og cache, i den rekkefølgen, fra gammelt til nytt navn.
    folders: [(PathBuf, PathBuf); 3],
    config_file: PathBuf,
    old_pictures: PathBuf,
    autostart: PathBuf,
}

impl Layout {
    fn current() -> Self {
        let config = glib::user_config_dir();
        let folders = [
            config.clone(),
            crate::user_state_dir(),
            glib::user_cache_dir(),
        ]
        .map(|base| (base.join(OLD_DIR_NAME), base.join(DIR_NAME)));
        Self {
            folders,
            config_file: Config::path(),
            old_pictures: config::pictures_dir().join(OLD_PICTURES_NAME),
            autostart: config.join("autostart"),
        }
    }

    /// En gammel mappe finnes, og den nye er ikke laget ennå.
    fn pending(&self) -> bool {
        self.folders.iter().any(|(old, new)| waiting(old, new))
    }

    /// Flytter mappene og det som hører til. Gir `true` når noe ble flyttet.
    fn move_files(&self, exe: Option<&Path>) -> bool {
        let mut moved = false;
        for (old, new) in &self.folders {
            if !waiting(old, new) {
                continue;
            }
            match move_dir(old, new) {
                Ok(()) => {
                    moved = true;
                    eprintln!(
                        "agentic-screenshot: flyttet {} til {}",
                        old.display(),
                        new.display()
                    );
                }
                Err(e) => eprintln!(
                    "agentic-screenshot: kunne ikke flytte {}: {e}",
                    old.display()
                ),
            }
        }
        if !moved {
            return false;
        }
        // Feilet flyttingen av config, må den nye mappen ikke lages nå. Da prøver
        // neste oppstart på nytt i stedet for å starte med standardvalg.
        let (old_config, new_config) = &self.folders[0];
        if !waiting(old_config, new_config) {
            match keep_old_pictures(&self.config_file, &self.old_pictures) {
                Ok(true) => eprintln!(
                    "agentic-screenshot: lagrer fortsatt i {}",
                    self.old_pictures.display()
                ),
                Ok(false) => {}
                Err(e) => eprintln!("agentic-screenshot: kunne ikke beholde bildemappen: {e}"),
            }
        }
        if let Some(exe) = exe
            && let Err(e) = move_autostart(&self.autostart, exe)
        {
            eprintln!("agentic-screenshot: kunne ikke flytte autostart: {e}");
        }
        true
    }
}

/// Kalles før innstillingene leses, og bare når appen faktisk skal starte.
/// Gjør ingenting når det ikke finnes noe fra 1.x å flytte.
pub fn migrate_legacy() {
    let layout = Layout::current();
    if !layout.pending() {
        return;
    }
    // Kjører den gamle appen fortsatt, ville den skrevet til de gamle mappene etterpå.
    quit_old_app();
    let exe = std::env::current_exe().ok();
    if !layout.move_files(exe.as_deref()) {
        return;
    }
    if extension::replace_extension(OLD_EXTENSION) {
        eprintln!(
            "agentic-screenshot: den nye Shell-utvidelsen er slått på i stedet for den gamle"
        );
    }
    if let Some(exe) = &exe {
        let changed = retarget_shortcuts(exe);
        if changed > 0 {
            eprintln!(
                "agentic-screenshot: {changed} egne hurtigtaster i GNOME peker nå på den nye appen"
            );
        }
    }
}

fn exists(path: &Path) -> bool {
    path.symlink_metadata().is_ok()
}

fn waiting(old: &Path, new: &Path) -> bool {
    old.is_dir() && !exists(new)
}

/// Gir mappen nytt navn, eller kopierer den når det ikke går. En kopi blir først
/// synlig når den er komplett, og originalen blir liggende.
fn move_dir(from: &Path, to: &Path) -> io::Result<()> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
    }
    if fs::rename(from, to).is_ok() {
        return Ok(());
    }
    let mut partial = to.as_os_str().to_owned();
    partial.push(format!(".flytter{}", std::process::id()));
    let partial = PathBuf::from(partial);
    let _ = fs::remove_dir_all(&partial);
    copy_dir(from, &partial)
        .and_then(|()| fs::rename(&partial, to))
        .inspect_err(|_| {
            let _ = fs::remove_dir_all(&partial);
        })
}

fn copy_dir(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let (source, target) = (entry.path(), to.join(entry.file_name()));
        let kind = entry.file_type()?;
        if kind.is_dir() {
            copy_dir(&source, &target)?;
        } else if kind.is_symlink() {
            std::os::unix::fs::symlink(fs::read_link(&source)?, &target)?;
        } else {
            fs::copy(&source, &target)?;
        }
    }
    Ok(())
}

/// Brukte 1.x standardmappen `Bilder/Klipp`, skrives den inn som egen mappe. Da
/// havner nye bilder samme sted som før, og hylla finner de gamle.
fn keep_old_pictures(config_file: &Path, old_pictures: &Path) -> io::Result<bool> {
    let Some(dir) = old_pictures.to_str().filter(|d| !d.contains('\n')) else {
        return Ok(false);
    };
    if !old_pictures.is_dir() {
        return Ok(false);
    }
    let mut text = match fs::read(config_file) {
        Ok(bytes) => {
            String::from_utf8(bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => format!("schema={}\n", config::SCHEMA),
        Err(e) => return Err(e),
    };
    if Config::parse(&text).custom_save_dir.is_some() {
        return Ok(false);
    }
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(&format!("save_dir={dir}\n"));
    if let Some(parent) = config_file.parent() {
        fs::create_dir_all(parent)?;
    }
    config::write_atomic(config_file, text.as_bytes())?;
    Ok(true)
}

/// Autostart fra README-en i 1.x får nytt navn og peker på den nye binæren.
fn move_autostart(dir: &Path, exe: &Path) -> io::Result<bool> {
    let old = dir.join(format!("{OLD_APP_ID}.desktop"));
    let new = dir.join(format!("{APP_ID}.desktop"));
    if exists(&new) || !old.is_file() {
        return Ok(false);
    }
    let text = fs::read_to_string(&old)?;
    config::write_atomic(&new, rewrite_autostart(&text, exe).as_bytes())?;
    fs::remove_file(&old)?;
    Ok(true)
}

fn rewrite_autostart(text: &str, exe: &Path) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let exec = line
            .strip_prefix("Exec=")
            .and_then(|command| retarget(command, &desktop_quote(exe)));
        match exec {
            Some(command) => out.push_str(&format!("Exec={command}")),
            None => out.push_str(&line.replace(OLD_APP_ID, APP_ID).replace(OLD_NAME, NAME)),
        }
        out.push('\n');
    }
    out
}

/// Programmet først i en kommando, uten anførselstegn, og resten av linja.
fn split_program(command: &str) -> Option<(String, &str)> {
    let command = command.trim_start();
    let mut program = String::new();
    let mut chars = command.char_indices();
    let end = match command.chars().next()? {
        quote @ ('"' | '\'') => {
            chars.next();
            loop {
                match chars.next()? {
                    (i, c) if c == quote => break i + 1,
                    (_, '\\') if quote == '"' => program.push(chars.next()?.1),
                    (_, c) => program.push(c),
                }
            }
        }
        _ => {
            let end = command.find(char::is_whitespace).unwrap_or(command.len());
            program.push_str(&command[..end]);
            end
        }
    };
    Some((program, &command[end..]))
}

/// Bytter programmet når kommandoen starter den gamle binæren `klipp`.
fn retarget(command: &str, program: &str) -> Option<String> {
    let (old, rest) = split_program(command)?;
    (Path::new(&old).file_name()? == OLD_BINARY).then(|| format!("{program}{rest}"))
}

/// Exec= i .desktop-filer: anførselstegn, og `\ " ` $` beskyttet for begge
/// lagene i spesifikasjonen, slik install.sh gjør.
fn desktop_quote(path: &Path) -> String {
    let mut s = String::from("\"");
    for c in path.to_string_lossy().chars() {
        match c {
            '\\' => s.push_str("\\\\\\\\"),
            '"' | '`' | '$' => {
                s.push_str("\\\\");
                s.push(c);
            }
            '%' => s.push_str("%%"),
            c => s.push(c),
        }
    }
    s.push('"');
    s
}

/// GNOME tolker egne hurtigtaster som skallkommandoer.
fn shell_quote(path: &Path) -> String {
    let path = path.to_string_lossy();
    if !path.is_empty()
        && path
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._-+".contains(c))
    {
        path.into_owned()
    } else {
        format!("'{}'", path.replace('\'', "'\\''"))
    }
}

/// Egne hurtigtaster i GNOME som starter `klipp`, peker på den nye binæren.
fn retarget_shortcuts(exe: &Path) -> usize {
    let has_schemas = gio::SettingsSchemaSource::default().is_some_and(|source| {
        source
            .lookup(MEDIA_KEYS, true)
            .is_some_and(|s| s.has_key("custom-keybindings"))
            && source
                .lookup(CUSTOM_SHORTCUT, true)
                .is_some_and(|s| s.has_key("command"))
    });
    if !has_schemas {
        return 0;
    }
    let program = shell_quote(exe);
    let mut changed = 0;
    for path in gio::Settings::new(MEDIA_KEYS)
        .strv("custom-keybindings")
        .iter()
    {
        let path = path.as_str();
        if !(path.starts_with('/') && path.ends_with('/') && !path.contains("//")) {
            continue;
        }
        let shortcut = gio::Settings::with_path(CUSTOM_SHORTCUT, path);
        if let Some(command) = retarget(&shortcut.string("command"), &program)
            && shortcut.set_string("command", &command).is_ok()
        {
            changed += 1;
        }
    }
    if changed > 0 {
        gio::Settings::sync();
    }
    changed
}

/// Ber en Agentic Klipp som fortsatt kjører om å avslutte, som `klipp --quit`.
fn quit_old_app() {
    let Ok(bus) = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE) else {
        return;
    };
    let running = || {
        bus.call_sync(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "NameHasOwner",
            Some(&(OLD_APP_ID,).to_variant()),
            glib::VariantTy::new("(b)").ok(),
            gio::DBusCallFlags::NONE,
            1000,
            gio::Cancellable::NONE,
        )
        .ok()
        .and_then(|reply| reply.get::<(bool,)>())
        .is_some_and(|(owned,)| owned)
    };
    if !running() {
        return;
    }
    let Ok(path) = glib::variant::ObjectPath::try_from("/org/gtk/Application/CommandLine") else {
        return;
    };
    let arguments: Vec<Vec<u8>> = [OLD_BINARY, "--quit"]
        .iter()
        .map(|arg| arg.bytes().chain([0]).collect())
        .collect();
    let platform_data = HashMap::<String, glib::Variant>::new();
    if let Err(e) = bus.call_sync(
        Some(OLD_APP_ID),
        OLD_OBJECT_PATH,
        "org.gtk.Application",
        "CommandLine",
        Some(&(path, arguments, platform_data).to_variant()),
        None,
        gio::DBusCallFlags::NONE,
        5000,
        gio::Cancellable::NONE,
    ) {
        eprintln!("agentic-screenshot: kunne ikke avslutte Agentic Klipp: {e}");
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    while running() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXE: &str = "/home/a/.local/bin/agentic-screenshot";

    struct Fixture {
        root: PathBuf,
        layout: Layout,
    }

    impl Fixture {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "agentic-screenshot-legacy-{name}-{}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&root);
            let base = |dir: &str| root.join(dir);
            let folders = [base("config"), base("state"), base("cache")]
                .map(|b| (b.join(OLD_DIR_NAME), b.join(DIR_NAME)));
            let layout = Layout {
                config_file: folders[0].1.join("config"),
                folders,
                old_pictures: root.join("Bilder").join(OLD_PICTURES_NAME),
                autostart: base("config").join("autostart"),
            };
            Self { root, layout }
        }

        fn old(&self, i: usize) -> &Path {
            &self.layout.folders[i].0
        }

        fn new_dir(&self, i: usize) -> &Path {
            &self.layout.folders[i].1
        }

        /// Slik 1.3.0 etterlot filene: config, Shell-tasten, hylla og thumbnails.
        fn seed_1_3(&self, config: &str) {
            for (old, _) in &self.layout.folders {
                fs::create_dir_all(old).unwrap();
            }
            fs::write(self.old(0).join("config"), config).unwrap();
            fs::write(self.old(0).join("shell-shortcut"), "<Control><Alt>p").unwrap();
            let shot = self
                .layout
                .old_pictures
                .join("klipp-2026-11-20_10-00-00.png");
            fs::create_dir_all(&self.layout.old_pictures).unwrap();
            fs::write(&shot, b"png").unwrap();
            fs::write(
                self.old(1).join("shelf.tsv"),
                format!("# klipp 1\n1\t2\t3\t0\t{}\n", shot.display()),
            )
            .unwrap();
            fs::create_dir_all(self.old(2).join("thumbs")).unwrap();
            fs::write(self.old(2).join("thumbs").join("ab.png"), b"thumb").unwrap();
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn moves_everything_and_keeps_saving_in_the_old_folder() {
        let f = Fixture::new("alt");
        f.seed_1_3("schema=2\ntheme=dark\nkey.global=Ctrl+Alt+P\n");
        assert!(f.layout.pending());
        assert!(f.layout.move_files(None));

        for i in 0..3 {
            assert!(!f.old(i).exists(), "den gamle mappen skal være borte");
        }
        let config = Config::load_from(&f.layout.config_file);
        assert_eq!(
            config.custom_save_dir.as_deref(),
            Some(&*f.layout.old_pictures)
        );
        assert_eq!(config.theme, crate::Theme::Dark);
        assert_eq!(config.key(crate::Action::Capture), Some("Ctrl+Alt+P"));
        assert_eq!(
            fs::read_to_string(f.new_dir(0).join("shell-shortcut")).unwrap(),
            "<Control><Alt>p"
        );
        let shelf = fs::read_to_string(f.new_dir(1).join("shelf.tsv")).unwrap();
        let shot = f.layout.old_pictures.join("klipp-2026-11-20_10-00-00.png");
        assert!(shelf.ends_with(&format!("\t{}\n", shot.display())));
        assert!(shot.is_file(), "bildene skal aldri flyttes");
        assert!(f.new_dir(2).join("thumbs").join("ab.png").is_file());
    }

    #[test]
    fn second_start_changes_nothing() {
        let f = Fixture::new("igjen");
        f.seed_1_3("schema=2\n");
        assert!(f.layout.move_files(None));
        let config = fs::read_to_string(&f.layout.config_file).unwrap();
        assert!(!f.layout.pending());
        assert!(!f.layout.move_files(None));
        assert_eq!(fs::read_to_string(&f.layout.config_file).unwrap(), config);
        assert_eq!(config.matches("save_dir=").count(), 1);
    }

    #[test]
    fn own_save_folder_is_kept() {
        let f = Fixture::new("egen");
        f.seed_1_3("schema=2\nsave_dir=/mnt/data/bilder\n");
        assert!(f.layout.move_files(None));
        let config = Config::load_from(&f.layout.config_file);
        assert_eq!(
            config.custom_save_dir,
            Some(PathBuf::from("/mnt/data/bilder"))
        );
    }

    #[test]
    fn new_install_without_old_pictures_uses_the_new_default() {
        let f = Fixture::new("uten-bilder");
        f.seed_1_3("theme=light");
        fs::remove_dir_all(&f.layout.old_pictures).unwrap();
        assert!(f.layout.move_files(None));
        let config = fs::read_to_string(&f.layout.config_file).unwrap();
        assert_eq!(config, "theme=light");
    }

    #[test]
    fn file_from_v1_0_0_is_still_migrated_after_the_move() {
        let f = Fixture::new("v1");
        f.seed_1_3("compact=false\nkey.global=Super+Shift+K");
        assert!(f.layout.move_files(None));
        let config = Config::load_from(&f.layout.config_file);
        assert!(config.compact);
        assert_eq!(config.key(crate::Action::Capture), Some("Alt+K"));
        assert_eq!(
            config.custom_save_dir.as_deref(),
            Some(&*f.layout.old_pictures)
        );
    }

    #[test]
    fn missing_config_file_gets_one_with_the_old_folder() {
        let f = Fixture::new("uten-config");
        f.seed_1_3("");
        fs::remove_file(f.old(0).join("config")).unwrap();
        assert!(f.layout.move_files(None));
        let text = fs::read_to_string(&f.layout.config_file).unwrap();
        assert_eq!(
            text,
            format!("schema=2\nsave_dir={}\n", f.layout.old_pictures.display())
        );
    }

    #[test]
    fn existing_new_folders_are_never_overwritten() {
        let f = Fixture::new("finnes");
        f.seed_1_3("theme=dark\n");
        fs::create_dir_all(f.new_dir(1)).unwrap();
        fs::write(f.new_dir(1).join("shelf.tsv"), "# agentic-screenshot 1\n").unwrap();
        assert!(f.layout.move_files(None));
        assert!(f.old(1).join("shelf.tsv").is_file());
        assert_eq!(
            fs::read_to_string(f.new_dir(1).join("shelf.tsv")).unwrap(),
            "# agentic-screenshot 1\n"
        );
        assert!(!f.old(0).exists() && !f.old(2).exists());
    }

    #[test]
    fn nothing_to_do_without_old_folders() {
        let f = Fixture::new("tom");
        assert!(!f.layout.pending());
        assert!(!f.layout.move_files(Some(Path::new(EXE))));
        assert!(!f.root.exists());
    }

    #[test]
    fn copy_keeps_files_folders_and_links() {
        let f = Fixture::new("kopi");
        let from = f.root.join("fra");
        fs::create_dir_all(from.join("under")).unwrap();
        fs::write(from.join("under").join("fil"), "innhold").unwrap();
        std::os::unix::fs::symlink("under/fil", from.join("lenke")).unwrap();
        let to = f.root.join("til");
        copy_dir(&from, &to).unwrap();
        assert_eq!(
            fs::read_to_string(to.join("under").join("fil")).unwrap(),
            "innhold"
        );
        assert_eq!(
            fs::read_link(to.join("lenke")).unwrap(),
            Path::new("under/fil")
        );
        assert!(from.join("under").join("fil").is_file());
    }

    #[test]
    fn autostart_from_the_readme_follows_the_new_binary() {
        let f = Fixture::new("autostart");
        f.seed_1_3("");
        fs::create_dir_all(&f.layout.autostart).unwrap();
        let old = f.layout.autostart.join(format!("{OLD_APP_ID}.desktop"));
        fs::write(
            &old,
            "[Desktop Entry]\nType=Application\nName=Agentic Klipp\n\
             Exec=\"/home/a/.local/bin/klipp\" --hidden\nIcon=io.github.AgenticKlipp\n\
             X-GNOME-Autostart-enabled=false\n",
        )
        .unwrap();
        assert!(f.layout.move_files(Some(Path::new(EXE))));
        assert!(!old.exists());
        let new = fs::read_to_string(f.layout.autostart.join(format!("{APP_ID}.desktop"))).unwrap();
        assert_eq!(
            new,
            format!(
                "[Desktop Entry]\nType=Application\nName=Agentic Screenshot\n\
                 Exec=\"{EXE}\" --hidden\nIcon={APP_ID}\nX-GNOME-Autostart-enabled=false\n"
            )
        );
    }

    #[test]
    fn existing_new_autostart_is_left_alone() {
        let f = Fixture::new("autostart-finnes");
        fs::create_dir_all(&f.layout.autostart).unwrap();
        let old = f.layout.autostart.join(format!("{OLD_APP_ID}.desktop"));
        let new = f.layout.autostart.join(format!("{APP_ID}.desktop"));
        fs::write(&old, "Exec=klipp --hidden\n").unwrap();
        fs::write(&new, "Exec=agentic-screenshot --hidden\n").unwrap();
        assert!(!move_autostart(&f.layout.autostart, Path::new(EXE)).unwrap());
        assert!(old.exists());
        assert_eq!(
            fs::read_to_string(&new).unwrap(),
            "Exec=agentic-screenshot --hidden\n"
        );
    }

    #[test]
    fn only_commands_that_start_klipp_are_retargeted() {
        let exe = shell_quote(Path::new(EXE));
        let to = |command: &str| retarget(command, &exe);
        assert_eq!(to("klipp --capture"), Some(format!("{EXE} --capture")));
        assert_eq!(
            to("/home/a/.local/bin/klipp --capture --mode omrade"),
            Some(format!("{EXE} --capture --mode omrade"))
        );
        assert_eq!(
            to("'/home/a b/.local/bin/klipp' --capture"),
            Some(format!("{EXE} --capture"))
        );
        assert_eq!(to("\"/usr/bin/klipp\""), Some(EXE.to_string()));
        assert_eq!(to("gnome-screenshot -a"), None);
        assert_eq!(to("klippy --capture"), None);
        assert_eq!(to("/opt/klipp/run --capture"), None);
        assert_eq!(to(""), None);
        assert_eq!(to("'uavsluttet"), None);
    }

    #[test]
    fn new_program_is_quoted_for_shell_and_desktop_files() {
        assert_eq!(shell_quote(Path::new(EXE)), EXE);
        assert_eq!(
            shell_quote(Path::new("/home/a b/it's/agentic-screenshot")),
            "'/home/a b/it'\\''s/agentic-screenshot'"
        );
        assert_eq!(
            desktop_quote(Path::new("/home/a \"b\" $c 100%/x")),
            "\"/home/a \\\\\"b\\\\\" \\\\$c 100%%/x\""
        );
    }
}
