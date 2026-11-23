//! Tegner operasjonene med cairo. Samme kode gir forhåndsvisningen og den endelige filen,
//! så det man ser er det man får. Trenger verken skjerm eller GTK-hovedtråd for å virke.

use crate::ops::{Color, HIGHLIGHT_ALPHA, LINE_HEIGHT, Op, arrow_head, visible};
use crate::pixels::{self, Region};
use gtk::{cairo, glib, pango};
use std::f64::consts::TAU;

const ARROW_SPREAD: f64 = 0.42;
const TEXT_WEIGHT: pango::Weight = pango::Weight::Semibold;
const MARKER_WEIGHT: pango::Weight = pango::Weight::Bold;

fn set_color(cr: &cairo::Context, c: Color) {
    cr.set_source_rgb(c.r, c.g, c.b);
}

/// Nytt bilde med innholdet i `src`.
pub fn surface_from(src: &[u8], width: i32, height: i32) -> Result<cairo::ImageSurface, String> {
    let err = |e: &dyn std::fmt::Display| format!("Kunne ikke tegne: {e}");
    let mut s =
        cairo::ImageSurface::create(cairo::Format::ARgb32, width, height).map_err(|e| err(&e))?;
    {
        let mut data = s.data().map_err(|e| err(&e))?;
        let dst: &mut [u8] = &mut data;
        if dst.len() != src.len() {
            return Err(err(&"uventet radlengde"));
        }
        dst.copy_from_slice(src);
    }
    Ok(s)
}

/// Gjør bildet om til minne som kan deles med teksturen og med bakgrunnstråder, uten kopi.
pub fn into_bytes(s: cairo::ImageSurface) -> Result<glib::Bytes, String> {
    s.flush();
    let data = s
        .take_data()
        .map_err(|e| format!("Kunne ikke tegne: {e}"))?;
    Ok(glib::Bytes::from_owned(data))
}

/// Brenner én operasjon inn i bildet.
pub fn burn(s: &mut cairo::ImageSurface, op: &Op, family: &str) -> Result<(), String> {
    let err = |e: &dyn std::fmt::Display| format!("Kunne ikke tegne: {e}");
    if let Op::Redact {
        x1,
        y1,
        x2,
        y2,
        block,
    } = *op
    {
        s.flush();
        let (w, h) = (s.width() as usize, s.height() as usize);
        let rect = (x1.min(x2), y1.min(y2), (x2 - x1).abs(), (y2 - y1).abs());
        if let Some(r) = Region::of_rect(rect.0, rect.1, rect.2, rect.3, w, h) {
            let mut data = s.data().map_err(|e| err(&e))?;
            pixels::pixelate(&mut data, w, r, block);
        }
        return Ok(());
    }
    if op.needs_replay() {
        return Ok(());
    }
    let cr = cairo::Context::new(&*s).map_err(|e| err(&e))?;
    draw(&cr, op, family).map_err(|e| err(&e))
}

/// Originalen med alle operasjonene spilt av.
pub fn replay(
    original: &[u8],
    width: i32,
    height: i32,
    ops: &[Op],
    family: &str,
) -> Result<cairo::ImageSurface, String> {
    let mut s = surface_from(original, width, height)?;
    for (_, op) in visible(ops) {
        burn(&mut s, op, family)?;
    }
    Ok(s)
}

