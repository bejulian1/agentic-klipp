//! Hovedvinduet: variant C «Stripe», og kompaktmodus med raden etter capture.

use crate::app::{Klipp, count_text};
use crate::capture::{
    self,
    geometry::{Mode, mode_icon, mode_label},
};
use crate::keys;
use crate::shelf::Shelf;
use adw::prelude::*;
use gtk::{gdk, glib};
use klipp_system::i18n::{tr, tr_fmt, trc, trn};
use klipp_system::{Action, Config};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

const STRIP_W: i32 = 236;
const MIN_W: i32 = 200;
const MIN_H: i32 = 40;
const STRIP_H: i32 = 560;
const COMPACT_W: i32 = 380;
const TOP_BAR: i32 = 32;
const DOCK: i32 = 56;
/// Bunnlinja glir mellom vanlig linje og handlingslinje.
const BAR_SLIDE_MS: u32 = 180;

pub struct MainUi {
    pub window: adw::ApplicationWindow,
    toasts: adw::ToastOverlay,
    header: adw::HeaderBar,
    split: adw::SplitButton,
    ny: gtk::Button,
    inline: adw::ToggleGroup,
    compact_btn: gtk::Button,
    content: gtk::Box,
    body: gtk::Box,
    stack: gtk::Stack,
    empty_plain: gtk::Label,
    empty_keyed: gtk::Label,
    bottom: gtk::Box,
    bar_stack: gtk::Stack,
    shelf_count: gtk::Label,
    sel_count: gtk::Label,
    sel_copy: gtk::Button,
    sel_trash: gtk::Button,
    clear_btn: gtk::Button,
    mode_rows: Vec<(Mode, gtk::ToggleButton, gtk::Label)>,
    /// Holder øye med knappene i vinduskontrollene så tipsene følger med.
    _control_buttons: gtk::gio::ListModel,
    revealer: gtk::Revealer,
    rev_thumb: gtk::Picture,
    rev_dims: gtk::Label,
    rev_status: gtk::Label,
    rev_timer: RefCell<Option<glib::SourceId>>,
    rev_path: RefCell<Option<std::path::PathBuf>>,
    syncing: Cell<bool>,
    /// Minimert med knappen. Wayland sier ikke fra før vinduet blir suspendert,
    /// og det tar noen sekunder.
    pub minimized: Rc<Cell<bool>>,
}

fn strip_height() -> i32 {
    let h = gdk::Display::default()
        .and_then(|d| d.monitors().item(0).and_downcast::<gdk::Monitor>())
        .map_or(STRIP_H, |m| m.geometry().height());
    STRIP_H.min(h - TOP_BAR - DOCK - 24).max(320)
}

fn clear_tip(cfg: &Config) -> String {
    let dir = klipp_system::pretty_dir(&cfg.save_dir());
    // Oversettere: {path} is the screenshot folder, where the files stay after clearing the shelf.
    tr_fmt(
        "Tøm hylla\nFilene blir liggende i {path}",
        &[("path", &dir)],
    )
}

fn icon_button(icon: &str, tip: &str) -> gtk::Button {
    gtk::Button::builder()
        .icon_name(icon)
        .tooltip_text(tip)
        .css_classes(["flat"])
        .build()
}

