//! Fullskjerm-overlegget: ett vindu per skjerm med det frosne bildet.

use super::geometry::{
    self, Bar, Escaped, Mode, Phase, Pt, Rect, Released, Selector, handle_points, mode_hint,
    mode_icon, mode_label,
};
use super::{Cut, Frozen, shell};
use crate::app::Klipp;
use crate::clipboard::{self, Source};
use crate::keys;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{cairo, gdk, glib, graphene, gsk};
use klipp_system::Action;
use std::cell::{Cell, OnceCell, RefCell};
use std::rc::{Rc, Weak};
use std::time::Duration;

/// Hvor mørk skjermen blir utenfor utvalget.
const DIM: f32 = 0.45;
const FADE_IN_MS: u32 = 110;
const FADE_OUT_MS: u32 = 80;
/// Uten utvidelsen zoomer GNOME vinduet opp først. Dempingen venter til det er over.
const GNOME_OPEN_ANIMATION_MS: u32 = 150;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Canvas {
        pub image: RefCell<Option<gsk::CairoNode>>,
        pub selector: RefCell<Option<Selector>>,
        pub session: RefCell<Weak<Session>>,
        pub index: Cell<usize>,
        pub parts: OnceCell<Parts>,
        pub start: Cell<(f64, f64)>,
        /// Demping akkurat nå, 0 til `DIM`.
        pub dim: Cell<f32>,
        /// Øker for hver toning, så en gammel toning vet at den er avløst.
        pub fade: Cell<u32>,
    }

    pub struct Parts {
        pub bar: gtk::Box,
        pub toggles: adw::ToggleGroup,
        pub actions: gtk::Box,
        pub dims: gtk::Label,
        pub hint: gtk::Label,
        pub size: gtk::Label,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Canvas {
        const NAME: &'static str = "KlippOverlayCanvas";
        type Type = super::Canvas;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("klipp-overlay");
        }
    }

    impl ObjectImpl for Canvas {
        fn dispose(&self) {
            if let Some(p) = self.parts.get() {
                p.bar.unparent();
                p.actions.unparent();
                p.hint.unparent();
                p.size.unparent();
            }
            self.image.replace(None);
        }
    }

    fn natural(w: &impl IsA<gtk::Widget>) -> (i32, i32) {
        let (_, nw, _, _) = w.measure(gtk::Orientation::Horizontal, -1);
        let (_, nh, _, _) = w.measure(gtk::Orientation::Vertical, nw);
        (nw, nh)
    }

    impl WidgetImpl for Canvas {
        fn measure(&self, _: gtk::Orientation, _: i32) -> (i32, i32, i32, i32) {
            (0, 0, -1, -1)
        }

        fn size_allocate(&self, width: i32, height: i32, _baseline: i32) {
            let Some(p) = self.parts.get() else { return };
            let (fw, fh) = (f64::from(width), f64::from(height));
            let (bw, bh) = natural(&p.bar);
            let bx = (width - bw) / 2;
            p.bar
                .size_allocate(&gtk::Allocation::new(bx, 10, bw, bh), -1);

            if p.hint.is_visible() {
                let (hw, hh) = natural(&p.hint);
                p.hint
                    .size_allocate(&gtk::Allocation::new((width - hw) / 2, 68, hw, hh), -1);
            }

            let selector = self.selector.borrow();
            let Some(st) = selector.as_ref() else { return };
            let f = fw / f64::from(st.w.max(1));
            let css = st.sel.map(|s| {
                (
                    f64::from(s.x) * f,
                    f64::from(s.y) * f,
                    f64::from(s.right()) * f,
                    f64::from(s.bottom()) * f,
                )
            });
            if let (true, Some(sel)) = (p.size.is_visible(), css) {
                let (lw, lh) = natural(&p.size);
                let (x, y) = geometry::place_size_label(sel, f64::from(lw), f64::from(lh), fw, fh);
                p.size
                    .size_allocate(&gtk::Allocation::new(x as i32, y as i32, lw, lh), -1);
            }
            if let (true, Some(sel)) = (p.actions.is_visible(), css) {
                let (aw, ah) = natural(&p.actions);
                let bar = Bar {
                    left: f64::from(bx),
                    right: f64::from(bx + bw),
                    bottom: f64::from(10 + bh),
                };
                let (x, y) =
                    geometry::place_actions(sel, f64::from(aw), f64::from(ah), bar, fw, fh);
                p.actions
                    .size_allocate(&gtk::Allocation::new(x as i32, y as i32, aw, ah), -1);
            }
        }

        fn snapshot(&self, s: &gtk::Snapshot) {
            let w = self.obj();
            let selector = self.selector.borrow();
            let (Some(st), Some(image)) = (selector.as_ref(), self.image.borrow().clone()) else {
                return;
            };
            let f = w.width() as f32 / st.w.max(1) as f32;
            let (iw, ih) = (st.w as f32, st.h as f32);
            let all = graphene::Rect::new(0.0, 0.0, iw, ih);
            let lw = (1.0 / f).round().max(1.0);
            let dim = self.dim.get();
            let fading = dim < DIM;

            s.save();
            s.scale(f, f);
            s.append_node(&image);
            s.append_color(&gdk::RGBA::new(0.0, 0.0, 0.0, dim), &all);
            if fading {
                s.push_opacity(f64::from(dim / DIM));
            }
            let accent = adw::StyleManager::default().accent_color_rgba();
            let white = gdk::RGBA::new(1.0, 1.0, 1.0, 0.9);

            if st.mode == Mode::Freehand && st.pts.len() > 1 {
                let b = gsk::PathBuilder::new();
                for (i, p) in st.pts.iter().enumerate() {
                    if i == 0 {
                        b.move_to(p.x as f32, p.y as f32);
                    } else {
                        b.line_to(p.x as f32, p.y as f32);
                    }
                }
                b.close();
                let path = b.to_path();
                s.push_fill(&path, gsk::FillRule::Winding);
                s.append_node(&image);
                s.pop();
                let stroke = gsk::Stroke::new(lw * 4.0);
                stroke.set_line_join(gsk::LineJoin::Round);
                s.append_stroke(&path, &stroke, &white);
                let stroke = gsk::Stroke::new(lw * 2.0);
                stroke.set_line_join(gsk::LineJoin::Round);
                s.append_stroke(&path, &stroke, &accent);
            } else if let Some(r) = st.sel.filter(|r| r.w > 0 && r.h > 0) {
                let rect = graphene::Rect::new(r.x as f32, r.y as f32, r.w as f32, r.h as f32);
                s.push_clip(&rect);
                s.append_node(&image);
                s.pop();
                ring(s, r, st.w, st.h, lw, lw * 4.0, &white);
                ring(s, r, st.w, st.h, lw, lw * 2.0, &accent);
                if st.mode == Mode::Area && st.phase == Phase::Selected {
                    let fd = f64::from(f);
                    let radius = (geometry::HANDLE_R / fd) as f32;
                    let stroke = gsk::Stroke::new(lw * 1.5);
                    for (_, x, y) in handle_points(r, st.w, st.h, fd) {
                        let b = gsk::PathBuilder::new();
                        b.add_circle(&graphene::Point::new(x as f32, y as f32), radius);
                        let path = b.to_path();
                        s.append_fill(&path, gsk::FillRule::Winding, &gdk::RGBA::WHITE);
                        s.append_stroke(&path, &stroke, &accent);
                    }
                }
            }
            if fading {
                s.pop();
            }
            s.restore();
            drop(selector);
            if fading {
                s.push_opacity(f64::from(dim / DIM));
            }
            self.parent_snapshot(s);
            if fading {
                s.pop();
            }
        }
    }

    /// Aksentlinje med lys kant, klemt inn så den også synes i skjermkantene.
    fn ring(s: &gtk::Snapshot, r: Rect, w: i32, h: i32, lw: f32, band: f32, c: &gdk::RGBA) {
        let half = band / 2.0;
        let cl = |v: f32, max: i32| v.max(half).min(max as f32 - half);
        let x0 = cl(r.x as f32 - lw, w);
        let y0 = cl(r.y as f32 - lw, h);
        let x1 = cl(r.right() as f32 + lw, w);
        let y1 = cl(r.bottom() as f32 + lw, h);
        let b = gsk::PathBuilder::new();
        b.add_rect(&graphene::Rect::new(x0, y0, x1 - x0, y1 - y0));
        s.append_stroke(&b.to_path(), &gsk::Stroke::new(band), c);
    }
}

