//! Innstillingsvinduet. Et eget toppnivåvindu i AdwPreferences-stil: en
//! `AdwPreferencesDialog` ville ha arvet størrelsen til hovedvinduet, som kan
//! være 47 piksler høyt.

use adw::prelude::*;
use gtk::{gdk, gio, glib};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::config::{CaptureMethod, Config, Theme, default_save_dir, pretty_dir};
use crate::extension::{ExtensionState, enable_extension, extension_state};
use crate::history;
use crate::hotkeys::{self, Action, KeyInput, Mods};
use crate::i18n::{Language, active_language, tr, tr_fmt, trc};
use crate::portal;
use crate::theme::apply_theme;

const CSS: &str = "
row.klipp-key-error .subtitle {
  color: var(--error-color);
  opacity: 1;
  font-weight: 600;
}
.klipp-signature {
  margin-top: 18px;
}
.klipp-signature-name {
  font-weight: 600;
  letter-spacing: 0.04em;
  opacity: 0.7;
}
.klipp-signature-note {
  opacity: 0.55;
}
";
/// Navnet på byggeserien appene hører til, likt i Agentic Desktop.
pub const SIGNATURE: &str = "Code Pro";

/// Endringer fra innstillingsvinduet. `Config` er allerede oppdatert og lagret.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsChange {
    Theme,
    Capture,
    Shortcuts,
    Restart,
}

/// Det innstillingene viser om selve appen.
#[derive(Debug, Clone, Copy)]
pub struct About {
    pub version: &'static str,
    pub build: &'static str,
    /// Shell-utvidelsen svarer, så bilder tas uten blits.
    pub silent_capture: bool,
}

struct KeyRow {
    action: Action,
    row: adw::ActionRow,
    keys: adw::ShortcutLabel,
    plain: gtk::Label,
}

struct State {
    config: Rc<RefCell<Config>>,
    on_change: Box<dyn Fn(SettingsChange)>,
    rows: RefCell<Vec<KeyRow>>,
    reset: RefCell<Option<gtk::Button>>,
    toasts: RefCell<Option<adw::ToastOverlay>>,
    listening: Cell<Option<Action>>,
    error: RefCell<Option<String>>,
}

impl State {
    fn render(&self) {
        let config = self.config.borrow();
        for key_row in self.rows.borrow().iter() {
            self.render_row(key_row, &config);
        }
        if let Some(reset) = &*self.reset.borrow() {
            reset.set_sensitive(!config.keys_are_default());
        }
    }

    fn render_row(&self, key_row: &KeyRow, config: &Config) {
        let KeyRow {
            action,
            row,
            keys,
            plain,
        } = key_row;
        let error = self.error.borrow();
        let listening = self.listening.get() == Some(*action);
        row.remove_css_class("klipp-key-error");
        plain.remove_css_class("accent");
        plain.remove_css_class("heading");
        plain.remove_css_class("dim-label");

        if listening {
            match &*error {
                Some(message) => {
                    row.add_css_class("klipp-key-error");
                    row.set_subtitle(message);
                }
                None => row.set_subtitle(tr("Esc avbryter · Backspace fjerner")),
            }
            plain.set_label(tr("Trykk ny snarvei …"));
            plain.add_css_class("accent");
            plain.add_css_class("heading");
            plain.set_visible(true);
            keys.set_visible(false);
            return;
        }

        row.set_subtitle(&match action {
            Action::Capture => portal::phase().text(),
            _ => String::new(),
        });
        match config.accel(*action) {
            Some(accel) => {
                keys.set_accelerator(&accel);
                keys.set_visible(true);
                plain.set_visible(false);
            }
            None => {
                plain.set_label(trc("shortcut-unassigned", "Ingen"));
                plain.add_css_class("dim-label");
                plain.set_visible(true);
                keys.set_visible(false);
            }
        }
    }

    fn cancel_listen(&self) {
        if self.listening.take().is_some() {
            self.error.take();
            self.render();
        }
    }

    fn persist(&self) -> bool {
        let saved = self.config.borrow().save();
        if let Err(message) = saved {
            eprintln!("klipp: {message}");
            if let Some(toasts) = &*self.toasts.borrow() {
                toasts.add_toast(adw::Toast::new(tr("Kunne ikke lagre innstillingene")));
            }
            return false;
        }
        true
    }

    fn store_key(&self, action: Action, combo: Option<String>) {
        {
            let mut config = self.config.borrow_mut();
            config.set_key(action, combo);
            if action == Action::Capture {
                config.hotkey_declined = None;
            }
        }
        self.persist();
        self.listening.set(None);
        self.error.take();
        self.render();
        (self.on_change)(SettingsChange::Shortcuts);
        if action == Action::Capture {
            portal::restart();
        }
    }