impl MainUi {
    pub fn build(app: &adw::Application, shelf: &Rc<Shelf>, cfg: &Config) -> Rc<Self> {
        // Modus-popover, som i prototypen: ikon, navn og hurtigtast.
        let popover = gtk::Popover::new();
        let rows_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        rows_box.set_size_request(198, -1);
        let mut mode_rows = Vec::new();
        let mut group: Option<gtk::ToggleButton> = None;
        for m in Mode::ALL {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
            row.append(&gtk::Image::from_icon_name(mode_icon(m)));
            row.append(&gtk::Label::new(Some(mode_label(m))));
            let accel = gtk::Label::builder()
                .hexpand(true)
                .xalign(1.0)
                .css_classes(["dim-label", "caption"])
                .build();
            row.append(&accel);
            let b = gtk::ToggleButton::builder()
                .child(&row)
                .css_classes(["flat", "klipp-mode-row"])
                .build();
            if let Some(g) = &group {
                b.set_group(Some(g));
            } else {
                group = Some(b.clone());
            }
            let pop = popover.clone();
            b.connect_clicked(move |_| {
                Klipp::get().set_mode(m);
                pop.popdown();
            });
            rows_box.append(&b);
            mode_rows.push((m, b, accel));
        }
        popover.set_child(Some(&rows_box));

        let split = adw::SplitButton::builder()
            // Oversettere: Keep the narrow header button label short. It starts a new screenshot.
            .label(trc("new-screenshot-button", "Ny"))
            .popover(&popover)
            .dropdown_tooltip(tr("Velg modus"))
            .valign(gtk::Align::Center)
            .css_classes(["suggested-action"])
            .build();
        split.connect_clicked(|_| capture::start(&Klipp::get(), None));

        let ny = gtk::Button::builder()
            .label(trc("new-screenshot-button", "Ny"))
            .css_classes(["suggested-action"])
            .build();
        ny.connect_clicked(|_| capture::start(&Klipp::get(), None));

        let inline = adw::ToggleGroup::new();
        for m in Mode::ALL {
            inline.add(
                adw::Toggle::builder()
                    .name(m.id())
                    .icon_name(mode_icon(m))
                    .build(),
            );
        }

        let compact_btn = icon_button("klipp-shelf-symbolic", tr("Skjul hylla"));
        compact_btn.connect_clicked(|_| {
            let k = Klipp::get();
            let on = !k.config.borrow().compact;
            k.set_compact(on);
        });
        let settings_btn = icon_button("klipp-gear-symbolic", tr("Innstillinger"));
        settings_btn.connect_clicked(|_| Klipp::get().open_settings());

        // Ekte vinduskontroller: minimer, og lukk som skjuler til systemfeltet.
        let controls = gtk::WindowControls::builder()
            .side(gtk::PackType::End)
            .decoration_layout(":minimize,close")
            .build();
        let header = adw::HeaderBar::builder()
            .show_title(false)
            .show_start_title_buttons(false)
            .show_end_title_buttons(false)
            .build();
        header.pack_start(&split);
        header.pack_start(&ny);
        header.pack_start(&inline);
        header.pack_end(&controls);
        header.pack_end(&settings_btn);
        header.pack_end(&compact_btn);
        let minimized = Rc::new(Cell::new(false));
        set_control_tooltips(&controls, &minimized);
        // GTK lager knappene på nytt når vinduet endrer seg, for eksempel når det
        // blir låst i kompaktmodus. Tipsene må følge med.
        let control_buttons = controls.observe_children();
        let (c, m) = (controls.clone(), minimized.clone());
        control_buttons.connect_items_changed(move |_, _, _, _| set_control_tooltips(&c, &m));

        // Raden etter capture i kompaktmodus.
        let rev_thumb = gtk::Picture::builder()
            .content_fit(gtk::ContentFit::Contain)
            .width_request(44)
            .height_request(36)
            .can_shrink(true)
            .overflow(gtk::Overflow::Hidden)
            .valign(gtk::Align::Center)
            .css_classes(["klipp-reveal-thumb"])
            .build();
        let rev_dims = gtk::Label::builder()
            .xalign(0.0)
            .css_classes(["klipp-reveal-dims"])
            .build();
        let rev_status = gtk::Label::builder()
            .xalign(0.0)
            .css_classes(["klipp-reveal-status"])
            .build();
        let rev_text = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .hexpand(true)
            .valign(gtk::Align::Center)
            .build();
        rev_text.append(&rev_dims);
        rev_text.append(&rev_status);
        let rev_annotate = gtk::Button::builder()
            // Oversettere: Opens the compact preview screenshot for annotation.
            .label(trc("annotate-button", "Annoter"))
            .valign(gtk::Align::Center)
            .build();
        let rev_shelf = gtk::Button::builder()
            .label(tr("Vis hylla"))
            .valign(gtk::Align::Center)
            .build();
        let rev_box = gtk::Box::builder()
            .spacing(10)
            .css_classes(["klipp-reveal"])
            .build();
        // Bildet ønsker seg full bredde. Clampen holder det på 44 px.
        let rev_clamp = adw::Clamp::builder()
            .maximum_size(44)
            .tightening_threshold(44)
            .child(&rev_thumb)
            .build();
        rev_box.append(&rev_clamp);
        rev_box.append(&rev_text);
        rev_box.append(&rev_annotate);
        rev_box.append(&rev_shelf);
        let revealer = gtk::Revealer::builder()
            // Glidning ville endret vindushøyden hvert bilde. Toning gjør det bare én gang.
            .transition_type(gtk::RevealerTransitionType::Crossfade)
            .transition_duration(160)
            .visible(false)
            .child(&rev_box)
            .build();
        // Toning beholder plassen selv når raden er lukket, så den må skjules etterpå.
        let thumb = rev_thumb.clone();
        revealer.connect_child_revealed_notify(move |r| {
            if !r.is_child_revealed() && !r.reveals_child() {
                r.set_visible(false);
                thumb.set_paintable(None::<&gdk::Paintable>);
            }
        });

        // Tom tilstand.
        let empty_keyed = gtk::Label::builder()
            .wrap(true)
            .justify(gtk::Justification::Center)
            .build();
        let empty_plain = gtk::Label::builder()
            .label(tr("Trykk Ny for å ta et skjermbilde"))
            .wrap(true)
            .justify(gtk::Justification::Center)
            .build();
        let empty_child = gtk::Box::new(gtk::Orientation::Vertical, 0);
        empty_child.append(&empty_keyed);
        empty_child.append(&empty_plain);
        let empty = adw::StatusPage::builder()
            .icon_name("klipp-shelf-symbolic")
            .title(tr("Hylla er tom"))
            .child(&empty_child)
            .css_classes(["compact"])
            .build();

        let stack = gtk::Stack::builder()
            .vexpand(true)
            .transition_type(gtk::StackTransitionType::Crossfade)
            .transition_duration(160)
            .build();
        stack.add_named(&empty, Some("empty"));
        stack.add_named(&shelf.scroller, Some("shelf"));

        // Bunnlinja: antall bilder, Velg og Tøm. I valgmodus en handlingslinje.
        let clear_btn = icon_button("klipp-clear-symbolic", &clear_tip(cfg));
        clear_btn.connect_clicked(|_| Klipp::get().clear_shelf());
        let select_btn = icon_button(
            "selection-mode-symbolic",
            tr("Velg flere bilder (Ctrl+A)\nCtrl+klikk på et bilde gjør det samme"),
        );
        select_btn.update_property(&[gtk::accessible::Property::Label(tr("Velg flere bilder"))]);
        select_btn.connect_clicked(|_| Klipp::get().shelf.set_selecting(true));
        let shelf_count = gtk::Label::builder()
            .xalign(0.0)
            .hexpand(true)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .css_classes(["klipp-bar-label", "dim-label"])
            .build();
        let normal_bar = gtk::Box::builder()
            .spacing(2)
            .css_classes(["klipp-bar-normal"])
            .build();
        normal_bar.append(&shelf_count);
        normal_bar.append(&select_btn);
        normal_bar.append(&clear_btn);

        let sel_cancel = icon_button("window-close-symbolic", tr("Avbryt (Esc)"));
        sel_cancel.update_property(&[gtk::accessible::Property::Label(trc(
            "selection-cancel",
            "Avbryt",
        ))]);
        sel_cancel.connect_clicked(|_| Klipp::get().shelf.set_selecting(false));
        let sel_count = gtk::Label::builder()
            .xalign(0.0)
            .hexpand(true)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .css_classes(["klipp-bar-label", "klipp-sel-count"])
            .build();
        let sel_trash = icon_button("user-trash-symbolic", tr("Legg i papirkurven (Delete)"));
        sel_trash.add_css_class("klipp-bar-trash");
        sel_trash.update_property(&[gtk::accessible::Property::Label(tr("Legg i papirkurven"))]);
        sel_trash.connect_clicked(|_| Klipp::get().trash_selected());
        let sel_copy = gtk::Button::builder()
            // Oversettere: Keep the button short. It copies all selected screenshots.
            .label(trc("shelf-copy", "Kopier"))
            .tooltip_text(tr("Kopier de valgte bildene (Ctrl+C)"))
            .css_classes(["suggested-action", "klipp-bar-copy"])
            .build();
        sel_copy.connect_clicked(|_| Klipp::get().copy_selected());
        let select_bar = gtk::Box::builder()
            .spacing(2)
            .css_classes(["klipp-bar-select"])
            .build();
        select_bar.append(&sel_cancel);
        select_bar.append(&sel_count);
        select_bar.append(&sel_trash);
        select_bar.append(&sel_copy);

        let bar_stack = gtk::Stack::builder()
            .hexpand(true)
            .transition_duration(BAR_SLIDE_MS)
            .build();
        bar_stack.add_named(&normal_bar, Some("normal"));
        bar_stack.add_named(&select_bar, Some("select"));
        let bottom = gtk::Box::builder().css_classes(["klipp-bottombar"]).build();
        bottom.append(&bar_stack);

        let body = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .vexpand(true)
            .build();
        body.append(&stack);
        body.append(&bottom);

        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.append(&header);
        content.append(&revealer);
        content.append(&body);

        let toasts = adw::ToastOverlay::new();
        toasts.set_child(Some(&content));

        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title("Klipp")
            .icon_name(klipp_system::APP_ID)
            .default_width(if cfg.compact { COMPACT_W } else { STRIP_W })
            .default_height(if cfg.compact { -1 } else { strip_height() })
            .content(&toasts)
            .build();
        window.add_css_class("klipp-main");

        window.connect_close_request(|_| {
            Klipp::get().hide_main();
            glib::Propagation::Stop
        });
        window.connect_is_active_notify(|w| {
            if w.is_active() {
                let k = Klipp::get();
                if k.ui.minimized.replace(false) {
                    k.sync_tray();
                }
                let (shelf, ui) = (k.shelf.clone(), k.ui.clone());
                let cfg = k.config.clone();
                k.shelf.prune(move || ui.sync_shelf(&shelf, &cfg.borrow()));
            }
        });

        let ui = Rc::new(Self {
            window,
            toasts,
            header,
            split,
            ny,
            inline,
            compact_btn,
            content,
            body,
            stack,
            empty_plain,
            empty_keyed,
            bottom,
            bar_stack,
            shelf_count,
            sel_count,
            sel_copy,
            sel_trash,
            clear_btn,
            mode_rows,
            _control_buttons: control_buttons,
            revealer,
            rev_thumb,
            rev_dims,
            rev_status,
            rev_timer: RefCell::new(None),
            rev_path: RefCell::new(None),
            syncing: Cell::new(false),
            minimized,
        });

        let weak = Rc::downgrade(&ui);
        ui.inline.connect_active_name_notify(move |g| {
            let Some(ui) = weak.upgrade() else { return };
            if ui.syncing.get() {
                return;
            }
            if let Some(m) = g.active_name().and_then(|n| Mode::from_id(&n)) {
                Klipp::get().set_mode(m);
            }
        });

        let weak = Rc::downgrade(&ui);
        rev_annotate.connect_clicked(move |_| {
            let Some(ui) = weak.upgrade() else { return };
            let path = ui.rev_path.borrow().clone();
            ui.hide_reveal();
            let k = Klipp::get();
            let shot = path.and_then(|p| k.shelf.find(&p));
            if let Some(shot) = shot {
                k.annotate(&shot);
            }
        });
        rev_shelf.connect_clicked(|_| Klipp::get().set_compact(false));

        // Musa over raden holder den åpen.
        let hover = gtk::EventControllerMotion::new();
        let weak = Rc::downgrade(&ui);
        hover.connect_enter(move |_, _, _| {
            if let Some(ui) = weak.upgrade() {
                ui.stop_reveal_timer();
            }
        });
        let weak = Rc::downgrade(&ui);
        hover.connect_leave(move |_| {
            if let Some(ui) = weak.upgrade()
                && ui.revealer.reveals_child()
            {
                ui.arm_reveal();
            }
        });
        rev_box.add_controller(hover);

        ui.install_keys();
        ui.apply_compact(cfg.compact);
        ui
    }