glib::wrapper! {
    pub struct Canvas(ObjectSubclass<imp::Canvas>) @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Canvas {
    fn parts(&self) -> Option<&imp::Parts> {
        self.imp().parts.get()
    }

    fn session(&self) -> Option<Rc<Session>> {
        self.imp().session.borrow().upgrade()
    }

    fn index(&self) -> usize {
        self.imp().index.get()
    }

    fn with<R>(&self, f: impl FnOnce(&mut Selector) -> R) -> Option<R> {
        self.imp().selector.borrow_mut().as_mut().map(f)
    }

    /// Ton dempingen mot `to`, styrt av klokka så den følger skjermens egen frekvens.
    fn fade(&self, to: f32, delay_ms: u32, ms: u32) {
        let imp = self.imp();
        let mine = imp.fade.get() + 1;
        imp.fade.set(mine);
        let from = imp.dim.get();
        let start = Cell::new(None);
        self.add_tick_callback(move |c, clock| {
            let imp = c.imp();
            if imp.fade.get() != mine {
                return glib::ControlFlow::Break;
            }
            let now = clock.frame_time();
            let t0 = start.get().unwrap_or(now);
            start.set(Some(t0));
            let spent = (now - t0) as f64 / 1000.0 - f64::from(delay_ms);
            let p = (spent / f64::from(ms)).clamp(0.0, 1.0);
            let eased = 1.0 - (1.0 - p).powi(3);
            imp.dim.set(from + (to - from) * eased as f32);
            c.queue_draw();
            if p < 1.0 {
                glib::ControlFlow::Continue
            } else {
                glib::ControlFlow::Break
            }
        });
    }

    /// CSS-piksler per bildepiksel.
    fn css_per_px(&self) -> f64 {
        let w = self.with(|s| s.w).unwrap_or(1);
        f64::from(self.width().max(1)) / f64::from(w.max(1))
    }

    fn point(&self, x: f64, y: f64) -> Pt {
        let f = self.css_per_px();
        let (w, h) = self.with(|s| (s.w, s.h)).unwrap_or((1, 1));
        geometry::to_pixel(x, y, f, w, h)
    }

    fn sync_ui(&self) {
        let Some(p) = self.parts() else { return };
        let Some((mode, phase, warn, sel, act)) =
            self.with(|s| (s.mode, s.phase, s.warn, s.sel, s.act.is_some()))
        else {
            return;
        };
        let hint = mode_hint(mode);
        p.hint
            .set_visible(phase == Phase::Idle && (warn || !hint.is_empty()));
        if warn {
            p.hint.add_css_class("warn");
            p.hint
                .set_label("For lite område. Prøv igjen, eller trykk Esc.");
        } else {
            p.hint.remove_css_class("warn");
            p.hint.set_label(&format!("{hint} · Esc lukker"));
        }
        let dragging = phase == Phase::Drag || act;
        p.size.set_visible(dragging && sel.is_some());
        if let Some(s) = sel {
            let txt = format!("{} × {} px", s.w, s.h);
            p.size.set_label(&txt);
            if s.w < geometry::MIN_SIZE || s.h < geometry::MIN_SIZE {
                p.size.add_css_class("warn");
            } else {
                p.size.remove_css_class("warn");
            }
            p.dims.set_label(&txt);
        }
        p.actions.set_visible(phase == Phase::Selected && !act);
        self.set_cursor_from_name(Some(if phase == Phase::Selected {
            "default"
        } else {
            "crosshair"
        }));
        self.queue_allocate();
        self.queue_draw();
    }

    fn install_input(&self) {
        let drag = gtk::GestureDrag::builder().button(1).build();
        let c = self.downgrade();
        drag.connect_drag_begin(move |_, x, y| {
            let Some(c) = c.upgrade() else { return };
            c.imp().start.set((x, y));
            if let Some(s) = c.session() {
                s.pressed(c.index());
            }
            let f = c.css_per_px();
            let p = c.point(x, y);
            c.with(|s| s.press(p, f));
            c.sync_ui();
        });
        let c = self.downgrade();
        drag.connect_drag_update(move |_, dx, dy| {
            let Some(c) = c.upgrade() else { return };
            let (x, y) = c.imp().start.get();
            let p = c.point(x + dx, y + dy);
            c.with(|s| s.motion(p));
            c.sync_ui();
        });
        let c = self.downgrade();
        drag.connect_drag_end(move |_, _, _| {
            let Some(c) = c.upgrade() else { return };
            let Some(released) = c.with(Selector::release) else {
                return;
            };
            c.sync_ui();
            if released == Released::NewSelection
                && let Some(s) = c.session()
            {
                s.selection_done(c.index());
            }
        });
        self.add_controller(drag);

        let secondary = gtk::GestureClick::builder().button(3).build();
        let c = self.downgrade();
        secondary.connect_pressed(move |_, _, _, _| {
            if let Some(s) = c.upgrade().and_then(|c| c.session()) {
                s.cancel();
            }
        });
        self.add_controller(secondary);

        let motion = gtk::EventControllerMotion::new();
        let c = self.downgrade();
        motion.connect_enter(move |_, _, _| {
            let Some(c) = c.upgrade() else { return };
            if let Some(s) = c.session() {
                s.active.set(c.index());
            }
        });
        let c = self.downgrade();
        motion.connect_motion(move |_, x, y| {
            let Some(c) = c.upgrade() else { return };
            let f = c.css_per_px();
            let p = c.point(x, y);
            let zone = c.with(|s| (s.phase, s.hit(p, f)));
            if let Some((Phase::Selected, zone)) = zone {
                c.set_cursor_from_name(Some(zone.map_or("default", |z| z.cursor())));
            }
        });
        self.add_controller(motion);
    }
}

struct MonView {
    window: gtk::Window,
    canvas: Canvas,
    region: Rect,
    phys: (i32, i32),
    monitor: geometry::LogicalMonitor,
}

pub struct Session {
    k: Rc<Klipp>,
    frozen: RefCell<Option<Frozen>>,
    mons: RefCell<Vec<MonView>>,
    active: Cell<usize>,
    keys: keys::RepeatFilter,
    syncing: Cell<bool>,
    ended: Cell<bool>,
}

fn monitors() -> Vec<gdk::Monitor> {
    let Some(d) = gdk::Display::default() else {
        return Vec::new();
    };
    let list = d.monitors();
    (0..list.n_items())
        .filter_map(|i| list.item(i).and_downcast::<gdk::Monitor>())
        .collect()
}

fn logical(m: &gdk::Monitor) -> geometry::LogicalMonitor {
    let g = m.geometry();
    geometry::LogicalMonitor {
        x: g.x(),
        y: g.y(),
        w: g.width(),
        h: g.height(),
        scale: m.scale(),
    }
}

/// Del av det frosne bildet som ferdig tegneoppskrift i Cairos eget pikselformat.
/// Samme node hvert bilde gjør at GTK bare tegner det som faktisk har endret seg.
fn region_node(f: &Frozen, r: Rect, phys: (i32, i32)) -> Option<gsk::CairoNode> {
    let mut surface = cairo::ImageSurface::create(cairo::Format::Rgb24, r.w, r.h).ok()?;
    {
        let dst_stride = surface.stride() as usize;
        let src_stride = f.w as usize * 4;
        let row_len = r.w as usize * 4;
        let mut data = surface.data().ok()?;
        for (y, dst) in data.chunks_exact_mut(dst_stride).enumerate() {
            let start = (r.y as usize + y) * src_stride + r.x as usize * 4;
            let src = &f.bytes[start..start + row_len];
            for (d, s) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
                d.copy_from_slice(&[s[2], s[1], s[0], 0xff]);
            }
        }
    }
    let bounds = graphene::Rect::new(0.0, 0.0, phys.0 as f32, phys.1 as f32);
    let node = gsk::CairoNode::new(&bounds);
    let cr = node.draw_context();
    cr.scale(
        f64::from(phys.0) / f64::from(r.w),
        f64::from(phys.1) / f64::from(r.h),
    );
    cr.set_source_surface(&surface, 0.0, 0.0).ok()?;
    // Har skjermen lavere skala enn bildet, treffer ikke pikslene 1:1.
    cr.source().set_filter(if phys == (r.w, r.h) {
        cairo::Filter::Nearest
    } else {
        cairo::Filter::Bilinear
    });
    cr.paint().ok()?;
    Some(node)
}