    fn reset_keys(&self) {
        let capture_changed = {
            let mut config = self.config.borrow_mut();
            let before = config.key(Action::Capture).map(str::to_owned);
            config.reset_keys();
            config.hotkey_declined = None;
            !before.is_some_and(|k| hotkeys::same_combo(&k, Action::Capture.default_key()))
        };
        self.persist();
        self.listening.set(None);
        self.error.take();
        self.render();
        (self.on_change)(SettingsChange::Shortcuts);
        if capture_changed {
            portal::restart();
        }
    }

    /// `true` hvis tastetrykket er tatt hånd om her.
    fn key_pressed(
        &self,
        window: &adw::Window,
        key: gdk::Key,
        keycode: u32,
        state: gdk::ModifierType,
    ) -> bool {
        let mods = Mods::from_gdk(state);
        let Some(action) = self.listening.get() else {
            if key == gdk::Key::Escape && mods.none() {
                window.close();
                return true;
            }
            return false;
        };
        let digit = base_digit(window, keycode);
        match hotkeys::classify_key(key, digit, mods) {
            KeyInput::Ignore => {}
            KeyInput::Cancel => self.cancel_listen(),
            KeyInput::Clear => self.store_key(action, None),
            KeyInput::Combo(combo) => {
                let checked = hotkeys::check_binding(&self.config.borrow(), action, &combo);
                match checked {
                    Ok(()) => self.store_key(action, Some(combo)),
                    Err(e) => {
                        self.error.replace(Some(e.message()));
                        self.render();
                    }
                }
            }
        }
        true
    }

    /// Bryter de strenge referansene til widgetene, ellers lever de evig.
    fn teardown(&self) {
        portal::set_listener(None);
        self.rows.borrow_mut().clear();
        self.reset.take();
        self.toasts.take();
    }
}

/// Sifferet på tastens fysiske plass, uansett Shift og tastaturoppsett.
fn base_digit(window: &adw::Window, keycode: u32) -> Option<char> {
    let entries = WidgetExt::display(window).map_keycode(keycode)?;
    entries
        .iter()
        .filter(|(key, _)| key.level() <= 1)
        .find_map(|(_, keyval)| keyval.to_unicode().filter(char::is_ascii_digit))
}

thread_local! {
    static OPEN: RefCell<glib::WeakRef<adw::Window>> = RefCell::new(glib::WeakRef::new());
    static CSS_LOADED: Cell<bool> = const { Cell::new(false) };
}

fn load_css(window: &adw::Window) {
    if CSS_LOADED.replace(true) {
        return;
    }
    let provider = gtk::CssProvider::new();
    provider.load_from_string(CSS);
    gtk::style_context_add_provider_for_display(
        &WidgetExt::display(window),
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

/// Åpner innstillingene, eller henter frem vinduet hvis det allerede er åpent.
pub fn open_settings(
    app: &adw::Application,
    config: Rc<RefCell<Config>>,
    about: About,
    on_change: impl Fn(SettingsChange) + 'static,
) {
    if let Some(window) = OPEN.with_borrow(glib::WeakRef::upgrade) {
        window.present();
        return;
    }

    let state = Rc::new(State {
        config,
        on_change: Box::new(on_change),
        rows: RefCell::new(Vec::new()),
        reset: RefCell::new(None),
        toasts: RefCell::new(None),
        listening: Cell::new(None),
        error: RefCell::new(None),
    });

    let restart = adw::Banner::new(tr("Språket tas i bruk når Klipp startes på nytt"));
    // Oversettere: Keep this settings banner button short.
    restart.set_button_label(Some(tr("Start på nytt")));
    restart.set_revealed(state.config.borrow().language.resolve() != active_language());
    let on_restart = Rc::downgrade(&state);
    restart.connect_button_clicked(move |_| {
        if let Some(state) = on_restart.upgrade() {
            (state.on_change)(SettingsChange::Restart);
        }
    });

    let page = adw::PreferencesPage::new();
    page.add(&appearance_group(&state, &restart));
    page.add(&capture_group(&state, about));
    page.add(&shortcuts_group(&state));
    page.add(&about_group(about));

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&page));
    toolbar.add_bottom_bar(&restart);

    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&toolbar));
    state.toasts.replace(Some(toasts.clone()));

    let window = adw::Window::builder()
        .application(app)
        .title(tr("Innstillinger"))
        .default_width(540)
        .default_height(660)
        .width_request(360)
        .height_request(320)
        .content(&toasts)
        .build();
    load_css(&window);
    connect_window(&window, &state);

    let listener: Rc<dyn Fn()> = {
        let state = state.clone();
        Rc::new(move || state.render())
    };
    portal::set_listener(Some(listener));

    state.render();
    OPEN.with_borrow(|open| open.set(Some(&window)));
    window.present();
}

