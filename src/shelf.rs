//! Hylla: virtualisert liste med thumbnails som lastes utenfor UI-tråden.

use crate::app::App;
use crate::clipboard;
use crate::imaging::Image;
use crate::storage::{self, ShelfSnapshot, ShelfStorage, ShotFile, ShotMeta};
use agentic_screenshot_system::i18n::{tr, tr_fmt};
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gdk, gio, glib, graphene, gsk};
use std::cell::{Cell, RefCell};
use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

const MAX_TILE_H: f32 = 300.0;
/// Samme radius som `shot-tile` i style.css.
const TILE_RADIUS: f32 = 9.0;
/// Kanten rundt et nytt bilde og rundt et valgt bilde.
const FRESH_RING: f32 = 2.0;
const SELECTED_RING: f32 = 3.0;
const CACHE_TEXTURES: usize = 24;
const MAX_LOADS: u32 = 2;

mod shot_imp {
    use super::*;

    #[derive(Default)]
    pub struct Shot {
        pub meta: RefCell<Option<ShotMeta>>,
        pub id: Cell<u64>,
        pub rev: Cell<u32>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Shot {
        const NAME: &'static str = "ShotItem";
        type Type = super::Shot;
    }

    impl ObjectImpl for Shot {}
}

glib::wrapper! {
    pub struct Shot(ObjectSubclass<shot_imp::Shot>);
}

impl Shot {
    fn new(id: u64, meta: ShotMeta) -> Self {
        let s: Self = glib::Object::new();
        s.imp().id.set(id);
        *s.imp().meta.borrow_mut() = Some(meta);
        s
    }

    pub fn meta(&self) -> ShotMeta {
        self.imp().meta.borrow().clone().unwrap_or(ShotMeta {
            path: Default::default(),
            ts: 0,
            w: 1,
            h: 1,
            alpha: false,
        })
    }

    pub fn id(&self) -> u64 {
        self.imp().id.get()
    }

    fn key(&self) -> (u64, u32) {
        (self.id(), self.imp().rev.get())
    }
}

/// Klokkeslett som i prototypen: 14:02, i går 14:02 eller 03.11. 14:02.
pub fn fmt_time(ts: i64) -> String {
    let Ok(d) = glib::DateTime::from_unix_local(ts) else {
        return String::new();
    };
    let Ok(now) = glib::DateTime::now_local() else {
        return String::new();
    };
    time_text(
        (d.year(), d.day_of_year()),
        d.add_days(1).map(|y| (y.year(), y.day_of_year())).ok(),
        (now.year(), now.day_of_year()),
        (d.hour(), d.minute()),
        (d.day_of_month(), d.month()),
    )
}

fn time_text(
    day: (i32, i32),
    next_day: Option<(i32, i32)>,
    today: (i32, i32),
    hm: (i32, i32),
    dm: (i32, i32),
) -> String {
    let t = format!("{:02}:{:02}", hm.0, hm.1);
    if day == today {
        t
    } else if next_day == Some(today) {
        tr_fmt("i går {time}", &[("time", &t)])
    } else {
        // Oversettere: Day and month are zero-padded numbers, e.g. 03 and 11.
        tr_fmt(
            "{day}.{month}. {time}",
            &[
                ("day", &format!("{:02}", dm.0)),
                ("month", &format!("{:02}", dm.1)),
                ("time", &t),
            ],
        )
    }
}

mod tile_imp {
    use super::*;

    #[derive(Default)]
    pub struct Tile {
        pub shot: RefCell<Option<Shot>>,
        pub texture: RefCell<Option<gdk::Texture>>,
        pub osd: RefCell<Option<gtk::Label>>,
        pub trash: RefCell<Option<gtk::Button>>,
        pub check: RefCell<Option<gtk::Image>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Tile {
        const NAME: &'static str = "ShotTile";
        type Type = super::Tile;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("shot-tile");
        }
    }

    impl ObjectImpl for Tile {
        fn constructed(&self) {
            self.parent_constructed();
            let w = self.obj();
            w.set_overflow(gtk::Overflow::Hidden);
            let osd = gtk::Label::builder()
                .xalign(0.0)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .css_classes(["shot-tile-osd"])
                .valign(gtk::Align::End)
                .can_target(false)
                .build();
            osd.set_parent(&*w);
            *self.osd.borrow_mut() = Some(osd);
            let trash = gtk::Button::builder()
                .icon_name("user-trash-symbolic")
                .tooltip_text(tr("Legg i papirkurven"))
                .halign(gtk::Align::End)
                .valign(gtk::Align::Start)
                .css_classes(["shot-tile-trash"])
                .build();
            let tile = w.downgrade();
            trash.connect_clicked(move |_| {
                if let Some(shot) = tile.upgrade().and_then(|t| t.current()) {
                    App::get().trash_shot(&shot);
                }
            });
            trash.set_parent(&*w);
            *self.trash.borrow_mut() = Some(trash);
            // Haken i valgmodus. Klikket går til ruta, så haken tar ikke imot noe.
            let check = gtk::Image::builder()
                .icon_name("object-select-symbolic")
                .halign(gtk::Align::Start)
                .valign(gtk::Align::Start)
                .can_target(false)
                .css_classes(["shot-tile-check"])
                .build();
            check.set_parent(&*w);
            *self.check.borrow_mut() = Some(check);
        }