fn build_parts(canvas: &Canvas, session: &Rc<Session>, mode: Mode) -> imp::Parts {
    let cfg = session.k.config.borrow();
    let toggles = adw::ToggleGroup::new();
    for m in Mode::ALL {
        let tip = format!("{}{}", mode_label(m), keys::hint(&cfg, Action::for_mode(m)));
        toggles.add(
            adw::Toggle::builder()
                .name(m.id())
                .icon_name(mode_icon(m))
                .tooltip(tip)
                .build(),
        );
    }
    toggles.set_active_name(Some(mode.id()));
    let weak = Rc::downgrade(session);
    toggles.connect_active_name_notify(move |g| {
        let Some(s) = weak.upgrade() else { return };
        if let Some(m) = g.active_name().and_then(|n| Mode::from_id(&n)) {
            s.set_mode(m);
        }
    });
    let close = gtk::Button::builder()
        .label("Lukk")
        .css_classes(["flat"])
        .build();
    let weak = Rc::downgrade(session);
    close.connect_clicked(move |_| {
        if let Some(s) = weak.upgrade() {
            s.cancel();
        }
    });
    let bar = gtk::Box::builder()
        .spacing(4)
        .css_classes(["klipp-ov-bar"])
        .build();
    bar.append(&toggles);
    let sep = gtk::Separator::new(gtk::Orientation::Vertical);
    sep.add_css_class("klipp-ov-sep");
    bar.append(&sep);
    bar.append(&close);

    let dims = gtk::Label::builder().css_classes(["klipp-ov-dims"]).build();
    let undo = gtk::Button::builder()
        .label("Angre")
        .tooltip_text("Angre (Ctrl+Z)")
        .css_classes(["flat"])
        .build();
    let idx = canvas.index();
    let weak = Rc::downgrade(session);
    undo.connect_clicked(move |_| {
        if let Some(s) = weak.upgrade() {
            s.clear(idx);
        }
    });
    let take = gtk::Button::builder()
        .label("Ta bilde")
        .tooltip_text("Ta bilde (Enter)")
        .css_classes(["suggested-action"])
        .build();
    let weak = Rc::downgrade(session);
    take.connect_clicked(move |_| {
        if let Some(s) = weak.upgrade() {
            s.take(idx);
        }
    });
    let actions = gtk::Box::builder()
        .spacing(4)
        .css_classes(["klipp-ov-bar"])
        .build();
    actions.append(&dims);
    actions.append(&undo);
    actions.append(&take);
    actions.set_visible(false);

    let hint = gtk::Label::builder().css_classes(["klipp-pill"]).build();
    let size = gtk::Label::builder()
        .css_classes(["klipp-pill", "size"])
        .visible(false)
        .can_target(false)
        .build();
    hint.set_can_target(false);

    bar.set_parent(canvas);
    actions.set_parent(canvas);
    hint.set_parent(canvas);
    size.set_parent(canvas);
    imp::Parts {
        bar,
        toggles,
        actions,
        dims,
        hint,
        size,
    }
}

