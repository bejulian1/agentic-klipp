//! Kjører innstillingsvinduet alene.
//!
//!     cargo run -p klipp-system --example settings -- [--hotkey] [--quit-after SEK]
//!
//! Bruk midlertidig `XDG_CONFIG_HOME` for å la din egen config være i fred.
//! Med `--hotkey` startes også global hurtigtast, slik at status-raden er ekte.

use adw::prelude::*;
use gtk::{gio, glib};
use klipp_system::{About, Config, HotkeyStatus, apply_theme, open_settings, start_hotkey};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

fn main() -> glib::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let hotkey = args.iter().any(|a| a == "--hotkey");
    let quit_after = args
        .iter()
        .position(|a| a == "--quit-after")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse().ok());

    let app = adw::Application::builder()
        .application_id("io.github.AgenticKlipp.DemoSettings")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.connect_activate(move |app| {
        let config = Rc::new(RefCell::new(Config::load()));
        apply_theme(config.borrow().theme);
        if hotkey {
            start_hotkey(
                config.clone(),
                || println!("aktivert"),
                |status: HotkeyStatus| println!("status: {status:?}"),
            );
        }
        let about = About {
            version: env!("CARGO_PKG_VERSION"),
            build: "eksempel",
            silent_capture: false,
        };
        open_settings(app, config.clone(), about, move |change| {
            println!("endring: {change:?}");
            println!("{}", config.borrow().serialize().replace('\n', " "));
        });
        if let Some(seconds) = quit_after {
            let app = app.clone();
            glib::timeout_add_local_once(Duration::from_secs(seconds), move || app.quit());
        }
    });
    app.run_with_args::<&str>(&[])
}
