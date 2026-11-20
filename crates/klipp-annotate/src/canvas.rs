//! Lerretet: viser bildet, tar imot tegning og holder operasjonslista.
//!
//! Originalen og det ferdig innbrente bildet ligger som uforanderlige `glib::Bytes`. Det
//! innbrente deler minne med teksturen, så et åpent bilde koster to kopier. Mens man drar
//! tegnes bare utkastet, innenfor sine egne grenser, så store bilder flyter like fint som
//! små: selve bildet ligger allerede på skjermkortet.

use crate::file::Loaded;
use crate::ops::{
    self, COLORS, Color, DEFAULT_THICK, HIGHLIGHT_WIDTH, History, LINE_HEIGHT, Op, THICK, Tool,
};
use crate::pixels::{self, BPP, Region};
use crate::raster;
use crate::view::{Stage, View};
use adw::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gdk, glib, graphene, gsk};
use klipp_system::i18n::tr;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, Ordering};

/// Et klikk uten å dra skal ikke gi en operasjon. I logiske piksler.
const MIN_DRAG: f64 = 3.0;
const MIN_BLOCK: f64 = 6.0;
/// Halve ruta i sjakkbrettet, `background-size: 12px`.
const CHECK: f32 = 6.0;
const TEXT_MIN_WIDTH: i32 = 40;
/// Hvor nær viskelæret må være en strek, i logiske piksler.
const ERASE_TOLERANCE: f64 = 6.0;
/// Markeringen av det viskelæret tar: rød som `--destructive-color`.
const ERASE_MARK: (f64, f64, f64) = (0.88, 0.11, 0.14);
const ERASE_DASH: [f64; 2] = [5.0, 3.0];

/// Stilklasse for hvert tekstfelt, siden fargen og størrelsen hører til det enkelte feltet.
static NEXT_TEXT_CLASS: AtomicU32 = AtomicU32::new(0);

/// Bilde klart til visning og lagring.
#[derive(Clone)]
pub struct Flat {
    pub bytes: glib::Bytes,
    pub width: u32,
    pub height: u32,
    pub alpha: bool,
}

pub struct Image {
    width: i32,
    height: i32,
    alpha: bool,
    original: glib::Bytes,
    committed: glib::Bytes,
    texture: gdk::MemoryTexture,
}

/// Tekst under arbeid. Plassering og størrelse er i bildekoordinater.
pub struct TextEdit {
    view: gtk::TextView,
    x: f64,
    y: f64,
    size: f64,
    c: Color,
}

fn texture(bytes: &glib::Bytes, width: i32, height: i32) -> gdk::MemoryTexture {
    gdk::MemoryTexture::new(
        width,
        height,
        gdk::MemoryFormat::B8g8r8a8Premultiplied,
        bytes,
        width as usize * BPP,
    )
}

mod imp {
    use super::*;

    pub struct Canvas {
        pub image: RefCell<Option<Image>>,
        pub history: RefCell<History<Op>>,
        pub draft: RefCell<Option<Op>>,
        pub view: Cell<Option<View>>,
        pub text: RefCell<Option<TextEdit>>,
        pub preedit: Rc<Cell<bool>>,
        pub tool: Cell<Tool>,
        pub color: Cell<Color>,
        pub thick: Cell<usize>,
        /// Det viskelæret har tatt så langt i dette draget.
        pub erasing: RefCell<Option<Vec<usize>>>,
        /// Det viskelæret står over nå.
        pub hover: Cell<Option<usize>>,
        pub text_css: gtk::CssProvider,
        pub text_class: String,
        pub scale_watch: RefCell<Option<(gdk::Surface, glib::SignalHandlerId)>>,
    }