/// Åpne overlegget. Det frosne bildet flyttes hit og slippes når sesjonen slutter.
pub fn open(k: &Rc<Klipp>, frozen: Frozen, mode: Mode) {
    let mons = monitors();
    let logical_mons: Vec<_> = mons.iter().map(logical).collect();
    let regions = geometry::monitor_regions(&logical_mons, frozen.w as i32, frozen.h as i32);
    let session = Rc::new(Session {
        k: k.clone(),
        frozen: RefCell::new(None),
        mons: RefCell::new(Vec::new()),
        active: Cell::new(0),
        keys: keys::RepeatFilter::default(),
        syncing: Cell::new(false),
        ended: Cell::new(false),
    });

    let token = k.activation.borrow_mut().take();
    let mut views = Vec::new();
    for (i, (mon, region)) in mons.iter().zip(regions.iter().copied()).enumerate() {
        if region.w < 1 || region.h < 1 {
            continue;
        }
        let canvas: Canvas = glib::Object::new();
        let imp = canvas.imp();
        imp.index.set(views.len());
        let phys = geometry::physical_size(&logical_mons[i]);
        imp.image.replace(region_node(&frozen, region, phys));
        imp.selector
            .replace(Some(Selector::new(phys.0, phys.1, mode)));
        imp.session.replace(Rc::downgrade(&session));
        let parts = build_parts(&canvas, &session, mode);
        let _ = imp.parts.set(parts);
        canvas.set_focusable(true);
        canvas.install_input();

        let window = gtk::Window::builder()
            .application(&k.app)
            .title("Klipp")
            .decorated(false)
            .child(&canvas)
            .build();
        window.add_css_class("klipp-overlay");
        if let Some(t) = &token {
            window.set_startup_id(t);
        }
        window.fullscreen_on_monitor(mon);
        session.install_window(&window);
        canvas.sync_ui();
        views.push(MonView {
            window,
            canvas,
            region,
            phys,
            monitor: logical_mons[i],
        });
    }
    if views.is_empty() {
        super::overlay_cancelled(k);
        k.ui.toast("Fant ingen skjerm å vise overlegget på");
        return;
    }
    session.frozen.replace(Some(frozen));
    session.mons.replace(views);
    k.capture.session.replace(Some(session.clone()));

    let delay = if shell::ready() {
        0
    } else {
        GNOME_OPEN_ANIMATION_MS
    };
    for v in session.mons.borrow().iter() {
        v.window.present();
        v.canvas.grab_focus();
        v.canvas.fade(DIM, delay, FADE_IN_MS);
    }
    k.capture.mark("overlegg vist");
    if let Some(v) = session.mons.borrow().first() {
        let n = Cell::new(0);
        let k2 = k.clone();
        v.window.add_tick_callback(move |_, _| {
            n.set(n.get() + 1);
            if n.get() == 2 {
                k2.capture.mark("overlegg tegnet");
                return glib::ControlFlow::Break;
            }
            glib::ControlFlow::Continue
        });
    }
}

