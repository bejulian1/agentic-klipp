//! Global hurtigtast via GlobalShortcuts-portalen.
//!
//! Slik virker det på GNOME 50, lest ut av kildekoden til backenden og
//! gnome-control-center:
//!
//! - En vertsapp (ikke Flatpak) må kalle `org.freedesktop.host.portal.Registry.Register`
//!   før noe annet portalkall på tilkoblingen, og app-ID-en må ha en installert
//!   `.desktop`-fil. Derfor har hurtigtasten sin egen D-Bus-tilkobling: da spiller
//!   det ingen rolle hva resten av appen allerede har sagt til portalen.
//! - `BindShortcuts` viser GNOMEs dialog bare for ID-er som ikke er lagret fra før
//!   (dconf: `/org/gnome/settings-daemon/global-shortcuts/<app-id>/`). Samme ID ved
//!   neste start er stille og gir den lagrede tasten, så den må bindes ved hver start.
//! - Bindingen lever så lenge økten: når tilkoblingen lukkes, slipper GNOME tasten.
//! - `preferred_trigger` leses av GNOME med `gtk_accelerator_parse`, ikke som
//!   `LOGO+SHIFT+k` fra XDG-formatet. Derfor sendes GTK-format.
//! - Portal v1 har ikke `ConfigureShortcuts`. For å få GNOME til å bekrefte en ny
//!   tast inneholder shortcut-ID-en selve tasten, så en ny tast gir en ny dialog.
//! - Dør bare `xdg-desktop-portal`, beholder backenden den gamle økten og tasten
//!   sin. Ny binding melder da suksess, men tasten fyrer ikke før ny innlogging.
//!   Dør begge, virker den automatiske nye bindingen.

use ashpd::desktop::global_shortcuts::{
    BindShortcutsOptions, GlobalShortcuts, NewShortcut, Shortcut,
};
use ashpd::desktop::{CreateSessionOptions, ResponseError};
use ashpd::zbus;
use ashpd::{AppID, Error, PortalError};
use futures_channel::oneshot;
use futures_util::{FutureExt, StreamExt, select_biased};
use gtk::glib;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::config::Config;
use crate::hotkeys::{Action, Mods, combo_text, key_label, same_combo};

const PORTAL: &str = "org.freedesktop.portal.Desktop";
const DESCRIPTION: &str = "Ta skjermbilde";

/// Status for global hurtigtast, vist i innstillingene.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotkeyStatus {
    Active,
    Unavailable(String),
}

/// Det innstillingene viser. Rikere enn `HotkeyStatus`, som hovedappen får.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Phase {
    NotStarted,
    Connecting,
    Active { trigger: String },
    Unavailable(String),
}

impl Phase {
    pub(crate) fn text(&self) -> String {
        match self {
            Phase::NotStarted => "Ikke aktiv: global hurtigtast er ikke startet".to_string(),
            Phase::Connecting => "GNOME bekrefter hurtigtasten selv. Svar i dialogen".to_string(),
            Phase::Active { trigger } if trigger.is_empty() => {
                "Aktiv via GlobalShortcuts-portalen".to_string()
            }
            Phase::Active { trigger } => format!("Aktiv: GNOME bruker {trigger}"),
            Phase::Unavailable(why) => format!("Ikke aktiv: {why}"),
        }
    }
}

struct Runner {
    config: Rc<RefCell<Config>>,
    on_activate: Rc<dyn Fn()>,
    on_status: Rc<dyn Fn(HotkeyStatus)>,
    stop: Option<oneshot::Sender<()>>,
    task: Option<glib::JoinHandle<()>>,
    generation: Rc<Cell<u64>>,
}

thread_local! {
    static RUNNER: RefCell<Option<Runner>> = const { RefCell::new(None) };
    static PHASE: RefCell<Phase> = const { RefCell::new(Phase::NotStarted) };
    static LISTENER: RefCell<Option<Rc<dyn Fn()>>> = const { RefCell::new(None) };
}

pub(crate) fn phase() -> Phase {
    PHASE.with_borrow(Clone::clone)
}

/// Innstillingsvinduet lytter her. Det finnes bare ett om gangen.
pub(crate) fn set_listener(listener: Option<Rc<dyn Fn()>>) {
    LISTENER.set(listener);
}

