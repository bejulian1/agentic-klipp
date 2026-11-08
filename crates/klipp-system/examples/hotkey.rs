//! Prøver global hurtigtast alene. Krever installert `.desktop`-fil, se README.
//! Skriver status og aktiveringer til stdout.
//!
//!     cargo run -p klipp-system --example hotkey -- [--quit-after SEK]

use adw::prelude::*;
use gtk::{gio, glib};
use klipp_system::{Config, HotkeyStatus, start_hotkey};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

fn main() -> glib::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let quit_after = args
        .iter()
        .position(|a| a == "--quit-after")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse().ok());

    let app = adw::Application::builder()
        .application_id("io.github.AgenticKlipp.DemoHotkey")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    let hold = RefCell::new(None);
    app.connect_activate(move |app| {
        hold.replace(Some(app.hold()));
        let config = Rc::new(RefCell::new(Config::load()));
        start_hotkey(
            config,
            || println!("aktivert"),
            |status: HotkeyStatus| println!("status: {status:?}"),
        );
        if let Some(seconds) = quit_after {
            let app = app.clone();
            glib::timeout_add_local_once(Duration::from_secs(seconds), move || app.quit());
        }
    });
    app.run_with_args::<&str>(&[])
}