impl Session {
    fn install_window(self: &Rc<Self>, window: &gtk::Window) {
        let keys_ctl = gtk::EventControllerKey::new();
        let weak = Rc::downgrade(self);
        keys_ctl.connect_key_released(move |_, _, _, _| {
            if let Some(s) = weak.upgrade() {
                s.keys.release();
            }
        });
        let weak = Rc::downgrade(self);
        keys_ctl.connect_key_pressed(move |c, key, code, state| match weak.upgrade() {
            Some(s) => s.key(key, code, c.group(), state),
            None => glib::Propagation::Proceed,
        });
        window.add_controller(keys_ctl);
        let weak = Rc::downgrade(self);
        window.connect_close_request(move |_| {
            if let Some(s) = weak.upgrade() {
                s.cancel();
            }
            glib::Propagation::Stop
        });
    }

    fn canvas(&self, i: usize) -> Option<Canvas> {
        self.mons.borrow().get(i).map(|m| m.canvas.clone())
    }

    fn each(&self, f: impl Fn(&Canvas)) {
        let canvases: Vec<Canvas> = self
            .mons
            .borrow()
            .iter()
            .map(|m| m.canvas.clone())
            .collect();
        canvases.iter().for_each(f);
    }

    /// Skjermen tastene gjelder: den med utvalg, ellers den musa er på.
    fn current(&self) -> Option<Canvas> {
        let screen = self
            .canvas(self.active.get())
            .and_then(|c| c.with(|s| s.mode));
        if screen != Some(Mode::Screen) {
            let found = self.mons.borrow().iter().find_map(|m| {
                let busy = m.canvas.with(|s| s.phase != Phase::Idle).unwrap_or(false);
                busy.then(|| m.canvas.clone())
            });
            if found.is_some() {
                return found;
            }
        }
        self.canvas(self.active.get())
    }

