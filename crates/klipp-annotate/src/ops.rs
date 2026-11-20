//! Operasjonslista for annotering. Ingen piksler redigeres direkte, alt spilles av
//! på originalen, så angre og gjør om er trivielt.
use klipp_system::i18n::trc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Arrow,
    Line,
    Rect,
    Ellipse,
    Free,
    Highlight,
    Text,
    Marker,
    Redact,
    Eraser,
}

impl Tool {
    pub const ALL: [Tool; 10] = [
        Tool::Arrow,
        Tool::Line,
        Tool::Rect,
        Tool::Ellipse,
        Tool::Free,
        Tool::Highlight,
        Tool::Text,
        Tool::Marker,
        Tool::Redact,
        Tool::Eraser,
    ];

    pub fn key(self) -> char {
        match self {
            Tool::Arrow => 'p',
            Tool::Line => 'l',
            Tool::Rect => 'r',
            Tool::Ellipse => 'o',
            Tool::Free => 'f',
            Tool::Highlight => 'h',
            Tool::Text => 't',
            Tool::Marker => 'm',
            Tool::Redact => 's',
            Tool::Eraser => 'e',
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Tool::Arrow => trc("annotation-tool", "Pil"),
            Tool::Line => trc("annotation-tool", "Linje"),
            Tool::Rect => trc("annotation-tool", "Rektangel"),
            Tool::Ellipse => trc("annotation-tool", "Ellipse"),
            Tool::Free => trc("annotation-tool", "Frihånd"),
            Tool::Highlight => trc("annotation-tool", "Merkepenn"),
            Tool::Text => trc("annotation-tool", "Tekst"),
            Tool::Marker => trc("annotation-tool", "Nummerert markør"),
            Tool::Redact => trc("annotation-tool", "Sladd"),
            Tool::Eraser => trc("annotation-tool", "Viskelær"),
        }
    }
    pub fn icon(self) -> &'static str {
        match self {
            Tool::Arrow => "klipp-arrow-symbolic",
            Tool::Line => "klipp-line-symbolic",
            Tool::Rect => "klipp-rect-symbolic",
            Tool::Ellipse => "klipp-ellipse-symbolic",
            Tool::Free => "klipp-free-symbolic",
            Tool::Highlight => "klipp-highlight-symbolic",
            Tool::Text => "klipp-text-symbolic",
            Tool::Marker => "klipp-marker-symbolic",
            Tool::Redact => "klipp-redact-symbolic",
            Tool::Eraser => "klipp-eraser-symbolic",
        }
    }
    pub fn from_key(c: char) -> Option<Tool> {
        Tool::ALL.into_iter().find(|t| t.key() == c)
    }
    /// Verktøy uten farge eller strek. Fargevalget gråes ut for dem.
    pub fn uses_color(self) -> bool {
        !matches!(self, Tool::Redact | Tool::Eraser)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub r: f64,
    pub g: f64,
    pub b: f64,
}

impl Color {
    /// Som i CSS, til tekstfeltet.
    pub fn hex(self) -> String {
        let byte = |v: f64| (v * 255.0).round() as u8;
        format!(
            "#{:02x}{:02x}{:02x}",
            byte(self.r),
            byte(self.g),
            byte(self.b)
        )
    }

    /// Hvit eller svart, det som synes best oppå fargen. Til tallet i markøren.
    pub fn contrast(self) -> Color {
        let luminance = 0.2126 * self.r + 0.7152 * self.g + 0.0722 * self.b;
        if luminance > 0.55 { BLACK } else { WHITE }
    }
}

pub const fn rgb(v: u32) -> Color {
    Color {
        r: ((v >> 16) & 0xff) as f64 / 255.0,
        g: ((v >> 8) & 0xff) as f64 / 255.0,
        b: (v & 0xff) as f64 / 255.0,
    }
}

pub const WHITE: Color = rgb(0xffffff);
pub const BLACK: Color = rgb(0x000000);

pub struct Swatch {
    pub label: fn() -> &'static str,
    pub color: Color,
}