        fn dispose(&self) {
            if let Some(osd) = self.osd.take() {
                osd.unparent();
            }
            if let Some(trash) = self.trash.take() {
                trash.unparent();
            }
            if let Some(check) = self.check.take() {
                check.unparent();
            }
        }
    }

    impl WidgetImpl for Tile {
        fn request_mode(&self) -> gtk::SizeRequestMode {
            gtk::SizeRequestMode::HeightForWidth
        }

        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            if orientation == gtk::Orientation::Horizontal {
                return (120, 224, -1, -1);
            }
            let width = if for_size > 0 { for_size } else { 224 };
            let h = self.obj().height_for(width);
            (h, h, -1, -1)
        }

        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            if let Some(osd) = self.osd.borrow().as_ref() {
                let (_, nat, _, _) = osd.measure(gtk::Orientation::Vertical, width);
                let nat = nat.min(height);
                osd.size_allocate(&gtk::Allocation::new(0, height - nat, width, nat), baseline);
            }
            if let Some(trash) = self.trash.borrow().as_ref() {
                let (_, w, _, _) = trash.measure(gtk::Orientation::Horizontal, -1);
                let (_, h, _, _) = trash.measure(gtk::Orientation::Vertical, -1);
                trash.size_allocate(&gtk::Allocation::new(width - w, 0, w, h), baseline);
            }
            if let Some(check) = self.check.borrow().as_ref() {
                let (_, w, _, _) = check.measure(gtk::Orientation::Horizontal, -1);
                let (_, h, _, _) = check.measure(gtk::Orientation::Vertical, -1);
                check.size_allocate(&gtk::Allocation::new(0, 0, w, h), baseline);
            }
        }

        fn state_flags_changed(&self, old: &gtk::StateFlags) {
            self.parent_state_flags_changed(old);
            self.obj().queue_draw();
        }

        fn snapshot(&self, s: &gtk::Snapshot) {
            let w = self.obj();
            let (bw, bh) = (w.width() as f32, w.height() as f32);
            let shot = self.shot.borrow();
            if shot.as_ref().is_some_and(|x| x.meta().alpha) {
                snapshot_checker(s, bw, bh);
            }
            if let Some(tex) = self.texture.borrow().as_ref() {
                let f = (bw / tex.width() as f32).min(bh / tex.height() as f32);
                let (dw, dh) = (tex.width() as f32 * f, tex.height() as f32 * f);
                let r = graphene::Rect::new((bw - dw) / 2.0, (bh - dh) / 2.0, dw, dh);
                s.append_scaled_texture(tex, gsk::ScalingFilter::Trilinear, &r);
            }
            let flags = w.state_flags();
            let shade = if flags.contains(gtk::StateFlags::ACTIVE) {
                Some(0.12)
            } else if flags.contains(gtk::StateFlags::PRELIGHT) {
                Some(0.05)
            } else {
                None
            };
            if let Some(a) = shade {
                let c = gdk::RGBA::new(1.0, 1.0, 1.0, a);
                s.append_color(&c, &graphene::Rect::new(0.0, 0.0, bw, bh));
            }
            let ring = if w.has_css_class("selected") {
                Some(SELECTED_RING)
            } else if w.has_css_class("fresh") {
                Some(FRESH_RING)
            } else {
                None
            };
            if let Some(width) = ring {
                let accent = adw::StyleManager::default().accent_color_rgba();
                let r =
                    gsk::RoundedRect::from_rect(graphene::Rect::new(0.0, 0.0, bw, bh), TILE_RADIUS);
                s.append_inset_shadow(&r, &accent, 0.0, 0.0, width, 0.0);
            }
            self.parent_snapshot(s);
        }
    }
}