    fn key(
        self: &Rc<Self>,
        key: gdk::Key,
        code: u32,
        group: u32,
        state: gdk::ModifierType,
    ) -> glib::Propagation {
        if self.ended.get() {
            return glib::Propagation::Stop;
        }
        let Some(c) = self.current() else {
            return glib::Propagation::Proceed;
        };
        let plain_ctrl = state.contains(gdk::ModifierType::CONTROL_MASK)
            && !state.intersects(gdk::ModifierType::ALT_MASK | gdk::ModifierType::SHIFT_MASK);
        match key {
            gdk::Key::Escape => {
                if c.with(Selector::escape) == Some(Escaped::Close) {
                    self.cancel();
                } else {
                    self.sync_after_mode_fallback(&c);
                }
                return glib::Propagation::Stop;
            }
            gdk::Key::Return | gdk::Key::KP_Enter | gdk::Key::ISO_Enter => {
                self.take(c.index());
                return glib::Propagation::Stop;
            }
            gdk::Key::z | gdk::Key::Z if plain_ctrl => {
                if c.with(|s| s.can_take()) == Some(true) {
                    self.clear(c.index());
                }
                return glib::Propagation::Stop;
            }
            _ => {}
        }
        if !self.keys.press(code) {
            return glib::Propagation::Proceed;
        }
        let Some(display) = gdk::Display::default() else {
            return glib::Propagation::Proceed;
        };
        let action = {
            let cfg = self.k.config.borrow();
            keys::action_for(&cfg, &display, key, code, group, state)
        };
        match action.and_then(Action::mode) {
            Some(m) => {
                self.set_mode(m);
                glib::Propagation::Stop
            }
            None => glib::Propagation::Proceed,
        }
    }