pub const COLORS: [Swatch; 8] = [
    Swatch {
        label: || trc("annotation-color", "Rød"),
        color: rgb(0xed333b),
    },
    Swatch {
        label: || trc("annotation-color", "Oransje"),
        color: rgb(0xff7800),
    },
    Swatch {
        label: || trc("annotation-color", "Gul"),
        color: rgb(0xf6d32d),
    },
    Swatch {
        label: || trc("annotation-color", "Grønn"),
        color: rgb(0x33d17a),
    },
    Swatch {
        label: || trc("annotation-color", "Blå"),
        color: rgb(0x62a0ea),
    },
    Swatch {
        label: || trc("annotation-color", "Lilla"),
        color: rgb(0xc061cb),
    },
    Swatch {
        label: || trc("annotation-color", "Hvit"),
        color: WHITE,
    },
    Swatch {
        label: || trc("annotation-color", "Svart"),
        color: rgb(0x1e1e1e),
    },
];

pub struct Thickness {
    pub label: fn() -> &'static str,
    pub line: f64,
    pub text: f64,
    pub r: f64,
    pub block: f64,
    pub dot: f64,
}

pub const THICK: [Thickness; 5] = [
    Thickness {
        label: || trc("annotation-thickness", "Ekstra tynn"),
        line: 2.0,
        text: 13.0,
        r: 9.0,
        block: 6.0,
        dot: 3.0,
    },
    Thickness {
        label: || trc("annotation-thickness", "Tynn"),
        line: 3.0,
        text: 16.0,
        r: 11.0,
        block: 10.0,
        dot: 5.0,
    },
    Thickness {
        label: || trc("annotation-thickness", "Middels"),
        line: 5.0,
        text: 22.0,
        r: 14.0,
        block: 14.0,
        dot: 7.0,
    },
    Thickness {
        label: || trc("annotation-thickness", "Tykk"),
        line: 8.0,
        text: 30.0,
        r: 18.0,
        block: 20.0,
        dot: 10.0,
    },
    Thickness {
        label: || trc("annotation-thickness", "Ekstra tykk"),
        line: 12.0,
        text: 40.0,
        r: 24.0,
        block: 28.0,
        dot: 13.0,
    },
];

/// Størrelsen et nytt vindu starter med.
pub const DEFAULT_THICK: usize = 2;

/// Merkepennen er bred og gjennomsiktig, så teksten under fortsatt kan leses.
pub const HIGHLIGHT_WIDTH: f64 = 3.5;
pub const HIGHLIGHT_ALPHA: f64 = 0.4;

/// Linjehøyden for tekst, i forhold til skriftstørrelsen.
pub const LINE_HEIGHT: f64 = 1.25;

#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    Arrow {
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
        lw: f64,
        c: Color,
    },
    Line {
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
        lw: f64,
        c: Color,
    },
    Rect {
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
        lw: f64,
        c: Color,
    },
    Ellipse {
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
        lw: f64,
        c: Color,
    },
    Free {
        pts: Vec<(f64, f64)>,
        lw: f64,
        c: Color,
    },
    Highlight {
        pts: Vec<(f64, f64)>,
        lw: f64,
        c: Color,
    },
    Text {
        x: f64,
        y: f64,
        size: f64,
        c: Color,
        text: String,
    },
    Marker {
        x: f64,
        y: f64,
        r: f64,
        c: Color,
        fg: Color,
        n: u32,
    },
    Redact {
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
        block: f64,
    },
    /// Viskelæret: skjuler operasjonene på disse plassene i lista. Står selv i lista, så
    /// angre henter dem tilbake.
    Erase { targets: Vec<usize> },
}

impl Op {
    /// Et lite klikk med et dra-verktøy skal ikke gi en operasjon.
    pub fn is_big_enough(&self, min: f64) -> bool {
        match self {
            Op::Arrow { x1, y1, x2, y2, .. }
            | Op::Line { x1, y1, x2, y2, .. }
            | Op::Rect { x1, y1, x2, y2, .. }
            | Op::Ellipse { x1, y1, x2, y2, .. }
            | Op::Redact { x1, y1, x2, y2, .. } => (x2 - x1).abs().max((y2 - y1).abs()) >= min,
            Op::Erase { targets } => !targets.is_empty(),
            _ => true,
        }
    }

    /// Kan ikke brennes oppå det som er tegnet, men krever at alt spilles av på nytt.
    pub fn needs_replay(&self) -> bool {
        matches!(self, Op::Erase { .. })
    }

