//! Stille opptak og global tast gjennom GNOME Shell-utvidelsen vår. Uten den brukes
//! portalen, som alltid gir hvit blits og lukkerlyd.

use crate::trace;
use gtk::glib::variant::ToVariant;
use gtk::{gio, glib};
use klipp_system::{CaptureMethod, Config};
use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

const BUS_NAME: &str = "io.github.AgenticKlipp.Shell";
const OBJECT_PATH: &str = "/io/github/AgenticKlipp/Shell";
const CALL_TIMEOUT_MS: i32 = 3000;

thread_local! {
    static READY: Cell<bool> = const { Cell::new(false) };
}

pub fn ready() -> bool {
    READY.get()
}

/// Følg med på utvidelsen. `on_change` kalles med en gang og når den slås av eller på.
pub fn watch(on_change: impl Fn(bool) + 'static) {
    let appeared = Rc::new(on_change);
    let vanished = appeared.clone();
    // Vaktposten lever like lenge som appen.
    gio::bus_watch_name(
        gio::BusType::Session,
        BUS_NAME,
        gio::BusNameWatcherFlags::NONE,
        move |_, _, _| {
            READY.set(true);
            appeared(true);
        },
        move |_, _| {
            READY.set(false);
            vanished(false);
        },
    );
}

/// Skal bildet tas gjennom utvidelsen? Brukeren kan ha valgt GNOMEs vei.
pub fn silent(config: &Config) -> bool {
    ready() && config.capture_method == CaptureMethod::Silent
}

async fn call(method: &str, args: glib::Variant) -> Result<(), glib::Error> {
    let bus = gio::bus_get_future(gio::BusType::Session).await?;
    bus.call_future(
        Some(BUS_NAME),
        OBJECT_PATH,
        BUS_NAME,
        method,
        Some(&args),
        None,
        gio::DBusCallFlags::NO_AUTO_START,
        CALL_TIMEOUT_MS,
    )
    .await
    .map(|_| ())
}

/// Ta bildet uten blits. `None` betyr at portalen må overta.
pub async fn capture() -> Option<PathBuf> {
    let name = format!("klipp-{}.png", std::process::id());
    let path = glib::user_runtime_dir().join(name);
    match call("Capture", (path.to_str()?,).to_variant()).await {
        Ok(()) => Some(path),
        Err(e) => {
            trace(&format!("Shell-opptak feilet: {e}"));
            None
        }
    }
}

/// Blits og lyd slik brukeren har valgt, nå som bildet er tatt. Uten `area` gjelder
/// blitsen hele skrivebordet.
pub fn shutter(config: &Config, area: Option<(i32, i32, i32, i32)>) {
    if !silent(config) || !(config.flash || config.sound) {
        return;
    }
    let (x, y, w, h) = area.unwrap_or_default();
    let args = (config.flash, config.sound, x, y, w, h).to_variant();
    glib::spawn_future_local(async move {
        if let Err(e) = call("Shutter", args).await {
            trace(&format!("blits og lyd feilet: {e}"));
        }
    });
}

/// Be utvidelsen holde tasten. Tom streng slipper den.
pub fn bind_shortcut(accel: String) {
    glib::spawn_future_local(async move {
        if let Err(e) = call("BindShortcut", (accel.as_str(),).to_variant()).await {
            eprintln!("klipp: utvidelsen tok ikke tasten {accel}: {e}");
        }
    });
}