    fn install_keys(&self) {
        let keys_ctl = gtk::EventControllerKey::new();
        let repeat = Rc::new(keys::RepeatFilter::default());
        let r = repeat.clone();
        keys_ctl.connect_key_released(move |_, _, _, _| r.release());
        keys_ctl.connect_key_pressed(move |c, key, code, state| {
            if !repeat.press(code) {
                return glib::Propagation::Proceed;
            }
            let Some(display) = gdk::Display::default() else {
                return glib::Propagation::Proceed;
            };
            let k = Klipp::get();
            let action =
                keys::action_for(&k.config.borrow(), &display, key, code, c.group(), state);
            // Egne snarveier først. Valgtastene virker uansett hvor fokus står.
            if action.is_none() && k.selection_key(key, state) {
                return glib::Propagation::Stop;
            }
            match action {
                Some(Action::ToggleShelf) => {
                    let on = !k.config.borrow().compact;
                    k.set_compact(on);
                    glib::Propagation::Stop
                }
                Some(a) => {
                    if let Some(m) = a.mode() {
                        k.set_mode(m);
                    }
                    glib::Propagation::Stop
                }
                None => glib::Propagation::Proceed,
            }
        });
        self.window.add_controller(keys_ctl);
    }

    pub fn set_blocked(&self, blocked: bool) {
        self.content.set_sensitive(!blocked);
    }