    impl Default for Canvas {
        fn default() -> Self {
            let id = NEXT_TEXT_CLASS.fetch_add(1, Ordering::Relaxed);
            Self {
                image: RefCell::default(),
                history: RefCell::default(),
                draft: RefCell::default(),
                view: Cell::default(),
                text: RefCell::default(),
                preedit: Rc::default(),
                tool: Cell::new(Tool::Arrow),
                color: Cell::new(COLORS[0].color),
                thick: Cell::new(DEFAULT_THICK),
                erasing: RefCell::default(),
                hover: Cell::default(),
                text_css: gtk::CssProvider::new(),
                text_class: format!("klipp-text-{id}"),
                scale_watch: RefCell::default(),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Canvas {
        const NAME: &'static str = "KlippAnnotateCanvas";
        type Type = super::Canvas;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for Canvas {
        fn signals() -> &'static [glib::subclass::Signal] {
            static SIGNALS: OnceLock<Vec<glib::subclass::Signal>> = OnceLock::new();
            SIGNALS.get_or_init(|| vec![glib::subclass::Signal::builder("history-changed").build()])
        }

        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            obj.set_focusable(true);
            obj.set_hexpand(true);
            obj.set_vexpand(true);
            obj.set_overflow(gtk::Overflow::Hidden);
            obj.add_css_class("view");
            obj.update_property(&[gtk::accessible::Property::Label(tr("Annoter skjermbilde"))]);
            self.setup_input();
        }

        fn dispose(&self) {
            self.close_text();
        }
    }

    impl WidgetImpl for Canvas {
        fn measure(&self, _: gtk::Orientation, _: i32) -> (i32, i32, i32, i32) {
            (160, 160, -1, -1)
        }

        fn realize(&self) {
            self.parent_realize();
            let obj = self.obj();
            gtk::style_context_add_provider_for_display(
                &obj.display(),
                &self.text_css,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
            if let Some(surface) = obj.native().and_then(|n| n.surface()) {
                let id = surface.connect_scale_notify(glib::clone!(
                    #[weak]
                    obj,
                    move |_| obj.queue_allocate()
                ));
                self.scale_watch.replace(Some((surface, id)));
            }
        }

        fn unrealize(&self) {
            if let Some((surface, id)) = self.scale_watch.take() {
                surface.disconnect(id);
            }
            gtk::style_context_remove_provider_for_display(&self.obj().display(), &self.text_css);
            self.parent_unrealize();
        }

        fn size_allocate(&self, width: i32, height: i32, _baseline: i32) {
            let Some((iw, ih)) = self.image_size() else {
                return;
            };
            let view = View::fit(iw, ih, self.stage(width, height));
            let rescaled = self.view.get().is_some_and(|old| old.scale != view.scale);
            self.view.set(Some(view));
            if rescaled && self.text.borrow().is_some() {
                // Som i prototypen: størrelsen ble valgt for den gamle skalaen.
                glib::idle_add_local_once(glib::clone!(
                    #[weak(rename_to = imp)]
                    self,
                    move || imp.commit_text()
                ));
            }
            self.allocate_text(&view);
        }

        fn snapshot(&self, sn: &gtk::Snapshot) {
            let obj = self.obj();
            let image = self.image.borrow();
            let (Some(v), Some(image)) = (self.view.get(), image.as_ref()) else {
                return;
            };
            let bounds = graphene::Rect::new(v.x as f32, v.y as f32, v.w as f32, v.h as f32);
            // Samme farge som --border-color i Adwaita.
            let border = obj.color().with_alpha(0.15);
            sn.append_outset_shadow(
                &gsk::RoundedRect::from_rect(bounds, 0.0),
                &border,
                0.0,
                0.0,
                1.0,
                0.0,
            );
            if image.alpha {
                checkerboard(sn, &bounds, adw::StyleManager::default().is_dark());
            }
            let filter = if v.is_minified() {
                gsk::ScalingFilter::Trilinear
            } else {
                gsk::ScalingFilter::Nearest
            };
            append_device_texture(sn, &image.texture, filter, &bounds, v.device_scale);
            self.snapshot_draft(sn, &v, image);
            self.snapshot_erase(sn, &v);
            if let Some(text) = self.text.borrow().as_ref() {
                obj.snapshot_child(&text.view, sn);
            }
        }
    }

    impl Canvas {
        pub fn image_size(&self) -> Option<(f64, f64)> {
            let image = self.image.borrow();
            let image = image.as_ref()?;
            Some((f64::from(image.width), f64::from(image.height)))
        }

        fn stage(&self, width: i32, height: i32) -> Stage {
            let obj = self.obj();
            let mut device_scale = f64::from(obj.scale_factor());
            let mut origin = (0.0, 0.0);
            if let Some(native) = obj.native() {
                if let Some(surface) = native.surface() {
                    device_scale = surface.scale();
                }
                let (sx, sy) = native.surface_transform();
                let zero = graphene::Point::new(0.0, 0.0);
                let at = obj.compute_point(&native, &zero).unwrap_or(zero);
                origin = (f64::from(at.x()) + sx, f64::from(at.y()) + sy);
            }
            Stage {
                width: f64::from(width),
                height: f64::from(height),
                device_scale,
                origin,
            }
        }

        fn family(&self) -> String {
            self.obj()
                .pango_context()
                .font_description()
                .and_then(|f| f.family())
                .map_or_else(|| "Sans".to_string(), |f| f.to_string())
        }

        fn snapshot_draft(&self, sn: &gtk::Snapshot, v: &View, image: &Image) {
            let draft = self.draft.borrow();
            let Some(op) = draft.as_ref() else {
                return;
            };
            if let Op::Redact {
                x1,
                y1,
                x2,
                y2,
                block,
            } = *op
            {
                let (w, h) = (image.width as usize, image.height as usize);
                let (x, y) = (x1.min(x2), y1.min(y2));
                let (rw, rh) = ((x2 - x1).abs(), (y2 - y1).abs());
                let Some(r) = Region::of_rect(x, y, rw, rh, w, h) else {
                    return;
                };
                let grid = pixels::block_preview(&image.committed, w, r, block);
                let tex = gdk::MemoryTexture::new(
                    grid.cols as i32,
                    grid.rows as i32,
                    gdk::MemoryFormat::B8g8r8a8Premultiplied,
                    &glib::Bytes::from_owned(grid.data),
                    grid.cols * BPP,
                );
                let clip = view_rect(v, (r.x0, r.y0), (r.x1, r.y1));
                let far = (
                    grid.origin.0 + grid.cols * grid.block,
                    grid.origin.1 + grid.rows * grid.block,
                );
                let bounds = view_rect(v, grid.origin, far);
                sn.push_clip(&clip);
                append_device_texture(
                    sn,
                    &tex,
                    gsk::ScalingFilter::Nearest,
                    &bounds,
                    v.device_scale,
                );
                sn.pop();
                return;
            }
            let (x0, y0, x1, y1) = op.bounds();
            let (vx0, vy0) = v.to_view(x0.max(0.0), y0.max(0.0));
            let (vx1, vy1) = v.to_view(x1.min(v.img_w), y1.min(v.img_h));
            if vx1 <= vx0 || vy1 <= vy0 {
                return;
            }
            let bounds = graphene::Rect::new(
                vx0 as f32,
                vy0 as f32,
                (vx1 - vx0) as f32,
                (vy1 - vy0) as f32,
            );
            let cr = sn.append_cairo(&bounds);
            cr.translate(v.x, v.y);
            cr.scale(v.w / v.img_w, v.h / v.img_h);
            let _ = raster::draw(&cr, op, &self.family());
        }

        /// Stiplet ramme rundt det viskelæret tar, og det det står over.
        fn snapshot_erase(&self, sn: &gtk::Snapshot, v: &View) {
            let mut marked: Vec<usize> = self.erasing.borrow().clone().unwrap_or_default();
            if let Some(i) = self.hover.get().filter(|i| !marked.contains(i)) {
                marked.push(i);
            }
            if marked.is_empty() {
                return;
            }
            let history = self.history.borrow();
            let ops = history.ops();
            let canvas = graphene::Rect::new(
                0.0,
                0.0,
                self.obj().width() as f32,
                self.obj().height() as f32,
            );
            let cr = sn.append_cairo(&canvas);
            let pad = 3.0;
            let (r, g, b) = ERASE_MARK;
            for op in marked.iter().filter_map(|&i| ops.get(i)) {
                let (x0, y0, x1, y1) = op.bounds();
                let (vx0, vy0) = v.to_view(x0.max(0.0), y0.max(0.0));
                let (vx1, vy1) = v.to_view(x1.min(v.img_w), y1.min(v.img_h));
                cr.rectangle(
                    vx0 - pad,
                    vy0 - pad,
                    vx1 - vx0 + pad * 2.0,
                    vy1 - vy0 + pad * 2.0,
                );
            }
            cr.set_source_rgba(r, g, b, 0.14);
            let _ = cr.fill_preserve();
            // Lys kant under den stiplede, så rammen synes på mørke bilder også.
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.75);
            cr.set_line_width(3.0);
            let _ = cr.stroke_preserve();
            cr.set_source_rgb(r, g, b);
            cr.set_line_width(1.5);
            cr.set_dash(&ERASE_DASH, 0.0);
            let _ = cr.stroke();
        }

        /// Viskelæret tar det øverste som ligger under punktet.
        fn erase_at(&self, v: &View, px: f64, py: f64) {
            let (x, y) = v.to_image(px, py);
            let hit = ops::top_hit(
                self.history.borrow().ops(),
                x,
                y,
                ERASE_TOLERANCE * v.unit(),
            );
            let mut erasing = self.erasing.borrow_mut();
            let (Some(i), Some(taken)) = (hit, erasing.as_mut()) else {
                return;
            };
            if !taken.contains(&i) {
                taken.push(i);
                self.obj().queue_draw();
            }
        }

        fn update_hover(&self, px: f64, py: f64) {
            let hit = match (self.tool.get(), self.view.get()) {
                (Tool::Eraser, Some(v)) if v.contains(px, py) => {
                    let (x, y) = v.to_image(px, py);
                    ops::top_hit(
                        self.history.borrow().ops(),
                        x,
                        y,
                        ERASE_TOLERANCE * v.unit(),
                    )
                }
                _ => None,
            };
            if self.hover.replace(hit) != hit {
                self.obj().queue_draw();
            }
        }

        pub fn clear_hover(&self) {
            if self.hover.take().is_some() {
                self.obj().queue_draw();
            }
        }

        fn setup_input(&self) {
            let obj = self.obj();
            let drag = gtk::GestureDrag::new();
            drag.connect_drag_begin(glib::clone!(
                #[weak(rename_to = imp)]
                self,
                move |g, x, y| imp.press(g, x, y)
            ));
            drag.connect_drag_update(glib::clone!(
                #[weak(rename_to = imp)]
                self,
                move |g, dx, dy| {
                    if let Some((x, y)) = g.start_point() {
                        imp.drag_to(x + dx, y + dy);
                    }
                }
            ));
            drag.connect_drag_end(glib::clone!(
                #[weak(rename_to = imp)]
                self,
                move |_, _, _| imp.release()
            ));
            drag.connect_cancel(glib::clone!(
                #[weak(rename_to = imp)]
                self,
                move |_, _| {
                    imp.cancel_draft();
                }
            ));
            obj.add_controller(drag);

            let motion = gtk::EventControllerMotion::new();
            motion.connect_motion(glib::clone!(
                #[weak(rename_to = imp)]
                self,
                move |_, x, y| {
                    imp.update_hover(x, y);
                    imp.update_cursor(x, y);
                }
            ));
            motion.connect_leave(glib::clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| imp.clear_hover()
            ));
            obj.add_controller(motion);
        }

        fn update_cursor(&self, x: f64, y: f64) {
            let inside = self.view.get().is_some_and(|v| v.contains(x, y));
            let name = match (inside, self.tool.get()) {
                (false, _) => None,
                (true, Tool::Text) => Some("text"),
                (true, Tool::Eraser) if self.hover.get().is_some() => Some("pointer"),
                (true, _) => Some("crosshair"),
            };
            self.obj().set_cursor_from_name(name);
        }

        fn press(&self, g: &gtk::GestureDrag, px: f64, py: f64) {
            let deny = || {
                g.set_state(gtk::EventSequenceState::Denied);
            };
            if self.text.borrow().is_some() {
                self.commit_text();
                deny();
                return;
            }
            let Some(v) = self.view.get().filter(|v| v.contains(px, py)) else {
                deny();
                return;
            };
            g.set_state(gtk::EventSequenceState::Claimed);
            self.obj().grab_focus();
            let (x, y) = v.to_image(px, py);
            let k = v.unit();
            let th = &THICK[self.thick.get()];
            let (lw, c) = (th.line * k, self.color.get());
            let draft = match self.tool.get() {
                Tool::Eraser => {
                    self.erasing.replace(Some(Vec::new()));
                    self.erase_at(&v, px, py);
                    return;
                }
                Tool::Text => {
                    self.begin_text(&v, x, y);
                    deny();
                    return;
                }
                Tool::Marker => {
                    let n = self.history.borrow().next_marker();
                    self.push(Op::Marker {
                        x,
                        y,
                        r: th.r * k,
                        c,
                        fg: c.contrast(),
                        n,
                    });
                    deny();
                    return;
                }
                Tool::Arrow => Op::Arrow {
                    x1: x,
                    y1: y,
                    x2: x,
                    y2: y,
                    lw,
                    c,
                },
                Tool::Line => Op::Line {
                    x1: x,
                    y1: y,
                    x2: x,
                    y2: y,
                    lw,
                    c,
                },
                Tool::Rect => Op::Rect {
                    x1: x,
                    y1: y,
                    x2: x,
                    y2: y,
                    lw,
                    c,
                },
                Tool::Ellipse => Op::Ellipse {
                    x1: x,
                    y1: y,
                    x2: x,
                    y2: y,
                    lw,
                    c,
                },
                Tool::Free => Op::Free {
                    pts: vec![(x, y)],
                    lw,
                    c,
                },
                Tool::Highlight => Op::Highlight {
                    pts: vec![(x, y)],
                    lw: lw * HIGHLIGHT_WIDTH,
                    c,
                },
                Tool::Redact => Op::Redact {
                    x1: x,
                    y1: y,
                    x2: x,
                    y2: y,
                    block: (th.block * k).max(MIN_BLOCK),
                },
            };
            self.draft.replace(Some(draft));
        }

        fn drag_to(&self, px: f64, py: f64) {
            let Some(v) = self.view.get() else {
                return;
            };
            if self.erasing.borrow().is_some() {
                self.erase_at(&v, px, py);
                return;
            }
            if let Some(op) = self.draft.borrow_mut().as_mut() {
                let (x, y) = v.to_image(px, py);
                op.drag_to(x, y);
            } else {
                return;
            }
            self.obj().queue_draw();
        }

        fn release(&self) {
            if let Some(targets) = self.erasing.take() {
                if targets.is_empty() {
                    self.obj().queue_draw();
                } else {
                    self.hover.set(None);
                    self.push(Op::Erase { targets });
                }
                return;
            }
            let Some(op) = self.draft.take() else {
                return;
            };
            let min = MIN_DRAG * self.view.get().map_or(1.0, |v| v.unit());
            if op.is_big_enough(min) {
                self.push(op);
            } else {
                self.obj().queue_draw();
            }
        }

        /// Forkaster utkastet. Svarer om det fantes et.
        pub fn cancel_draft(&self) -> bool {
            let had = self.draft.take().is_some() | self.erasing.take().is_some();
            if had {
                self.obj().queue_draw();
            }
            had
        }

        fn install(&self, surface: gtk::cairo::ImageSurface) -> Result<(), String> {
            let bytes = raster::into_bytes(surface)?;
            if let Some(image) = self.image.borrow_mut().as_mut() {
                image.texture = texture(&bytes, image.width, image.height);
                image.committed = bytes;
            }
            self.obj().queue_draw();
            Ok(())
        }

        /// Spiller av `ops` fra originalen.
        fn replay(&self, ops: &[Op]) -> Result<(), String> {
            let surface = {
                let image = self.image.borrow();
                let image = image.as_ref().ok_or("Bildet er lukket")?;
                raster::replay(
                    &image.original,
                    image.width,
                    image.height,
                    ops,
                    &self.family(),
                )?
            };
            self.install(surface)
        }

        /// Brenner `op` inn i det som allerede er tegnet.
        fn burn(&self, op: &Op) -> Result<(), String> {
            let mut surface = {
                let image = self.image.borrow();
                let image = image.as_ref().ok_or("Bildet er lukket")?;
                raster::surface_from(&image.committed, image.width, image.height)?
            };
            raster::burn(&mut surface, op, &self.family())?;
            self.install(surface)
        }

        fn changed(&self) {
            self.hover.set(None);
            self.obj().emit_by_name::<()>("history-changed", &[]);
        }

        /// Viskelæret kan ikke brennes inn oppå, så da spilles alt av på nytt.
        fn apply(&self, op: &Op) -> Result<(), String> {
            if op.needs_replay() {
                let mut ops = self.history.borrow().ops().to_vec();
                ops.push(op.clone());
                self.replay(&ops)
            } else {
                self.burn(op)
            }
        }

        fn push(&self, op: Op) {
            match self.apply(&op) {
                Ok(()) => self.history.borrow_mut().push(op),
                Err(e) => glib::g_warning!("klipp", "{e}"),
            }
            self.changed();
        }

        pub fn undo(&self) {
            if self.draft.borrow().is_some() {
                return;
            }
            let ops = self.history.borrow().ops().to_vec();
            let Some((_, rest)) = ops.split_last() else {
                return;
            };
            match self.replay(rest) {
                Ok(()) => {
                    self.history.borrow_mut().undo();
                }
                Err(e) => glib::g_warning!("klipp", "{e}"),
            }
            self.changed();
        }

        pub fn redo(&self) {
            if self.draft.borrow().is_some() {
                return;
            }
            // Viskelæret spilles av mot lista slik den var før det, så den flyttes etterpå.
            let Some(op) = self.history.borrow().next_redo().cloned() else {
                return;
            };
            match self.apply(&op) {
                Ok(()) => {
                    self.history.borrow_mut().redo();
                }
                Err(e) => glib::g_warning!("klipp", "{e}"),
            }
            self.changed();
        }

        fn begin_text(&self, v: &View, x: f64, y: f64) {
            let obj = self.obj();
            let th = &THICK[self.thick.get()];
            let c = self.color.get();
            let (x, y) = v.text_anchor(x, y, th.text);
            let family = self.family().replace('"', "");
            self.text_css.load_from_string(&format!(
                ".{class} {{ color: {hex}; caret-color: {hex}; font-family: \"{family}\"; \
                 font-weight: 600; font-size: {px}px; line-height: {lh}px; }}",
                class = self.text_class,
                hex = c.hex(),
                px = th.text,
                lh = th.text * LINE_HEIGHT,
            ));
            let view = gtk::TextView::new();
            view.set_css_classes(&["klipp-text", &self.text_class]);
            view.set_wrap_mode(gtk::WrapMode::None);
            view.set_accepts_tab(false);
            view.update_property(&[gtk::accessible::Property::Label(tr("Tekst i bildet"))]);
            view.set_parent(&*obj);

            view.buffer().connect_changed(glib::clone!(
                #[weak]
                obj,
                move |_| obj.queue_allocate()
            ));
            self.preedit.set(false);
            view.connect_preedit_changed(glib::clone!(
                #[strong(rename_to = preedit)]
                self.preedit,
                move |_, text| preedit.set(!text.is_empty())
            ));

            let keys = gtk::EventControllerKey::new();
            keys.set_propagation_phase(gtk::PropagationPhase::Capture);
            keys.connect_key_pressed(glib::clone!(
                #[weak(rename_to = imp)]
                self,
                #[upgrade_or]
                glib::Propagation::Proceed,
                move |_, key, _, state| imp.text_key(key, state)
            ));
            view.add_controller(keys);

            let focus = gtk::EventControllerFocus::new();
            focus.connect_leave(glib::clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    // Fokus lander først etterpå, og en meny i feltet er ikke å forlate det.
                    glib::idle_add_local_once(glib::clone!(
                        #[weak]
                        imp,
                        move || imp.text_focus_left()
                    ));
                }
            ));
            view.add_controller(focus);

