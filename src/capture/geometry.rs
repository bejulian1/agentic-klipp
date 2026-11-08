//! Ren geometri og tilstandsmaskin for utvalget i overlegget.
//!
//! Utvalget ligger alltid i ekte bildepiksler for skjermen det hører til. `f` er
//! CSS-piksler (GTK-koordinater) per bildepiksel, som i prototypen.

pub const MIN_SIZE: i32 = 4;
/// Håndtakets radius i CSS-piksler.
pub const HANDLE_R: f64 = 5.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }
    pub fn right(&self) -> i32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pt {
    pub x: i32,
    pub y: i32,
}

pub use klipp_system::Mode;

pub fn mode_label(m: Mode) -> &'static str {
    match m {
        Mode::Area => "Område",
        Mode::Freehand => "Frihånd",
        Mode::Window => "Vindu",
        Mode::Screen => "Hele skjermen",
    }
}

pub fn mode_hint(m: Mode) -> &'static str {
    match m {
        Mode::Area => "Dra for å velge område",
        Mode::Freehand => "Tegn en lukket form",
        Mode::Window => "Klikk på et vindu",
        Mode::Screen => "",
    }
}

pub fn mode_icon(m: Mode) -> &'static str {
    match m {
        Mode::Area => "klipp-area-symbolic",
        Mode::Freehand => "klipp-lasso-symbolic",
        Mode::Window => "klipp-window-symbolic",
        Mode::Screen => "klipp-full-symbolic",
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zone {
    Nw,
    Ne,
    Se,
    Sw,
    N,
    E,
    S,
    W,
    Move,
}

impl Zone {
    pub fn cursor(self) -> &'static str {
        match self {
            Zone::Nw | Zone::Se => "nwse-resize",
            Zone::Ne | Zone::Sw => "nesw-resize",
            Zone::N | Zone::S => "ns-resize",
            Zone::E | Zone::W => "ew-resize",
            Zone::Move => "move",
        }
    }
    fn has_w(self) -> bool {
        matches!(self, Zone::Nw | Zone::Sw | Zone::W)
    }
    fn has_e(self) -> bool {
        matches!(self, Zone::Ne | Zone::Se | Zone::E)
    }
    fn has_n(self) -> bool {
        matches!(self, Zone::Nw | Zone::Ne | Zone::N)
    }
    fn has_s(self) -> bool {
        matches!(self, Zone::Sw | Zone::Se | Zone::S)
    }
}

/// De åtte håndtakene, hjørner først, klemt inn så de synes i skjermkanten.
pub fn handle_points(s: Rect, w: i32, h: i32, f: f64) -> [(Zone, f64, f64); 8] {
    let half = HANDLE_R / f;
    let (fw, fh) = (f64::from(w), f64::from(h));
    let cx = |x: f64| x.max(half).min(fw - half);
    let cy = |y: f64| y.max(half).min(fh - half);
    let (x0, x1) = (f64::from(s.x), f64::from(s.right()));
    let (y0, y1) = (f64::from(s.y), f64::from(s.bottom()));
    let (xm, ym) = (
        f64::from(s.x) + f64::from(s.w) / 2.0,
        f64::from(s.y) + f64::from(s.h) / 2.0,
    );
    [
        (Zone::Nw, cx(x0), cy(y0)),
        (Zone::Ne, cx(x1), cy(y0)),
        (Zone::Se, cx(x1), cy(y1)),
        (Zone::Sw, cx(x0), cy(y1)),
        (Zone::N, cx(xm), cy(y0)),
        (Zone::E, cx(x1), cy(ym)),
        (Zone::S, cx(xm), cy(y1)),
        (Zone::W, cx(x0), cy(ym)),
    ]
}