    pub fn apply_compact(&self, on: bool) {
        self.hide_reveal();
        self.toasts.dismiss_all();
        self.split.set_visible(!on);
        self.ny.set_visible(on);
        self.inline.set_visible(on);
        self.body.set_visible(!on);
        if on {
            self.header.remove_css_class("flat");
            self.header.remove_css_class("klipp-narrow");
        } else {
            self.header.add_css_class("flat");
            self.header.add_css_class("klipp-narrow");
        }
        // AdwApplicationWindow har ellers 360 × 200 px som minste størrelse.
        self.window.set_size_request(MIN_W, MIN_H);
        let h = strip_height();
        if on {
            self.content.set_size_request(COMPACT_W, -1);
            self.window.set_default_size(COMPACT_W, -1);
            self.window.set_resizable(false);
        } else if self.window.is_mapped() {
            // GTK endrer ikke størrelsen på et vist vindu med set_default_size. Et
            // vindu som ikke kan endres følger innholdet, så vi går veien om det.
            self.window.set_resizable(false);
            self.content.set_size_request(STRIP_W, h);
            let (content, window) = (self.content.clone(), self.window.clone());
            glib::timeout_add_local_once(Duration::from_millis(150), move || {
                content.set_size_request(-1, -1);
                window.set_default_size(STRIP_W, h);
                window.set_resizable(true);
            });
        } else {
            self.content.set_size_request(-1, -1);
            self.window.set_default_size(STRIP_W, h);
            self.window.set_resizable(true);
        }
        let label = if on {
            tr("Vis hylla")
        } else {
            tr("Skjul hylla")
        };
        self.compact_btn
            .update_property(&[gtk::accessible::Property::Label(label)]);
    }

