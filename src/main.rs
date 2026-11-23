//! Agentic Screenshot: slankt skjermbildeverktøy for AI-kodeagenter.

mod app;
mod capture;
mod cli;
mod clipboard;
mod imaging;
mod keys;
mod shelf;
mod storage;
mod window;

use agentic_screenshot_system::Config;
use agentic_screenshot_system::i18n::{self, tr, tr_fmt};
use gtk::glib;
use gtk::prelude::*;
use std::process::ExitCode;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const BUILD: &str = env!("AGENTIC_SCREENSHOT_BUILD");

/// Tidslogg til stderr når `AGENTIC_SCREENSHOT_TRACE` er satt. Brukes til ytelsesmålinger.
pub fn trace(msg: &str) {
    static START: OnceLock<Option<Instant>> = OnceLock::new();
    let start =
        START.get_or_init(|| std::env::var_os("AGENTIC_SCREENSHOT_TRACE").map(|_| Instant::now()));
    if let Some(s) = start {
        eprintln!(
            "[agentic-screenshot {:9.3} ms] {msg}",
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
/// NVIDIA. Agentic Screenshot tegner lite og sjelden, så Cairo holder. `GSK_RENDERER` overstyrer.
fn choose_renderer() {
    if std::env::var_os("GSK_RENDERER").is_none() {
        // SAFETY: Kalles før GTK og før noen tråder er startet.
        unsafe { std::env::set_var("GSK_RENDERER", "cairo") };
    }
}

/// Prosessen er borte, eller den er avsluttet og venter bare på at forelderen
/// rydder opp (zombie). `kill(pid, 0)` lykkes for zombier, så tilstanden i
/// `/proc` må sjekkes i tillegg.
fn process_gone(pid: i32) -> bool {
    // SAFETY: A positive PID and signal 0 only check whether the process exists.
    if unsafe { libc::kill(pid, 0) } == -1
        && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
    {
        return true;
    }
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .is_ok_and(|stat| matches!(process_state(&stat), Some('Z' | 'X')))
}

/// Tilstandsbokstaven i `/proc/<pid>/stat`. Programnavnet i parentes kan selv
/// inneholde `)`, så tilstanden leses etter den siste.
fn process_state(stat: &str) -> Option<char> {
    stat.rsplit_once(')')?.1.trim_start().chars().next()
}

fn wait_for_exit(pid: i32) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if process_gone(pid) {
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
    let args: Vec<String> = std::env::args().skip(1).collect();
    // Filene fra Agentic Klipp flyttes før innstillingene og språket leses.
    if cli::parse(&args).is_ok_and(cli::Command::starts_app) {
        agentic_screenshot_system::migrate_legacy();
    }
    i18n::init(Config::load().language);
    let command = match cli::parse(&args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("agentic-screenshot: {e}\n\n{}", cli::help());
            return ExitCode::from(2);
        }
    };
    match command {
        cli::Command::Version => {
            // Oversettere: {version} is the app version and {build} is the build identifier.
            let version = tr_fmt(
                "agentic-screenshot {version} (bygg {build})",
                &[("version", VERSION), ("build", BUILD)],
            );
            println!("{version}");
            return ExitCode::SUCCESS;
        }
        cli::Command::Help => {
            print!("{}", cli::help());
            return ExitCode::SUCCESS;
        }
        // Starter uansett: Å bli borte uten beskjed er verre enn å møte en
        // gammel prosess som fortsatt holder på å avslutte.
        cli::Command::RestartAfter(pid) if !wait_for_exit(pid) => {
            let error = tr("Agentic Screenshot avsluttet ikke i tide");
            eprintln!("agentic-screenshot: {error}");
        }
        _ => {}
    }

    tune_malloc();
    choose_renderer();
    glib::set_prgname(Some("agentic-screenshot"));
    glib::set_application_name("Agentic Screenshot");
    gtk::gio::resources_register_include!("agentic-screenshot.gresource")
        .expect("innebygde ressurser");

    let app = app::build_application();
    let mut argv: Vec<String> = std::env::args().collect();
    if matches!(command, cli::Command::RestartAfter(_)) {
        argv.truncate(1);
    }
    let code = app.run_with_args(&argv);
    ExitCode::from(code.get().clamp(0, 255) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_state_after_the_last_parenthesis() {
        assert_eq!(process_state("42 (agentic-screenshot) S 1 42"), Some('S'));
        assert_eq!(process_state("42 (a) b) Z 1 42"), Some('Z'));
        assert_eq!(process_state("42 (uten slutt"), None);
    }

    #[test]
    fn a_zombie_counts_as_gone() {
        // Barnet avslutter, men blir ikke ryddet opp før `wait`, slik som når
        // forelderen ikke venter på det.
        let mut child = std::process::Command::new("true")
            .spawn()
            .expect("start true");
        let pid = child.id() as i32;
        assert!(wait_for_exit(pid), "zombien ble ikke sett som avsluttet");
        child.wait().expect("rydd opp");
        assert!(process_gone(pid));
    }

    #[test]
    fn a_running_process_is_not_gone() {
        assert!(!process_gone(std::process::id() as i32));
    }
}
