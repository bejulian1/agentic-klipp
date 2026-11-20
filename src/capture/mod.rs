//! Capture-løpet: skjul vinduene, frys skjermen, overlegg, lagre, kopier.

pub mod geometry;
pub mod overlay;
pub mod portal;
pub mod shell;

use crate::app::Klipp;
use crate::clipboard::{self, Slot, Source};
use crate::imaging;
use crate::shelf;
use crate::storage::{self, ShotMeta};
use crate::trace;
use geometry::{Mode, Pt, Rect};
use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use klipp_system::i18n::tr;
use portal::Outcome;
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

/// Hvor lenge vi venter på fokus etter Vindu-modus før brukeren får beskjed.
const FOCUS_WAIT: Duration = Duration::from_millis(900);
/// GNOMEs lukkeanimasjon er 150 ms. Pluss to bilderuter, så flaten er borte fra skjermen.
const CLOSE_ANIMATION: Duration = Duration::from_millis(185);
const ONE_FRAME: Duration = Duration::from_millis(17);
/// Uten dialog svarer portalen på under et sekund. Vinduene skal ikke bli borte for godt.
const PORTAL_TIMEOUT: Duration = Duration::from_secs(15);
/// GNOMEs velger venter på brukeren, men ikke for alltid: uten svar ville Klipp stå
/// som opptatt, og hurtigtasten ville ikke reagert før appen ble startet på nytt.
const PICKER_TIMEOUT: Duration = Duration::from_secs(300);

/// Det frosne skjermbildet. Deles uten kopi mellom overlegg og lagring.
pub struct Frozen {
    pub bytes: glib::Bytes,
    pub w: u32,
    pub h: u32,
}

pub enum Cut {
    /// Utsnitt av portalbildet, og størrelsen det skal ha i fysiske piksler.
    Rect(Rect, (u32, u32)),
    Poly(Rect, Vec<Pt>, (u32, u32)),
    Whole,
}

#[derive(Default)]
pub struct Controller {
    busy: Cell<bool>,
    hidden: RefCell<Vec<gtk::Window>>,
    pub session: RefCell<Option<Rc<overlay::Session>>>,
    started: Cell<Option<Instant>>,
}

impl Controller {
    pub fn mark(&self, what: &str) {
        if let Some(t) = self.started.get() {
            trace(&format!(
                "capture +{:.1} ms: {what}",
                t.elapsed().as_secs_f64() * 1000.0
            ));
        }
    }
}

pub fn start(k: &Rc<Klipp>, mode: Option<Mode>) {
    if k.capture.busy.replace(true) {
        return;
    }
    k.capture.started.set(Some(Instant::now()));
    if let Some(m) = mode {
        k.set_mode(m);
    }
    k.ui.hide_reveal();
    let k = k.clone();
    glib::spawn_future_local(async move {
        let mode = k.config.borrow().last_mode;
        if mode == Mode::Window {
            hide_windows(&k).await;
            window_capture(k).await;
        } else {
            frozen_capture(k, mode).await;
        }
    });
}

/// Skjul vinduene våre og vent til kompositoren har tegnet uten dem. GNOME tar
/// bort et vindu med en animasjon, så med animasjoner på må vi vente den ut, med
/// mindre utvidelsen vår har slått den av. Er ingen av vinduene våre synlige, tar
/// vi bildet med en gang.
async fn hide_windows(k: &Rc<Klipp>) {
    let mut wins: Vec<gtk::Window> = gtk::Window::list_toplevels()
        .into_iter()
        .filter_map(|w| w.downcast::<gtk::Window>().ok())
        .filter(|w| w.is_visible())
        .collect();
    if wins.is_empty() {
        return;
    }
    // Hovedvinduet først, så vises dialogene over det igjen etterpå.
    wins.sort_by_key(|w| !w.has_css_class("klipp-main"));
    for w in &wins {
        w.set_visible(false);
    }
    if let Some(d) = gdk::Display::default() {
        d.sync();
    }
    let animated =
        !shell::ready() && gtk::Settings::default().is_some_and(|s| s.is_gtk_enable_animations());
    glib::timeout_future(if animated {
        CLOSE_ANIMATION
    } else {
        ONE_FRAME * 2
    })
    .await;
    k.capture.hidden.replace(wins);
    k.sync_tray();
}

fn restore_windows(k: &Rc<Klipp>, show_main: bool) {
    let mut wins = k.capture.hidden.take();
    let main: gtk::Window = k.ui.window.clone().upcast();
    if show_main && !wins.contains(&main) {
        wins.insert(0, main);
    }
    for w in &wins {
        w.present();
    }
    k.sync_tray();
}

fn finish(k: &Rc<Klipp>) {
    k.capture.busy.set(false);
    k.capture.started.set(None);
    glib::timeout_add_local_once(Duration::from_millis(200), || {
        gio::spawn_blocking(|| {
            // SAFETY: malloc_trim har ingen forutsetninger og er trådsikker.
            unsafe { libc::malloc_trim(0) };
        });
    });
}