fn set_phase(phase: Phase, on_status: &dyn Fn(HotkeyStatus)) {
    let public = match &phase {
        Phase::Active { .. } => Some(HotkeyStatus::Active),
        Phase::Unavailable(why) => Some(HotkeyStatus::Unavailable(why.clone())),
        Phase::NotStarted | Phase::Connecting => None,
    };
    PHASE.set(phase);
    if let Some(status) = public {
        on_status(status);
    }
    if let Some(listener) = LISTENER.with_borrow(Clone::clone) {
        listener();
    }
}

/// Starter global hurtigtast for capture. `on_activate` kommer på GTK-hovedtråden.
/// Kan kalles på nytt, og binder da tasten fra `config` på nytt.
pub fn start_hotkey(
    config: Rc<RefCell<Config>>,
    on_activate: impl Fn() + 'static,
    on_status: impl Fn(HotkeyStatus) + 'static,
) {
    RUNNER.with_borrow_mut(|runner| {
        let (stop, task, generation) = match runner.take() {
            Some(old) => (old.stop, old.task, old.generation),
            None => (None, None, Rc::new(Cell::new(0))),
        };
        *runner = Some(Runner {
            config,
            on_activate: Rc::new(on_activate),
            on_status: Rc::new(on_status),
            stop,
            task,
            generation,
        });
    });
    restart();
}

/// Lukk en eventuell økt og bind tasten fra config på nytt.
pub(crate) fn restart() {
    RUNNER.with_borrow_mut(|runner| {
        let Some(runner) = runner else { return };
        if let Some(stop) = runner.stop.take() {
            let _ = stop.send(());
        }
        runner.generation.set(runner.generation.get() + 1);
        let job = Job {
            config: runner.config.clone(),
            on_activate: runner.on_activate.clone(),
            on_status: runner.on_status.clone(),
            generation: runner.generation.clone(),
            mine: runner.generation.get(),
        };
        let (stop_tx, stop_rx) = oneshot::channel();
        let previous = runner.task.take();
        runner.stop = Some(stop_tx);
        runner.task = Some(glib::spawn_future_local(async move {
            if let Some(previous) = previous {
                let _ = previous.await;
            }
            job.run(stop_rx).await;
        }));
    });
}

/// Shell-utvidelsen holder tasten nå. Slipper portaløkten, så samme trykk ikke tar
/// to bilder, og lar innstillingene vise tasten. `start_hotkey` tar den tilbake.
pub fn hotkey_handed_over(combo: Option<&str>) {
    RUNNER.with_borrow_mut(|runner| {
        if let Some(mut old) = runner.take() {
            old.generation.set(old.generation.get() + 1);
            if let Some(stop) = old.stop.take() {
                let _ = stop.send(());
            }
        }
    });
    let phase = match combo {
        Some(combo) => Phase::Active {
            trigger: combo.to_string(),
        },
        None => Phase::Unavailable("ingen tast er valgt for Ta skjermbilde".to_string()),
    };
    set_phase(phase, &|_| {});
}

struct Job {
    config: Rc<RefCell<Config>>,
    on_activate: Rc<dyn Fn()>,
    on_status: Rc<dyn Fn(HotkeyStatus)>,
    generation: Rc<Cell<u64>>,
    mine: u64,
}

enum Ended {
    /// Innstillingene ba om ny binding.
    Stopped,
    /// Dialogen ble lukket uten at brukeren godtok tasten.
    Refused,
    PortalRestarted,
}

impl Job {
    fn current(&self) -> bool {
        self.generation.get() == self.mine
    }

    fn report(&self, phase: Phase) {
        if self.current() {
            set_phase(phase, self.on_status.as_ref());
        }
    }

