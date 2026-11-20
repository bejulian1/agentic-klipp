//! Bildet på utklippstavla eies av tilbyderen og kan leses etter at vinduet lukkes.

use crate::canvas::Flat;
use crate::file;
use futures_util::future::{FutureExt, LocalBoxFuture, Shared};
use gtk::gdk::subclass::prelude::*;
use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use klipp_system::i18n::tr;
use std::cell::OnceCell;
use std::future::Future;
use std::pin::Pin;

type Encoding = Shared<LocalBoxFuture<'static, Result<glib::Bytes, glib::Error>>>;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct PngProvider(pub OnceCell<Encoding>);

    #[glib::object_subclass]
    impl ObjectSubclass for PngProvider {
        const NAME: &'static str = "KlippAnnotationPng";
        type Type = super::PngProvider;
        type ParentType = gdk::ContentProvider;
    }

    impl ObjectImpl for PngProvider {}

    impl ContentProviderImpl for PngProvider {
        fn formats(&self) -> gdk::ContentFormats {
            gdk::ContentFormats::new(&["image/png"])
        }

        fn write_mime_type_future(
            &self,
            mime: &str,
            stream: &gio::OutputStream,
            priority: glib::Priority,
        ) -> Pin<Box<dyn Future<Output = Result<(), glib::Error>> + 'static>> {
            let encoding = self.0.get().cloned();
            let stream = stream.clone();
            let is_png = mime == "image/png";
            Box::pin(async move {
                let Some(encoding) = encoding.filter(|_| is_png) else {
                    return Err(glib::Error::new(
                        gio::IOErrorEnum::NotSupported,
                        tr("Ukjent format"),
                    ));
                };
                stream
                    .write_all_future(encoding.await?, priority)
                    .await
                    .map(|_| ())
                    .map_err(|(_, error)| error)
            })
        }
    }
}

glib::wrapper! {
    pub struct PngProvider(ObjectSubclass<imp::PngProvider>) @extends gdk::ContentProvider;
}

impl PngProvider {
    pub fn new(flat: Flat) -> Self {
        let provider: Self = glib::Object::new();
        let worker = gio::spawn_blocking(move || {
            file::encode_bytes(&flat.bytes, flat.width, flat.height, flat.alpha)
        });
        let encoding = async move {
            worker
                .await
                .unwrap_or_else(|_| Err(tr("Bildet kunne ikke kodes som PNG").into()))
                .map(glib::Bytes::from_owned)
                .map_err(|e| glib::Error::new(gio::IOErrorEnum::Failed, &e))
        }
        .boxed_local()
        .shared();
        assert!(provider.imp().0.set(encoding).is_ok());
        provider
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn png_can_be_requested_after_the_editor_releases_its_pixels() {
        let pixels = glib::Bytes::from_owned(vec![12, 34, 56, 255]);
        let provider = PngProvider::new(Flat {
            bytes: pixels.clone(),
            width: 1,
            height: 1,
            alpha: false,
        });
        drop(pixels);

        glib::MainContext::default().block_on(async {
            let stream = gio::MemoryOutputStream::new_resizable();
            provider
                .write_mime_type_future("image/png", &stream, glib::Priority::DEFAULT)
                .await
                .unwrap();
            stream.close(gio::Cancellable::NONE).unwrap();
            let decoded = file::decode(Cursor::new(stream.steal_as_bytes())).unwrap();
            assert_eq!((decoded.width, decoded.height), (1, 1));
            assert_eq!(decoded.pixels, [12, 34, 56, 255]);
        });
    }
}