    /// Esc og Angre kan bytte Hele skjermen til Område. Da må alle skjermer følge med.
    fn sync_after_mode_fallback(&self, c: &Canvas) {
        let mode = c.with(|s| s.mode).unwrap_or(Mode::Area);
        self.each(|o| {
            if o != c && o.with(|s| s.mode) != Some(mode) {
                o.with(|s| s.set_mode(mode));
            }
            self.sync_toggles(o, mode);
            o.sync_ui();
        });
    }

    fn sync_toggles(&self, c: &Canvas, mode: Mode) {
        self.syncing.set(true);
        if let Some(p) = c.parts() {
            p.toggles.set_active_name(Some(mode.id()));
        }
        self.syncing.set(false);
    }

    fn pressed(&self, i: usize) {
        self.active.set(i);
        self.each(|c| {
            if c.index() != i
                && c.with(|s| s.mode != Mode::Screen && s.phase != Phase::Idle) == Some(true)
            {
                c.with(Selector::reset);
                c.sync_ui();
            }
        });
    }

    fn selection_done(self: &Rc<Self>, i: usize) {
        if self.k.config.borrow().instant_capture {
            self.take(i);
        }
    }

    fn set_mode(&self, m: Mode) {
        if self.syncing.get() || self.ended.get() {
            return;
        }
        self.k.set_mode(m);
        if m == Mode::Window {
            self.end();
            super::overlay_to_window(&self.k);
            return;
        }
        self.each(|c| {
            c.with(|s| s.set_mode(m));
            self.sync_toggles(c, m);
            c.sync_ui();
        });
        if let Some(c) = self.canvas(self.active.get()) {
            c.grab_focus();
        }
    }

