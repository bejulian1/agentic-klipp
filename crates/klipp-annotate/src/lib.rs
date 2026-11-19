//! Annoteringsvinduet i Agentic Klipp.
//!
//! All tegning lagres som en operasjonsliste over originalen. Bildet leses og skrives i
//! bakgrunnstråder og lever bare så lenge vinduet er åpent.

mod canvas;
mod clipboard;
mod file;
mod keys;
mod ops;
mod pixels;
mod raster;
mod view;
mod window;

use adw::prelude::*;
use gtk::{gio, glib};
use std::cell::RefCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use window::AnnotateWindow;

/// Hendelser fra annoteringsvinduet. Alle kommer på GTK-hovedtråden.
#[derive(Debug, Clone)]
pub enum AnnotateEvent {
    /// Ferdig: filen er skrevet på nytt med annoteringene brent inn.
    Saved(PathBuf),
    /// Vinduet ble lukket uten lagring. Også Ferdig uten annoteringer, som lar filen stå.
    Cancelled,
}

thread_local! {
    /// Bilder som leses inn og ennå ikke har vindu.
    static OPENING: RefCell<HashSet<PathBuf>> = RefCell::default();
}

/// Åpner annoteringsvinduet for `path`.
///
/// Returnerer med en gang: bildet leses i bakgrunnen, og `on_event` kalles én gang når vinduet
/// lukkes. Er `path` allerede åpen, hentes det vinduet fram, og `on_event` kalles aldri.
/// Kan ikke bildet leses, vises feilen i et lite vindu, og det gir `Cancelled` når det lukkes.
pub fn open(app: &adw::Application, path: &Path, on_event: impl Fn(AnnotateEvent) + 'static) {
    keep_big_buffers_off_the_heap();
    let existing = app
        .windows()
        .into_iter()
        .filter_map(|w| w.downcast::<AnnotateWindow>().ok())
        .find(|w| w.path() == Some(path));
    if let Some(window) = existing {
        window.present();
        return;
    }
    let path = path.to_path_buf();
    if !OPENING.with_borrow_mut(|o| o.insert(path.clone())) {
        return;
    }
    let app = app.clone();
    // Uten denne kan appen avslutte seg selv før det finnes et vindu.
    let hold = app.hold();
    glib::spawn_future_local(async move {
        let source = path.clone();
        let loaded = gio::spawn_blocking(move || file::load(&source))
            .await
            .unwrap_or_else(|_| Err("Bildet kunne ikke leses".to_string()));
        OPENING.with_borrow_mut(|o| o.remove(&path));
        match loaded {
            Ok(image) => AnnotateWindow::new(&app, &path, image, on_event).present(),
            Err(error) => show_error(&app, &error, on_event),
        }
        drop(hold);
    });
}

/// Bilder dekodes i bakgrunnstråder med hver sin glibc-arena, og der blir frigjorte blokker
/// liggende selv etter `malloc_trim`. Med fast terskel får store blokker egen mmap, og
/// `free` gir dem tilbake til systemet med en gang vinduet lukkes.
fn keep_big_buffers_off_the_heap() {
    const THRESHOLD: libc::c_int = 1 << 20;
    // SAFETY: mallopt endrer bare en tallverdi i allokatoren og er trygg å kalle når som helst.
    unsafe {
        libc::mallopt(libc::M_MMAP_THRESHOLD, THRESHOLD);
    }
}

fn show_error(app: &adw::Application, error: &str, on_event: impl Fn(AnnotateEvent) + 'static) {
    let page = adw::StatusPage::builder()
        .icon_name("dialog-error-symbolic")
        .title("Kunne ikke åpne bildet")
        .description(glib::markup_escape_text(error).as_str())
        .build();
    let content = adw::ToolbarView::new();
    content.add_top_bar(&adw::HeaderBar::new());
    content.set_content(Some(&page));
    let window = adw::Window::builder()
        .application(app)
        .title("Annoter")
        .default_width(420)
        .default_height(320)
        .content(&content)
        .build();
    window.connect_close_request(move |_| {
        on_event(AnnotateEvent::Cancelled);
        glib::Propagation::Proceed
    });
    window.present();
}