glib::wrapper! {
    pub struct Tile(ObjectSubclass<tile_imp::Tile>) @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

/// Sjakkbrett bak bilder med transparens.
pub fn snapshot_checker(s: &gtk::Snapshot, bw: f32, bh: f32) {
    let dark = adw::StyleManager::default().is_dark();
    let (a, b) = if dark {
        (
            gdk::RGBA::new(0.227, 0.227, 0.243, 1.0),
            gdk::RGBA::new(0.173, 0.173, 0.188, 1.0),
        )
    } else {
        (
            gdk::RGBA::new(1.0, 1.0, 1.0, 1.0),
            gdk::RGBA::new(0.894, 0.894, 0.91, 1.0),
        )
    };
    let cell = 6.0;
    let bounds = graphene::Rect::new(0.0, 0.0, bw, bh);
    s.push_repeat(
        &bounds,
        Some(&graphene::Rect::new(0.0, 0.0, cell * 2.0, cell * 2.0)),
    );
    s.append_color(&a, &graphene::Rect::new(0.0, 0.0, cell * 2.0, cell * 2.0));
    s.append_color(&b, &graphene::Rect::new(cell, 0.0, cell, cell));
    s.append_color(&b, &graphene::Rect::new(0.0, cell, cell, cell));
    s.pop();
}

fn tile_tip(selecting: bool) -> &'static str {
    if selecting {
        // Oversettere: Tooltip for a shelf image while selecting multiple screenshots.
        tr("Klikk for å velge\nShift+klikk velger alle imellom\nDra de valgte til terminalen")
    } else {
        // Oversettere: Tooltip for a shelf image. Keep one action per line.
        tr(
            "Klikk for å tegne\nCtrl+klikk velger flere\nShift+høyreklikk kopierer bildet\nShift+klikk kopierer filstien\nDra til terminalen",
        )
    }
}

impl Tile {
    fn new() -> Self {
        let t: Self = glib::Object::new();
        t.set_tooltip_text(Some(tile_tip(false)));
        t.install_input();
        t
    }

    fn height_for(&self, width: i32) -> i32 {
        let (iw, ih) = self.imp().shot.borrow().as_ref().map_or((16, 9), |s| {
            let m = s.meta();
            (m.w, m.h)
        });
        let h = width as f32 * ih as f32 / iw.max(1) as f32;
        (h.round() as i32).clamp(24, MAX_TILE_H as i32)
    }

    fn bind(&self, shot: Option<&Shot>, fresh: bool) {
        let imp = self.imp();
        *imp.shot.borrow_mut() = shot.cloned();
        *imp.texture.borrow_mut() = None;
        if let (Some(shot), Some(osd)) = (shot, imp.osd.borrow().as_ref()) {
            let m = shot.meta();
            osd.set_label(&tr_fmt(
                "{size} · {time}",
                &[("size", &m.dims()), ("time", &fmt_time(m.ts))],
            ));
        }
        if fresh {
            self.add_css_class("fresh");
        } else {
            self.remove_css_class("fresh");
        }
        self.queue_resize();
    }

    /// Valgmodus og valgt-tilstand, som CSS-klasser. Søppelbøtta viker for haken.
    fn set_selection(&self, selecting: bool, selected: bool) {
        if self.has_css_class("selecting") != selecting {
            self.set_tooltip_text(Some(tile_tip(selecting)));
            if let Some(trash) = self.imp().trash.borrow().as_ref() {
                trash.set_can_target(!selecting);
                trash.set_focusable(!selecting);
            }
        }
        for (class, on) in [("selecting", selecting), ("selected", selected)] {
            if on {
                self.add_css_class(class);
            } else {
                self.remove_css_class(class);
            }
        }
        self.update_state(&[gtk::accessible::State::Selected(
            selecting.then_some(selected),
        )]);
        self.queue_draw();
    }

    fn set_texture(&self, tex: gdk::Texture) {
        *self.imp().texture.borrow_mut() = Some(tex);
        self.queue_draw();
    }

    fn current(&self) -> Option<Shot> {
        self.imp().shot.borrow().clone()
    }