pub fn hit_test(s: Rect, p: Pt, w: i32, h: i32, f: f64) -> Option<Zone> {
    let r = 10.0 / f;
    let e = 6.0 / f;
    let (px, py) = (f64::from(p.x), f64::from(p.y));
    let corner = handle_points(s, w, h, f)
        .into_iter()
        .take(4)
        .find(|&(_, hx, hy)| (px - hx).abs() <= r && (py - hy).abs() <= r);
    if let Some((z, _, _)) = corner {
        return Some(z);
    }
    let (x0, x1) = (f64::from(s.x), f64::from(s.right()));
    let (y0, y1) = (f64::from(s.y), f64::from(s.bottom()));
    let in_x = px >= x0 - e && px <= x1 + e;
    let in_y = py >= y0 - e && py <= y1 + e;
    if in_x && (py - y0).abs() <= e {
        return Some(Zone::N);
    }
    if in_x && (py - y1).abs() <= e {
        return Some(Zone::S);
    }
    if in_y && (px - x0).abs() <= e {
        return Some(Zone::W);
    }
    if in_y && (px - x1).abs() <= e {
        return Some(Zone::E);
    }
    (px > x0 && px < x1 && py > y0 && py < y1).then_some(Zone::Move)
}

/// Flytt eller juster `orig` med `(dx, dy)`, innenfor skjermen og med minste størrelse.
pub fn apply_zone(zone: Zone, orig: Rect, dx: i32, dy: i32, w: i32, h: i32) -> Rect {
    if zone == Zone::Move {
        return Rect::new(
            (orig.x + dx).clamp(0, (w - orig.w).max(0)),
            (orig.y + dy).clamp(0, (h - orig.h).max(0)),
            orig.w,
            orig.h,
        );
    }
    let (mut x0, mut y0, mut x1, mut y1) = (orig.x, orig.y, orig.right(), orig.bottom());
    if zone.has_w() {
        x0 = (x0 + dx).clamp(0, (x1 - MIN_SIZE).max(0));
    }
    if zone.has_e() {
        x1 = (x1 + dx).clamp((x0 + MIN_SIZE).min(w), w);
    }
    if zone.has_n() {
        y0 = (y0 + dy).clamp(0, (y1 - MIN_SIZE).max(0));
    }
    if zone.has_s() {
        y1 = (y1 + dy).clamp((y0 + MIN_SIZE).min(h), h);
    }
    Rect::new(x0, y0, x1 - x0, y1 - y0)
}

