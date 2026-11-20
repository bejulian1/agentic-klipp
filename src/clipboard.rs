//! Kopiering. På Wayland må en klient ha tastaturfokus for å sette utklippstavla,
//! så kopiering lykkes bare mens et av Klipps vinduer er aktivt. Alt her svarer ærlig
//! på om det skjedde, og bildet tilbys som `image/png` uten å ligge i minnet.

use gtk::gdk::subclass::prelude::*;
use gtk::prelude::*;

use gtk::{gdk, gio, glib};
use klipp_system::i18n::tr;
use std::cell::RefCell;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::rc::Rc;
use std::time::Duration;

#[derive(Clone, Debug)]
pub enum Source {
    /// Fila skrives fremdeles.
    Pending,
    File(PathBuf),
    Bytes(glib::Bytes),
    Failed,
}

/// Der PNG-en kommer fra. Deles med tråden som lagrer bildet.
pub type Slot = Rc<RefCell<Source>>;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct LazyPng {
        pub slot: RefCell<Option<Slot>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for LazyPng {
        const NAME: &'static str = "KlippLazyPng";
        type Type = super::LazyPng;
        type ParentType = gdk::ContentProvider;
    }

    impl ObjectImpl for LazyPng {}

    impl ContentProviderImpl for LazyPng {
        fn formats(&self) -> gdk::ContentFormats {
            gdk::ContentFormats::new(&["image/png"])
        }

        fn write_mime_type_future(
            &self,
            mime: &str,
            stream: &gio::OutputStream,
            priority: glib::Priority,
        ) -> Pin<Box<dyn Future<Output = Result<(), glib::Error>> + 'static>> {
            let slot = self.slot.borrow().clone();
            let stream = stream.clone();
            let is_png = mime == "image/png";
            Box::pin(async move {
                let fail = |m: &str| glib::Error::new(gio::IOErrorEnum::Failed, m);
                let slot = slot
                    .filter(|_| is_png)
                    .ok_or_else(|| fail(tr("ukjent format")))?;
                let bytes = loop {
                    let src = slot.borrow().clone();
                    match src {
                        Source::Pending => glib::timeout_future(Duration::from_millis(10)).await,
                        Source::Bytes(b) => break b,
                        Source::Failed => return Err(fail(tr("bildet ble ikke lagret"))),
                        Source::File(p) => {
                            let read = gio::spawn_blocking(move || std::fs::read(p)).await;
                            match read {
                                Ok(Ok(v)) => break glib::Bytes::from_owned(v),
                                _ => return Err(fail(tr("kunne ikke lese bildet"))),
                            }
                        }
                    }
                };
                stream
                    .write_all_future(bytes.to_vec(), priority)
                    .await
                    .map(|_| ())
                    .map_err(|(_, e)| e)
            })
        }
    }
}

glib::wrapper! {
    pub struct LazyPng(ObjectSubclass<imp::LazyPng>) @extends gdk::ContentProvider;
}

fn provider(slot: &Slot) -> LazyPng {
    let p: LazyPng = glib::Object::new();
    *p.imp().slot.borrow_mut() = Some(slot.clone());
    p
}

/// Sann når et av Klipps vinduer har fokus, altså når Wayland godtar kopiering.
pub fn has_focus() -> bool {
    gtk::Window::list_toplevels()
        .into_iter()
        .filter_map(|w| w.downcast::<gtk::Window>().ok())
        .any(|w| w.is_active())
}

fn clipboard() -> Option<gdk::Clipboard> {
    gdk::Display::default().map(|d| d.clipboard())
}

/// Legg bildet på utklippstavla. Gir `None` uten fokus, ellers plassen å fylle.
pub fn offer(source: Source) -> Option<Slot> {
    if !has_focus() {
        return None;
    }
    let slot: Slot = Rc::new(RefCell::new(source));
    let cb = clipboard()?;
    cb.set_content(Some(provider(&slot).upcast_ref::<gdk::ContentProvider>()))
        .ok()?;
    Some(slot)
}