/// Avbrutt: vinduene tilbake slik de var, uten å kreve noe mer.
fn abort(k: &Rc<Klipp>) {
    restore_windows(k, false);
    finish(k);
}

fn fail(k: &Rc<Klipp>, msg: &str) {
    eprintln!("klipp: {msg}");
    restore_windows(k, true);
    k.ui.toast(msg);
    finish(k);
}

/// Les fila fra portalen, og fjern den så vi ikke etterlater duplikater.
async fn load_portal_file(k: &Rc<Klipp>, path: PathBuf) -> Result<Frozen, String> {
    let own = k.config.borrow().save_dir();
    gio::spawn_blocking(move || {
        let img = imaging::decode_png(&path)?;
        if let Err(e) = storage::remove_portal_file(&path, &own) {
            eprintln!("klipp: kunne ikke fjerne {}: {e}", path.display());
        }
        Ok(Frozen {
            w: img.w,
            h: img.h,
            bytes: glib::Bytes::from_owned(img.data),
        })
    })
    .await
    .unwrap_or_else(|_| Err(tr("Bildet kunne ikke leses").to_string()))
}

/// Første gang må brukeren svare på GNOMEs tilgangsdialog, og den vises bare for
/// appen som har fokus. Da viser vi vinduet, spør, og tar det ekte bildet etterpå.
async fn ask_permission(k: &Rc<Klipp>) -> Result<Option<PathBuf>, String> {
    restore_windows(k, true);
    wait_for_focus().await;
    match portal::take(false, Some(PICKER_TIMEOUT)).await? {
        Outcome::File(p) => {
            // Dette bildet viser Klipp selv og brukes ikke.
            let own = k.config.borrow().save_dir();
            let _ = storage::remove_portal_file(&p, &own);
        }
        Outcome::Cancelled => return Ok(None),
        Outcome::Denied(_) => return Err(tr("Klipp har ikke tilgang til skjermbilder").to_string()),
    }
    hide_windows(k).await;
    match portal::take(false, Some(PORTAL_TIMEOUT)).await? {
        Outcome::File(p) => Ok(Some(p)),
        Outcome::Cancelled => Ok(None),
        Outcome::Denied(_) => Err(tr("Klipp har ikke tilgang til skjermbilder").to_string()),
    }
}

/// Portalen ser hele skjermen, så vinduene våre må være borte først.
async fn portal_shot(k: &Rc<Klipp>) -> Result<Option<PathBuf>, String> {
    hide_windows(k).await;
    k.capture.mark("vinduer skjult");
    match portal::take(false, Some(PORTAL_TIMEOUT)).await? {
        Outcome::File(p) => Ok(Some(p)),
        Outcome::Cancelled => Ok(None),
        Outcome::Denied(why) => {
            trace(&why);
            ask_permission(k).await
        }
    }
}

async fn frozen_capture(k: Rc<Klipp>, mode: Mode) {
    let use_shell = shell::silent(&k.config.borrow());
    let silent = if use_shell {
        shell::capture().await
    } else {
        None
    };
    let path = match silent {
        Some(p) => p,
        None => match portal_shot(&k).await {
            Ok(Some(p)) => p,
            Ok(None) => return abort(&k),
            Err(e) => return fail(&k, &e),
        },
    };
    k.capture.mark("bilde tatt");
    let frozen = match load_portal_file(&k, path).await {
        Ok(f) => f,
        Err(e) => return fail(&k, &e),
    };
    k.capture.mark("bilde dekodet");
    if k.config.borrow().instant_capture && mode == Mode::Screen {
        restore_windows(&k, true);
        shell::shutter(&k.config.borrow(), None);
        deliver(k, frozen, Cut::Whole, None);
        return;
    }
    overlay::open(&k, frozen, mode);
}

/// Vindu-modus: GNOMEs egen velger, resultatet behandles som ethvert annet bilde.
pub(crate) async fn window_capture(k: Rc<Klipp>) {
    let path = match portal::take(true, Some(PICKER_TIMEOUT)).await {
        Ok(Outcome::File(p)) => p,
        // GNOMEs velger svarer «nei» også når brukeren trykker Esc.
        Ok(Outcome::Cancelled | Outcome::Denied(_)) => return abort(&k),
        Err(e) => return fail(&k, &e),
    };
    let frozen = match load_portal_file(&k, path).await {
        Ok(f) => f,
        Err(e) => return fail(&k, &e),
    };
    restore_windows(&k, true);
    deliver(k, frozen, Cut::Whole, None);
}

/// Overlegget lukket uten bilde.
pub(crate) fn overlay_cancelled(k: &Rc<Klipp>) {
    k.capture.session.replace(None);
    abort(k);
}

