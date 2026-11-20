//! Agentic Klipp: slankt skjermbildeverktøy for AI-kodeagenter.

mod app;
mod capture;
mod cli;
mod clipboard;
mod imaging;
mod keys;
mod shelf;
mod storage;
mod window;

use gtk::glib;
use gtk::prelude::*;
use klipp_system::Config;
use klipp_system::i18n::{self, tr, tr_fmt};
use std::process::ExitCode;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const BUILD: &str = env!("KLIPP_BUILD");

/// Tidslogg til stderr når `KLIPP_TRACE` er satt. Brukes til ytelsesmålinger.
pub fn trace(msg: &str) {
    static START: OnceLock<Option<Instant>> = OnceLock::new();
    let start = START.get_or_init(|| std::env::var_os("KLIPP_TRACE").map(|_| Instant::now()));
    if let Some(s) = start {
        eprintln!(
            "[klipp {:9.3} ms] {msg}",
            s.elapsed().as_secs_f64() * 1000.0
        );
    }
}

/// Store bilder skal tilbake til systemet med en gang de frigjøres.
fn tune_malloc() {
    // SAFETY: mallopt endrer bare allokatorens parametre og kalles før GTK starter.
    unsafe {
        libc::mallopt(libc::M_ARENA_MAX, 2);
        libc::mallopt(libc::M_MMAP_THRESHOLD, 1 << 20);
        libc::mallopt(libc::M_TRIM_THRESHOLD, 1 << 20);
    }
}

/// GPU-rendereren laster hele drivere inn i prosessen: 200 MB RSS mot 55 MB på
/// NVIDIA. Klipp tegner lite og sjelden, så Cairo holder. `GSK_RENDERER` overstyrer.
fn choose_renderer() {
    if std::env::var_os("GSK_RENDERER").is_none() {
        // SAFETY: Kalles før GTK og før noen tråder er startet.
        unsafe { std::env::set_var("GSK_RENDERER", "cairo") };
    }
}

fn wait_for_exit(pid: i32) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        // SAFETY: A positive PID and signal 0 only check whether the process exists.
        if unsafe { libc::kill(pid, 0) } == -1
            && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
        {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn main() -> ExitCode {
    trace("start");
    i18n::init(Config::load().language);
    let command = match cli::parse(std::env::args().skip(1)) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("klipp: {e}\n\n{}", cli::help());
            return ExitCode::from(2);
        }
    };
    match command {
        cli::Command::Version => {
            // Oversettere: {version} is the app version and {build} is the build identifier.
            let version = tr_fmt(
                "klipp {version} (bygg {build})",
                &[("version", VERSION), ("build", BUILD)],
            );
            println!("{version}");
            return ExitCode::SUCCESS;
        }
        cli::Command::Help => {
            print!("{}", cli::help());
            return ExitCode::SUCCESS;
        }
        cli::Command::RestartAfter(pid) if !wait_for_exit(pid) => {
            let error = tr("Klipp avsluttet ikke i tide");
            eprintln!("klipp: {error}");
            return ExitCode::FAILURE;
        }
        _ => {}
    }

    tune_malloc();
    choose_renderer();
    glib::set_prgname(Some("klipp"));
    glib::set_application_name("Agentic Klipp");
    gtk::gio::resources_register_include!("klipp.gresource").expect("innebygde ressurser");

    let app = app::build_application();
    let mut argv: Vec<String> = std::env::args().collect();
    if matches!(command, cli::Command::RestartAfter(_)) {
        argv.truncate(1);
    }
    let code = app.run_with_args(&argv);
    ExitCode::from(code.get().clamp(0, 255) as u8)
}