    async fn run(self, mut stop: oneshot::Receiver<()>) {
        let (combo, accel, declined) = {
            let config = self.config.borrow();
            let combo = config.key(Action::Capture).map(str::to_owned);
            let accel = config.accel(Action::Capture);
            let declined = match (&config.hotkey_declined, &combo) {
                (Some(no), Some(combo)) => same_combo(no, combo),
                _ => false,
            };
            (combo, accel, declined)
        };
        let (Some(combo), Some(accel)) = (combo, accel) else {
            self.report(Phase::Unavailable(
                "ingen tast er valgt for Ta skjermbilde".to_string(),
            ));
            return;
        };
        if declined {
            self.report(Phase::Unavailable(refused(&combo)));
            return;
        }
        self.report(Phase::Connecting);
        match self.bind_and_listen(&combo, &accel, &mut stop).await {
            Ok(Ended::Stopped) => {}
            Ok(Ended::Refused) => {
                self.remember_refusal(&combo);
                self.report(Phase::Unavailable(refused(&combo)));
            }
            Ok(Ended::PortalRestarted) => {
                self.report(Phase::Unavailable(
                    "portalen startet på nytt, prøver igjen".to_string(),
                ));
                glib::timeout_future_seconds(2).await;
                if self.current() {
                    restart();
                }
            }
            Err(why) => self.report(Phase::Unavailable(why)),
        }
    }

    fn remember_refusal(&self, combo: &str) {
        let saved = {
            let mut config = self.config.borrow_mut();
            config.hotkey_declined = Some(combo.to_string());
            config.save()
        };
        if let Err(e) = saved {
            eprintln!("klipp: {e}");
        }
    }

    /// Holder økten så lenge tasten skal være bundet. Hele løpet lytter etter
    /// `stop` og portalomstart, også mens GNOMEs dialog står åpen.
    async fn bind_and_listen(
        &self,
        combo: &str,
        accel: &str,
        stop: &mut oneshot::Receiver<()>,
    ) -> Result<Ended, String> {
        let conn = zbus::Connection::session()
            .await
            .map_err(|e| format!("får ikke kontakt med D-Bus ({e})"))?;
        let app_id = AppID::try_from(crate::APP_ID).map_err(|e| format!("ugyldig app-ID ({e})"))?;
        ashpd::register_host_app_with_connection(conn.clone(), app_id)
            .await
            .map_err(|e| explain_register(&e))?;

        let dbus = zbus::fdo::DBusProxy::new(&conn)
            .await
            .map_err(|e| format!("får ikke fulgt med på portalen ({e})"))?;
        let mut owner = dbus
            .receive_name_owner_changed_with_args(&[(0, PORTAL)])
            .await
            .map_err(|e| format!("får ikke fulgt med på portalen ({e})"))?
            .fuse();

        let portal = GlobalShortcuts::with_connection(conn.clone())
            .await
            .map_err(|e| explain_portal(&e))?;
        let session = portal
            .create_session(CreateSessionOptions::default())
            .await
            .map_err(|e| explain_portal(&e))?;

        // Abonner før bindingen, så ingen hendelser går tapt.
        let subscribed = async {
            Ok::<_, Error>((
                portal.receive_activated().await?,
                portal.receive_shortcuts_changed().await?,
                session.receive_closed().await?,
            ))
        };
        let (activated, changed, closed) = subscribed.await.map_err(|e| explain_portal(&e))?;
        let mut activated = activated.fuse();
        let mut changed = changed.fuse();
        let closed = closed.fuse();
        let mut closed = std::pin::pin!(closed);

        let id = shortcut_id(accel);
        let shortcut = NewShortcut::new(id.clone(), DESCRIPTION).preferred_trigger(accel);
        let shortcuts = [shortcut];
        let bind = async {
            portal
                .bind_shortcuts(&session, &shortcuts, None, BindShortcutsOptions::default())
                .await?
                .response()
        };
        let bind = bind.fuse();
        let mut bind = std::pin::pin!(bind);

        // Svaret kommer først når brukeren har svart på dialogen.
        let response = select_biased! {
            _ = &mut *stop => {
                let _ = session.close().await;
                return Ok(Ended::Stopped);
            }
            _ = owner.next() => return Ok(Ended::PortalRestarted),
            response = bind => response,
        };
        let bound = match response {
            Ok(bound) => bound,
            // GNOME svarer «annet» og ikke «avbrutt» når dialogen lukkes.
            Err(Error::Response(ResponseError::Cancelled | ResponseError::Other)) => {
                let _ = session.close().await;
                return Ok(Ended::Refused);
            }
            Err(e) => return Err(explain_portal(&e)),
        };
        let Some(trigger) = trigger_of(bound.shortcuts(), &id) else {
            return Err(format!(
                "GNOME bandt ikke {combo}, sjekk Innstillinger > Tastatur"
            ));
        };
        self.report(Phase::Active { trigger });

        loop {
            select_biased! {
                _ = &mut *stop => {
                    let _ = session.close().await;
                    return Ok(Ended::Stopped);
                }
                signal = activated.next() => {
                    let Some(signal) = signal else {
                        return Err("portalen sluttet å svare".to_string());
                    };
                    if signal.shortcut_id() == id {
                        (self.on_activate)();
                    }
                }
                signal = changed.next() => {
                    if let Some(signal) = signal
                        && let Some(trigger) = trigger_of(signal.shortcuts(), &id)
                    {
                        self.report(Phase::Active { trigger });
                    }
                }
                _ = closed.next() => {
                    return Err("GNOME lukket økten for hurtigtasten".to_string());
                }
                _ = owner.next() => return Ok(Ended::PortalRestarted),
            }
        }
    }
}