pub fn bbox_of(pts: &[Pt]) -> Rect {
    let x0 = pts.iter().map(|p| p.x).min().unwrap_or(0);
    let y0 = pts.iter().map(|p| p.y).min().unwrap_or(0);
    let x1 = pts.iter().map(|p| p.x).max().unwrap_or(0);
    let y1 = pts.iter().map(|p| p.y).max().unwrap_or(0);
    Rect::new(x0, y0, x1 - x0, y1 - y0)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Drag,
    Selected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Act {
    pub zone: Zone,
    pub start: Pt,
    pub sel: Rect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Released {
    Nothing,
    /// Justering ferdig. Utvalget står, linja vises igjen.
    AdjustDone,
    /// Nytt gyldig utvalg. Kan tas med en gang om innstillingen sier det.
    NewSelection,
    /// For lite område. Advarselen vises.
    TooSmall,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Escaped {
    /// Noe ble angret, overlegget står.
    Handled,
    /// Ingenting å angre, lukk overlegget.
    Close,
}

/// Tilstanden til utvalget på én skjerm. Faser: idle, drag og selected.
#[derive(Clone, Debug)]
pub struct Selector {
    pub w: i32,
    pub h: i32,
    pub mode: Mode,
    pub phase: Phase,
    pub sel: Option<Rect>,
    pub pts: Vec<Pt>,
    pub start: Option<Pt>,
    pub act: Option<Act>,
    pub warn: bool,
}

impl Selector {
    pub fn new(w: i32, h: i32, mode: Mode) -> Self {
        let mut s = Self {
            w,
            h,
            mode,
            phase: Phase::Idle,
            sel: None,
            pts: Vec::new(),
            start: None,
            act: None,
            warn: false,
        };
        s.reset();
        s
    }

    pub fn reset(&mut self) {
        self.sel = None;
        self.pts.clear();
        self.start = None;
        self.act = None;
        self.phase = Phase::Idle;
        self.warn = false;
        if self.mode == Mode::Screen {
            self.sel = Some(Rect::new(0, 0, self.w, self.h));
            self.phase = Phase::Selected;
        }
    }

    pub fn set_mode(&mut self, mode: Mode) {
        self.mode = mode;
        self.reset();
    }

    /// Angre: Hele skjermen har ikke noe å dra, så den faller tilbake til Område.
    pub fn clear(&mut self) {
        if self.mode == Mode::Screen {
            self.mode = Mode::Area;
        }
        self.reset();
    }

    pub fn hit(&self, p: Pt, f: f64) -> Option<Zone> {
        if self.mode != Mode::Area || self.phase != Phase::Selected {
            return None;
        }
        hit_test(self.sel?, p, self.w, self.h, f)
    }

    pub fn press(&mut self, p: Pt, f: f64) {
        match (self.phase, self.mode) {
            (Phase::Selected, _) => {
                if let (Some(zone), Some(sel)) = (self.hit(p, f), self.sel) {
                    self.act = Some(Act {
                        zone,
                        start: p,
                        sel,
                    });
                }
            }
            (Phase::Idle, Mode::Area) => {
                self.phase = Phase::Drag;
                self.warn = false;
                self.start = Some(p);
                self.sel = Some(Rect::new(p.x, p.y, 0, 0));
            }
            (Phase::Idle, Mode::Freehand) => {
                self.phase = Phase::Drag;
                self.warn = false;
                self.pts.clear();
                self.pts.push(p);
            }
            _ => {}
        }
    }

    pub fn motion(&mut self, p: Pt) {
        if let Some(a) = self.act {
            let dx = p.x - a.start.x;
            let dy = p.y - a.start.y;
            self.sel = Some(apply_zone(a.zone, a.sel, dx, dy, self.w, self.h));
        } else if self.phase == Phase::Drag {
            if self.mode == Mode::Freehand {
                if let Some(l) = self.pts.last() {
                    let (dx, dy) = (f64::from(p.x - l.x), f64::from(p.y - l.y));
                    if dx.hypot(dy) >= 2.0 {
                        self.pts.push(p);
                    }
                }
                self.sel = Some(bbox_of(&self.pts));
            } else if let Some(s) = self.start {
                self.sel = Some(Rect::new(
                    s.x.min(p.x),
                    s.y.min(p.y),
                    (p.x - s.x).abs(),
                    (p.y - s.y).abs(),
                ));
            }
        }
    }

    pub fn release(&mut self) -> Released {
        if self.act.take().is_some() {
            return Released::AdjustDone;
        }
        if self.phase != Phase::Drag {
            return Released::Nothing;
        }
        let small = match self.sel {
            Some(s) => {
                s.w < MIN_SIZE
                    || s.h < MIN_SIZE
                    || (self.mode == Mode::Freehand && self.pts.len() < 3)
            }
            None => true,
        };
        if small {
            self.reset();
            self.warn = true;
            return Released::TooSmall;
        }
        self.phase = Phase::Selected;
        Released::NewSelection
    }

    pub fn escape(&mut self) -> Escaped {
        if let Some(a) = self.act.take() {
            self.sel = Some(a.sel);
            Escaped::Handled
        } else if self.phase == Phase::Drag {
            self.reset();
            Escaped::Handled
        } else if self.phase == Phase::Selected {
            self.clear();
            Escaped::Handled
        } else {
            Escaped::Close
        }
    }

    pub fn can_take(&self) -> bool {
        self.phase == Phase::Selected && self.act.is_none() && self.sel.is_some()
    }
}

/// Mållappen: under høyre hjørne, ellers over, alltid på skjermen.
pub fn place_size_label(sel: (f64, f64, f64, f64), lw: f64, lh: f64, w: f64, h: f64) -> (f64, f64) {
    let (_, _, right, bottom) = sel;
    let x = right - lw;
    let mut y = bottom + 8.0;
    if y + lh > h - 4.0 {
        y = bottom - lh - 8.0;
    }
    (clampf(x, 4.0, w - lw - 4.0), clampf(y, 4.0, h - lh - 4.0))
}

fn clampf(v: f64, lo: f64, hi: f64) -> f64 {
    v.max(lo).min(hi.max(lo))
}

/// Topplinjas plass, i samme koordinater som handlingslinja.
#[derive(Clone, Copy, Debug)]
pub struct Bar {
    pub left: f64,
    pub right: f64,
    pub bottom: f64,
}

/// Handlingslinja: utenfor utvalget om det går, ellers inni, alltid på skjermen.
pub fn place_actions(
    sel: (f64, f64, f64, f64),
    bw: f64,
    bh: f64,
    bar: Bar,
    w: f64,
    h: f64,
) -> (f64, f64) {
    const M: f64 = 8.0;
    const G: f64 = 10.0;
    let (x0, y0, x1, y1) = sel;
    let on_bar = |x: f64, y: f64| x < bar.right + M && x + bw > bar.left - M && y < bar.bottom + M;
    let fits =
        |x: f64, y: f64| x >= M && y >= M && x + bw <= w - M && y + bh <= h - M && !on_bar(x, y);
    let ax = clampf(x1 - bw, M, w - bw - M);
    let ay = clampf(y1 - bh, M, h - bh - M);
    let candidates = [
        (ax, y1 + G),
        (ax, y0 - bh - G),
        (x1 + G, ay),
        (x0 - bw - G, ay),
    ];
    if let Some(p) = candidates.into_iter().find(|&(x, y)| fits(x, y)) {
        return (p.0.round(), p.1.round());
    }
    let x = clampf(x1 - bw - 12.0, M, w - bw - M);
    let mut y = clampf(y1 - bh - 12.0, M, h - bh - M);
    if on_bar(x, y) {
        y = bar.bottom + M;
    }
    (x.round(), y.round())
}

/// Én skjerm slik GDK ser den: logiske koordinater og fraksjonell skala.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LogicalMonitor {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub scale: f64,
}

/// Portalbildet dekker hele skrivebordet. Her regnes hver skjerms del av bildet ut,
/// i bildepiksler. GNOME tegner hele bildet med den høyeste skalaen blant skjermene.
pub fn monitor_regions(mons: &[LogicalMonitor], img_w: i32, img_h: i32) -> Vec<Rect> {
    let x0 = mons.iter().map(|m| m.x).min().unwrap_or(0);
    let y0 = mons.iter().map(|m| m.y).min().unwrap_or(0);
    let x1 = mons.iter().map(|m| m.x + m.w).max().unwrap_or(img_w);
    let y1 = mons.iter().map(|m| m.y + m.h).max().unwrap_or(img_h);
    let (lw, lh) = (f64::from((x1 - x0).max(1)), f64::from((y1 - y0).max(1)));
    let sx = f64::from(img_w) / lw;
    let sy = f64::from(img_h) / lh;
    mons.iter()
        .map(|m| {
            let l = (f64::from(m.x - x0) * sx).round() as i32;
            let t = (f64::from(m.y - y0) * sy).round() as i32;
            let r = (f64::from(m.x + m.w - x0) * sx).round() as i32;
            let b = (f64::from(m.y + m.h - y0) * sy).round() as i32;
            let l = l.clamp(0, img_w);
            let t = t.clamp(0, img_h);
            Rect::new(l, t, r.clamp(l, img_w) - l, b.clamp(t, img_h) - t)
        })
        .collect()
}

/// Skjermens virkelige piksler. Selve utvalget regnes i disse.
pub fn physical_size(m: &LogicalMonitor) -> (i32, i32) {
    (
        (f64::from(m.w) * m.scale).round() as i32,
        (f64::from(m.h) * m.scale).round() as i32,
    )
}

/// Utvalg i skjermens fysiske piksler som logisk flate på skrivebordet.
pub fn logical_area(sel: Rect, mon: &LogicalMonitor) -> (i32, i32, i32, i32) {
    let scaled = |v: i32| (f64::from(v) / mon.scale).round() as i32;
    (
        mon.x + scaled(sel.x),
        mon.y + scaled(sel.y),
        scaled(sel.w),
        scaled(sel.h),
    )
}

/// Punkt i skjermens fysiske piksler til portalbildet. Har skjermen lavere skala enn
/// den høyeste, er delen av bildet forstørret av GNOME, og faktoren er større enn 1.
pub fn phys_to_image(p: Pt, phys: (i32, i32), region: Rect) -> Pt {
    Pt {
        x: region.x
            + (f64::from(p.x) * f64::from(region.w) / f64::from(phys.0.max(1))).round() as i32,
        y: region.y
            + (f64::from(p.y) * f64::from(region.h) / f64::from(phys.1.max(1))).round() as i32,
    }
}

pub fn rect_to_image(r: Rect, phys: (i32, i32), region: Rect) -> Rect {
    let a = phys_to_image(Pt { x: r.x, y: r.y }, phys, region);
    let b = phys_to_image(
        Pt {
            x: r.right(),
            y: r.bottom(),
        },
        phys,
        region,
    );
    Rect::new(a.x, a.y, b.x - a.x, b.y - a.y)
}

/// GTK-punkt til bildepiksel på skjermen, klemt til skjermen.
pub fn to_pixel(x: f64, y: f64, f: f64, w: i32, h: i32) -> Pt {
    Pt {
        x: ((x / f).round() as i32).clamp(0, w),
        y: ((y / f).round() as i32).clamp(0, h),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_becomes_a_logical_area() {
        let mon = LogicalMonitor {
            x: 1920,
            y: 100,
            w: 1536,
            h: 864,
            scale: 1.25,
        };
        assert_eq!(
            logical_area(Rect::new(100, 200, 400, 300), &mon),
            (2000, 260, 320, 240)
        );
    }

    #[test]
    fn handles_are_clamped_inside_the_screen() {
        let p = handle_points(Rect::new(0, 0, 100, 50), 100, 50, 1.0);
        assert_eq!(p[0], (Zone::Nw, 5.0, 5.0));
        assert_eq!(p[2], (Zone::Se, 95.0, 45.0));
        assert_eq!(p[4], (Zone::N, 50.0, 5.0));
    }

    #[test]
    fn hit_test_finds_corners_edges_and_move() {
        let s = Rect::new(100, 100, 200, 100);
        let hit = |x, y| hit_test(s, Pt { x, y }, 1000, 1000, 1.0);
        assert_eq!(hit(95, 95), Some(Zone::Nw));
        assert_eq!(hit(305, 205), Some(Zone::Se));
        assert_eq!(hit(200, 96), Some(Zone::N));
        assert_eq!(hit(304, 150), Some(Zone::E));
        assert_eq!(hit(200, 150), Some(Zone::Move));
        assert_eq!(hit(50, 50), None);
    }

    #[test]
    fn hit_test_scales_with_css_per_pixel() {
        let s = Rect::new(100, 100, 200, 100);
        assert_eq!(
            hit_test(s, Pt { x: 85, y: 85 }, 1000, 1000, 0.5),
            Some(Zone::Nw)
        );
        assert_eq!(hit_test(s, Pt { x: 85, y: 85 }, 1000, 1000, 1.0), None);
    }

    #[test]
    fn resize_keeps_min_size_and_screen() {
        let o = Rect::new(100, 100, 50, 50);
        assert_eq!(
            apply_zone(Zone::W, o, 500, 0, 1000, 1000),
            Rect::new(146, 100, 4, 50)
        );
        assert_eq!(
            apply_zone(Zone::Se, o, 5000, 5000, 1000, 1000),
            Rect::new(100, 100, 900, 900)
        );
        assert_eq!(
            apply_zone(Zone::Nw, o, -500, -500, 1000, 1000),
            Rect::new(0, 0, 150, 150)
        );
        assert_eq!(
            apply_zone(Zone::N, o, 0, 9000, 1000, 1000),
            Rect::new(100, 146, 50, 4)
        );
    }

    #[test]
    fn move_stays_on_screen() {
        let o = Rect::new(100, 100, 50, 50);
        assert_eq!(
            apply_zone(Zone::Move, o, 2000, -500, 1000, 800),
            Rect::new(950, 0, 50, 50)
        );
    }

    #[test]
    fn drag_gives_normalized_selection() {
        let mut s = Selector::new(1000, 800, Mode::Area);
        s.press(Pt { x: 300, y: 300 }, 1.0);
        assert_eq!(s.phase, Phase::Drag);
        s.motion(Pt { x: 100, y: 200 });
        assert_eq!(s.sel, Some(Rect::new(100, 200, 200, 100)));
        assert_eq!(s.release(), Released::NewSelection);
        assert_eq!(s.phase, Phase::Selected);
        assert!(s.can_take());
    }

    #[test]
    fn too_small_area_warns_and_resets() {
        let mut s = Selector::new(1000, 800, Mode::Area);
        s.press(Pt { x: 300, y: 300 }, 1.0);
        s.motion(Pt { x: 302, y: 400 });
        assert_eq!(s.release(), Released::TooSmall);
        assert_eq!(s.phase, Phase::Idle);
        assert!(s.warn);
        assert_eq!(s.sel, None);
    }

    #[test]
    fn freehand_needs_three_points() {
        let mut s = Selector::new(1000, 800, Mode::Freehand);
        s.press(Pt { x: 10, y: 10 }, 1.0);
        s.motion(Pt { x: 100, y: 100 });
        assert_eq!(s.release(), Released::TooSmall);
        s.press(Pt { x: 10, y: 10 }, 1.0);
        s.motion(Pt { x: 100, y: 10 });
        s.motion(Pt { x: 100, y: 100 });
        s.motion(Pt { x: 10, y: 100 });
        assert_eq!(s.release(), Released::NewSelection);
        assert_eq!(s.sel, Some(Rect::new(10, 10, 90, 90)));
        assert_eq!(s.pts.len(), 4);
    }

    #[test]
    fn freehand_skips_points_closer_than_two_pixels() {
        let mut s = Selector::new(100, 100, Mode::Freehand);
        s.press(Pt { x: 10, y: 10 }, 1.0);
        s.motion(Pt { x: 11, y: 10 });
        assert_eq!(s.pts.len(), 1);
        s.motion(Pt { x: 12, y: 10 });
        assert_eq!(s.pts.len(), 2);
    }

    #[test]
    fn escape_steps_back_like_the_prototype() {
        let mut s = Selector::new(1000, 800, Mode::Area);
        s.press(Pt { x: 100, y: 100 }, 1.0);
        s.motion(Pt { x: 300, y: 300 });
        s.release();
        s.press(Pt { x: 200, y: 200 }, 1.0);
        s.motion(Pt { x: 250, y: 250 });
        assert_eq!(s.sel, Some(Rect::new(150, 150, 200, 200)));
        assert_eq!(s.escape(), Escaped::Handled);
        assert_eq!(s.sel, Some(Rect::new(100, 100, 200, 200)));
        assert_eq!(s.phase, Phase::Selected);
        assert_eq!(s.escape(), Escaped::Handled);
        assert_eq!(s.phase, Phase::Idle);
        assert_eq!(s.escape(), Escaped::Close);
    }

    #[test]
    fn full_screen_falls_back_to_area_on_undo() {
        let mut s = Selector::new(1920, 1080, Mode::Screen);
        assert_eq!(s.sel, Some(Rect::new(0, 0, 1920, 1080)));
        assert!(s.can_take());
        s.clear();
        assert_eq!(s.mode, Mode::Area);
        assert_eq!(s.phase, Phase::Idle);
    }

    #[test]
    fn no_handles_in_free_or_full_mode() {
        let s = Selector::new(100, 100, Mode::Screen);
        assert_eq!(s.hit(Pt { x: 50, y: 50 }, 1.0), None);
    }

    const BAR: Bar = Bar {
        left: 600.0,
        right: 1000.0,
        bottom: 60.0,
    };

    #[test]
    fn actions_go_below_selection_first() {
        let p = place_actions(
            (100.0, 100.0, 400.0, 300.0),
            220.0,
            44.0,
            BAR,
            1600.0,
            900.0,
        );
        assert_eq!(p, (180.0, 310.0));
    }

    #[test]
    fn actions_go_above_when_no_room_below() {
        let p = place_actions(
            (100.0, 500.0, 400.0, 890.0),
            220.0,
            44.0,
            BAR,
            1600.0,
            900.0,
        );
        assert_eq!(p, (180.0, 446.0));
    }

    #[test]
    fn actions_never_cover_the_top_bar() {
        let p = place_actions((700.0, 70.0, 900.0, 890.0), 220.0, 44.0, BAR, 1600.0, 900.0);
        assert!(p.0 >= 900.0 || p.0 + 220.0 <= 700.0, "{p:?}");
    }

    #[test]
    fn actions_fall_back_inside_on_full_screen() {
        let p = place_actions((0.0, 0.0, 1600.0, 900.0), 220.0, 44.0, BAR, 1600.0, 900.0);
        assert_eq!(p, (1368.0, 844.0));
    }

    #[test]
    fn size_label_flips_above_at_the_bottom() {
        assert_eq!(
            place_size_label((0.0, 0.0, 300.0, 100.0), 80.0, 20.0, 800.0, 600.0),
            (220.0, 108.0)
        );
        assert_eq!(
            place_size_label((0.0, 0.0, 300.0, 590.0), 80.0, 20.0, 800.0, 600.0),
            (220.0, 562.0)
        );
    }

    #[test]
    fn monitor_regions_at_fractional_scale() {
        let m = [LogicalMonitor {
            x: 0,
            y: 0,
            w: 1536,
            h: 864,
            scale: 1.25,
        }];
        assert_eq!(
            monitor_regions(&m, 1920, 1080),
            vec![Rect::new(0, 0, 1920, 1080)]
        );
    }

    #[test]
    fn monitor_regions_side_by_side_with_mixed_scale() {
        // 1920x1080 med skala 1 ved siden av 2560x1440 med skala 2: GNOME bruker skala 2.
        let m = [
            LogicalMonitor {
                x: 0,
                y: 0,
                w: 1920,
                h: 1080,
                scale: 1.0,
            },
            LogicalMonitor {
                x: 1920,
                y: 0,
                w: 1280,
                h: 720,
                scale: 2.0,
            },
        ];
        assert_eq!(
            monitor_regions(&m, 6400, 2160),
            vec![Rect::new(0, 0, 3840, 2160), Rect::new(3840, 0, 2560, 1440)]
        );
    }

    #[test]
    fn monitor_regions_handle_negative_origin() {
        let m = [
            LogicalMonitor {
                x: -1280,
                y: 0,
                w: 1280,
                h: 1024,
                scale: 1.0,
            },
            LogicalMonitor {
                x: 0,
                y: 0,
                w: 1920,
                h: 1080,
                scale: 1.0,
            },
        ];
        assert_eq!(
            monitor_regions(&m, 3200, 1080),
            vec![Rect::new(0, 0, 1280, 1024), Rect::new(1280, 0, 1920, 1080)]
        );
    }

    #[test]
    fn same_scale_maps_one_to_one() {
        let region = Rect::new(1920, 0, 2560, 1440);
        let r = Rect::new(10, 20, 300, 200);
        assert_eq!(
            rect_to_image(r, (2560, 1440), region),
            Rect::new(1930, 20, 300, 200)
        );
    }

    #[test]
    fn lower_scale_monitor_maps_into_enlarged_region() {
        // Skjerm 1600x900 med skala 1 i et bilde tegnet med skala 1,5.
        let m = LogicalMonitor {
            x: 1280,
            y: 0,
            w: 1600,
            h: 900,
            scale: 1.0,
        };
        assert_eq!(physical_size(&m), (1600, 900));
        let region = Rect::new(1920, 0, 2400, 1350);
        let r = Rect::new(100, 100, 200, 100);
        assert_eq!(
            rect_to_image(r, (1600, 900), region),
            Rect::new(2070, 150, 300, 150)
        );
    }

    #[test]
    fn fractional_scale_gives_physical_size() {
        let m = LogicalMonitor {
            x: 0,
            y: 0,
            w: 1536,
            h: 864,
            scale: 1.25,
        };
        assert_eq!(physical_size(&m), (1920, 1080));
    }

    #[test]
    fn pixel_mapping_rounds_and_clamps() {
        assert_eq!(to_pixel(100.4, 50.0, 0.8, 1920, 1080), Pt { x: 126, y: 63 });
        assert_eq!(
            to_pixel(-5.0, 5000.0, 1.0, 1920, 1080),
            Pt { x: 0, y: 1080 }
        );
    }
}
