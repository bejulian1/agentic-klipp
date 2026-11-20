//! Applikasjonen: single instance, kommandolinje og samspillet mellom delene.

use crate::capture::{self, geometry::Mode};
use crate::shelf::{Shelf, Shot};
use crate::storage::{self, ShotFile};
use crate::window::MainUi;
use crate::{cli, clipboard, trace};
use adw::prelude::*;
use gtk::{gdk, gio, glib};
use klipp_annotate::AnnotateEvent;
use klipp_system::{APP_ID, About, Action, Config, HotkeyStatus, SettingsChange, Tray, TrayEvent};
use std::cell::{Cell, OnceCell, RefCell};
use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// Minste tid mellom to bilder i papirkurven.
const TRASH_GUARD: Duration = Duration::from_millis(400);

thread_local! {
    static KLIPP: OnceCell<Rc<Klipp>> = const { OnceCell::new() };
}

pub struct Klipp {
    pub app: adw::Application,
    pub config: Rc<RefCell<Config>>,
    pub shelf: Rc<Shelf>,
    pub ui: Rc<MainUi>,
    pub capture: capture::Controller,
    pub tray: RefCell<Option<Tray>>,
    /// Aktiveringstoken fra den som startet oss, til neste vindu som vises.
    pub activation: RefCell<Option<String>>,
    started: Cell<bool>,
    first_frame: Cell<bool>,
    last_trash: Cell<Option<Instant>>,
    _hold: gio::ApplicationHoldGuard,
}

pub fn build_application() -> adw::Application {
    let app = adw::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .resource_base_path("/io/github/AgenticKlipp")
        .build();
    app.connect_startup(Klipp::init);
    app.connect_shutdown(|_| {
        KLIPP.with(|k| {
            if let Some(k) = k.get() {
                k.shelf.finish_saving();
            }
        });
    });
    app.connect_command_line(|_, line| {
        let args: Vec<String> = line
            .arguments()
            .iter()
            .skip(1)
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let k = Klipp::get();
        let token = line
            .getenv("XDG_ACTIVATION_TOKEN")
            .or_else(|| line.getenv("DESKTOP_STARTUP_ID"));
        k.activation.replace(token.map(|t| t.to_string()));
        let first = !k.started.replace(true);
        match cli::parse(args) {
            Ok(cli::Command::Capture(mode)) => capture::start(&k, mode),
            Ok(cli::Command::Hidden) => {
                if !first {
                    return glib::ExitCode::SUCCESS;
                }
            }
            Ok(cli::Command::Show) => k.show_main(),
            Ok(cli::Command::Quit) => k.app.quit(),
            Ok(_) => {}
            // Argumentene er allerede sjekket i prosessen som sendte dem hit.
            Err(_) => return glib::ExitCode::from(2),
        }
        glib::ExitCode::SUCCESS
    });
    app
}

impl Klipp {
    pub fn get() -> Rc<Klipp> {
        KLIPP.with(|k| k.get().cloned()).expect("Klipp er startet")
    }