    fn install_input(&self) {
        let click = gtk::GestureClick::builder().button(1).build();
        let tile = self.downgrade();
        click.connect_released(move |g, n, _, _| {
            let Some(t) = tile.upgrade() else { return };
            let Some(shot) = t.current() else { return };
            let state = g.current_event_state();
            let shift = state.contains(gdk::ModifierType::SHIFT_MASK);
            let ctrl = state.contains(gdk::ModifierType::CONTROL_MASK);
            let k = App::get();
            if k.shelf.selecting() {
                // Hvert klikk teller, også det andre i et raskt dobbeltklikk.
                if shift {
                    k.shelf.select_range(&shot);
                } else {
                    k.shelf.toggle(&shot);
                }
                return;
            }
            match n {
                1 if ctrl => k.shelf.toggle(&shot),
                1 if shift => k.copy_path(&shot),
                1 => k.annotate(&shot),
                _ => {}
            }
        });
        self.add_controller(click);

        let secondary = gtk::GestureClick::builder().button(3).build();
        let tile = self.downgrade();
        secondary.connect_released(move |g, _, _, _| {
            let shift = g
                .current_event_state()
                .contains(gdk::ModifierType::SHIFT_MASK);
            if shift && let Some(shot) = tile.upgrade().and_then(|t| t.current()) {
                App::get().copy_shot(&shot);
            }
        });
        self.add_controller(secondary);

        let drag = gtk::DragSource::builder()
            .actions(gdk::DragAction::COPY)
            .build();
        let tile = self.downgrade();
        drag.connect_prepare(move |_, _, _| {
            let shot = tile.upgrade()?.current()?;
            // Et valgt bilde tar med seg resten av utvalget.
            let shelf = App::get().shelf.clone();
            let paths = if shelf.is_selected(&shot) {
                shelf.selected().iter().map(|s| s.meta().path).collect()
            } else {
                vec![shot.meta().path]
            };
            drag_content(&paths)
        });
        let tile = self.downgrade();
        drag.connect_drag_begin(move |d, _| {
            let Some(t) = tile.upgrade() else { return };
            d.set_icon(Some(&gtk::WidgetPaintable::new(Some(&t))), 24, 24);
            let shelf = App::get().shelf.clone();
            if t.current().is_some_and(|s| shelf.is_selected(&s)) {
                shelf.set_dragging(true);
            } else {
                t.add_css_class("dragging");
            }
        });
        let tile = self.downgrade();
        drag.connect_drag_end(move |_, _, _| {
            if let Some(t) = tile.upgrade() {
                t.remove_css_class("dragging");
            }
            App::get().shelf.set_dragging(false);
        });
        self.add_controller(drag);
    }
}

/// Filer til Nautilus og andre, `text/uri-list` til terminaler, stier som tekst ellers.
pub fn drag_content(paths: &[PathBuf]) -> Option<gdk::ContentProvider> {
    if paths.is_empty() {
        return None;
    }
    Some(gdk::ContentProvider::new_union(&clipboard::file_formats(
        paths,
    )))
}

/// Valgmodus: hvilke bilder som er valgt, og hvor et Shift-klikk regner fra.
#[derive(Default)]
struct Selection {
    active: Cell<bool>,
    ids: RefCell<HashSet<u64>>,
    anchor: Cell<Option<u64>>,
    changed: RefCell<Option<Box<dyn Fn()>>>,
}

impl Selection {
    fn has(&self, id: u64) -> bool {
        self.active.get() && self.ids.borrow().contains(&id)
    }
}

/// Bildene fra og med `anchor` til og med `target`, i hyllas rekkefølge.
/// Uten anker, eller med et anker som er borte, bare `target`.
fn range_ids(order: &[u64], anchor: Option<u64>, target: u64) -> Vec<u64> {
    let Some(t) = order.iter().position(|&id| id == target) else {
        return Vec::new();
    };
    let a = anchor
        .and_then(|a| order.iter().position(|&id| id == a))
        .unwrap_or(t);
    order[a.min(t)..=a.max(t)].to_vec()
}

pub fn to_texture(img: Image) -> gdk::Texture {
    let stride = img.w as usize * 4;
    gdk::MemoryTexture::new(
        img.w as i32,
        img.h as i32,
        gdk::MemoryFormat::R8g8b8a8,
        &glib::Bytes::from_owned(img.data),
        stride,
    )
    .upcast()
}

struct Request {
    shot: Shot,
    tile: glib::WeakRef<Tile>,
}

/// Laster thumbnails utenfor UI-tråden, med en liten LRU så rulling ikke blinker.
#[derive(Default)]
struct Loader {
    cache: RefCell<VecDeque<((u64, u32), gdk::Texture)>>,
    queue: RefCell<VecDeque<Request>>,
    active: Cell<u32>,
}

impl Loader {
    fn cached(&self, key: (u64, u32)) -> Option<gdk::Texture> {
        let mut c = self.cache.borrow_mut();
        let pos = c.iter().position(|(k, _)| *k == key)?;
        let item = c.remove(pos)?;
        let tex = item.1.clone();
        c.push_front(item);
        Some(tex)
    }

    fn insert(&self, key: (u64, u32), tex: gdk::Texture) {
        let mut c = self.cache.borrow_mut();
        c.retain(|(k, _)| k.0 != key.0);
        c.push_front((key, tex));
        c.truncate(CACHE_TEXTURES);
    }
}

fn pump(loader: Rc<Loader>, boxed: (u32, u32)) {
    while loader.active.get() < MAX_LOADS {
        let Some(req) = loader.queue.borrow_mut().pop_front() else {
            return;
        };
        let wanted = req
            .tile
            .upgrade()
            .is_some_and(|t| t.current().is_some_and(|s| s.key() == req.shot.key()));
        if !wanted {
            continue;
        }
        loader.active.set(loader.active.get() + 1);
        let path = req.shot.meta().path;
        let l = loader.clone();
        glib::spawn_future_local(async move {
            let res =
                gio::spawn_blocking(move || storage::load_thumb(&path, boxed.0, boxed.1)).await;
            l.active.set(l.active.get() - 1);
            if let Ok(Ok(img)) = res {
                let tex = to_texture(img);
                l.insert(req.shot.key(), tex.clone());
                if let Some(t) = req.tile.upgrade()
                    && t.current().is_some_and(|s| s.key() == req.shot.key())
                {
                    t.set_texture(tex);
                }
            }
            pump(l, boxed);
        });
    }
}

