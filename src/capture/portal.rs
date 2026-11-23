//! Skjermbilde-portalen via ashpd, og registrering av appens identitet.

use crate::trace;
use agentic_screenshot_system::APP_ID;
use agentic_screenshot_system::i18n::{tr, tr_fmt};
use ashpd::desktop::ResponseError;
use ashpd::desktop::screenshot::Screenshot;
use futures_util::FutureExt;
use futures_util::future::{LocalBoxFuture, Shared};
use gtk::glib;
use std::cell::RefCell;
use std::path::PathBuf;
use std::time::Duration;

pub enum Outcome {
    File(PathBuf),
    Cancelled,
    /// Portalen sa nei. GNOME viser bare tilgangsdialogen for appen som har fokus,
    /// så uten lagret tillatelse svarer den slik mens vinduene våre er skjult.
    Denied(String),
}

type Registered = Shared<LocalBoxFuture<'static, bool>>;

thread_local! {
    static REGISTERED: RefCell<Option<Registered>> = const { RefCell::new(None) };
}

/// Host-apper må si hvem de er før første portalkall. Det krever installert
/// .desktop-fil. Skjermbilde-portalen virker også uten, så feil er ikke fatalt.
async fn ensure_registered() -> bool {
    let fut = REGISTERED.with(|r| {
        r.borrow_mut()
            .get_or_insert_with(|| {
                async {
                    let Ok(id) = ashpd::AppID::try_from(APP_ID) else {
                        return false;
                    };
                    match ashpd::register_host_app(id).await {
                        Ok(()) => {
                            trace("registrert hos portalen");
                            true
                        }
                        Err(e) => {
                            trace(&format!("registrering hos portalen feilet: {e}"));
                            false
                        }
                    }
                }
                .boxed_local()
                .shared()
            })
            .clone()
    });
    fut.await
}

/// Ta skjermbilde. Med `interactive` viser GNOME sin egen velger, og brukeren får
/// den tiden hen trenger. Ellers gir vi opp etter `limit`.
pub async fn take(interactive: bool, limit: Option<Duration>) -> Result<Outcome, String> {
    #[cfg(debug_assertions)]
    if let Some(src) = std::env::var_os("AGENTIC_SCREENSHOT_TEST_SCREENSHOT") {
        return test_source(src.into());
    }
    match limit {
        Some(d) => glib::future_with_timeout(d, request(interactive))
            .await
            .unwrap_or_else(|_| Err(tr("Portalen svarte ikke i tide").to_string())),
        None => request(interactive).await,
    }
}

async fn request(interactive: bool) -> Result<Outcome, String> {
    ensure_registered().await;
    let req = Screenshot::request()
        .interactive(interactive)
        .modal(false)
        .send()
        .await
        .map_err(|e| {
            tr_fmt(
                "Portalen svarte ikke: {error}",
                &[("error", &e.to_string())],
            )
        })?;
    match req.response() {
        Ok(shot) => {
            let uri = shot.uri().to_string();
            trace(&format!("portal: {uri}"));
            crate::storage::local_path_of_uri(&uri)
                .map(Outcome::File)
                .ok_or_else(|| tr("Portalen ga ikke en lokal fil").to_string())
        }
        Err(ashpd::Error::Response(ResponseError::Cancelled)) => Ok(Outcome::Cancelled),
        Err(ashpd::Error::Response(ResponseError::Other)) => {
            Ok(Outcome::Denied("Portalen avviste skjermbildet".to_string()))
        }
        Err(e) => Err(tr_fmt(
            "Skjermbildet feilet: {error}",
            &[("error", &e.to_string())],
        )),
    }
}

/// Testkilde for debug-bygg: en kopi av et PNG, slik portalen ville gitt en egen fil.
#[cfg(debug_assertions)]
fn test_source(src: PathBuf) -> Result<Outcome, String> {
    let tmp = std::env::temp_dir().join(format!(
        "agentic-screenshot-test-portal-{}.png",
        std::process::id()
    ));
    std::fs::copy(&src, &tmp)
        .map_err(|e| tr_fmt("Testkilde: {error}", &[("error", &e.to_string())]))?;
    Ok(Outcome::File(tmp))
}
