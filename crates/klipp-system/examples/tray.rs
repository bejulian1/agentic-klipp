//! Kjører tray alene. Skriver hendelser til stdout.
//!
//!     cargo run -p klipp-system --example tray -- [--quit-after SEK]
//!
//! "Vis/Skjul Klipp" bytter menyteksten, "Avslutt" stopper eksempelet.

use adw::prelude::*;
use gtk::{gio, glib};
use klipp_system::{Tray, TrayEvent, tray_online, tray_set_window_visible};
use std::cell::{Cell, RefCell};
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
        .application_id("io.github.AgenticKlipp.DemoTray")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    let tray: Rc<RefCell<Option<Tray>>> = Rc::default();
    let hold = RefCell::new(None);
    app.connect_activate(move |app| {
        hold.replace(Some(app.hold()));
        let visible = Cell::new(true);
        let quit = app.clone();
        let main_thread = std::thread::current().id();
        let started = Tray::start(move |event| {
            let on_main = std::thread::current().id() == main_thread;
            println!("hendelse: {event:?} (hovedtråd: {on_main})");
            match event {
                TrayEvent::Capture => {}
                TrayEvent::ToggleWindow => {
                    visible.set(!visible.get());
                    tray_set_window_visible(visible.get());
                }
                TrayEvent::Quit => quit.quit(),
            }
        });
        println!(
            "Tray::start: {}",
            if started.is_some() { "Some" } else { "None" }
        );
        tray.replace(started);
        glib::timeout_add_seconds_local(1, || {
            println!("online: {}", tray_online());
            glib::ControlFlow::Continue
        });
        if let Some(seconds) = quit_after {
            let app = app.clone();
            glib::timeout_add_local_once(Duration::from_secs(seconds), move || app.quit());
        }
    });
    app.run_with_args::<&str>(&[])
}