    fn clear(&self, i: usize) {
        let Some(c) = self.canvas(i) else { return };
        c.with(Selector::clear);
        self.sync_after_mode_fallback(&c);
        c.grab_focus();
    }

    fn take(self: &Rc<Self>, i: usize) {
        if self.ended.get() {
            return;
        }
        let Some(c) = self.canvas(i) else { return };
        if c.with(|s| s.can_take()) != Some(true) {
            return;
        }
        let Some((region, phys, monitor)) = self
            .mons
            .borrow()
            .get(i)
            .map(|m| (m.region, m.phys, m.monitor))
        else {
            return;
        };
        let Some((sel, mode, pts)) = c.with(|s| (s.sel, s.mode, s.pts.clone())) else {
            return;
        };
        let Some(sel) = sel else { return };
        let area = geometry::logical_area(sel, &monitor);
        let moved = geometry::rect_to_image(sel, phys, region);
        let out = (sel.w as u32, sel.h as u32);
        let cut = if mode == Mode::Freehand {
            let pts = pts
                .iter()
                .map(|p| geometry::phys_to_image(*p, phys, region))
                .collect();
            Cut::Poly(moved, pts, out)
        } else {
            Cut::Rect(moved, out)
        };
        // Må skje nå, mens overlegget har fokus. Etter lukking nekter Wayland det.
        let slot = if self.k.config.borrow().auto_copy {
            clipboard::offer(Source::Pending)
        } else {
            None
        };
        let frozen = self.frozen.take();
        let k = self.k.clone();
        self.close(move || {
            if let Some(frozen) = frozen {
                super::overlay_taken(&k, frozen, cut, slot, area);
            }
        });
    }

    fn cancel(self: &Rc<Self>) {
        if self.ended.get() {
            return;
        }
        let k = self.k.clone();
        self.close(move || super::overlay_cancelled(&k));
    }

    /// Ton skjermen tilbake og lukk, så `then` først kjører når alt er borte.
    fn close(self: &Rc<Self>, then: impl FnOnce() + 'static) {
        self.ended.set(true);
        self.each(|c| {
            c.set_can_target(false);
            c.fade(0.0, 0, FADE_OUT_MS);
        });
        let me = self.clone();
        let wait = Duration::from_millis(FADE_OUT_MS.into());
        glib::timeout_add_local_once(wait, move || {
            me.end();
            then();
        });
    }

    /// Lukk vinduene og slipp teksturene med en gang.
    fn end(&self) {
        self.ended.set(true);
        self.frozen.take();
        for m in self.mons.take() {
            m.canvas.imp().image.replace(None);
            m.window.destroy();
        }
    }
}
