//! Åpner annoteringsvinduet uten resten av appen.
//!
//!     demo <fil.png>                 åpner filen og skriver hendelsen til stdout
//!     demo --lag <B>x<H> [--alfa] <fil.png>   lager et testbilde og avslutter
//!     demo --gjenta <antall> <fil.png>        åpner på nytt hver gang vinduet lukkes
//!     demo --skjul <fil.png>         skjuler og viser vinduet etter to sekunder, som ved fanging
//!     demo --dobbel <fil.png>        åpner samme fil to ganger
//!     demo --tema <fil.png>          bytter mellom lyst og mørkt etter to sekunder
//!
//! Ikonene ligger i rotpakken, så de registreres her.

use adw::prelude::*;
use gtk::{gdk, gio, glib};
use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

const PAUSE_AFTER_CLOSE: u32 = 2;

fn main() -> glib::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "--lag") {
        return make_image(&args[1..]);
    }
    let (rounds, file, mode) = match args.as_slice() {
        [flag, n, file] if flag == "--gjenta" => (n.parse().unwrap_or(1), file, ""),
        [flag, file] if ["--skjul", "--dobbel", "--tema"].contains(&flag.as_str()) => {
            (1, file, flag.as_str())
        }
        [file] => (1, file, ""),
        _ => {
            eprintln!("Bruk: demo [--gjenta <antall> | --skjul | --dobbel | --tema] <fil.png>");
            return glib::ExitCode::FAILURE;
        }
    };
    let mode = mode.to_string();
    let file = PathBuf::from(file);

    let app = adw::Application::new(None::<&str>, gio::ApplicationFlags::NON_UNIQUE);
    app.connect_startup(|_| {
        let icons = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/symbolic/scalable/actions"
        );
        let display = gdk::Display::default().expect("trenger en skjerm");
        gtk::IconTheme::for_display(&display).add_search_path(icons);
    });
    watch_frames(&app);
    let left = Rc::new(Cell::new(rounds));
    app.connect_activate(move |app| {
        run(app, &file, left.clone());
        match mode.as_str() {
            "--dobbel" => run(app, &file, left.clone()),
            "--skjul" => hide_and_show(app),
            "--tema" => switch_theme(),
            _ => {}
        }
    });
    app.run_with_args::<&str>(&[])
}

fn run(app: &adw::Application, file: &Path, left: Rc<Cell<u32>>) {
    let (app, file) = (app.clone(), file.to_path_buf());
    let path = file.clone();
    agentic_screenshot_annotate::open(&app.clone(), &path, move |event| {
        println!("{event:?}");
        left.set(left.get().saturating_sub(1));
        // Står litt etter hver lukking, så minnet kan måles med vinduet borte.
        let hold = app.hold();
        let (app, file, left) = (app.clone(), file.clone(), left.clone());
        glib::timeout_add_seconds_local_once(PAUSE_AFTER_CLOSE, move || {
            let _hold = hold;
            if left.get() > 0 {
                run(&app, &file, left);
            }
        });
    });
}

/// Med AGENTIC_SCREENSHOT_DEMO_RAMMER=1 logges hvert rammegap over 25 ms med tidspunkt.
fn watch_frames(app: &adw::Application) {
    if std::env::var_os("AGENTIC_SCREENSHOT_DEMO_RAMMER").is_none() {
        return;
    }
    app.connect_window_added(|_, window| {
        let last = Cell::new(None::<i64>);
        window.add_tick_callback(move |_, clock| {
            let now = clock.frame_time();
            if let Some(before) = last.replace(Some(now)) {
                let gap = (now - before) / 1000;
                if gap > 25 {
                    eprintln!("RAMME {} {gap}", glib::real_time() / 1000);
                }
            }
            glib::ControlFlow::Continue
        });
    });
}

/// Samme som hovedutvikleren gjør med alle toppvinduer før skjermen fryses.
fn hide_and_show(app: &adw::Application) {
    let app = app.clone();
    glib::timeout_add_seconds_local_once(2, move || {
        let windows = app.windows();
        for w in &windows {
            w.set_visible(false);
        }
        glib::timeout_add_seconds_local_once(1, move || {
            for w in &windows {
                w.set_visible(true);
            }
        });
    });
}

fn switch_theme() {
    glib::timeout_add_seconds_local_once(2, || {
        let style = adw::StyleManager::default();
        style.set_color_scheme(if style.is_dark() {
            adw::ColorScheme::ForceLight
        } else {
            adw::ColorScheme::ForceDark
        });
    });
}

fn make_image(args: &[String]) -> glib::ExitCode {
    let alpha = args.iter().any(|a| a == "--alfa");
    let mut rest = args.iter().filter(|a| *a != "--alfa");
    let (Some(size), Some(file)) = (rest.next(), rest.next()) else {
        eprintln!("Bruk: demo --lag <B>x<H> [--alfa] <fil.png>");
        return glib::ExitCode::FAILURE;
    };
    let Some((w, h)) = size
        .split_once('x')
        .and_then(|(w, h)| Some((w.parse::<u32>().ok()?, h.parse::<u32>().ok()?)))
    else {
        eprintln!("Ugyldig størrelse: {size}");
        return glib::ExitCode::FAILURE;
    };
    let mut rgba = Vec::with_capacity(w as usize * h as usize * 4);
    for y in 0..h {
        for x in 0..w {
            let (fx, fy) = (f64::from(x) / f64::from(w), f64::from(y) / f64::from(h));
            let stripe = (x / 16 + y / 16) % 2 == 0;
            let r = (fx * 255.0) as u8;
            let g = (fy * 255.0) as u8;
            let b = if stripe { 200 } else { 60 };
            // Med alfa: en ellipse med myk kant på helt gjennomsiktig bakgrunn.
            let d = ((fx - 0.5) * 2.0).hypot((fy - 0.5) * 2.0);
            let a = if alpha {
                ((1.0 - d) * 40.0).clamp(0.0, 1.0) * 255.0
            } else {
                255.0
            };
            rgba.extend_from_slice(&[r, g, b, a as u8]);
        }
    }
    let out = std::fs::File::create(file).expect("kunne ikke lage filen");
    let mut enc = png::Encoder::new(std::io::BufWriter::new(out), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_compression(png::Compression::Fast);
    let mut writer = enc.write_header().expect("PNG-hode");
    writer.write_image_data(&rgba).expect("PNG-data");
    writer.finish().expect("PNG-slutt");
    glib::ExitCode::SUCCESS
}