pub struct Shelf {
    pub store: gio::ListStore,
    pub list: gtk::ListView,
    pub scroller: gtk::ScrolledWindow,
    loader: Rc<Loader>,
    thumb_box: (u32, u32),
    next_id: Cell<u64>,
    fresh: Rc<Cell<u64>>,
    sel: Rc<Selection>,
    storage: RefCell<Option<ShelfStorage>>,
    loaded: Cell<bool>,
}

impl Shelf {
    pub fn new() -> Rc<Self> {
        let store = gio::ListStore::new::<Shot>();
        let factory = gtk::SignalListItemFactory::new();
        let loader = Rc::new(Loader::default());
        let fresh = Rc::new(Cell::new(0u64));
        let sel = Rc::new(Selection::default());
        let thumb_box = thumb_box();

        factory.connect_setup(|_, item| {
            if let Some(item) = item.downcast_ref::<gtk::ListItem>() {
                item.set_child(Some(&Tile::new()));
                // Radens egne gester ville spist dobbeltklikket.
                item.set_activatable(false);
                item.set_selectable(false);
            }
        });
        let (l, f, se) = (loader.clone(), fresh.clone(), sel.clone());
        factory.connect_bind(move |_, item| {
            let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                return;
            };
            let Some(tile) = item.child().and_downcast::<Tile>() else {
                return;
            };
            let Some(shot) = item.item().and_downcast::<Shot>() else {
                return;
            };
            tile.bind(Some(&shot), f.get() == shot.id());
            tile.set_selection(se.active.get(), se.has(shot.id()));
            if let Some(tex) = l.cached(shot.key()) {
                tile.set_texture(tex);
            } else {
                l.queue.borrow_mut().push_back(Request {
                    shot,
                    tile: tile.downgrade(),
                });
                pump(l.clone(), thumb_box);
            }
        });
        factory.connect_unbind(|_, item| {
            if let Some(item) = item.downcast_ref::<gtk::ListItem>()
                && let Some(tile) = item.child().and_downcast::<Tile>()
            {
                tile.bind(None, false);
                tile.set_selection(false, false);
            }
        });