fn connect_window(window: &adw::Window, state: &Rc<State>) {
    // Opptak går foran alt annet, ellers ville Esc og Tab ha fyrt i vinduet.
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let weak = window.downgrade();
    let on_key = state.clone();
    keys.connect_key_pressed(move |_, key, keycode, modifiers| match weak.upgrade() {
        Some(window) if on_key.key_pressed(&window, key, keycode, modifiers) => {
            glib::Propagation::Stop
        }
        _ => glib::Propagation::Proceed,
    });
    window.add_controller(keys);

    // Klikk utenfor raden som lytter avbryter, som i prototypen.
    let click = gtk::GestureClick::new();
    click.set_propagation_phase(gtk::PropagationPhase::Capture);
    let on_click = state.clone();
    let weak = window.downgrade();
    click.connect_pressed(move |_, _, x, y| {
        let (Some(action), Some(window)) = (on_click.listening.get(), weak.upgrade()) else {
            return;
        };
        let target = window.pick(x, y, gtk::PickFlags::DEFAULT);
        let on_row = on_click.rows.borrow().iter().any(|r| {
            r.action == action
                && target
                    .as_ref()
                    .is_some_and(|t| *t == r.row || t.is_ancestor(&r.row))
        });
        if !on_row {
            on_click.cancel_listen();
        }
    });
    window.add_controller(click);

    let on_active = state.clone();
    window.connect_is_active_notify(move |window| {
        if !window.is_active() {
            on_active.cancel_listen();
        }
    });

    let on_close = state.clone();
    window.connect_close_request(move |_| {
        on_close.cancel_listen();
        on_close.teardown();
        glib::Propagation::Proceed
    });
}

fn appearance_group(state: &Rc<State>, restart: &adw::Banner) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title(tr("Utseende"))
        .build();
    let toggles = adw::ToggleGroup::builder()
        .valign(gtk::Align::Center)
        .build();
    for theme in Theme::ALL {
        toggles.add(
            adw::Toggle::builder()
                .name(theme.id())
                .label(theme.label())
                .build(),
        );
    }
    toggles.set_active_name(Some(state.config.borrow().theme.id()));

    let on_theme = state.clone();
    toggles.connect_active_name_notify(move |toggles| {
        let Some(theme) = toggles.active_name().and_then(|n| Theme::from_id(&n)) else {
            return;
        };
        if std::mem::replace(&mut on_theme.config.borrow_mut().theme, theme) == theme {
            return;
        }
        apply_theme(theme);
        on_theme.persist();
        (on_theme.on_change)(SettingsChange::Theme);
    });

    let row = adw::ActionRow::builder().title(tr("Tema")).build();
    row.add_suffix(&toggles);
    group.add(&row);
    group.add(&language_row(state, restart));
    group
}

fn language_row(state: &Rc<State>, restart: &adw::Banner) -> adw::ComboRow {
    let labels = gtk::StringList::new(&[]);
    // Oversettere: {language} is the system language's endonym, e.g. Norsk bokmål.
    labels.append(&tr_fmt(
        "Systemspråk ({language})",
        &[("language", Language::System.endonym())],
    ));
    for language in Language::ALL.into_iter().skip(1) {
        labels.append(language.endonym());
    }
    let selected = Language::ALL
        .iter()
        .position(|&language| language == state.config.borrow().language);
    let row = adw::ComboRow::builder()
        .title(tr("Språk"))
        .model(&labels)
        .selected(selected.unwrap_or_default() as u32)
        .build();
    let (state, restart) = (state.clone(), restart.clone());
    row.connect_selected_notify(move |row| {
        let Some(&language) = Language::ALL.get(row.selected() as usize) else {
            return;
        };
        let previous = std::mem::replace(&mut state.config.borrow_mut().language, language);
        if previous == language {
            return;
        }
        if state.persist() {
            restart.set_revealed(true);
        } else {
            state.config.borrow_mut().language = previous;
            if let Some(idx) = Language::ALL
                .iter()
                .position(|&language| language == previous)
            {
                row.set_selected(idx as u32);
            }
        }
    });
    row
}