    pub fn sync_save_dir(&self, cfg: &Config) {
        self.clear_btn.set_tooltip_text(Some(&clear_tip(cfg)));
    }

    pub fn sync_all(&self, shelf: &Shelf, cfg: &Config) {
        self.sync_modes(cfg);
        self.sync_shelf(shelf, cfg);
    }

    pub fn sync_modes(&self, cfg: &Config) {
        let cur = cfg.last_mode;
        self.syncing.set(true);
        for (m, b, accel) in &self.mode_rows {
            b.set_active(*m == cur);
            accel.set_label(&keys::label(cfg, Action::for_mode(*m)).unwrap_or_default());
        }
        self.inline.set_active_name(Some(cur.id()));
        for m in Mode::ALL {
            if let Some(t) = self.inline.toggle_by_name(m.id()) {
                t.set_tooltip(&keys::hint(cfg, Action::for_mode(m), mode_label(m)));
            }
        }
        self.syncing.set(false);
        // Oversettere: {mode} is a capture mode such as Area or Window.
        let tip = tr_fmt("Ta skjermbilde: {mode}", &[("mode", mode_label(cur))]);
        self.split.set_tooltip_text(Some(&tip));
        self.ny.set_tooltip_text(Some(&tip));
        let base = if cfg.compact {
            tr("Vis hylla")
        } else {
            tr("Skjul hylla")
        };
        self.compact_btn
            .set_tooltip_text(Some(&keys::hint(cfg, Action::ToggleShelf, base)));
    }

    pub fn sync_shelf(&self, shelf: &Shelf, cfg: &Config) {
        let empty = shelf.is_empty();
        self.stack
            .set_visible_child_name(if empty { "empty" } else { "shelf" });
        self.bottom.set_visible(!empty);
        self.sync_selection(shelf);
        let key = keys::label(cfg, Action::Capture);
        self.empty_keyed.set_visible(key.is_some());
        self.empty_plain.set_visible(key.is_none());
        if let Some(key) = key {
            // Oversettere: {key} is the capture shortcut. Keep the <b> and </b> markup around it.
            let hint = tr_fmt(
                "Trykk <b>{key}</b>\nfor å ta et skjermbilde",
                &[("key", &glib::markup_escape_text(&key))],
            );
            self.empty_keyed.set_markup(&hint);
        }
    }