        let selection = gtk::NoSelection::new(Some(store.clone()));
        let list = gtk::ListView::builder()
            .model(&selection)
            .factory(&factory)
            .css_classes(["shot-strip"])
            .build();
        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&list)
            .build();

        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let lv = list.clone();
        let st = store.clone();
        keys.connect_key_pressed(move |_, key, _, state| {
            // Lista har Ctrl+A selv, så valgtastene må tas før den ser dem.
            let k = App::get();
            if k.selection_key(key, state) {
                return glib::Propagation::Stop;
            }
            let Some(shot) = focused_shot(&lv) else {
                return glib::Propagation::Proceed;
            };
            let pos = st.find(&shot).unwrap_or(0);
            let last = st.n_items().saturating_sub(1);
            let target = match key {
                gdk::Key::Up | gdk::Key::KP_Up => Some(pos.saturating_sub(1)),
                gdk::Key::Down | gdk::Key::KP_Down => Some((pos + 1).min(last)),
                gdk::Key::Home | gdk::Key::KP_Home => Some(0),
                gdk::Key::End | gdk::Key::KP_End => Some(last),
                _ => None,
            };
            if let Some(t) = target {
                focus_row(&lv, t);
                return glib::Propagation::Stop;
            }
            if !matches!(
                key,
                gdk::Key::Return | gdk::Key::KP_Enter | gdk::Key::ISO_Enter | gdk::Key::space
            ) {
                return glib::Propagation::Proceed;
            }
            if k.shelf.selecting() {
                if state.contains(gdk::ModifierType::SHIFT_MASK) {
                    k.shelf.select_range(&shot);
                } else {
                    k.shelf.toggle(&shot);
                }
            } else if state.contains(gdk::ModifierType::CONTROL_MASK) {
                k.annotate(&shot);
            } else if state.contains(gdk::ModifierType::SHIFT_MASK) {
                k.copy_path(&shot);
            } else {
                k.copy_shot(&shot);
            }
            glib::Propagation::Stop
        });
        list.add_controller(keys);

        let shelf = Rc::new(Self {
            store,
            list,
            scroller,
            loader,
            thumb_box,
            next_id: Cell::new(1),
            fresh,
            sel,
            storage: RefCell::new(None),
            loaded: Cell::new(false),
        });
        // Bilder som forsvinner, forsvinner også fra utvalget.
        let weak = Rc::downgrade(&shelf);
        shelf
            .store
            .connect_items_changed(move |store, _, removed, _| {
                let Some(this) = weak.upgrade() else { return };
                if removed == 0 || !this.sel.active.get() {
                    return;
                }
                if store.n_items() == 0 {
                    this.set_selecting(false);
                    return;
                }
                let alive: HashSet<u64> = this.shots().iter().map(Shot::id).collect();
                let before = this.sel.ids.borrow().len();
                this.sel.ids.borrow_mut().retain(|id| alive.contains(id));
                if this.sel.ids.borrow().len() != before {
                    this.selection_changed();
                }
            });
        shelf
    }

    /// Kalles når valgmodus eller utvalget endrer seg.
    pub fn connect_selection_changed(&self, f: impl Fn() + 'static) {
        self.sel.changed.replace(Some(Box::new(f)));
    }

    fn selection_changed(&self) {
        self.restyle_visible();
        if let Some(f) = self.sel.changed.borrow().as_ref() {
            f();
        }
    }

    pub fn selecting(&self) -> bool {
        self.sel.active.get()
    }

    /// Inn i eller ut av valgmodus. Begge veier starter med tomt utvalg.
    pub fn set_selecting(&self, on: bool) {
        if self.sel.active.get() == on {
            return;
        }
        self.sel.active.set(on);
        self.sel.ids.borrow_mut().clear();
        self.sel.anchor.set(None);
        self.selection_changed();
    }

    pub fn is_selected(&self, shot: &Shot) -> bool {
        self.sel.has(shot.id())
    }

    pub fn selected_count(&self) -> usize {
        if self.sel.active.get() {
            self.sel.ids.borrow().len()
        } else {
            0
        }
    }

    /// De valgte bildene, ovenfra og ned som i hylla.
    pub fn selected(&self) -> Vec<Shot> {
        self.shots()
            .into_iter()
            .filter(|s| self.is_selected(s))
            .collect()
    }

    /// Velg eller fjern ett bilde. Starter valgmodus om den ikke er på.
    pub fn toggle(&self, shot: &Shot) {
        self.sel.active.set(true);
        let id = shot.id();
        {
            let mut ids = self.sel.ids.borrow_mut();
            if !ids.remove(&id) {
                ids.insert(id);
            }
        }
        self.sel.anchor.set(Some(id));
        self.selection_changed();
    }

    /// Shift-klikk: alt fra forrige klikk til dette blir valgt.
    pub fn select_range(&self, shot: &Shot) {
        self.sel.active.set(true);
        let order: Vec<u64> = self.shots().iter().map(Shot::id).collect();
        let range = range_ids(&order, self.sel.anchor.get(), shot.id());
        self.sel.ids.borrow_mut().extend(range);
        if self.sel.anchor.get().is_none() {
            self.sel.anchor.set(Some(shot.id()));
        }
        self.selection_changed();
    }

    pub fn select_all(&self) {
        self.sel.active.set(true);
        self.sel
            .ids
            .borrow_mut()
            .extend(self.shots().iter().map(Shot::id));
        self.selection_changed();
    }

    /// Alle valgte ruter blekner mens de dras ut sammen.
    pub fn set_dragging(&self, on: bool) {
        self.for_each_visible(|t| {
            if on && t.current().is_some_and(|s| self.is_selected(&s)) {
                t.add_css_class("dragging");
            } else if !on {
                t.remove_css_class("dragging");
            }
        });
    }

    pub fn thumb_box(&self) -> (u32, u32) {
        self.thumb_box
    }

    fn make_shot(&self, meta: ShotMeta) -> Shot {
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        Shot::new(id, meta)
    }

    pub fn is_empty(&self) -> bool {
        self.store.n_items() == 0
    }

    pub fn metas(&self) -> Vec<ShotMeta> {
        self.shots().iter().map(Shot::meta).collect()
    }

    fn shots(&self) -> Vec<Shot> {
        (0..self.store.n_items())
            .filter_map(|i| self.store.item(i).and_downcast::<Shot>())
            .collect()
    }

    /// Les hylla fra disk utenfor UI-tråden.
    pub fn load(self: &Rc<Self>, done: impl FnOnce() + 'static) {
        let (storage, history) = ShelfStorage::start(storage::shelf_path());
        self.storage.replace(Some(storage));
        let this = self.clone();
        glib::spawn_future_local(async move {
            if let Ok(metas) = history.await {
                // Nye opptak kan allerede ligge øverst på hylla.
                let shots: Vec<Shot> = metas.into_iter().map(|m| this.make_shot(m)).collect();
                this.store.splice(this.store.n_items(), 0, &shots);
                this.loaded.set(true);
                done();
            }
        });
    }

    fn save(&self) {
        if let Some(storage) = self.storage.borrow().as_ref() {
            let snapshot = if self.loaded.get() {
                ShelfSnapshot::Complete(self.metas())
            } else {
                ShelfSnapshot::BeforeLoad(self.metas())
            };
            storage.save(snapshot);
        }
    }

    pub fn finish_saving(&self) {
        if let Some(storage) = self.storage.take() {
            storage.finish();
        }
    }

    /// Nytt skjermbilde øverst, merket i ca. ett sekund, med fokus på ruta.
    pub fn add(self: &Rc<Self>, meta: ShotMeta, thumb: Image) -> Shot {
        let shot = self.make_shot(meta);
        self.loader.insert(shot.key(), to_texture(thumb));
        self.fresh.set(shot.id());
        self.store.insert(0, &shot);
        self.save();
        // Raden finnes ikke før lista har lagt seg ut på nytt.
        let list = self.list.clone();
        glib::timeout_add_local_once(Duration::from_millis(60), move || {
            // Skjult i kompaktmodus: fokus på en skjult rad ville stoppet alle taster.
            if list.is_mapped() {
                focus_row(&list, 0);
            }
        });
        self.flash(&shot);
        shot
    }

    fn flash(self: &Rc<Self>, shot: &Shot) {
        self.fresh.set(shot.id());
        let this = self.clone();
        let id = shot.id();
        glib::timeout_add_local_once(Duration::from_millis(1300), move || {
            if this.fresh.get() == id {
                this.fresh.set(0);
                this.restyle_visible();
            }
        });
    }

    fn for_each_visible(&self, f: impl Fn(&Tile)) {
        let mut child = self.list.first_child();
        while let Some(row) = child {
            if let Some(t) = row.first_child().and_downcast::<Tile>() {
                f(&t);
            }
            child = row.next_sibling();
        }
    }

    /// Ny-markering og valg på rutene som vises. Lista binder ikke på nytt selv.
    fn restyle_visible(&self) {
        self.for_each_visible(|t| {
            let Some(shot) = t.current() else { return };
            if shot.id() == self.fresh.get() {
                t.add_css_class("fresh");
            } else {
                t.remove_css_class("fresh");
            }
            t.set_selection(self.sel.active.get(), self.sel.has(shot.id()));
        });
    }

    /// Last thumbnailet på nytt for ruta som viser bildet nå. Samme objekt på samme
    /// plass gir ingen ny binding i lista, så det må gjøres her.
    fn reload_visible(self: &Rc<Self>, shot: &Shot) {
        let mut child = self.list.first_child();
        while let Some(row) = child {
            if let Some(t) = row.first_child().and_downcast::<Tile>()
                && t.current().as_ref() == Some(shot)
            {
                self.loader.queue.borrow_mut().push_back(Request {
                    shot: shot.clone(),
                    tile: t.downgrade(),
                });
                pump(self.loader.clone(), self.thumb_box);
                return;
            }
            child = row.next_sibling();
        }
    }

    /// Fila er skrevet på nytt: nytt thumbnail og kort markering.
    pub fn replaced(self: &Rc<Self>, path: &Path) {
        let Some(shot) = self.shots().into_iter().find(|s| s.meta().path == path) else {
            return;
        };
        shot.imp().rev.set(shot.imp().rev.get() + 1);
        self.loader
            .cache
            .borrow_mut()
            .retain(|(k, _)| k.0 != shot.id());
        self.flash(&shot);
        self.restyle_visible();
        self.reload_visible(&shot);
        self.save();
    }

    /// Fokus tilbake på ruta, som i prototypen når annoteringen lukkes.
    pub fn focus_path(&self, path: &Path) {
        let pos = self.find(path).and_then(|s| self.store.find(&s));
        if let Some(pos) = pos.filter(|_| self.list.is_mapped()) {
            focus_row(&self.list, pos);
        }
    }

    pub fn find(&self, path: &Path) -> Option<Shot> {
        self.shots().into_iter().find(|s| s.meta().path == path)
    }

    pub fn remove(&self, shot: &Shot) {
        if let Some(pos) = self.store.find(shot) {
            self.store.remove(pos);
            self.save();
        }
    }

    /// Flere på én gang, med ett lagringskall.
    pub fn remove_many(&self, shots: &[Shot]) {
        let mut changed = false;
        for shot in shots {
            if let Some(pos) = self.store.find(shot) {
                self.store.remove(pos);
                changed = true;
            }
        }
        if changed {
            self.save();
        }
    }

    /// Tøm hylla og gi tilbake det som lå der, til Angre.
    pub fn take_all(&self) -> Vec<ShotMeta> {
        let metas = self.metas();
        self.store.remove_all();
        self.loader.cache.borrow_mut().clear();
        self.save();
        metas
    }

    /// Angre: flett tilbake, nyeste først.
    pub fn restore(self: &Rc<Self>, saved: Vec<ShotMeta>) {
        let mut all = self.metas();
        all.extend(saved);
        all.sort_by(|a, b| b.ts.cmp(&a.ts));
        let shots: Vec<Shot> = all.into_iter().map(|m| self.make_shot(m)).collect();
        self.store.remove_all();
        self.store.splice(0, 0, &shots);
        self.save();
    }

    /// Fjern bekreftet slettede filer, sjekket utenfor UI-tråden.
    pub fn prune(self: &Rc<Self>, done: impl FnOnce() + 'static) {
        let paths: Vec<_> = self
            .shots()
            .into_iter()
            .map(|s| (s.id(), s.meta().path))
            .collect();
        let this = self.clone();
        glib::spawn_future_local(async move {
            let gone = gio::spawn_blocking(move || {
                paths
                    .into_iter()
                    .filter_map(|(id, path)| {
                        matches!(storage::shot_file(&path), ShotFile::Deleted).then_some(id)
                    })
                    .collect::<HashSet<_>>()
            })
            .await
            .unwrap_or_default();
            let removed: Vec<_> = this
                .shots()
                .into_iter()
                .filter(|s| gone.contains(&s.id()))
                .collect();
            if removed.is_empty() {
                return;
            }
            this.remove_many(&removed);
            done();
        });
    }
}