/// Stille opptak krever utvidelsen, så blitsen må kunne forklares og fjernes her.
fn extension_row(about: About) -> adw::ActionRow {
    let current = extension_state(about.silent_capture);
    let row = adw::ActionRow::builder()
        .title(tr("Klipp-utvidelsen"))
        .subtitle(current.text())
        .build();
    let enable = gtk::Button::builder()
        .label(tr("Slå på"))
        .valign(gtk::Align::Center)
        .css_classes(["suggested-action"])
        .visible(current == ExtensionState::Off)
        .build();
    let shown = row.clone();
    enable.connect_clicked(move |button| {
        if enable_extension() {
            shown.set_subtitle(ExtensionState::NeedsLogin.text());
            button.set_visible(false);
        } else {
            shown.set_subtitle(tr("GNOME godtok ikke endringen"));
        }
    });
    row.add_suffix(&enable);
    row
}

/// Opptaksmåte, med blits og lyd som bare betyr noe for stille opptak. GNOMEs vei har
/// dem alltid, så der er bryterne ute av spill.
fn method_rows(state: &Rc<State>, group: &adw::PreferencesGroup) {
    let (method, flash, sound) = {
        let config = state.config.borrow();
        (config.capture_method, config.flash, config.sound)
    };
    let toggles = adw::ToggleGroup::builder()
        .valign(gtk::Align::Center)
        .build();
    for m in CaptureMethod::ALL {
        toggles.add(adw::Toggle::builder().name(m.id()).label(m.label()).build());
    }
    toggles.set_active_name(Some(method.id()));
    let row = adw::ActionRow::builder()
        .title(tr("Opptaksmåte"))
        .subtitle(method.hint())
        .build();
    row.add_suffix(&toggles);
    group.add(&row);

    let silent = method == CaptureMethod::Silent;
    let flash_row = switch_row(
        state,
        tr("Blits"),
        tr("Lyser opp området når bildet er tatt"),
        flash,
        |config, on| config.flash = on,
    );
    let sound_row = switch_row(
        state,
        tr("Lukkerlyd"),
        tr("Spiller GNOMEs lyd når bildet er tatt"),
        sound,
        |config, on| config.sound = on,
    );
    flash_row.set_sensitive(silent);
    sound_row.set_sensitive(silent);
    group.add(&flash_row);
    group.add(&sound_row);

    let state = state.clone();
    toggles.connect_active_name_notify(move |toggles| {
        let Some(method) = toggles
            .active_name()
            .and_then(|n| CaptureMethod::from_id(&n))
        else {
            return;
        };
        state.config.borrow_mut().capture_method = method;
        row.set_subtitle(method.hint());
        let silent = method == CaptureMethod::Silent;
        flash_row.set_sensitive(silent);
        sound_row.set_sensitive(silent);
        state.persist();
        (state.on_change)(SettingsChange::Capture);
    });
}

fn save_dir_row(state: &Rc<State>) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(tr("Lagre skjermbilder i"))
        .subtitle(pretty_dir(&state.config.borrow().save_dir()))
        .subtitle_selectable(true)
        .build();
    let reset = gtk::Button::builder()
        .label(trc("save-folder-reset", "Standard"))
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .visible(state.config.borrow().custom_save_dir.is_some())
        .build();
    let choose = gtk::Button::builder()
        .label(tr("Velg mappe"))
        .valign(gtk::Align::Center)
        .build();

    let apply: Rc<dyn Fn(Option<std::path::PathBuf>)> = {
        let (state, row, reset) = (state.clone(), row.clone(), reset.clone());
        Rc::new(move |dir| {
            reset.set_visible(dir.is_some());
            state.config.borrow_mut().custom_save_dir = dir;
            row.set_subtitle(&pretty_dir(&state.config.borrow().save_dir()));
            state.persist();
            (state.on_change)(SettingsChange::Capture);
        })
    };
    let to_default = apply.clone();
    reset.connect_clicked(move |_| to_default(None));

    let (config, shown) = (state.config.clone(), state.clone());
    choose.connect_clicked(move |button| {
        let dialog = gtk::FileDialog::builder()
            .title(tr("Velg mappe for skjermbilder"))
            .accept_label(trc("folder-chooser", "Velg"))
            .modal(true)
            .build();
        let start = config.borrow().save_dir();
        if start.is_dir() {
            dialog.set_initial_folder(Some(&gio::File::for_path(&start)));
        } else if let Some(parent) = default_save_dir().parent() {
            dialog.set_initial_folder(Some(&gio::File::for_path(parent)));
        }
        let parent = button.root().and_downcast::<gtk::Window>();
        let (apply, shown) = (apply.clone(), shown.clone());
        dialog.select_folder(parent.as_ref(), gio::Cancellable::NONE, move |picked| {
            let Some(dir) = picked.ok().and_then(|f| f.path()) else {
                return;
            };
            // Innstillingene lagres linje for linje, så stien må være vanlig tekst.
            if dir.to_str().is_none_or(|d| d.contains('\n')) {
                if let Some(toasts) = &*shown.toasts.borrow() {
                    toasts.add_toast(adw::Toast::new(tr("Klipp kan ikke bruke den mappen")));
                }
                return;
            }
            apply(Some(dir));
        });
    });
    row.add_suffix(&reset);
    row.add_suffix(&choose);
    row
}