fn refused(combo: &str) -> String {
    format!("GNOME godtok ikke {combo}. Trykk tasten her på nytt for å bli spurt igjen")
}

/// Tasten GNOME faktisk har bundet. GNOME beskriver den med en oversatt setning
/// som `Trykk på <Shift><Super>k`, så vi plukker ut akseleratoren og viser den
/// slik resten av Klipp viser taster.
fn trigger_of(shortcuts: &[Shortcut], id: &str) -> Option<String> {
    let description = shortcuts
        .iter()
        .find(|s| s.id() == id)?
        .trigger_description();
    let readable = description
        .split_whitespace()
        .next_back()
        .and_then(gtk::accelerator_parse)
        .and_then(|(key, mods)| Some(combo_text(Mods::from_gdk(mods), &key_label(key)?)));
    Some(readable.unwrap_or_else(|| description.to_string()))
}

/// GNOME spør bare om ukjente ID-er, så ny tast må gi ny ID.
pub(crate) fn shortcut_id(accel: &str) -> String {
    let slug: String = accel
        .chars()
        .map(|c| match c {
            '<' => '-',
            c if c.is_ascii_alphanumeric() => c.to_ascii_lowercase(),
            _ => '-',
        })
        .collect();
    let slug = slug.split('-').filter(|p| !p.is_empty());
    let mut id = String::from("capture");
    for part in slug {
        id.push('-');
        id.push_str(part);
    }
    id
}

fn explain_register(e: &Error) -> String {
    let text = e.to_string();
    if text.contains("App info not found") {
        eprintln!("klipp: {text}");
        format!(
            "Klipp er ikke installert: {}.desktop mangler. Kjør install.sh",
            crate::APP_ID
        )
    } else if matches!(e, Error::PortalNotFound(_)) || text.contains("Unknown") {
        format!("portalen kan ikke registrere vertsapper ({text})")
    } else {
        format!("portalen godtok ikke Klipp ({text})")
    }
}

fn explain_portal(e: &Error) -> String {
    match e {
        Error::PortalNotFound(_) => "GlobalShortcuts-portalen finnes ikke her".to_string(),
        Error::RequiresVersion(need, have) => {
            format!("portalen er for gammel (trenger v{need}, har v{have})")
        }
        Error::Portal(PortalError::NotAllowed(why)) => format!("portalen nekter ({why})"),
        other => format!("portalen feilet ({other})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcut_id_follows_the_key() {
        assert_eq!(shortcut_id("<Shift><Super>k"), "capture-shift-super-k");
        assert_eq!(shortcut_id("<Primary><Alt>F5"), "capture-primary-alt-f5");
        assert_ne!(shortcut_id("<Super>k"), shortcut_id("<Super>j"));
    }

    #[test]
    fn status_text_is_honest() {
        let active = Phase::Active {
            trigger: "Super+Shift+K".into(),
        };
        assert_eq!(active.text(), "Aktiv: GNOME bruker Super+Shift+K");
        assert!(
            Phase::Unavailable("x".into())
                .text()
                .starts_with("Ikke aktiv")
        );
        assert!(Phase::NotStarted.text().starts_with("Ikke aktiv"));
    }
}