            self.text.replace(Some(TextEdit {
                view: view.clone(),
                x,
                y,
                size: th.text * v.unit(),
                c,
            }));
            obj.queue_allocate();
            view.grab_focus();
        }

        fn text_focus_left(&self) {
            let text = self.text.borrow();
            let Some(view) = text.as_ref().map(|t| t.view.clone()) else {
                return;
            };
            drop(text);
            let window = self.obj().root().and_downcast::<gtk::Window>();
            let active = window.as_ref().is_some_and(|w| w.is_active());
            let inside = window
                .and_then(|w| gtk::prelude::GtkWindowExt::focus(&w))
                .is_some_and(|f| f == view || f.is_ancestor(&view));
            if !(active && inside) {
                self.commit_text();
            }
        }

        fn text_key(&self, key: gdk::Key, state: gdk::ModifierType) -> glib::Propagation {
            if self.preedit.get() {
                return glib::Propagation::Proceed;
            }
            let enter = matches!(
                key,
                gdk::Key::Return | gdk::Key::KP_Enter | gdk::Key::ISO_Enter
            );
            if enter && !state.contains(gdk::ModifierType::SHIFT_MASK) {
                self.commit_text();
                glib::Propagation::Stop
            } else if key == gdk::Key::Escape {
                self.drop_text();
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        }

        fn close_text(&self) -> Option<TextEdit> {
            let text = self.text.take()?;
            text.view.unparent();
            self.obj().queue_allocate();
            Some(text)
        }

        pub fn commit_text(&self) {
            let Some(text) = self.text.borrow().as_ref().map(|t| t.view.clone()) else {
                return;
            };
            let buffer = text.buffer();
            let value = buffer
                .text(&buffer.start_iter(), &buffer.end_iter(), false)
                .trim_end()
                .to_string();
            self.obj().grab_focus();
            let Some(t) = self.close_text() else {
                return;
            };
            if !value.is_empty() {
                self.push(Op::Text {
                    x: t.x,
                    y: t.y,
                    size: t.size,
                    c: t.c,
                    text: value,
                });
            }
        }

        pub fn drop_text(&self) {
            if self.text.borrow().is_some() {
                self.obj().grab_focus();
                self.close_text();
            }
        }

        fn allocate_text(&self, v: &View) {
            let text = self.text.borrow();
            let Some(t) = text.as_ref() else {
                return;
            };
            let (min_w, ..) = t.view.measure(gtk::Orientation::Horizontal, -1);
            let (min_h, nat_h, ..) = t.view.measure(gtk::Orientation::Vertical, -1);
            // Lengste linje og ett tegn til, så teksten ikke rykker i det man skriver.
            let buffer = t.view.buffer();
            let text = buffer.text(&buffer.start_iter(), &buffer.end_iter(), false);
            let widest = text
                .split('\n')
                .map(|line| {
                    let sample = format!("{line}M");
                    t.view.create_pango_layout(Some(&sample)).pixel_size().0
                })
                .max()
                .unwrap_or(0);
            let w = widest.max(TEXT_MIN_WIDTH).max(min_w);
            let (x, y) = v.to_view(t.x, t.y);
            let at = graphene::Point::new(x as f32, y as f32);
            t.view.allocate(
                w,
                nat_h.max(min_h),
                -1,
                Some(gsk::Transform::new().translate(&at)),
            );
        }
    }