    /// Området operasjonen kan farge, i bildekoordinater, med plass til streker og pilhode.
    pub fn bounds(&self) -> (f64, f64, f64, f64) {
        let around = |x1: f64, y1: f64, x2: f64, y2: f64, m: f64| {
            (
                x1.min(x2) - m,
                y1.min(y2) - m,
                x1.max(x2) + m,
                y1.max(y2) + m,
            )
        };
        match self {
            Op::Arrow {
                x1, y1, x2, y2, lw, ..
            } => around(*x1, *y1, *x2, *y2, arrow_head(*lw, f64::MAX) + lw),
            Op::Line {
                x1, y1, x2, y2, lw, ..
            }
            | Op::Rect {
                x1, y1, x2, y2, lw, ..
            }
            | Op::Ellipse {
                x1, y1, x2, y2, lw, ..
            } => around(*x1, *y1, *x2, *y2, *lw),
            Op::Redact { x1, y1, x2, y2, .. } => around(*x1, *y1, *x2, *y2, 0.0),
            Op::Marker { x, y, r, .. } => around(*x, *y, *x, *y, r * 1.2),
            Op::Text {
                x, y, size, text, ..
            } => {
                let longest = text.lines().map(|l| l.chars().count()).max().unwrap_or(0) as f64;
                let lines = text.lines().count() as f64;
                around(
                    *x,
                    *y,
                    x + longest * size,
                    y + lines * size * LINE_HEIGHT,
                    *size,
                )
            }
            Op::Erase { .. } => (0.0, 0.0, 0.0, 0.0),
            Op::Free { pts, lw, .. } | Op::Highlight { pts, lw, .. } => {
                let mut b = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
                for &(x, y) in pts {
                    b = (b.0.min(x), b.1.min(y), b.2.max(x), b.3.max(y));
                }
                around(b.0, b.1, b.2, b.3, *lw)
            }
        }
    }

    pub fn drag_to(&mut self, x: f64, y: f64) {
        match self {
            Op::Arrow { x2, y2, .. }
            | Op::Line { x2, y2, .. }
            | Op::Rect { x2, y2, .. }
            | Op::Ellipse { x2, y2, .. }
            | Op::Redact { x2, y2, .. } => {
                *x2 = x;
                *y2 = y;
            }
            Op::Free { pts, .. } | Op::Highlight { pts, .. } => {
                let far = pts.last().is_none_or(|l| (x - l.0).hypot(y - l.1) >= 1.0);
                if far {
                    pts.push((x, y));
                }
            }
            _ => {}
        }
    }
}

impl Op {
    /// Treffer punktet streken eller flaten? `tol` er slingringsmonn i bildepiksler.
    pub fn hits(&self, x: f64, y: f64, tol: f64) -> bool {
        match self {
            Op::Arrow {
                x1, y1, x2, y2, lw, ..
            }
            | Op::Line {
                x1, y1, x2, y2, lw, ..
            } => segment_distance((x, y), (*x1, *y1), (*x2, *y2)) <= lw / 2.0 + tol,
            Op::Rect {
                x1, y1, x2, y2, lw, ..
            } => {
                let (l, t, r, b) = (x1.min(*x2), y1.min(*y2), x1.max(*x2), y1.max(*y2));
                let corners = [(l, t), (r, t), (r, b), (l, b), (l, t)];
                let edge = corners
                    .windows(2)
                    .map(|w| segment_distance((x, y), w[0], w[1]))
                    .fold(f64::MAX, f64::min);
                edge <= lw / 2.0 + tol
            }
            Op::Ellipse {
                x1, y1, x2, y2, lw, ..
            } => {
                let (cx, cy) = ((x1 + x2) / 2.0, (y1 + y2) / 2.0);
                let (rx, ry) = (((x2 - x1) / 2.0).abs(), ((y2 - y1) / 2.0).abs());
                if rx < 1.0 || ry < 1.0 {
                    return segment_distance((x, y), (*x1, *y1), (*x2, *y2)) <= lw / 2.0 + tol;
                }
                let k = ((x - cx) / rx).hypot((y - cy) / ry);
                (k - 1.0).abs() * rx.min(ry) <= lw / 2.0 + tol
            }
            Op::Free { pts, lw, .. } | Op::Highlight { pts, lw, .. } => {
                let near = match pts.as_slice() {
                    [] => f64::MAX,
                    [only] => (x - only.0).hypot(y - only.1),
                    _ => pts
                        .windows(2)
                        .map(|w| segment_distance((x, y), w[0], w[1]))
                        .fold(f64::MAX, f64::min),
                };
                near <= lw / 2.0 + tol
            }
            Op::Marker {
                x: mx, y: my, r, ..
            } => (x - mx).hypot(y - my) <= r + tol,
            Op::Text { .. } | Op::Redact { .. } => {
                let (l, t, r, b) = self.bounds();
                x >= l - tol && x <= r + tol && y >= t - tol && y <= b + tol
            }
            Op::Erase { .. } => false,
        }
    }
}