/// Rull til raden og gi den fokus. `ListScrollFlags::FOCUS` alene flytter ikke
/// fokus inn i lista når det står et annet sted i vinduet.
fn focus_row(list: &gtk::ListView, pos: u32) {
    list.scroll_to(pos, gtk::ListScrollFlags::NONE, None);
    let Some(model) = list.model() else { return };
    let Some(want) = model.item(pos).and_downcast::<Shot>() else {
        return;
    };
    grab_row(list.clone(), want, 4);
}

/// Raden kan mangle til lista har lagt seg ut, så vi prøver noen ganger.
fn grab_row(list: gtk::ListView, want: Shot, tries: u32) {
    glib::timeout_add_local_once(Duration::from_millis(16), move || {
        let mut child = list.first_child();
        while let Some(row) = child {
            let tile = row.first_child().and_downcast::<Tile>();
            if tile.and_then(|t| t.current()).is_some_and(|s| s == want) {
                row.grab_focus();
                return;
            }
            child = row.next_sibling();
        }
        if tries > 0 {
            grab_row(list, want, tries - 1);
        }
    });
}

fn focused_shot(list: &gtk::ListView) -> Option<Shot> {
    let row = list.focus_child()?;
    row.first_child().and_downcast::<Tile>()?.current()
}

/// Thumbnails trenger ikke være større enn ruta på denne skjermen.
fn thumb_box() -> (u32, u32) {
    let mut scale = 1.0f64;
    if let Some(d) = gdk::Display::default() {
        let m = d.monitors();
        for i in 0..m.n_items() {
            if let Some(mon) = m.item(i).and_downcast::<gdk::Monitor>() {
                scale = scale.max(mon.scale());
            }
        }
    }
    let s = (scale.ceil() as u32).clamp(1, 2);
    (224 * s, MAX_TILE_H as u32 * s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_runs_both_ways_and_includes_ends() {
        let order = [7, 3, 9, 4, 1];
        assert_eq!(range_ids(&order, Some(3), 4), vec![3, 9, 4]);
        assert_eq!(range_ids(&order, Some(4), 3), vec![3, 9, 4]);
        assert_eq!(range_ids(&order, Some(9), 9), vec![9]);
        assert_eq!(range_ids(&order, Some(7), 1), order.to_vec());
    }

    #[test]
    fn range_without_anchor_is_just_the_target() {
        let order = [7, 3, 9];
        assert_eq!(range_ids(&order, None, 3), vec![3]);
        // Ankeret er borte fra hylla.
        assert_eq!(range_ids(&order, Some(42), 9), vec![9]);
        // Målet er borte: ingenting.
        assert!(range_ids(&order, Some(7), 42).is_empty());
    }

    #[test]
    fn time_text_today_yesterday_and_older() {
        let today = (2026, 306);
        assert_eq!(time_text(today, None, today, (9, 5), (2, 11)), "09:05");
        assert_eq!(
            time_text((2026, 305), Some(today), today, (23, 59), (1, 11)),
            "i går 23:59"
        );
        assert_eq!(
            time_text((2026, 300), Some((2026, 301)), today, (8, 0), (27, 10)),
            "27.10. 08:00"
        );
    }
}
