//! Annoteringsvinduet: tittellinje, verktøylinje, lagring og kopiering.

use crate::AnnotateEvent;
use crate::canvas::Canvas;
use crate::clipboard::PngProvider;
use crate::file::{self, Loaded};
use crate::keys::{self, KeyAction, KeyPress};
use crate::ops::{COLORS, Color, DEFAULT_THICK, THICK, Tool};
use crate::view;
use adw::prelude::*;
use adw::subclass::prelude::*;
use agentic_screenshot_system::i18n::{tr, tr_fmt, trc};
use gtk::{cairo, gdk, gio, glib};
use std::cell::{Cell, OnceCell, RefCell};
use std::f64::consts::TAU;
use std::path::{Path, PathBuf};
use std::rc::Rc;

const TOOLBAR_HEIGHT: i32 = 46;
const SWATCH: i32 = 24;
const DOT_BOX: i32 = 16;
/// `opts.timeout` på 2200 ms i prototypen. AdwToast teller i hele sekunder.
const TOAST_SECONDS: u32 = 2;

const CSS: &str = "
.shot-swatch:checked, .shot-swatch:checked:hover { background: none; }
.shot-size-row { font-weight: normal; padding: 6px 10px; }
.shot-size-row label { margin-left: 10px; }
.shot-size-key { margin-left: 18px; }
textview.shot-text {
  background: rgba(0, 0, 0, .25);
  outline: 1px dashed rgba(255, 255, 255, .65);
  outline-offset: 3px;
}
textview.shot-text > text { background: none; }
";

thread_local! {
    static CSS_LOADED: Cell<bool> = const { Cell::new(false) };
}