fn segment_distance(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    let t = if len2 == 0.0 {
        0.0
    } else {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0)
    };
    (p.0 - (a.0 + t * dx)).hypot(p.1 - (a.1 + t * dy))
}

/// Plassene i lista som viskelæret har skjult.
pub fn erased(ops: &[Op]) -> std::collections::HashSet<usize> {
    ops.iter()
        .filter_map(|op| match op {
            Op::Erase { targets } => Some(targets.iter().copied()),
            _ => None,
        })
        .flatten()
        .collect()
}

/// Det som faktisk tegnes, i rekkefølge, med plassen i lista.
pub fn visible(ops: &[Op]) -> impl Iterator<Item = (usize, &Op)> {
    let gone = erased(ops);
    ops.iter()
        .enumerate()
        .filter(move |(i, op)| !op.needs_replay() && !gone.contains(i))
}

/// Øverste synlige operasjon under punktet.
pub fn top_hit(ops: &[Op], x: f64, y: f64, tol: f64) -> Option<usize> {
    let shown: Vec<_> = visible(ops).collect();
    shown
        .into_iter()
        .rev()
        .find(|(_, op)| op.hits(x, y, tol))
        .map(|(i, _)| i)
}

/// Angre og gjør om over en operasjonsliste.
#[derive(Clone, Debug)]
pub struct History<T> {
    ops: Vec<T>,
    redo: Vec<T>,
}

impl<T> Default for History<T> {
    fn default() -> Self {
        Self {
            ops: Vec::new(),
            redo: Vec::new(),
        }
    }
}

impl<T> History<T> {
    pub fn push(&mut self, op: T) {
        self.ops.push(op);
        self.redo.clear();
    }
    pub fn undo(&mut self) -> bool {
        match self.ops.pop() {
            Some(op) => {
                self.redo.push(op);
                true
            }
            None => false,
        }
    }
    pub fn redo(&mut self) -> Option<&T> {
        let op = self.redo.pop()?;
        self.ops.push(op);
        self.ops.last()
    }
    pub fn ops(&self) -> &[T] {
        &self.ops
    }
    /// Det `redo` vil hente tilbake, uten å flytte det.
    pub fn next_redo(&self) -> Option<&T> {
        self.redo.last()
    }
    pub fn can_undo(&self) -> bool {
        !self.ops.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }
}

impl History<Op> {
    /// Neste nummer: antall markører som står i lista, pluss én.
    pub fn next_marker(&self) -> u32 {
        self.ops
            .iter()
            .filter(|o| matches!(o, Op::Marker { .. }))
            .count() as u32
            + 1
    }
}

