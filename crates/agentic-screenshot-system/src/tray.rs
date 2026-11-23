//! Ikon i systemfeltet: StatusNotifierItem via ksni.
//!
//! GNOME har ingen StatusNotifierWatcher uten utvidelsen `ubuntu-appindicators`,
//! og den kan slås på og av mens Agentic Screenshot kjører. Derfor følger vi navnet på bussen
//! og starter ksni først når en watcher finnes. Uten watcher koster tray
//! ingenting. Etter første start tar ksni selv hånd om at watcheren forsvinner
//! og kommer tilbake.

use crate::i18n::tr;
use ksni::{MenuItem, OfflineReason, TrayMethods, menu::StandardItem};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use gtk::gio;
use gtk::glib;

const DBUS: &str = "org.freedesktop.DBus";
const WATCHER: &str = "org.kde.StatusNotifierWatcher";
const ICON: &str = "io.github.AgenticScreenshot-symbolic";

/// Hendelser fra tray. Kommer på GTK-hovedtråden.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayEvent {
    Capture,
    ToggleWindow,
    Quit,
}

/// Bevis på at tray kjørte ved oppstart.
pub struct Tray {
    _private: (),
}

struct Shared {
    on_event: Box<dyn Fn(TrayEvent)>,
    window_visible: Cell<bool>,
    online: Cell<bool>,
    spawning: Cell<bool>,
    handle: RefCell<Option<ksni::Handle<ShotTray>>>,
}

thread_local! {
    static SHARED: RefCell<Option<Rc<Shared>>> = const { RefCell::new(None) };
}

fn shared() -> Option<Rc<Shared>> {
    SHARED.with_borrow(Clone::clone)
}

/// ksni kjører på en egen tråd. `timeout_add_once` legger hendelsen på
/// hovedkonteksten, der GTK-tråden tar den. `MainContext::invoke` holder ikke:
/// den kan kjøre i kalltråden hvis konteksten er ledig akkurat da.
fn send(event: TrayEvent) {
    glib::timeout_add_once(Duration::ZERO, move || {
        if let Some(shared) = shared() {
            (shared.on_event)(event);
        }
    });
}

struct ShotTray {
    window_visible: bool,
}

impl ksni::Tray for ShotTray {
    // Prototypen åpner menyen på vanlig klikk.
    const MENU_ON_ACTIVATE: bool = true;

    fn id(&self) -> String {
        crate::APP_ID.to_string()
    }

    fn title(&self) -> String {
        "Agentic Screenshot".to_string()
    }

    fn icon_name(&self) -> String {
        ICON.to_string()
    }

    fn category(&self) -> ksni::Category {
        ksni::Category::ApplicationStatus
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let toggle = if self.window_visible {
            tr("Skjul Agentic Screenshot")
        } else {
            tr("Vis Agentic Screenshot")
        };
        vec![
            item(tr("Ta skjermbilde"), TrayEvent::Capture),
            item(toggle, TrayEvent::ToggleWindow),
            MenuItem::Separator,
            item(tr("Avslutt"), TrayEvent::Quit),
        ]
    }

    /// Hold tjenesten i live: utvidelsen kan bli slått av og på igjen.
    fn watcher_offline(&self, _reason: OfflineReason) -> bool {
        true
    }
}

fn item(label: &str, event: TrayEvent) -> MenuItem<ShotTray> {
    StandardItem {
        label: label.to_string(),
        activate: Box::new(move |_| send(event)),
        ..Default::default()
    }
    .into()
}

/// Startet ksni første gang en watcher dukker opp.
fn appeared(shared: &Rc<Shared>) {
    shared.online.set(true);
    if shared.handle.borrow().is_some() || shared.spawning.replace(true) {
        return;
    }
    let shared = shared.clone();
    glib::spawn_future_local(async move {
        let tray = ShotTray {
            window_visible: shared.window_visible.get(),
        };
        match tray.assume_sni_available(true).spawn().await {
            Ok(handle) => {
                // Vinduet kan ha endret seg mens tjenesten startet.
                let visible = shared.window_visible.get();
                handle.update(|t| t.window_visible = visible).await;
                shared.handle.replace(Some(handle));
            }
            Err(e) => eprintln!("agentic-screenshot: kunne ikke starte tray: {e}"),
        }
        shared.spawning.set(false);
    });
}