fn install_css(display: &gdk::Display) {
    if CSS_LOADED.replace(true) {
        return;
    }
    let provider = gtk::CssProvider::new();
    provider.load_from_string(CSS);
    gtk::style_context_add_provider_for_display(
        display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

type Callback = Box<dyn Fn(AnnotateEvent)>;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct AnnotateWindow {
        pub path: OnceCell<PathBuf>,
        pub canvas: OnceCell<Canvas>,
        pub toasts: OnceCell<adw::ToastOverlay>,
        pub undo: OnceCell<gtk::Button>,
        pub redo: OnceCell<gtk::Button>,
        pub tools: RefCell<Vec<(Tool, gtk::ToggleButton)>>,
        pub colors: RefCell<Vec<gtk::ToggleButton>>,
        pub thick: RefCell<Vec<gtk::ToggleButton>>,
        pub palette: OnceCell<gtk::Box>,
        pub size_menu: OnceCell<gtk::MenuButton>,
        /// Fargen brukeren har valgt selv. Delt med tegningen av knappen.
        pub custom: Rc<Cell<Option<Color>>>,
        pub custom_toggled: Cell<bool>,
        /// Siste faste farge, som vi går tilbake til hvis fargevelgeren avbrytes.
        pub last_color: Cell<usize>,
        pub on_event: RefCell<Option<Callback>>,
        pub saving: Cell<bool>,
        pub held: Cell<Option<gdk::Key>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for AnnotateWindow {
        const NAME: &'static str = "ShotAnnotateWindow";
        type Type = super::AnnotateWindow;
        type ParentType = adw::Window;
    }

    impl ObjectImpl for AnnotateWindow {}
    impl WidgetImpl for AnnotateWindow {}

    impl WindowImpl for AnnotateWindow {
        fn close_request(&self) -> glib::Propagation {
            if !self.saving.get() {
                self.finish(AnnotateEvent::Cancelled);
            }
            glib::Propagation::Stop
        }
    }

    impl AdwWindowImpl for AnnotateWindow {}

    impl AnnotateWindow {
        pub fn canvas(&self) -> &Canvas {
            self.canvas.get().expect("lerretet lages i new()")
        }

        pub fn build(&self, canvas: Canvas, title: &str) {
            let obj = self.obj();
            self.canvas.set(canvas.clone()).ok();

            let header = adw::HeaderBar::new();
            header.set_show_start_title_buttons(false);
            header.set_show_end_title_buttons(false);
            header.set_title_widget(Some(&adw::WindowTitle::new(
                trc("annotation-title", "Annoter"),
                title,
            )));

            let cancel = text_button(trc("annotation-cancel", "Avbryt"), tr("Avbryt (Esc)"));
            cancel.connect_clicked(glib::clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| imp.cancel()
            ));
            let undo = icon_button(
                "shot-undo-symbolic",
                trc("annotation-undo", "Angre (Ctrl+Z)"),
                trc("annotation-undo", "Angre"),
            );
            undo.connect_clicked(glib::clone!(
                #[weak]
                canvas,
                move |_| canvas.undo()
            ));
            let redo = icon_button(
                "shot-redo-symbolic",
                tr("Gjør om (Ctrl+Shift+Z)"),
                trc("annotation-redo", "Gjør om"),
            );
            redo.connect_clicked(glib::clone!(
                #[weak]
                canvas,
                move |_| canvas.redo()
            ));
            header.pack_start(&cancel);
            header.pack_start(&undo);
            header.pack_start(&redo);

            // Oversettere: Saves annotations and closes the editor. Keep the header button short.
            let done = text_button(trc("annotation-save", "Ferdig"), tr("Ferdig (Ctrl+Enter)"));
            done.add_css_class("suggested-action");
            done.connect_clicked(glib::clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| imp.done()
            ));
            // Oversettere: Copies the annotated image. Keep the header button short.
            let copy = text_button(trc("annotation-copy", "Kopier"), tr("Kopier (Ctrl+C)"));
            copy.connect_clicked(glib::clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| imp.copy()
            ));
            header.pack_end(&done);
            header.pack_end(&copy);
            self.undo.set(undo).ok();
            self.redo.set(redo).ok();

            let toasts = adw::ToastOverlay::new();
            toasts.set_child(Some(&canvas));

            let content = adw::ToolbarView::new();
            content.set_top_bar_style(adw::ToolbarStyle::RaisedBorder);
            content.add_top_bar(&header);
            content.add_top_bar(&self.build_toolbar(&canvas));
            content.set_content(Some(&toasts));
            obj.set_content(Some(&content));
            self.toasts.set(toasts).ok();

            // Capture: ellers kan et barn stjele tasten. En åpen toast tar for eksempel Esc.
            let keys = gtk::EventControllerKey::new();
            keys.set_propagation_phase(gtk::PropagationPhase::Capture);
            keys.connect_key_pressed(glib::clone!(
                #[weak(rename_to = imp)]
                self,
                #[upgrade_or]
                glib::Propagation::Proceed,
                move |_, key, _, state| imp.key_pressed(key, state)
            ));
            keys.connect_key_released(glib::clone!(
                #[weak(rename_to = imp)]
                self,
                move |_, _, _, _| imp.held.set(None)
            ));
            obj.add_controller(keys);
            // Slippet av en tast går til det nye vinduet hvis fokus flytter seg.
            obj.connect_is_active_notify(glib::clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| imp.held.set(None)
            ));

            canvas.connect_history_changed(glib::clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| imp.sync_undo()
            ));
            self.sync_undo();
        }

        fn build_toolbar(&self, canvas: &Canvas) -> gtk::Box {
            let bar = gtk::Box::new(gtk::Orientation::Horizontal, 2);
            bar.set_halign(gtk::Align::Center);
            bar.set_height_request(TOOLBAR_HEIGHT);
            bar.update_property(&[gtk::accessible::Property::Label(tr("Annoteringsverktøy"))]);

            let mut tools = Vec::new();
            for tool in Tool::ALL {
                // Oversettere: {label} is an annotation tool and {key} is its fixed shortcut.
                let tip = tr_fmt(
                    "{label} ({key})",
                    &[
                        ("label", tool.label()),
                        ("key", &tool.key().to_ascii_uppercase().to_string()),
                    ],
                );
                let b = gtk::ToggleButton::new();
                b.set_icon_name(tool.icon());
                setup_toggle(&b, &tip, tool.label(), canvas);
                b.set_group(tools.first().map(|(_, first)| first));
                b.set_active(tool == Tool::Arrow);
                b.connect_toggled(glib::clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |b| {
                        if b.is_active() {
                            imp.canvas().set_tool(tool);
                            imp.sync_tool_controls(tool);
                        }
                    }
                ));
                bar.append(&b);
                tools.push((tool, b));
            }
            self.tools.replace(tools);
            bar.append(&separator());

            let palette = gtk::Box::new(gtk::Orientation::Horizontal, 2);
            let mut colors: Vec<gtk::ToggleButton> = Vec::new();
            for (i, swatch) in COLORS.iter().enumerate() {
                let b = gtk::ToggleButton::new();
                b.add_css_class("shot-swatch");
                b.add_css_class("image-button");
                let color = swatch.color;
                b.set_child(Some(&swatch_dot(&b, move || Some(color))));
                let label = (swatch.label)();
                let tip = tr_fmt(
                    "{label} ({key})",
                    &[("label", label), ("key", &(i + 1).to_string())],
                );
                // Oversettere: Accessible label for a color swatch.
                let accessible = tr_fmt("Farge: {color}", &[("color", label)]);
                setup_toggle(&b, &tip, &accessible, canvas);
                b.set_group(colors.first());
                b.set_active(i == 0);
                b.connect_toggled(glib::clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |b| {
                        if b.is_active() {
                            imp.last_color.set(i);
                            imp.canvas().set_color(color);
                        }
                    }
                ));
                palette.append(&b);
                colors.push(b);
            }
            palette.append(&self.custom_button(canvas, colors.first()));
            self.colors.replace(colors);
            bar.append(&palette);
            self.palette.set(palette).ok();
            bar.append(&separator());

            let menu = self.size_menu(canvas);
            bar.append(&menu);
            self.size_menu.set(menu).ok();
            bar
        }

        /// Den siste fargeknappen: egen farge fra GNOMEs fargevelger. Står den allerede
        /// valgt, åpner et nytt klikk velgeren igjen.
        fn custom_button(
            &self,
            canvas: &Canvas,
            group: Option<&gtk::ToggleButton>,
        ) -> gtk::ToggleButton {
            let b = gtk::ToggleButton::new();
            b.add_css_class("shot-swatch");
            b.add_css_class("image-button");
            let custom = self.custom.clone();
            b.set_child(Some(&swatch_dot(&b, move || custom.get())));
            // Oversettere: The fixed number key opens the custom color picker.
            let tip = tr_fmt(
                "Egen farge ({key})",
                &[("key", &(COLORS.len() + 1).to_string())],
            );
            setup_toggle(&b, &tip, tr("Egen farge"), canvas);
            b.set_group(group);
            b.connect_toggled(glib::clone!(
                #[weak(rename_to = imp)]
                self,
                move |b| {
                    if !b.is_active() {
                        return;
                    }
                    imp.custom_toggled.set(true);
                    match imp.custom.get() {
                        Some(c) => imp.canvas().set_color(c),
                        None => imp.choose_custom(),
                    }
                }
            ));
            b.connect_clicked(glib::clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    if !imp.custom_toggled.replace(false) {
                        imp.choose_custom();
                    }
                }
            ));
            b
        }

        fn custom_toggle(&self) -> Option<gtk::ToggleButton> {
            self.palette
                .get()?
                .last_child()
                .and_downcast::<gtk::ToggleButton>()
        }

        fn choose_custom(&self) {
            let start = self
                .custom
                .get()
                .unwrap_or(COLORS[self.last_color.get()].color);
            let initial = gdk::RGBA::new(start.r as f32, start.g as f32, start.b as f32, 1.0);
            let dialog = gtk::ColorDialog::builder()
                .title(tr("Egen farge"))
                .modal(true)
                .with_alpha(false)
                .build();
            let window = self.obj().clone();
            dialog.choose_rgba(
                Some(&window),
                Some(&initial),
                gio::Cancellable::NONE,
                glib::clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move |picked| match picked {
                        Ok(rgba) => {
                            let c = Color {
                                r: rgba.red().into(),
                                g: rgba.green().into(),
                                b: rgba.blue().into(),
                            };
                            imp.custom.set(Some(c));
                            imp.canvas().set_color(c);
                            if let Some(b) = imp.custom_toggle() {
                                imp.custom_toggled.set(true);
                                b.set_active(true);
                                imp.custom_toggled.set(false);
                                if let Some(dot) = b.child() {
                                    dot.queue_draw();
                                }
                            }
                        }
                        // Avbrutt før noen egen farge fantes: tilbake til den forrige.
                        Err(_) if imp.custom.get().is_none() => {
                            if let Some(b) = imp.colors.borrow().get(imp.last_color.get()) {
                                b.set_active(true);
                            }
                        }
                        Err(_) => {}
                    }
                ),
            );
        }

        /// Størrelsen som en liten meny, så verktøylinja holder seg smal.
        fn size_menu(&self, canvas: &Canvas) -> gtk::MenuButton {
            let current = gtk::DrawingArea::new();
            current.set_content_width(DOT_BOX);
            current.set_content_height(DOT_BOX);
            current.set_valign(gtk::Align::Center);
            current.set_draw_func(glib::clone!(
                #[weak]
                canvas,
                move |area, cr, w, h| paint_dot(area, cr, w, h, THICK[canvas.thick()].dot)
            ));
            let arrow = gtk::Image::from_icon_name("pan-down-symbolic");
            arrow.add_css_class("dim-label");
            let face = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            face.append(&current);
            face.append(&arrow);

            let list = gtk::Box::new(gtk::Orientation::Vertical, 0);
            let popover = gtk::Popover::builder().child(&list).build();
            let menu = gtk::MenuButton::builder()
                .child(&face)
                .popover(&popover)
                .valign(gtk::Align::Center)
                .tooltip_text(tr("Størrelse (+ / −)"))
                .css_classes(["flat"])
                .build();
            menu.set_focus_on_click(false);

            let mut rows: Vec<gtk::ToggleButton> = Vec::new();
            for (i, t) in THICK.iter().enumerate() {
                let line = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                line.append(&thick_dot(t.dot));
                let name = gtk::Label::builder()
                    .label((t.label)())
                    .xalign(0.0)
                    .hexpand(true)
                    .build();
                line.append(&name);
                let b = gtk::ToggleButton::builder()
                    .child(&line)
                    .css_classes(["flat", "shot-size-row"])
                    .build();
                b.set_group(rows.first());
                b.set_active(i == DEFAULT_THICK);
                b.connect_toggled(glib::clone!(
                    #[weak]
                    canvas,
                    #[weak]
                    menu,
                    #[weak]
                    current,
                    #[weak]
                    popover,
                    move |b| {
                        if b.is_active() {
                            canvas.set_thick(i);
                            current.queue_draw();
                            menu.update_property(&[gtk::accessible::Property::Label(&tr_fmt(
                                "Størrelse: {size}",
                                &[("size", (THICK[i].label)())],
                            ))]);
                            popover.popdown();
                        }
                    }
                ));
                list.append(&b);
                rows.push(b);
            }
            let hint = gtk::Label::builder()
                .label(tr("+ og − bytter størrelse"))
                .css_classes(["dim-label", "caption"])
                .margin_top(6)
                .margin_bottom(2)
                .build();
            list.append(&hint);
            menu.update_property(&[gtk::accessible::Property::Label(&tr_fmt(
                "Størrelse: {size}",
                &[("size", (THICK[DEFAULT_THICK].label)())],
            ))]);
            // Fokus på den valgte størrelsen, ikke den første, når menyen åpnes.
            popover.connect_show(glib::clone!(
                #[weak]
                canvas,
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    if let Some(row) = imp.thick.borrow().get(canvas.thick()) {
                        row.grab_focus();
                    }
                }
            ));
            self.thick.replace(rows);
            menu
        }

        /// Farge gir ingen mening for sladd og viskelær, og størrelse ikke for viskelæret.
        fn sync_tool_controls(&self, tool: Tool) {
            if let Some(palette) = self.palette.get() {
                palette.set_sensitive(tool.uses_color());
            }
            if let Some(menu) = self.size_menu.get() {
                menu.set_sensitive(tool != Tool::Eraser);
            }
        }

        fn sync_undo(&self) {
            let canvas = self.canvas();
            if let (Some(undo), Some(redo)) = (self.undo.get(), self.redo.get()) {
                undo.set_sensitive(canvas.can_undo());
                redo.set_sensitive(canvas.can_redo());
            }
        }

        fn key_pressed(&self, key: gdk::Key, state: gdk::ModifierType) -> glib::Propagation {
            if self.saving.get() || self.canvas().is_editing_text() {
                return glib::Propagation::Proceed;
            }
            let repeat = self.held.replace(Some(key)) == Some(key);
            let press = KeyPress {
                ch: key.to_unicode().and_then(|c| c.to_lowercase().next()),
                enter: matches!(
                    key,
                    gdk::Key::Return | gdk::Key::KP_Enter | gdk::Key::ISO_Enter
                ),
                escape: key == gdk::Key::Escape,
                ctrl: state
                    .intersects(gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::META_MASK),
                alt: state.contains(gdk::ModifierType::ALT_MASK),
                shift: state.contains(gdk::ModifierType::SHIFT_MASK),
                repeat,
            };
            let Some(action) = keys::action(press) else {
                return glib::Propagation::Proceed;
            };
            self.perform(action);
            glib::Propagation::Stop
        }

        fn perform(&self, action: KeyAction) {
            let canvas = self.canvas();
            match action {
                KeyAction::Escape => {
                    if !canvas.cancel_draft() {
                        self.cancel();
                    }
                }
                KeyAction::Undo => canvas.undo(),
                KeyAction::Redo => canvas.redo(),
                KeyAction::Copy => self.copy(),
                KeyAction::Done => self.done(),
                KeyAction::Tool(tool) => {
                    if let Some((_, b)) = self.tools.borrow().iter().find(|(t, _)| *t == tool) {
                        b.set_active(true);
                    }
                }
                KeyAction::Color(i) if i < COLORS.len() => {
                    if let Some(b) = self.colors.borrow().get(i) {
                        b.set_active(true);
                    }
                }
                KeyAction::Color(_) => match self.custom_toggle() {
                    Some(b) if b.is_active() => self.choose_custom(),
                    Some(b) => b.set_active(true),
                    None => {}
                },
                KeyAction::Thicker | KeyAction::Thinner => {
                    let step = if action == KeyAction::Thicker { 1 } else { -1 };
                    let i = canvas
                        .thick()
                        .saturating_add_signed(step)
                        .min(THICK.len() - 1);
                    if let Some(b) = self.thick.borrow().get(i) {
                        b.set_active(true);
                    }
                }
            }
        }

        fn cancel(&self) {
            if !self.saving.get() {
                self.finish(AnnotateEvent::Cancelled);
            }
        }

        /// Lukker vinduet og melder fra, én gang.
        fn finish(&self, event: AnnotateEvent) {
            let Some(callback) = self.on_event.take() else {
                return;
            };
            self.obj().destroy();
            callback(event);
        }

        fn toast(&self, message: &str) {
            if let Some(toasts) = self.toasts.get() {
                toasts.dismiss_all();
                let toast = adw::Toast::new(message);
                toast.set_timeout(TOAST_SECONDS);
                toasts.add_toast(toast);
            }
        }

        fn set_saving(&self, saving: bool) {
            self.saving.set(saving);
            if let Some(content) = self.obj().content() {
                content.set_sensitive(!saving);
            }
        }

        fn done(&self) {
            if self.saving.get() {
                return;
            }
            let canvas = self.canvas();
            canvas.commit_text();
            canvas.cancel_draft();
            let (Some(flat), false) = (canvas.flat(), canvas.is_empty()) else {
                // Ingenting å brenne inn: filen står urørt, som i prototypen.
                self.finish(AnnotateEvent::Cancelled);
                return;
            };
            self.set_saving(true);
            let path = self.path.get().cloned().unwrap_or_default();
            let target = path.clone();
            let window = self.obj().downgrade();
            glib::spawn_future_local(async move {
                let result = gio::spawn_blocking(move || {
                    file::save_atomic(&target, &flat.bytes, flat.width, flat.height, flat.alpha)
                })
                .await
                .unwrap_or_else(|_| Err(tr("Kunne ikke lagre bildet").to_string()));
                let Some(window) = window.upgrade() else {
                    return;
                };
                let imp = window.imp();
                imp.set_saving(false);
                match result {
                    Ok(()) => imp.finish(AnnotateEvent::Saved(path)),
                    Err(e) => imp.toast(&e),
                }
            });
        }

        fn copy(&self) {
            let canvas = self.canvas();
            canvas.commit_text();
            if self.saving.get() {
                return;
            }
            let Some(flat) = canvas.flat() else {
                return;
            };
            let obj = self.obj();
            let clipboard = obj.clipboard();
            let provider = PngProvider::new(flat);
            // Ta eierskap mens vinduet har fokus. PNG-en kodes i bakgrunnen.
            let copied = obj.is_active() && clipboard.set_content(Some(&provider)).is_ok() && {
                clipboard.display().sync();
                clipboard.is_local()
            };
            self.toast(if copied {
                tr("Kopiert!")
            } else {
                tr("Kunne ikke kopiere bildet")
            });
        }
    }

    fn text_button(label: &str, tip: &str) -> gtk::Button {
        let b = gtk::Button::with_label(label);
        b.set_tooltip_text(Some(tip));
        b.set_focus_on_click(false);
        b
    }

    fn icon_button(icon: &str, tip: &str, label: &str) -> gtk::Button {
        let b = gtk::Button::from_icon_name(icon);
        b.set_tooltip_text(Some(tip));
        b.set_focus_on_click(false);
        b.update_property(&[gtk::accessible::Property::Label(label)]);
        b
    }

    fn setup_toggle(b: &gtk::ToggleButton, tip: &str, label: &str, canvas: &Canvas) {
        // Et klikk i verktøylinja avslutter teksten, også på det som allerede er valgt.
        b.connect_clicked(glib::clone!(
            #[weak]
            canvas,
            move |_| canvas.commit_text()
        ));
        b.add_css_class("flat");
        b.set_valign(gtk::Align::Center);
        b.set_tooltip_text(Some(tip));
        b.set_focus_on_click(false);
        b.update_property(&[gtk::accessible::Property::Label(label)]);
    }

    fn separator() -> gtk::Separator {
        let s = gtk::Separator::new(gtk::Orientation::Vertical);
        s.set_margin_start(8);
        s.set_margin_end(8);
        s.set_valign(gtk::Align::Center);
        s.set_height_request(22);
        s
    }

    fn circle(cr: &cairo::Context, w: i32, h: i32, r: f64) {
        cr.arc(f64::from(w) / 2.0, f64::from(h) / 2.0, r, 0.0, TAU);
    }

    fn paint_dot(area: &gtk::DrawingArea, cr: &cairo::Context, w: i32, h: i32, diameter: f64) {
        let fg = area.color();
        cr.set_source_rgba(
            fg.red().into(),
            fg.green().into(),
            fg.blue().into(),
            fg.alpha().into(),
        );
        circle(cr, w, h, diameter / 2.0);
        let _ = cr.fill();
    }

    /// Regnbuering for egen farge før noen er valgt.
    fn rainbow(cr: &cairo::Context, w: i32, h: i32, r: f64) {
        const HUES: [u32; 6] = [0xed333b, 0xf6d32d, 0x33d17a, 0x62a0ea, 0xc061cb, 0xff7800];
        let (cx, cy) = (f64::from(w) / 2.0, f64::from(h) / 2.0);
        let step = TAU / HUES.len() as f64;
        cr.set_line_width(4.0);
        for (i, hue) in HUES.iter().enumerate() {
            let c = crate::ops::rgb(*hue);
            cr.set_source_rgb(c.r, c.g, c.b);
            let start = i as f64 * step;
            cr.arc(cx, cy, r - 2.0, start, start + step);
            let _ = cr.stroke();
        }
    }

    /// Fargeflekk. Valgt farge krymper og får ring, som i prototypen. En tynn kant gjør
    /// at hvit og svart også synes på verktøylinja.
    fn swatch_dot(
        button: &gtk::ToggleButton,
        color: impl Fn() -> Option<Color> + 'static,
    ) -> gtk::DrawingArea {
        let area = gtk::DrawingArea::new();
        area.set_content_width(SWATCH);
        area.set_content_height(SWATCH);
        area.set_draw_func(glib::clone!(
            #[weak]
            button,
            move |area, cr, w, h| {
                let on = button.is_active();
                let r = if on { 8.0 } else { 10.0 };
                let fg = area.color();
                let (fr, fg_, fb) = (fg.red().into(), fg.green().into(), fg.blue().into());
                match color() {
                    Some(c) => {
                        circle(cr, w, h, r);
                        cr.set_source_rgb(c.r, c.g, c.b);
                        let _ = cr.fill_preserve();
                        cr.set_source_rgba(fr, fg_, fb, 0.22);
                        cr.set_line_width(1.0);
                        let _ = cr.stroke();
                    }
                    None => rainbow(cr, w, h, r),
                }
                if on {
                    cr.set_source_rgba(fr, fg_, fb, fg.alpha().into());
                    cr.set_line_width(2.0);
                    circle(cr, w, h, 11.0);
                    let _ = cr.stroke();
                }
            }
        ));
        button.connect_toggled(glib::clone!(
            #[weak]
            area,
            move |_| area.queue_draw()
        ));
        area
    }

    /// Prikk i tekstfargen, så stor som streken den står for.
    fn thick_dot(diameter: f64) -> gtk::DrawingArea {
        let area = gtk::DrawingArea::new();
        area.set_content_width(DOT_BOX);
        area.set_content_height(DOT_BOX);
        area.set_halign(gtk::Align::Center);
        area.set_valign(gtk::Align::Center);
        area.set_draw_func(move |area, cr, w, h| paint_dot(area, cr, w, h, diameter));
        area
    }
}