/// Lengde, retning og hode for en pil, som i prototypen.
pub fn arrow_head(lw: f64, len: f64) -> f64 {
    (lw * 3.4).max(lw + 8.0).min(len * 0.7)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_redo_walks_the_list() {
        let mut h = History::default();
        h.push(1);
        h.push(2);
        assert!(h.undo());
        assert_eq!(h.ops(), &[1]);
        assert_eq!(h.redo(), Some(&2));
        assert_eq!(h.ops(), &[1, 2]);
        assert_eq!(h.redo(), None);
    }

    #[test]
    fn new_op_clears_redo() {
        let mut h = History::default();
        h.push(1);
        h.undo();
        assert!(h.can_redo());
        h.push(3);
        assert!(!h.can_redo());
        assert_eq!(h.ops(), &[3]);
    }

    #[test]
    fn undo_on_empty_is_a_no_op() {
        let mut h: History<u8> = History::default();
        assert!(!h.undo());
        assert!(!h.can_undo());
    }

    #[test]
    fn markers_count_from_the_list() {
        let mut h = History::default();
        let m = |n| Op::Marker {
            x: 0.0,
            y: 0.0,
            r: 1.0,
            c: WHITE,
            fg: BLACK,
            n,
        };
        assert_eq!(h.next_marker(), 1);
        h.push(m(1));
        h.push(Op::Rect {
            x1: 0.0,
            y1: 0.0,
            x2: 9.0,
            y2: 9.0,
            lw: 1.0,
            c: WHITE,
        });
        h.push(m(2));
        assert_eq!(h.next_marker(), 3);
        h.undo();
        assert_eq!(h.next_marker(), 2);
    }

    #[test]
    fn tiny_drags_are_dropped() {
        let op = Op::Arrow {
            x1: 0.0,
            y1: 0.0,
            x2: 1.0,
            y2: 2.0,
            lw: 1.0,
            c: WHITE,
        };
        assert!(!op.is_big_enough(3.0));
        assert!(op.is_big_enough(2.0));
    }

    #[test]
    fn freehand_skips_tiny_moves() {
        let mut op = Op::Free {
            pts: vec![(0.0, 0.0)],
            lw: 1.0,
            c: WHITE,
        };
        op.drag_to(0.5, 0.0);
        op.drag_to(2.0, 0.0);
        let Op::Free { pts, .. } = op else { panic!() };
        assert_eq!(pts.len(), 2);
    }

    #[test]
    fn tool_keys_match_prototype() {
        assert_eq!(Tool::from_key('s'), Some(Tool::Redact));
        assert_eq!(Tool::from_key('p'), Some(Tool::Arrow));
        assert_eq!(Tool::from_key('x'), None);
    }

    #[test]
    fn bounds_cover_the_stroke_in_any_drag_direction() {
        let op = Op::Rect {
            x1: 10.0,
            y1: 20.0,
            x2: 0.0,
            y2: 5.0,
            lw: 2.0,
            c: WHITE,
        };
        assert_eq!(op.bounds(), (-2.0, 3.0, 12.0, 22.0));
        let free = Op::Free {
            pts: vec![(1.0, 9.0), (5.0, 2.0)],
            lw: 1.0,
            c: WHITE,
        };
        assert_eq!(free.bounds(), (0.0, 1.0, 6.0, 10.0));
    }

    fn line(x2: f64) -> Op {
        Op::Line {
            x1: 0.0,
            y1: 0.0,
            x2,
            y2: 0.0,
            lw: 4.0,
            c: WHITE,
        }
    }

    #[test]
    fn hit_testing_follows_the_stroke() {
        let l = line(100.0);
        assert!(l.hits(50.0, 3.0, 2.0));
        assert!(!l.hits(50.0, 6.0, 2.0));
        assert!(!l.hits(110.0, 0.0, 2.0));
        let rect = Op::Rect {
            x1: 0.0,
            y1: 0.0,
            x2: 100.0,
            y2: 50.0,
            lw: 4.0,
            c: WHITE,
        };
        assert!(rect.hits(100.0, 25.0, 1.0));
        assert!(!rect.hits(50.0, 25.0, 1.0), "midt i rammen er tomt");
        let ellipse = Op::Ellipse {
            x1: 0.0,
            y1: 0.0,
            x2: 100.0,
            y2: 50.0,
            lw: 4.0,
            c: WHITE,
        };
        assert!(ellipse.hits(100.0, 25.0, 1.0));
        assert!(ellipse.hits(50.0, 0.0, 1.0));
        assert!(!ellipse.hits(50.0, 25.0, 1.0));
        let redact = Op::Redact {
            x1: 0.0,
            y1: 0.0,
            x2: 10.0,
            y2: 10.0,
            block: 4.0,
        };
        assert!(redact.hits(5.0, 5.0, 0.0), "sladd er en flate");
    }

    #[test]
    fn eraser_hides_and_undo_brings_back() {
        let mut h = History::default();
        h.push(line(100.0));
        h.push(line(50.0));
        assert_eq!(top_hit(h.ops(), 20.0, 0.0, 1.0), Some(1), "øverste først");
        h.push(Op::Erase { targets: vec![1] });
        assert_eq!(top_hit(h.ops(), 20.0, 0.0, 1.0), Some(0));
        assert_eq!(visible(h.ops()).map(|(i, _)| i).collect::<Vec<_>>(), [0]);
        h.undo();
        assert_eq!(visible(h.ops()).count(), 2);
        assert!(!Op::Erase { targets: vec![] }.is_big_enough(0.0));
    }

    #[test]
    fn marker_number_contrasts_with_its_colour() {
        assert_eq!(COLORS[0].color.contrast(), WHITE);
        assert_eq!(COLORS[2].color.contrast(), BLACK);
        assert_eq!(WHITE.contrast(), BLACK);
        assert_eq!(COLORS[7].color.contrast(), WHITE);
    }

    #[test]
    fn colours_print_as_css() {
        assert_eq!(COLORS[0].color.hex(), "#ed333b");
    }

    #[test]
    fn arrow_head_shrinks_on_short_arrows() {
        assert_eq!(arrow_head(5.0, 100.0), 17.0);
        assert_eq!(arrow_head(5.0, 10.0), 7.0);
    }
}