fn watcher_exists(bus: &gio::DBusConnection) -> bool {
    bus.call_sync(
        Some(DBUS),
        "/org/freedesktop/DBus",
        DBUS,
        "NameHasOwner",
        Some(&(WATCHER,).into()),
        Some(glib::VariantTy::new("(b)").expect("gyldig type")),
        gio::DBusCallFlags::NONE,
        1000,
        gio::Cancellable::NONE,
    )
    .ok()
    .and_then(|reply| reply.get::<(bool,)>())
    .is_some_and(|(owned,)| owned)
}

/// Følger watcheren på bussen. `gio::bus_watch_name` passer ikke her: den kaller
/// `vanished` med en NULL-tilkobling når bussen forsvinner, og det gir panikk i
/// Rust-bindingen.
fn watch(bus: &gio::DBusConnection, shared: Rc<Shared>) {
    let subscription = bus.subscribe_to_signal(
        Some(DBUS),
        Some(DBUS),
        Some("NameOwnerChanged"),
        Some("/org/freedesktop/DBus"),
        Some(WATCHER),
        gio::DBusSignalFlags::NONE,
        move |signal| {
            let new_owner = signal
                .parameters
                .get::<(String, String, String)>()
                .map(|(_, _, new_owner)| new_owner);
            match new_owner {
                Some(owner) if !owner.is_empty() => appeared(&shared),
                _ => shared.online.set(false),
            }
        },
    );
    // Overvåkingen skal vare like lenge som prosessen.
    std::mem::forget(subscription);
}

impl Tray {
    /// Starter tray. Gir `None` hvis ingen StatusNotifierWatcher finnes nå, og
    /// appen skal tåle det. Agentic Screenshot følger likevel med: slår brukeren på utvidelsen
    /// senere, dukker ikonet opp av seg selv og `on_event` kalles som vanlig.
    /// Bruk [`tray_set_window_visible`], som også virker når `start` ga `None`.
    pub fn start(on_event: impl Fn(TrayEvent) + 'static) -> Option<Self> {
        if shared().is_some() {
            return None;
        }
        let bus = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE).ok()?;
        let state = Rc::new(Shared {
            on_event: Box::new(on_event),
            window_visible: Cell::new(true),
            online: Cell::new(false),
            spawning: Cell::new(false),
            handle: RefCell::new(None),
        });
        SHARED.set(Some(state.clone()));

        // Abonner før sjekken, så en watcher som dukker opp imellom ikke går tapt.
        watch(&bus, state.clone());
        let exists = watcher_exists(&bus);
        if exists {
            appeared(&state);
        }
        exists.then_some(Self { _private: () })
    }

    /// Oppdaterer menyteksten Vis Agentic Screenshot / Skjul Agentic Screenshot.
    pub fn set_window_visible(&self, visible: bool) {
        tray_set_window_visible(visible);
    }
}

/// Oppdaterer menyteksten Vis Agentic Screenshot / Skjul Agentic Screenshot, også hvis tray kom senere enn
/// oppstart og `Tray::start` ga `None`.
pub fn tray_set_window_visible(visible: bool) {
    let Some(shared) = shared() else { return };
    if shared.window_visible.replace(visible) == visible {
        return;
    }
    if shared.handle.borrow().is_none() {
        return;
    }
    glib::spawn_future_local(async move {
        let handle = shared.handle.borrow().clone();
        if let Some(handle) = handle {
            handle.update(|t| t.window_visible = visible).await;
        }
    });
}

/// Om en StatusNotifierWatcher finnes akkurat nå. Bruk den til å avgjøre om
/// lukking av vinduet kan skjule appen, siden ikonet er veien tilbake.
pub fn tray_online() -> bool {
    shared().is_some_and(|s| s.online.get())
}