    /// Tegner en tekstur med enhetspiksler som koordinater. Ved annen skala enn 1 går GSK
    /// ellers via en mellombuffer i logisk oppløsning, som gjør bildet uskarpt på HiDPI.
    fn append_device_texture(
        sn: &gtk::Snapshot,
        texture: &impl IsA<gdk::Texture>,
        filter: gsk::ScalingFilter,
        bounds: &graphene::Rect,
        device_scale: f64,
    ) {
        let d = device_scale as f32;
        let device = graphene::Rect::new(
            bounds.x() * d,
            bounds.y() * d,
            bounds.width() * d,
            bounds.height() * d,
        );
        sn.save();
        sn.scale(1.0 / d, 1.0 / d);
        sn.append_scaled_texture(texture, filter, &device);
        sn.restore();
    }

    fn view_rect(v: &View, from: (usize, usize), to: (usize, usize)) -> graphene::Rect {
        let (x0, y0) = v.to_view(from.0 as f64, from.1 as f64);
        let (x1, y1) = v.to_view(to.0 as f64, to.1 as f64);
        graphene::Rect::new(x0 as f32, y0 as f32, (x1 - x0) as f32, (y1 - y0) as f32)
    }

    /// Sjakkbrett som viser transparens, med fargene fra prototypen.
    fn checkerboard(sn: &gtk::Snapshot, bounds: &graphene::Rect, dark: bool) {
        let (a, b) = if dark {
            (rgb(0x3a, 0x3a, 0x3e), rgb(0x2c, 0x2c, 0x30))
        } else {
            (rgb(0xff, 0xff, 0xff), rgb(0xe4, 0xe4, 0xe8))
        };
        let (x, y) = (bounds.x(), bounds.y());
        let tile = graphene::Rect::new(x, y, CHECK * 2.0, CHECK * 2.0);
        sn.push_repeat(bounds, Some(&tile));
        sn.append_color(&a, &tile);
        sn.append_color(&b, &graphene::Rect::new(x, y, CHECK, CHECK));
        sn.append_color(&b, &graphene::Rect::new(x + CHECK, y + CHECK, CHECK, CHECK));
        sn.pop();
    }