glib::wrapper! {
    pub struct AnnotateWindow(ObjectSubclass<imp::AnnotateWindow>)
        @extends adw::Window, gtk::Window, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget,
                    gtk::Native, gtk::Root, gtk::ShortcutManager;
}

impl AnnotateWindow {
    pub fn new(
        app: &adw::Application,
        path: &Path,
        image: Loaded,
        on_event: impl Fn(AnnotateEvent) + 'static,
    ) -> Self {
        let obj: Self = glib::Object::builder()
            .property("application", app)
            .property("title", trc("annotation-title", "Annoter"))
            .build();
        let imp = obj.imp();
        imp.path.set(path.to_path_buf()).ok();
        imp.on_event.replace(Some(Box::new(on_event)));

        let size = (f64::from(image.width), f64::from(image.height));
        let display = WidgetExt::display(&obj);
        install_css(&display);
        let (width, height) = default_size(app, &display, size);
        obj.set_default_size(width, height);

        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let canvas = Canvas::new(image);
        imp.build(canvas.clone(), &name);
        obj.update_property(&[gtk::accessible::Property::Label(tr("Annoter skjermbilde"))]);
        gtk::prelude::GtkWindowExt::set_focus(&obj, Some(&canvas));
        obj
    }

    pub fn path(&self) -> Option<&Path> {
        self.imp().path.get().map(PathBuf::as_path)
    }

    pub fn canvas(&self) -> &Canvas {
        self.imp().canvas()
    }
}

/// Størrelsen ut fra skjermen vinduet trolig havner på. Lerretet tilpasser seg uansett.
fn default_size(app: &adw::Application, display: &gdk::Display, img: (f64, f64)) -> (i32, i32) {
    let monitor = app
        .active_window()
        .and_then(|w| w.surface())
        .and_then(|s| display.monitor_at_surface(&s))
        .or_else(|| {
            display
                .monitors()
                .item(0)
                .and_then(|m| m.downcast::<gdk::Monitor>().ok())
        });
    let Some(monitor) = monitor else {
        return (1000, 700);
    };
    let geometry = monitor.geometry();
    view::window_size(
        img,
        (f64::from(geometry.width()), f64::from(geometry.height())),
        monitor.scale(),
    )
}