/// Tegner alt unntatt sladd, som leser piksler og derfor går via [`burn`].
pub fn draw(cr: &cairo::Context, op: &Op, family: &str) -> Result<(), cairo::Error> {
    cr.save()?;
    cr.set_line_cap(cairo::LineCap::Round);
    cr.set_line_join(cairo::LineJoin::Round);
    match op {
        &Op::Arrow {
            x1,
            y1,
            x2,
            y2,
            lw,
            c,
        } => draw_arrow(cr, (x1, y1), (x2, y2), lw, c)?,
        &Op::Line {
            x1,
            y1,
            x2,
            y2,
            lw,
            c,
        } => {
            set_color(cr, c);
            cr.set_line_width(lw);
            cr.move_to(x1, y1);
            cr.line_to(x2, y2);
            cr.stroke()?;
        }
        &Op::Rect {
            x1,
            y1,
            x2,
            y2,
            lw,
            c,
        } => {
            set_color(cr, c);
            cr.set_line_width(lw);
            cr.rectangle(x1.min(x2), y1.min(y2), (x2 - x1).abs(), (y2 - y1).abs());
            cr.stroke()?;
        }
        &Op::Ellipse {
            x1,
            y1,
            x2,
            y2,
            lw,
            c,
        } => draw_ellipse(cr, (x1, y1), (x2, y2), lw, c)?,
        Op::Free { pts, lw, c } => {
            set_color(cr, *c);
            draw_free(cr, pts, *lw)?;
        }
        Op::Highlight { pts, lw, c } => {
            // Én sammenhengende strek, så den ikke blir mørkere der den krysser seg selv.
            cr.set_source_rgba(c.r, c.g, c.b, HIGHLIGHT_ALPHA);
            cr.set_line_cap(cairo::LineCap::Square);
            draw_free(cr, pts, *lw)?;
        }
        Op::Text {
            x,
            y,
            size,
            c,
            text,
        } => draw_text(cr, (*x, *y), *size, *c, text, family)?,
        &Op::Marker { x, y, r, c, fg, n } => draw_marker(cr, (x, y), r, c, fg, n, family)?,
        Op::Redact { .. } | Op::Erase { .. } => {}
    }
    cr.restore()
}

fn draw_arrow(
    cr: &cairo::Context,
    from: (f64, f64),
    to: (f64, f64),
    lw: f64,
    c: Color,
) -> Result<(), cairo::Error> {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let len = dx.hypot(dy);
    if len < 1.0 {
        return Ok(());
    }
    let a = dy.atan2(dx);
    let head = arrow_head(lw, len);
    set_color(cr, c);
    cr.set_line_width(lw);
    cr.move_to(from.0, from.1);
    cr.line_to(to.0 - a.cos() * head * 0.8, to.1 - a.sin() * head * 0.8);
    cr.stroke()?;
    let wing = |angle: f64| (to.0 - head * angle.cos(), to.1 - head * angle.sin());
    let (l, r) = (wing(a - ARROW_SPREAD), wing(a + ARROW_SPREAD));
    cr.move_to(to.0, to.1);
    cr.line_to(l.0, l.1);
    cr.line_to(r.0, r.1);
    cr.close_path();
    cr.fill()
}