    /// Bunnlinja følger valgmodus: glir opp til handlingene og ned igjen.
    pub fn sync_selection(&self, shelf: &Shelf) {
        let n = shelf.selected_count();
        self.shelf_count
            .set_label(&count_text(shelf.store.n_items() as usize));
        self.sel_count.set_label(&if n == 0 {
            tr("Velg bilder").to_string()
        } else {
            // Oversettere: Number of selected screenshots. Both forms are the same in Norwegian.
            trn("{n} valgt", "{n} valgt", n)
        });
        if n == 0 {
            self.sel_count.add_css_class("dim-label");
        } else {
            self.sel_count.remove_css_class("dim-label");
        }
        self.sel_copy.set_sensitive(n > 0);
        self.sel_trash.set_sensitive(n > 0);
        let (want, slide) = if shelf.selecting() {
            ("select", gtk::StackTransitionType::SlideUp)
        } else {
            ("normal", gtk::StackTransitionType::SlideDown)
        };
        if self.bar_stack.visible_child_name().as_deref() != Some(want) {
            // Skjult linje skal ikke gli når den vises igjen.
            let animate = self.bottom.is_mapped();
            self.bar_stack.set_transition_type(if animate {
                slide
            } else {
                gtk::StackTransitionType::None
            });
            self.bar_stack.set_visible_child_name(want);
        }
    }

    pub fn toast(&self, title: &str) {
        self.toasts.dismiss_all();
        let t = adw::Toast::builder()
            .custom_title(&toast_label(title))
            .timeout(2)
            .build();
        self.toasts.add_toast(t);
    }

    pub fn toast_with_action(
        &self,
        title: &str,
        action: &str,
        seconds: u32,
        on_action: impl Fn() + 'static,
    ) {
        self.toasts.dismiss_all();
        let t = adw::Toast::builder()
            .custom_title(&toast_label(title))
            .button_label(action)
            .timeout(seconds)
            .build();
        t.connect_button_clicked(move |_| on_action());
        self.toasts.add_toast(t);
    }

    /// Kompakt: raden viser bildet i ca. fire sekunder etter capture.
    pub fn show_reveal(
        self: &Rc<Self>,
        thumb: &gdk::Texture,
        dims: &str,
        status: &str,
        path: &std::path::Path,
        alpha: bool,
    ) {
        self.rev_thumb.set_paintable(Some(thumb));
        if alpha {
            self.rev_thumb.add_css_class("alpha");
        } else {
            self.rev_thumb.remove_css_class("alpha");
        }
        self.rev_dims.set_label(dims);
        self.rev_status.set_label(status);
        self.rev_path.replace(Some(path.to_path_buf()));
        self.revealer.set_visible(true);
        self.revealer.set_reveal_child(true);
        self.arm_reveal();
    }

    fn arm_reveal(self: &Rc<Self>) {
        self.stop_reveal_timer();
        let weak = Rc::downgrade(self);
        let id = glib::timeout_add_local_once(Duration::from_secs(4), move || {
            if let Some(ui) = weak.upgrade() {
                ui.rev_timer.replace(None);
                ui.hide_reveal();
            }
        });
        self.rev_timer.replace(Some(id));
    }

    fn stop_reveal_timer(&self) {
        if let Some(id) = self.rev_timer.take() {
            id.remove();
        }
    }

    pub fn hide_reveal(&self) {
        self.stop_reveal_timer();
        self.revealer.set_reveal_child(false);
    }
}

/// Stripa er smal, så toast-teksten må kunne bryte over to linjer.
fn toast_label(title: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(title)
        .wrap(true)
        .max_width_chars(20)
        .justify(gtk::Justification::Center)
        .build()
}

/// Knappene er GTK sine egne. Tips og navn settes på knappene inne i kontrollene,
/// og minimer-knappen merkes så systemfeltet vet at vinduet er borte.
fn set_control_tooltips(controls: &gtk::WindowControls, minimized: &Rc<Cell<bool>>) {
    fn walk(w: gtk::Widget, minimized: &Rc<Cell<bool>>) {
        if let Some(b) = w.downcast_ref::<gtk::Button>() {
            let (tip, label) = if b.has_css_class("close") {
                (tr("Skjul til systemfeltet"), tr("Lukk vinduet"))
            } else if b.has_css_class("minimize") {
                // Kontrollene går gjennom knappene igjen ved hver endring.
                if !b.has_css_class("klipp-hooked") {
                    b.add_css_class("klipp-hooked");
                    let m = minimized.clone();
                    b.connect_clicked(move |_| {
                        m.set(true);
                        Klipp::get().sync_tray();
                    });
                }
                (tr("Minimer"), tr("Minimer"))
            } else {
                return;
            };
            b.set_tooltip_text(Some(tip));
            b.update_property(&[gtk::accessible::Property::Label(label)]);
            return;
        }
        let mut c = w.first_child();
        while let Some(child) = c {
            c = child.next_sibling();
            walk(child, minimized);
        }
    }
    walk(controls.clone().upcast(), minimized);
}