/// Brukeren valgte Vindu i overlegget.
pub(crate) fn overlay_to_window(k: &Rc<Klipp>) {
    k.capture.session.replace(None);
    let k = k.clone();
    glib::spawn_future_local(async move {
        // GNOMEs velger fryser skjermen selv, og overlegget skal ikke være med.
        if !shell::ready() {
            glib::timeout_future(CLOSE_ANIMATION).await;
        }
        hide_windows(&k).await;
        window_capture(k).await;
    });
}

/// Overlegget tok et bilde. Kopieringen er allerede satt i gang mens det hadde fokus.
pub(crate) fn overlay_taken(
    k: &Rc<Klipp>,
    frozen: Frozen,
    cut: Cut,
    slot: Option<Slot>,
    area: (i32, i32, i32, i32),
) {
    k.capture.session.replace(None);
    // Vinduet skal ikke sprette opp og ta fokus fra appen brukeren jobber i.
    restore_windows(k, false);
    shell::shutter(&k.config.borrow(), Some(area));
    deliver(k.clone(), frozen, cut, slot);
}

fn work(
    frozen: Frozen,
    cut: Cut,
    dir: &Path,
    bx: (u32, u32),
) -> Result<(ShotMeta, imaging::Image), String> {
    let (img, alpha) = match cut {
        Cut::Rect(r, out) => {
            let img = imaging::crop_raw(&frozen.bytes, frozen.w, frozen.h, r);
            (imaging::downscale(&img, out.0, out.1), false)
        }
        Cut::Poly(r, pts, out) => {
            let img = imaging::crop_polygon_raw(&frozen.bytes, frozen.w, frozen.h, r, &pts);
            (imaging::downscale(&img, out.0, out.1), true)
        }
        Cut::Whole => {
            let alpha = frozen.bytes.chunks_exact(4).any(|p| p[3] != 255);
            (
                imaging::crop_raw(
                    &frozen.bytes,
                    frozen.w,
                    frozen.h,
                    Rect::new(0, 0, frozen.w as i32, frozen.h as i32),
                ),
                alpha,
            )
        }
    };
    drop(frozen);
    storage::save_new_shot(dir, &img, alpha, bx.0, bx.1)
}

/// Vent til et av Klipps vinduer har fokus, ellers nekter Wayland kopiering.
async fn wait_for_focus() -> bool {
    let start = Instant::now();
    while !clipboard::has_focus() && start.elapsed() < FOCUS_WAIT {
        glib::timeout_future(Duration::from_millis(15)).await;
    }
    clipboard::has_focus()
}

fn deliver(k: Rc<Klipp>, frozen: Frozen, cut: Cut, slot: Option<Slot>) {
    let dir = k.config.borrow().save_dir();
    let bx = k.shelf.thumb_box();
    glib::spawn_future_local(async move {
        k.capture.mark("lagrer");
        let done = gio::spawn_blocking(move || work(frozen, cut, &dir, bx)).await;
        let (meta, thumb) = match done {
            Ok(Ok(v)) => v,
            Ok(Err(e)) => {
                if let Some(s) = &slot {
                    s.replace(Source::Failed);
                }
                k.ui.toast(&e);
                eprintln!("klipp: {e}");
                return finish(&k);
            }
            Err(_) => {
                if let Some(s) = &slot {
                    s.replace(Source::Failed);
                }
                k.ui.toast(tr("Bildet kunne ikke lagres"));
                return finish(&k);
            }
        };
        k.capture.mark("lagret");
        let want_copy = k.config.borrow().auto_copy;
        let copied = if let Some(s) = slot {
            s.replace(Source::File(meta.path.clone()));
            true
        } else if want_copy {
            // Uten overlegg med fokus må appen få fokus først.
            wait_for_focus().await && clipboard::copy_file(meta.path.clone())
        } else {
            false
        };

        let compact = k.config.borrow().compact;
        let tex = compact.then(|| shelf::to_texture(thumb.clone()));
        let dims = meta.dims();
        let (path, ts, alpha) = (meta.path.clone(), meta.ts, meta.alpha);
        k.shelf.add(meta, thumb);
        k.ui.sync_shelf(&k.shelf, &k.config.borrow());

        if let Some(tex) = tex {
            let status = match (want_copy, copied) {
                (true, true) => tr("Kopiert!").to_string(),
                (true, false) => tr("Ikke kopiert").to_string(),
                _ => shelf::fmt_time(ts),
            };
            k.ui.show_reveal(&tex, &dims, &status, &path, alpha);
        } else if want_copy {
            k.ui.toast(if copied {
                tr("Kopiert!")
            } else {
                tr("Lagret, men ikke kopiert. Shift+høyreklikk på bildet.")
            });
        }
        k.capture.mark("ferdig");
        finish(&k);
    });
}
