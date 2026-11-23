//! Koordinater mellom widget og bilde. Bildet beholder full oppløsning og vises skalert.
//! Alle `View`-verdier er i logiske piksler, og bilderektangelet legges på hele
//! enhetspiksler, så punktene treffer riktig også med brøkskalering.

/// Luft rundt bildet, som `padding` på `.ann-stage`.
pub const STAGE_PAD: f64 = 16.0;

const MIN_WINDOW: (f64, f64) = (720.0, 380.0);
const MONITOR_MARGIN: f64 = 40.0;
const PANEL_MARGIN: f64 = 20.0;
const TOP_BAR: f64 = 32.0;
const DOCK: f64 = 56.0;
/// Høyden på tittellinja og verktøylinja.
const CHROME: f64 = 47.0 + 46.0;

/// Visningsskala: passer i rommet, små bilder forstørres inntil 3 ganger.
pub fn view_scale(w: f64, h: f64, room_w: f64, room_h: f64, device_scale: f64) -> f64 {
    let fit = (room_w / w).min(room_h / h);
    let zoom = (320.0 / w.max(h)).clamp(1.0, 3.0) / device_scale;
    fit.min(zoom)
}

/// Vindusstørrelse for et bilde på en skjerm, som `fitAnnotate` i prototypen.
pub fn window_size(img: (f64, f64), monitor: (f64, f64), device_scale: f64) -> (i32, i32) {
    let pad = STAGE_PAD * 2.0;
    let work = monitor.1 - TOP_BAR - DOCK;
    let max_w = monitor.0 - MONITOR_MARGIN - pad;
    let max_h = work - PANEL_MARGIN - CHROME - pad;
    let s = view_scale(img.0, img.1, max_w, max_h, device_scale);
    let (cw, ch) = ((img.0 * s).round().max(1.0), (img.1 * s).round().max(1.0));
    let w = (cw + pad).max(MIN_WINDOW.0).min(monitor.0 - MONITOR_MARGIN);
    let h = (ch + pad + CHROME)
        .max(MIN_WINDOW.1)
        .min(work - PANEL_MARGIN);
    (w as i32, h as i32)
}

/// Vinduet åpner bredt nok til at hele verktøylinja synes, så langt skjermen rekker.
/// På en smalere skjerm ruller verktøylinja sidelengs.
pub fn fit_toolbar(width: i32, toolbar: i32, monitor_width: f64) -> i32 {
    width.max(toolbar.min((monitor_width - MONITOR_MARGIN) as i32))
}

/// Plassen bildet skal få i widgeten.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stage {
    pub width: f64,
    pub height: f64,
    /// Enhetspiksler per logisk piksel. Kan være en brøk.
    pub device_scale: f64,
    /// Widgetens hjørne i overflaten, i logiske piksler.
    pub origin: (f64, f64),
}

/// Hvor bildet ligger i widgeten.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub img_w: f64,
    pub img_h: f64,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    /// Logiske piksler per bildepiksel, `A.s` i prototypen.
    pub scale: f64,
    pub device_scale: f64,
}

/// Plasserer `v` slik at den havner på en hel enhetspiksel i overflaten.
fn snap(v: f64, origin: f64, device_scale: f64) -> f64 {
    ((v + origin) * device_scale).round() / device_scale - origin
}

impl View {
    pub fn fit(img_w: f64, img_h: f64, stage: Stage) -> View {
        let dpr = stage.device_scale;
        let room_w = (stage.width - STAGE_PAD * 2.0).max(1.0);
        let room_h = (stage.height - STAGE_PAD * 2.0).max(1.0);
        let scale = view_scale(img_w, img_h, room_w, room_h, dpr);
        let w = (img_w * scale * dpr).round().max(1.0) / dpr;
        let h = (img_h * scale * dpr).round().max(1.0) / dpr;
        View {
            img_w,
            img_h,
            x: snap((stage.width - w) / 2.0, stage.origin.0, dpr),
            y: snap((stage.height - h) / 2.0, stage.origin.1, dpr),
            w,
            h,
            scale,
            device_scale: dpr,
        }
    }

    /// Fra widget til bilde, klippet til bildekanten.
    pub fn to_image(self, px: f64, py: f64) -> (f64, f64) {
        (
            ((px - self.x) * self.img_w / self.w).clamp(0.0, self.img_w),
            ((py - self.y) * self.img_h / self.h).clamp(0.0, self.img_h),
        )
    }

    /// Fra bilde til widget.
    pub fn to_view(self, x: f64, y: f64) -> (f64, f64) {
        (
            self.x + x * self.w / self.img_w,
            self.y + y * self.h / self.img_h,
        )
    }

    pub fn contains(&self, px: f64, py: f64) -> bool {
        (self.x..=self.x + self.w).contains(&px) && (self.y..=self.y + self.h).contains(&py)
    }

    /// Bildepiksler per logisk piksel. Streker lagres i bildepiksler, men skal se like
    /// tykke ut på skjermen uansett hvor mye bildet er skalert ned.
    pub fn unit(&self) -> f64 {
        1.0 / self.scale
    }

    /// Bildet vises med færre enhetspiksler enn det har, og trenger utjevning.
    pub fn is_minified(&self) -> bool {
        self.w * self.device_scale < self.img_w - 0.5
    }