fn draw_ellipse(
    cr: &cairo::Context,
    a: (f64, f64),
    b: (f64, f64),
    lw: f64,
    c: Color,
) -> Result<(), cairo::Error> {
    let (rx, ry) = (((b.0 - a.0) / 2.0).abs(), ((b.1 - a.1) / 2.0).abs());
    if rx < 0.5 || ry < 0.5 {
        return Ok(());
    }
    // Skaleringen gjelder bare formen. Streken tegnes etterpå, så den er like tykk overalt.
    cr.save()?;
    cr.translate((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
    cr.scale(rx, ry);
    cr.arc(0.0, 0.0, 1.0, 0.0, TAU);
    cr.restore()?;
    set_color(cr, c);
    cr.set_line_width(lw);
    cr.stroke()
}

/// Frihånd i fargen som allerede er valgt.
fn draw_free(cr: &cairo::Context, p: &[(f64, f64)], lw: f64) -> Result<(), cairo::Error> {
    let Some(&first) = p.first() else {
        return Ok(());
    };
    if p.len() == 1 {
        cr.arc(first.0, first.1, lw / 2.0, 0.0, TAU);
        return cr.fill();
    }
    cr.set_line_width(lw);
    cr.move_to(first.0, first.1);
    // Kvadratiske kurver gjennom midtpunktene, som cairo vil ha som kubiske.
    let mut at = first;
    for pair in p[1..].windows(2) {
        let (ctrl, next) = (pair[0], pair[1]);
        let end = ((ctrl.0 + next.0) / 2.0, (ctrl.1 + next.1) / 2.0);
        cr.curve_to(
            at.0 + (ctrl.0 - at.0) * 2.0 / 3.0,
            at.1 + (ctrl.1 - at.1) * 2.0 / 3.0,
            end.0 + (ctrl.0 - end.0) * 2.0 / 3.0,
            end.1 + (ctrl.1 - end.1) * 2.0 / 3.0,
            end.0,
            end.1,
        );
        at = end;
    }
    let last = p[p.len() - 1];
    cr.line_to(last.0, last.1);
    cr.stroke()
}

/// Skrift som skalerer likt i forhåndsvisning og full oppløsning, uten hinting.
fn layout(
    cr: &cairo::Context,
    family: &str,
    weight: pango::Weight,
    px: f64,
) -> Result<pango::Layout, cairo::Error> {
    let layout = pangocairo::functions::create_layout(cr);
    let mut options = cairo::FontOptions::new()?;
    options.set_hint_metrics(cairo::HintMetrics::Off);
    options.set_hint_style(cairo::HintStyle::None);
    pangocairo::functions::context_set_font_options(&layout.context(), Some(&options));
    let mut font = pango::FontDescription::new();
    font.set_family(family);
    font.set_weight(weight);
    font.set_absolute_size(px * f64::from(pango::SCALE));
    layout.set_font_description(Some(&font));
    pangocairo::functions::update_layout(cr, &layout);
    Ok(layout)
}

fn draw_text(
    cr: &cairo::Context,
    at: (f64, f64),
    size: f64,
    c: Color,
    text: &str,
    family: &str,
) -> Result<(), cairo::Error> {
    let layout = layout(cr, family, TEXT_WEIGHT, size)?;
    let line_height = size * LINE_HEIGHT;
    for (i, line) in text.split('\n').enumerate() {
        if line.is_empty() {
            continue;
        }
        layout.set_text(line);
        let (_, logical) = layout.extents();
        let natural = f64::from(logical.height()) / f64::from(pango::SCALE);
        // Samme plassering som tekstfeltet gir med fast linjehøyde.
        let y = at.1 + i as f64 * line_height + (line_height - natural) / 2.0;
        cr.move_to(at.0, y);
        pangocairo::functions::layout_path(cr, &layout);
    }
    cr.set_line_width(size / 5.0);
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.8);
    cr.stroke_preserve()?;
    set_color(cr, c);
    cr.fill()
}

fn draw_marker(
    cr: &cairo::Context,
    at: (f64, f64),
    r: f64,
    c: Color,
    fg: Color,
    n: u32,
    family: &str,
) -> Result<(), cairo::Error> {
    cr.arc(at.0, at.1, r, 0.0, TAU);
    set_color(cr, c);
    cr.fill_preserve()?;
    cr.set_line_width(r * 0.16);
    cr.set_source_rgb(1.0, 1.0, 1.0);
    cr.stroke()?;
    let px = r * if n > 9 { 1.0 } else { 1.2 };
    let layout = layout(cr, family, MARKER_WEIGHT, px)?;
    layout.set_text(&n.to_string());
    let (ink, _) = layout.extents();
    let unit = f64::from(pango::SCALE);
    let cx = (f64::from(ink.x()) + f64::from(ink.width()) / 2.0) / unit;
    let cy = (f64::from(ink.y()) + f64::from(ink.height()) / 2.0) / unit;
    cr.move_to(at.0 - cx, at.1 - cy);
    set_color(cr, fg);
    pangocairo::functions::layout_path(cr, &layout);
    cr.fill()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::{BLACK, COLORS, WHITE};

    const W: i32 = 200;
    const H: i32 = 120;

    fn blank() -> Vec<u8> {
        [40u8, 40, 40, 255].repeat((W * H) as usize)
    }

    fn render(src: &[u8], ops: &[Op]) -> Vec<u8> {
        into_bytes(replay(src, W, H, ops, "Sans").unwrap())
            .unwrap()
            .to_vec()
    }

    fn px(buf: &[u8], x: i32, y: i32) -> [u8; 4] {
        let at = ((y * W + x) * 4) as usize;
        buf[at..at + 4].try_into().unwrap()
    }

    fn red() -> Color {
        COLORS[0].color
    }

    fn changed(a: &[u8], b: &[u8]) -> usize {
        a.chunks_exact(4)
            .zip(b.chunks_exact(4))
            .filter(|(x, y)| x != y)
            .count()
    }

    #[test]
    fn rect_strokes_the_edge_and_not_the_middle() {
        let src = blank();
        let op = Op::Rect {
            x1: 10.0,
            y1: 10.0,
            x2: 100.0,
            y2: 80.0,
            lw: 4.0,
            c: red(),
        };
        let out = render(&src, &[op]);
        assert_eq!(px(&out, 50, 10), [0x3b, 0x33, 0xed, 255]);
        assert_eq!(px(&out, 50, 40), px(&src, 50, 40));
    }

    #[test]
    fn dragging_backwards_gives_the_same_rect() {
        let src = blank();
        let rect = |x1, y1, x2, y2| Op::Rect {
            x1,
            y1,
            x2,
            y2,
            lw: 3.0,
            c: red(),
        };
        assert_eq!(
            render(&src, &[rect(10.0, 10.0, 90.0, 60.0)]),
            render(&src, &[rect(90.0, 60.0, 10.0, 10.0)])
        );
    }

    #[test]
    fn arrow_has_a_head_at_the_end() {
        let src = blank();
        let op = Op::Arrow {
            x1: 10.0,
            y1: 60.0,
            x2: 150.0,
            y2: 60.0,
            lw: 3.0,
            c: red(),
        };
        let out = render(&src, &[op]);
        // Hodet er bredere enn streken like foran spissen, men ikke ved halen.
        assert_eq!(px(&out, 141, 57), [0x3b, 0x33, 0xed, 255]);
        assert_eq!(px(&out, 20, 57), px(&src, 20, 57));
    }

    #[test]
    fn single_point_freehand_is_a_dot() {
        let src = blank();
        let op = Op::Free {
            pts: vec![(50.0, 50.0)],
            lw: 8.0,
            c: red(),
        };
        let out = render(&src, &[op]);
        assert_eq!(px(&out, 50, 50), [0x3b, 0x33, 0xed, 255]);
    }

    #[test]
    fn freehand_passes_through_the_stroke() {
        let src = blank();
        let op = Op::Free {
            pts: vec![(10.0, 10.0), (60.0, 10.0), (110.0, 10.0), (160.0, 10.0)],
            lw: 6.0,
            c: red(),
        };
        let out = render(&src, &[op]);
        assert_eq!(px(&out, 85, 10), [0x3b, 0x33, 0xed, 255]);
    }

    #[test]
    fn text_paints_outline_and_fill() {
        let src = blank();
        let op = Op::Text {
            x: 10.0,
            y: 10.0,
            size: 40.0,
            c: WHITE,
            text: "HEI\nDU".into(),
        };
        let out = render(&src, &[op]);
        assert!(changed(&src, &out) > 500);
        // Andre linje havner under første.
        let below = (0..W).any(|x| px(&out, x, 75) != px(&src, x, 75));
        assert!(below);
    }

    #[test]
    fn marker_is_a_filled_disc_with_white_ring_and_digit() {
        let src = blank();
        let op = Op::Marker {
            x: 60.0,
            y: 60.0,
            r: 30.0,
            c: red(),
            fg: BLACK,
            n: 7,
        };
        let out = render(&src, &[op]);
        assert_eq!(px(&out, 60 + 30, 60), [255, 255, 255, 255]);
        assert_eq!(px(&out, 60 - 24, 60), [0x3b, 0x33, 0xed, 255]);
        let dark = (40..80).any(|x| px(&out, x, 60) == [0, 0, 0, 255]);
        assert!(dark, "sifferet mangler");
    }

    #[test]
    fn redaction_destroys_detail_under_it() {
        let mut src = blank();
        for chunk in src.chunks_exact_mut(4).step_by(3) {
            chunk[..3].fill(250);
        }
        let op = Op::Redact {
            x1: 80.0,
            y1: 60.0,
            x2: 20.0,
            y2: 20.0,
            block: 10.0,
        };
        let out = render(&src, &[op]);
        for y in (20..60).step_by(10) {
            for x in (20..80).step_by(10) {
                let first = px(&out, x, y);
                assert!((0..10).all(|d| px(&out, x + d, y + d) == first));
            }
        }
        assert_ne!(px(&out, 20, 20), px(&src, 20, 20));
        assert_eq!(px(&out, 100, 100), px(&src, 100, 100));
    }

    #[test]
    fn redaction_sees_earlier_annotations() {
        let src = blank();
        let rect = Op::Rect {
            x1: 0.0,
            y1: 0.0,
            x2: 40.0,
            y2: 40.0,
            lw: 20.0,
            c: red(),
        };
        let redact = Op::Redact {
            x1: 0.0,
            y1: 0.0,
            x2: 20.0,
            y2: 20.0,
            block: 20.0,
        };
        let out = render(&src, &[rect, redact]);
        assert_eq!(px(&out, 5, 5), px(&out, 15, 15));
        assert_ne!(px(&out, 5, 5), px(&src, 5, 5));
    }

    #[test]
    fn ellipse_strokes_the_ring_and_not_the_middle() {
        let src = blank();
        let op = Op::Ellipse {
            x1: 20.0,
            y1: 20.0,
            x2: 120.0,
            y2: 80.0,
            lw: 4.0,
            c: red(),
        };
        let out = render(&src, &[op]);
        assert_ne!(px(&out, 120, 50), px(&src, 120, 50), "høyre kant er tegnet");
        assert_eq!(px(&out, 70, 50), px(&src, 70, 50), "midten er urørt");
        assert_eq!(
            px(&out, 21, 21),
            px(&src, 21, 21),
            "hjørnet utenfor ringen er urørt"
        );
    }

    #[test]
    fn highlighter_lets_the_picture_through() {
        let src = blank();
        let op = Op::Highlight {
            pts: vec![(10.0, 50.0), (150.0, 50.0)],
            lw: 18.0,
            c: red(),
        };
        let out = render(&src, &[op]);
        let [_, _, r, _] = px(&out, 80, 50);
        assert!(
            r > 60 && r < 220,
            "halvveis mellom bakgrunn og rødt, var {r}"
        );
    }

    #[test]
    fn erased_operations_are_not_drawn() {
        let src = blank();
        let rect = Op::Rect {
            x1: 10.0,
            y1: 10.0,
            x2: 100.0,
            y2: 80.0,
            lw: 5.0,
            c: red(),
        };
        let line = Op::Line {
            x1: 10.0,
            y1: 100.0,
            x2: 190.0,
            y2: 100.0,
            lw: 5.0,
            c: red(),
        };
        let erased = render(
            &src,
            &[rect.clone(), line.clone(), Op::Erase { targets: vec![0] }],
        );
        assert_eq!(erased, render(&src, &[line]));
    }

    #[test]
    fn incremental_burning_matches_a_full_replay() {
        // Ny operasjon brennes inn i bildet så langt, angre spiller alt av på nytt.
        let src = blank();
        let ops = [
            Op::Rect {
                x1: 5.0,
                y1: 5.0,
                x2: 90.0,
                y2: 70.0,
                lw: 5.0,
                c: red(),
            },
            Op::Redact {
                x1: 20.0,
                y1: 20.0,
                x2: 150.0,
                y2: 100.0,
                block: 8.0,
            },
            Op::Marker {
                x: 30.0,
                y: 30.0,
                r: 14.0,
                c: red(),
                fg: WHITE,
                n: 1,
            },
        ];
        let full = render(&src, &ops);
        let mut bytes = src.clone();
        for op in &ops {
            let mut s = surface_from(&bytes, W, H).unwrap();
            burn(&mut s, op, "Sans").unwrap();
            bytes = into_bytes(s).unwrap().to_vec();
        }
        assert_eq!(full, bytes);
    }

    #[test]
    fn transparent_pixels_stay_transparent_beside_redaction() {
        let mut src = vec![0u8; (W * H * 4) as usize];
        // Bare venstre halvdel er dekkende.
        for y in 0..H {
            for x in 0..W / 2 {
                let at = ((y * W + x) * 4) as usize;
                src[at..at + 4].copy_from_slice(&[10, 20, 200, 255]);
            }
        }
        let op = Op::Redact {
            x1: 0.0,
            y1: 0.0,
            x2: W as f64,
            y2: H as f64,
            block: 16.0,
        };
        let out = render(&src, &[op]);
        assert_eq!(px(&out, W - 1, 5), [0, 0, 0, 0]);
        assert_eq!(px(&out, 5, 5)[3], 255);
    }

    #[test]
    fn nothing_escapes_the_image_at_the_edges() {
        let src = blank();
        let op = Op::Rect {
            x1: -50.0,
            y1: -50.0,
            x2: 5000.0,
            y2: 5000.0,
            lw: 5.0,
            c: red(),
        };
        assert_eq!(render(&src, &[op]), src);
    }
}