pub fn copy_file(path: PathBuf) -> bool {
    offer(Source::File(path)).is_some()
}

/// Filene som filer, URI-er og stier. Felles for dra og kopier.
pub fn file_formats(paths: &[PathBuf]) -> Vec<gdk::ContentProvider> {
    let files: Vec<gio::File> = paths.iter().map(gio::File::for_path).collect();
    let uris: Vec<String> = files.iter().map(|f| f.uri().to_string()).collect();
    vec![
        gdk::ContentProvider::for_value(&gdk::FileList::from_array(&files).to_value()),
        gdk::ContentProvider::for_bytes(
            "text/uri-list",
            &glib::Bytes::from_owned(uri_list(&uris).into_bytes()),
        ),
        gdk::ContentProvider::for_value(&path_lines(paths).to_value()),
    ]
}

/// Flere bilder på én gang: filer til Nautilus og chatapper, stier som tekst til
/// terminalen. Ett bilde tilbys også som `image/png`, som ved vanlig kopiering.
pub fn copy_files(paths: &[PathBuf]) -> bool {
    if paths.is_empty() || !has_focus() {
        return false;
    }
    let Some(cb) = clipboard() else {
        return false;
    };
    let mut parts = Vec::new();
    if let [one] = paths {
        let slot: Slot = Rc::new(RefCell::new(Source::File(one.clone())));
        parts.push(provider(&slot).upcast::<gdk::ContentProvider>());
    }
    parts.extend(file_formats(paths));
    let uris: Vec<String> = paths
        .iter()
        .map(|p| gio::File::for_path(p).uri().to_string())
        .collect();
    parts.push(gdk::ContentProvider::for_bytes(
        "x-special/gnome-copied-files",
        &glib::Bytes::from_owned(gnome_copied_files(&uris).into_bytes()),
    ));
    cb.set_content(Some(&gdk::ContentProvider::new_union(&parts)))
        .is_ok()
}

/// `text/uri-list` etter RFC 2483: én URI per linje, avsluttet med CRLF.
fn uri_list(uris: &[String]) -> String {
    uris.iter().map(|u| format!("{u}\r\n")).collect()
}

/// Nautilus sitt eget format: handlingen først, så én URI per linje.
fn gnome_copied_files(uris: &[String]) -> String {
    std::iter::once("copy")
        .chain(uris.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Fulle stier, én per linje. Claude Code gjør limte bildestier om til vedlegg.
fn path_lines(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|p| p.to_string_lossy())
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn copy_text(text: &str) -> bool {
    if !has_focus() {
        return false;
    }
    match clipboard() {
        Some(cb) => {
            cb.set_text(text);
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uris() -> Vec<String> {
        vec![
            "file:///home/a/Bilder/Klipp/en.png".into(),
            "file:///home/a/Bilder/Klipp/to%20b.png".into(),
        ]
    }

    #[test]
    fn uri_list_ends_every_line_with_crlf() {
        assert_eq!(
            uri_list(&uris()),
            "file:///home/a/Bilder/Klipp/en.png\r\nfile:///home/a/Bilder/Klipp/to%20b.png\r\n"
        );
        assert_eq!(uri_list(&[]), "");
    }

    #[test]
    fn gnome_copied_files_matches_nautilus() {
        assert_eq!(
            gnome_copied_files(&uris()),
            "copy\nfile:///home/a/Bilder/Klipp/en.png\nfile:///home/a/Bilder/Klipp/to%20b.png"
        );
    }

    #[test]
    fn path_lines_are_plain_absolute_paths() {
        let paths = [PathBuf::from("/a/en.png"), PathBuf::from("/a/to b.png")];
        assert_eq!(path_lines(&paths), "/a/en.png\n/a/to b.png");
        assert_eq!(path_lines(&paths[..1]), "/a/en.png");
    }
}