    /// Der tekstfeltet åpner seg i bildet: holdes innenfor kanten, som `startText`.
    pub fn text_anchor(&self, x: f64, y: f64, font_px: f64) -> (f64, f64) {
        let s = self.scale;
        let left = (x * s).clamp(0.0, (self.w - 60.0).max(0.0));
        let top = (y * s).clamp(0.0, (self.h - font_px * 1.25).max(0.0));
        (left / s, top / s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stage(w: f64, h: f64, dpr: f64) -> Stage {
        Stage {
            width: w,
            height: h,
            device_scale: dpr,
            origin: (0.0, 0.0),
        }
    }

    #[test]
    fn view_scale_zooms_small_shots_and_fits_big_ones() {
        assert_eq!(view_scale(100.0, 80.0, 1000.0, 800.0, 1.0), 3.0);
        assert_eq!(view_scale(100.0, 80.0, 1000.0, 800.0, 2.0), 1.5);
        assert_eq!(
            view_scale(3840.0, 2160.0, 1920.0, 1000.0, 1.0),
            0.46296296296296297
        );
    }

    #[test]
    fn big_shot_on_a_big_stage_is_one_to_one_in_device_pixels() {
        for dpr in [1.0, 1.25, 1.5, 1.75, 2.0] {
            let v = View::fit(1000.0, 500.0, stage(3000.0, 2000.0, dpr));
            assert_eq!(v.w * dpr, 1000.0);
            assert_eq!(v.h * dpr, 500.0);
            assert!(!v.is_minified());
        }
    }

    #[test]
    fn corners_map_to_image_corners_at_every_scale() {
        for dpr in [1.0, 1.25, 1.5, 1.75, 2.0, 2.25] {
            let v = View::fit(3840.0, 2160.0, stage(1283.0, 777.0, dpr));
            assert_eq!(v.to_image(v.x, v.y), (0.0, 0.0));
            let (x, y) = v.to_image(v.x + v.w, v.y + v.h);
            assert!((x - 3840.0).abs() < 1e-6 && (y - 2160.0).abs() < 1e-6);
        }
    }

    #[test]
    fn image_and_view_round_trip() {
        let v = View::fit(3840.0, 2160.0, stage(1283.0, 777.0, 1.25));
        let (px, py) = v.to_view(1234.5, 987.25);
        let (x, y) = v.to_image(px, py);
        assert!((x - 1234.5).abs() < 1e-9 && (y - 987.25).abs() < 1e-9);
    }

    #[test]
    fn points_outside_are_clamped() {
        let v = View::fit(100.0, 100.0, stage(800.0, 600.0, 1.0));
        assert_eq!(v.to_image(-50.0, -50.0), (0.0, 0.0));
        assert_eq!(v.to_image(9000.0, 9000.0), (100.0, 100.0));
        assert!(!v.contains(v.x - 1.0, v.y));
        assert!(v.contains(v.x, v.y));
    }

    #[test]
    fn view_lands_on_whole_device_pixels_despite_origin_offset() {
        let s = Stage {
            width: 1001.0,
            height: 701.0,
            device_scale: 1.25,
            origin: (13.0, 47.0),
        };
        let v = View::fit(800.0, 600.0, s);
        let dev_x = (v.x + 13.0) * 1.25;
        let dev_y = (v.y + 47.0) * 1.25;
        assert!((dev_x - dev_x.round()).abs() < 1e-9);
        assert!((dev_y - dev_y.round()).abs() < 1e-9);
    }

    #[test]
    fn stroke_unit_follows_the_view() {
        let v = View::fit(3840.0, 2160.0, stage(1952.0, 1112.0, 1.0));
        assert!((v.unit() - 2.0).abs() < 1e-9);
        assert!(v.is_minified());
    }

    #[test]
    fn text_anchor_stays_inside() {
        let v = View::fit(400.0, 200.0, stage(1000.0, 800.0, 1.0));
        assert_eq!(v.text_anchor(399.0, 199.0, 22.0), (340.0, 172.5));
        assert_eq!(v.text_anchor(10.0, 10.0, 22.0), (10.0, 10.0));
    }

    #[test]
    fn window_follows_image_within_limits() {
        // Stort bilde: høyden på skjermen bestemmer, og bredden følger bildet.
        assert_eq!(
            window_size((3840.0, 2160.0), (1920.0, 1080.0), 1.0),
            (1538, 972)
        );
        // Lite bilde: aldri under minstemålet.
        assert_eq!(
            window_size((100.0, 80.0), (1920.0, 1080.0), 1.0),
            (720, 380)
        );
        // HiDPI: 1:1 i enhetspiksler.
        assert_eq!(
            window_size((1920.0, 1080.0), (1920.0, 1080.0), 2.0),
            (992, 665)
        );
    }

    #[test]
    fn window_shows_the_whole_toolbar_when_the_monitor_has_room() {
        assert_eq!(fit_toolbar(720, 790, 1920.0), 790);
        assert_eq!(fit_toolbar(1538, 790, 1920.0), 1538);
        // Smal skjerm: vinduet holder seg innenfor, og verktøylinja ruller.
        assert_eq!(fit_toolbar(600, 790, 640.0), 600);
        assert_eq!(fit_toolbar(500, 790, 640.0), 600);
    }
}