    fn init(app: &adw::Application) {
        trace("startup");
        let config = Rc::new(RefCell::new(Config::load()));
        klipp_system::apply_theme(config.borrow().theme);

        if let Some(display) = gdk::Display::default() {
            let css = gtk::CssProvider::new();
            css.load_from_resource("/io/github/AgenticKlipp/style.css");
            gtk::style_context_add_provider_for_display(
                &display,
                &css,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }

        let shelf = Shelf::new();
        let ui = MainUi::build(app, &shelf, &config.borrow());
        let k = Rc::new(Klipp {
            app: app.clone(),
            config,
            shelf: shelf.clone(),
            ui,
            capture: capture::Controller::default(),
            tray: RefCell::new(None),
            activation: RefCell::new(None),
            started: Cell::new(false),
            first_frame: Cell::new(false),
            last_trash: Cell::new(None),
            _hold: app.hold(),
        });
        KLIPP.with(|c| {
            let _ = c.set(k.clone());
        });

        let refresh = k.clone();
        shelf.load(move || {
            refresh
                .ui
                .sync_shelf(&refresh.shelf, &refresh.config.borrow());
            trace("hylla lastet");
        });
        k.ui.sync_all(&k.shelf, &k.config.borrow());
        // Systemfeltet sier «Vis vindu» når vinduet er minimert.
        k.ui.window.connect_realize(|w| {
            if let Some(t) = w.surface().and_downcast::<gdk::Toplevel>() {
                t.connect_state_notify(|_| Klipp::get().sync_tray());
            }
        });
        k.shelf.connect_selection_changed(|| {
            let k = Klipp::get();
            k.ui.sync_selection(&k.shelf);
        });

        // Vinduer som er åpne ved siden av hovedvinduet gjør det inaktivt.
        let toplevels = gtk::Window::toplevels();
        let model = toplevels.clone();
        toplevels.connect_items_changed(move |_, _, _, _| {
            let blocked = (0..model.n_items())
                .filter_map(|i| model.item(i).and_downcast::<gtk::Window>())
                .any(|w| !w.has_css_class("klipp-main") && !w.has_css_class("klipp-overlay"));
            if let Some(k) = KLIPP.with(|k| k.get().cloned()) {
                k.ui.set_blocked(blocked);
            }
        });
        // Modellen må leve like lenge som appen.
        std::mem::forget(toplevels);

        // Tray og hurtigtast skal ikke forsinke første vindu.
        glib::idle_add_local_once(|| {
            let k = Klipp::get();
            let tray = Tray::start(|e| {
                let k = Klipp::get();
                match e {
                    TrayEvent::Capture => capture::start(&k, None),
                    TrayEvent::ToggleWindow => k.toggle_main(),
                    TrayEvent::Quit => k.app.quit(),
                }
            });
            k.tray.replace(tray);
            capture::shell::watch(|on| {
                let k = Klipp::get();
                if on {
                    k.give_key_to_shell();
                } else {
                    k.give_key_to_portal();
                }
            });
            trace("tray og hurtigtast startet");
        });
    }

    fn give_key_to_shell(&self) {
        trace("global hurtigtast via Shell-utvidelsen");
        let config = self.config.borrow();
        let accel = config.accel(Action::Capture).unwrap_or_default();
        capture::shell::bind_shortcut(accel);
        klipp_system::hotkey_handed_over(config.key(Action::Capture));
    }

    fn give_key_to_portal(&self) {
        klipp_system::start_hotkey(
            self.config.clone(),
            || capture::start(&Klipp::get(), Some(Mode::Area)),
            |status| match status {
                HotkeyStatus::Active => trace("global hurtigtast aktiv"),
                HotkeyStatus::Unavailable(why) => {
                    trace(&format!("global hurtigtast utilgjengelig: {why}"));
                }
            },
        );
    }

    pub fn save_config(&self) {
        if let Err(e) = self.config.borrow().save() {
            eprintln!("klipp: kunne ikke lagre innstillingene: {e}");
        }
    }

    pub fn show_main(&self) {
        if self.ui.window.is_visible() && !self.main_shown() {
            // Minimert eller dekket: vist på nytt havner det øverst. `present`
            // alene gjenoppretter ikke uten aktiveringstoken på Wayland, og GTK
            // minimerer igjen ved neste visning til `unminimize` er kalt.
            self.ui.window.unminimize();
            self.ui.window.set_visible(false);
            self.ui.minimized.set(false);
        }
        if let Some(token) = self.activation.borrow_mut().take() {
            self.ui.window.set_startup_id(&token);
        }
        self.ui.window.present();
        if !self.first_frame.replace(true) {
            let n = Cell::new(0);
            self.ui.window.add_tick_callback(move |_, _| {
                n.set(n.get() + 1);
                if n.get() < 2 {
                    return glib::ControlFlow::Continue;
                }
                trace("første bilde vist");
                glib::ControlFlow::Break
            });
        }
        self.sync_tray();
        let (shelf, ui) = (self.shelf.clone(), self.ui.clone());
        let cfg = self.config.clone();
        self.shelf
            .prune(move || ui.sync_shelf(&shelf, &cfg.borrow()));
    }

    pub fn hide_main(&self) {
        self.ui.minimized.set(false);
        // Et gammelt utvalg skal ikke vente når vinduet åpnes igjen.
        self.shelf.set_selecting(false);
        self.ui.window.set_visible(false);
        self.sync_tray();
        self.show_close_hint();
    }

    fn toggle_main(&self) {
        if self.main_shown() {
            self.hide_main();
        } else {
            self.show_main();
        }
    }

    /// Vinduet er på skjermen: synlig og verken minimert eller helt dekket.
    fn main_shown(&self) -> bool {
        let w = &self.ui.window;
        let away = w
            .surface()
            .and_downcast::<gdk::Toplevel>()
            .is_some_and(|t| {
                t.state()
                    .intersects(gdk::ToplevelState::MINIMIZED | gdk::ToplevelState::SUSPENDED)
            });
        w.is_visible() && !away && !self.ui.minimized.get()
    }

    pub fn sync_tray(&self) {
        klipp_system::tray_set_window_visible(self.main_shown());
    }

    /// Første gang vinduet lukkes sier vi fra om at Klipp kjører videre.
    fn show_close_hint(&self) {
        if std::mem::replace(&mut self.config.borrow_mut().close_hint_shown, true) {
            return;
        }
        self.save_config();
        let body = if klipp_system::tray_online() {
            "Åpne den igjen fra ikonet i systemfeltet."
        } else {
            "Kjør klipp for å åpne den igjen."
        };
        notify("Klipp kjører i bakgrunnen", body);
    }

    pub fn copy_shot(&self, shot: &Shot) {
        let m = shot.meta();
        if !self.shot_available(shot, &m.path) {
            return;
        }
        if clipboard::copy_file(m.path) {
            self.ui.toast("Kopiert!");
        } else {
            self.ui.toast("Ikke kopiert. Klipp mangler fokus.");
        }
    }

    pub fn copy_path(&self, shot: &Shot) {
        let m = shot.meta();
        if clipboard::copy_text(&m.path.to_string_lossy()) {
            self.ui.toast("Filsti kopiert");
        } else {
            self.ui.toast("Ikke kopiert. Klipp mangler fokus.");
        }
    }

    /// Alle valgte bilder på utklippstavla på én gang, ovenfra og ned.
    pub fn copy_selected(&self) {
        let mut paths = Vec::new();
        let mut missing = Vec::new();
        let mut unavailable = false;
        for shot in self.shelf.selected() {
            let path = shot.meta().path;
            match storage::shot_file(&path) {
                ShotFile::Available => paths.push(path),
                ShotFile::Deleted => missing.push(shot),
                ShotFile::Unavailable => unavailable = true,
            }
        }
        self.shelf.remove_many(&missing);
        if !missing.is_empty() {
            self.ui.sync_shelf(&self.shelf, &self.config.borrow());
        }
        if paths.is_empty() {
            if unavailable {
                self.ui.toast("Filene er utilgjengelige");
            } else if !missing.is_empty() {
                self.ui.toast("Filene finnes ikke lenger");
            }
            return;
        }
        if clipboard::copy_files(&paths) {
            self.ui
                .toast(&format!("{} kopiert", count_text(paths.len())));
        } else {
            self.ui.toast("Ikke kopiert. Klipp mangler fokus.");
        }
    }

    /// Fila går i papirkurven, så den kan hentes tilbake derfra.
    pub fn trash_shot(&self, shot: &Shot) {
        self.trash_shots(vec![shot.clone()]);
    }

    /// Alle valgte i papirkurven. Valgmodus slutter når alt gikk bra.
    pub fn trash_selected(&self) {
        self.trash_shots(self.shelf.selected());
    }

    fn trash_shots(&self, shots: Vec<Shot>) {
        // Neste bilde rykker opp under musa. Et dobbeltklikk skal ikke ta det også.
        let now = Instant::now();
        if shots.is_empty() || self.last_trash.get().is_some_and(|t| now - t < TRASH_GUARD) {
            return;
        }
        self.last_trash.set(Some(now));
        glib::spawn_future_local(async move {
            let k = Klipp::get();
            let mut tally = TrashTally::default();
            let mut removed = Vec::new();
            for shot in shots {
                let path = shot.meta().path;
                let file = gio::File::for_path(&path);
                match file.trash_future(glib::Priority::DEFAULT).await {
                    Ok(()) => tally.trashed += 1,
                    Err(e)
                        if e.matches(gio::IOErrorEnum::NotFound)
                            && matches!(storage::shot_file(&path), ShotFile::Deleted) =>
                    {
                        tally.gone += 1;
                    }
                    Err(e) => {
                        eprintln!("klipp: kunne ikke legge i papirkurven: {e}");
                        tally.failed += 1;
                        continue;
                    }
                }
                removed.push(shot);
            }
            k.shelf.remove_many(&removed);
            if tally.failed == 0 {
                k.shelf.set_selecting(false);
            }
            k.ui.sync_shelf(&k.shelf, &k.config.borrow());
            k.ui.toast(&tally.message());
        });
    }

    /// Tastene i valgmodus, og Ctrl+A som starter den. Sann når tasten er brukt.
    pub fn selection_key(&self, key: gdk::Key, state: gdk::ModifierType) -> bool {
        if self.config.borrow().compact || self.shelf.is_empty() {
            return false;
        }
        let mods = state
            & (gdk::ModifierType::CONTROL_MASK
                | gdk::ModifierType::SHIFT_MASK
                | gdk::ModifierType::ALT_MASK
                | gdk::ModifierType::SUPER_MASK);
        let ctrl = mods == gdk::ModifierType::CONTROL_MASK;
        let plain = mods.is_empty();
        let selecting = self.shelf.selecting();
        match key.to_lower() {
            gdk::Key::a if ctrl => self.shelf.select_all(),
            gdk::Key::Escape if plain && selecting => self.shelf.set_selecting(false),
            gdk::Key::c if ctrl && selecting => self.copy_selected(),
            gdk::Key::Delete | gdk::Key::KP_Delete if plain && selecting => self.trash_selected(),
            _ => return false,
        }
        true
    }

    pub fn annotate(&self, shot: &Shot) {
        let m = shot.meta();
        if !self.shot_available(shot, &m.path) {
            return;
        }
        let path = m.path.clone();
        klipp_annotate::open(&self.app, &m.path, move |e| {
            let k = Klipp::get();
            if let AnnotateEvent::Saved(saved) = e {
                k.shelf.replaced(&saved);
            }
            k.shelf.focus_path(&path);
        });
    }

    fn shot_available(&self, shot: &Shot, path: &Path) -> bool {
        match storage::shot_file(path) {
            ShotFile::Available => return true,
            ShotFile::Deleted => {
                self.shelf.remove(shot);
                self.ui.sync_shelf(&self.shelf, &self.config.borrow());
                self.ui.toast("Filen finnes ikke lenger");
            }
            ShotFile::Unavailable => self.ui.toast("Filen er utilgjengelig"),
        }
        false
    }

    pub fn open_settings(&self) {
        let about = About {
            version: crate::VERSION,
            build: crate::BUILD,
            silent_capture: capture::shell::ready(),
        };
        klipp_system::open_settings(&self.app, self.config.clone(), about, |change| {
            let k = Klipp::get();
            match change {
                SettingsChange::Theme => klipp_system::apply_theme(k.config.borrow().theme),
                SettingsChange::Shortcuts => {
                    k.ui.sync_all(&k.shelf, &k.config.borrow());
                    if capture::shell::ready() {
                        k.give_key_to_shell();
                    }
                }
                SettingsChange::Capture => k.ui.sync_save_dir(&k.config.borrow()),
            }
        });
    }

    pub fn set_mode(&self, mode: Mode) {
        if self.config.borrow().last_mode == mode {
            return;
        }
        self.config.borrow_mut().last_mode = mode;
        self.save_config();
        self.ui.sync_modes(&self.config.borrow());
    }

    pub fn set_compact(&self, on: bool) {
        if self.config.borrow().compact == on {
            return;
        }
        self.shelf.set_selecting(false);
        self.config.borrow_mut().compact = on;
        self.save_config();
        self.ui.apply_compact(on);
        self.ui.sync_shelf(&self.shelf, &self.config.borrow());
    }

    pub fn clear_shelf(&self) {
        if self.shelf.is_empty() {
            return;
        }
        self.shelf.set_selecting(false);
        let saved = self.shelf.take_all();
        self.ui.sync_shelf(&self.shelf, &self.config.borrow());
        let saved = RefCell::new(Some(saved));
        self.ui
            .toast_with_action("Tømt. Filene er beholdt.", "Angre", 6, move || {
                let k = Klipp::get();
                if let Some(s) = saved.borrow_mut().take() {
                    k.shelf.restore(s);
                    k.ui.sync_shelf(&k.shelf, &k.config.borrow());
                }
            });
    }
}

/// «1 bilde», «3 bilder».
pub fn count_text(n: usize) -> String {
    if n == 1 {
        "1 bilde".to_string()
    } else {
        format!("{n} bilder")
    }
}

/// Hvordan det gikk med papirkurven, til én ærlig toast.
#[derive(Default, Debug, PartialEq)]
struct TrashTally {
    trashed: usize,
    gone: usize,
    failed: usize,
}

impl TrashTally {
    fn message(&self) -> String {
        // Ett bilde: de korte tekstene fra søppelbøtta på ruta.
        match (self.trashed, self.gone, self.failed) {
            (1, 0, 0) => return "Lagt i papirkurven".into(),
            (0, 1, 0) => return "Filen finnes ikke lenger".into(),
            (0, 0, 1) => return "Kunne ikke legge i papirkurven".into(),
            _ => {}
        }
        let mut parts = Vec::new();
        if self.trashed > 0 {
            parts.push(format!("{} lagt i papirkurven", count_text(self.trashed)));
        }
        if self.gone > 0 {
            parts.push(format!("{} fantes ikke lenger", count_text(self.gone)));
        }
        if self.failed > 0 {
            parts.push(format!("{} kunne ikke flyttes", count_text(self.failed)));
        }
        parts.join(". ")
    }
}

/// Vanlig skrivebordsvarsel. `org.gtk.Notifications` krever en installert
/// .desktop-fil, så vi går rett på freedesktop-grensesnittet.
fn notify(summary: &str, body: &str) {
    let actions: Vec<String> = Vec::new();
    let hints = std::collections::HashMap::<String, glib::Variant>::new();
    let args = (
        "Klipp", 0u32, APP_ID, summary, body, actions, hints, 5000i32,
    )
        .to_variant();
    gio::bus_get(gio::BusType::Session, gio::Cancellable::NONE, move |bus| {
        if let Ok(bus) = bus {
            bus.call(
                Some("org.freedesktop.Notifications"),
                "/org/freedesktop/Notifications",
                "org.freedesktop.Notifications",
                "Notify",
                Some(&args),
                None,
                gio::DBusCallFlags::NONE,
                2000,
                gio::Cancellable::NONE,
                |_| {},
            );
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tally(trashed: usize, gone: usize, failed: usize) -> String {
        TrashTally {
            trashed,
            gone,
            failed,
        }
        .message()
    }

    #[test]
    fn count_text_singular_and_plural() {
        assert_eq!(count_text(1), "1 bilde");
        assert_eq!(count_text(0), "0 bilder");
        assert_eq!(count_text(12), "12 bilder");
    }

    #[test]
    fn single_trash_keeps_short_messages() {
        assert_eq!(tally(1, 0, 0), "Lagt i papirkurven");
        assert_eq!(tally(0, 1, 0), "Filen finnes ikke lenger");
        assert_eq!(tally(0, 0, 1), "Kunne ikke legge i papirkurven");
    }

    #[test]
    fn bulk_trash_reports_every_outcome() {
        assert_eq!(tally(3, 0, 0), "3 bilder lagt i papirkurven");
        assert_eq!(
            tally(2, 1, 0),
            "2 bilder lagt i papirkurven. 1 bilde fantes ikke lenger"
        );
        assert_eq!(
            tally(1, 0, 2),
            "1 bilde lagt i papirkurven. 2 bilder kunne ikke flyttes"
        );
        assert_eq!(tally(0, 0, 3), "3 bilder kunne ikke flyttes");
    }
}