fn about_group(about: About) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title(tr("Om Klipp"))
        .build();
    for (title, value) in [(tr("Versjon"), about.version), (tr("Bygg"), about.build)] {
        let label = gtk::Label::builder()
            .label(value)
            .selectable(true)
            .css_classes(["dim-label", "numeric"])
            .build();
        let row = adw::ActionRow::builder().title(title).build();
        row.add_suffix(&label);
        group.add(&row);
    }
    let history = adw::ActionRow::builder()
        .title(tr("Versjonslogg"))
        .subtitle(tr("Endringer fra første til nyeste versjon"))
        .activatable(true)
        .build();
    history.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    history.connect_activated(move |row| {
        if let Some(window) = row.root().and_downcast::<adw::Window>() {
            history::open(&window, about);
        }
    });
    group.add(&history);
    group.add(&signature());
    group
}

fn signature() -> gtk::Box {
    let name = gtk::Label::builder()
        .label(SIGNATURE)
        .css_classes(["klipp-signature-name"])
        .build();
    let note = gtk::Label::builder()
        .label(tr("Navnet på byggeserien denne versjonen hører til"))
        .wrap(true)
        .justify(gtk::Justification::Center)
        .css_classes(["caption", "klipp-signature-note"])
        .build();
    let b = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .halign(gtk::Align::Center)
        .css_classes(["klipp-signature"])
        .build();
    b.append(&name);
    b.append(&note);
    b
}

fn capture_group(state: &Rc<State>, about: About) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title(tr("Skjermbilde"))
        .build();
    method_rows(state, &group);
    group.add(&extension_row(about));
    group.add(&save_dir_row(state));
    let (instant, auto_copy) = {
        let config = state.config.borrow();
        (config.instant_capture, config.auto_copy)
    };
    group.add(&switch_row(
        state,
        tr("Ta bildet med en gang"),
        tr("Bildet tas når du slipper musa, uten bekreftelse"),
        instant,
        |config, on| config.instant_capture = on,
    ));
    group.add(&switch_row(
        state,
        tr("Kopier automatisk"),
        tr("Legger hvert nye bilde på utklippstavlen"),
        auto_copy,
        |config, on| config.auto_copy = on,
    ));
    group
}

fn switch_row(
    state: &Rc<State>,
    title: &str,
    subtitle: &str,
    active: bool,
    apply: impl Fn(&mut Config, bool) + 'static,
) -> adw::SwitchRow {
    let row = adw::SwitchRow::builder()
        .title(title)
        .subtitle(subtitle)
        .active(active)
        .build();
    let state = state.clone();
    row.connect_active_notify(move |row| {
        apply(&mut state.config.borrow_mut(), row.is_active());
        state.persist();
        (state.on_change)(SettingsChange::Capture);
    });
    row
}

fn shortcuts_group(state: &Rc<State>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title(tr("Hurtigtaster"))
        .build();

    let reset = gtk::Button::builder()
        .label(tr("Tilbakestill"))
        .valign(gtk::Align::Center)
        .build();
    let on_reset = state.clone();
    reset.connect_clicked(move |_| on_reset.reset_keys());
    group.set_header_suffix(Some(&reset));
    state.reset.replace(Some(reset));

    for action in Action::ALL {
        let row = adw::ActionRow::builder()
            .title(action.label())
            .activatable(true)
            .use_markup(false)
            .build();
        let keys = adw::ShortcutLabel::new("");
        keys.set_valign(gtk::Align::Center);
        let plain = gtk::Label::builder().valign(gtk::Align::Center).build();
        row.add_suffix(&keys);
        row.add_suffix(&plain);

        let on_activate = state.clone();
        row.connect_activated(move |_| {
            on_activate.listening.set(Some(action));
            on_activate.error.take();
            on_activate.render();
        });
        group.add(&row);
        state.rows.borrow_mut().push(KeyRow {
            action,
            row,
            keys,
            plain,
        });
    }
    group
}