    fn rgb(r: u8, g: u8, b: u8) -> gdk::RGBA {
        let f = |v: u8| f32::from(v) / 255.0;
        gdk::RGBA::new(f(r), f(g), f(b), 1.0)
    }
}

glib::wrapper! {
    pub struct Canvas(ObjectSubclass<imp::Canvas>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Canvas {
    pub fn new(image: Loaded) -> Self {
        let obj: Self = glib::Object::new();
        let (width, height) = (image.width as i32, image.height as i32);
        let original = glib::Bytes::from_owned(image.pixels);
        let tex = texture(&original, width, height);
        obj.imp().image.replace(Some(Image {
            width,
            height,
            alpha: image.alpha,
            committed: original.clone(),
            original,
            texture: tex,
        }));
        obj
    }

    pub fn connect_history_changed(&self, f: impl Fn(&Self) + 'static) {
        self.connect_local("history-changed", false, move |args| {
            let canvas = args[0].get::<Self>().expect("signalen kommer fra lerretet");
            f(&canvas);
            None
        });
    }

    pub fn set_tool(&self, tool: Tool) {
        self.commit_text();
        self.imp().cancel_draft();
        self.imp().clear_hover();
        self.imp().tool.set(tool);
    }

    pub fn set_color(&self, c: Color) {
        self.commit_text();
        self.imp().color.set(c);
    }

    /// Fargen det tegnes med. Ikke `color()`, den er GTKs tekstfarge.
    pub fn ink(&self) -> Color {
        self.imp().color.get()
    }

    pub fn set_thick(&self, i: usize) {
        self.commit_text();
        self.imp().thick.set(i);
    }

    pub fn thick(&self) -> usize {
        self.imp().thick.get()
    }

    pub fn undo(&self) {
        self.commit_text();
        self.imp().undo();
    }

    pub fn redo(&self) {
        self.commit_text();
        self.imp().redo();
    }

    pub fn can_undo(&self) -> bool {
        self.imp().history.borrow().can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.imp().history.borrow().can_redo()
    }

    pub fn is_empty(&self) -> bool {
        self.imp().history.borrow().is_empty()
    }

    pub fn is_editing_text(&self) -> bool {
        self.imp().text.borrow().is_some()
    }

    pub fn cancel_draft(&self) -> bool {
        self.imp().cancel_draft()
    }

    pub fn commit_text(&self) {
        self.imp().commit_text();
    }

    /// Bildet med alt tegnet brent inn. Deler minne og kan sendes til en bakgrunnstråd.
    pub fn flat(&self) -> Option<Flat> {
        let image = self.imp().image.borrow();
        let image = image.as_ref()?;
        Some(Flat {
            bytes: image.committed.clone(),
            width: image.width as u32,
            height: image.height as u32,
            alpha: image.alpha,
        })
    }
}
