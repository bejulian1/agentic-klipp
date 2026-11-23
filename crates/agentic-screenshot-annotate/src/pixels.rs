//! Piksler uten GTK. Bildet ligger som cairos ARGB32, altså BGRA med forhåndsmultiplisert
//! alfa i byterekkefølgen til en little endian-maskin.

/// Antall byte per piksel.
pub const BPP: usize = 4;

fn mul(c: u8, a: u8) -> u8 {
    ((u32::from(c) * u32::from(a) + 127) / 255) as u8
}

fn unmul(c: u8, a: u8) -> u8 {
    match a {
        0 => 0,
        255 => c,
        a => ((u32::from(c) * 255 + u32::from(a) / 2) / u32::from(a)).min(255) as u8,
    }
}

/// RGBA med rett alfa til BGRA med forhåndsmultiplisert alfa, på stedet.
/// Svarer om bildet har noen piksler som ikke er helt dekkende.
pub fn rgba_to_premul_bgra(buf: &mut [u8]) -> bool {
    let mut alpha = false;
    for px in buf.chunks_exact_mut(BPP) {
        let (r, g, b, a) = (px[0], px[1], px[2], px[3]);
        alpha |= a != 255;
        px[0] = mul(b, a);
        px[1] = mul(g, a);
        px[2] = mul(r, a);
    }
    alpha
}

/// BGRA med forhåndsmultiplisert alfa til RGBA med rett alfa. Alfakanalen røres ikke.
pub fn premul_bgra_to_rgba(src: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(src.len());
    for px in src.chunks_exact(BPP) {
        let a = px[3];
        out.extend_from_slice(&[unmul(px[2], a), unmul(px[1], a), unmul(px[0], a), a]);
    }
    out
}

/// Som [`premul_bgra_to_rgba`], men dropper alfa. Bare for helt dekkende bilder.
pub fn opaque_bgra_to_rgb(src: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(src.len() / BPP * 3);
    for px in src.chunks_exact(BPP) {
        out.extend_from_slice(&[px[2], px[1], px[0]]);
    }
    out
}

/// Et ikke-tomt utsnitt av bildet, i hele piksler.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Region {
    pub x0: usize,
    pub y0: usize,
    pub x1: usize,
    pub y1: usize,
}

impl Region {
    /// Pikslene et flyttallsrektangel berører, klippet til bildet.
    pub fn of_rect(x: f64, y: f64, w: f64, h: f64, img_w: usize, img_h: usize) -> Option<Region> {
        let lo = |v: f64, max: usize| (v.floor().max(0.0) as usize).min(max);
        let hi = |v: f64, max: usize| (v.ceil().max(0.0) as usize).min(max);
        let r = Region {
            x0: lo(x, img_w),
            y0: lo(y, img_h),
            x1: hi(x + w, img_w),
            y1: hi(y + h, img_h),
        };
        (r.x0 < r.x1 && r.y0 < r.y1).then_some(r)
    }
}

/// Blokkstørrelsen sladden faktisk bruker.
pub fn block_size(block: f64) -> usize {
    (block.round().max(2.0)) as usize
}

/// Summene for én blokk: forhåndsmultipliserte farger og alfa.
fn block_sum(src: &[u8], img_w: usize, x0: usize, y0: usize, x1: usize, y1: usize) -> [u64; 4] {
    let mut sum = [0u64; 4];
    for y in y0..y1 {
        let row = &src[(y * img_w + x0) * BPP..(y * img_w + x1) * BPP];
        for px in row.chunks_exact(BPP) {
            sum[0] += u64::from(px[0]);
            sum[1] += u64::from(px[1]);
            sum[2] += u64::from(px[2]);
            sum[3] += u64::from(px[3]);
        }
    }
    sum
}

/// Blokkene i `r`. Rutenettet er forankret i bildet, så blokkene står stille mens området
/// dras. Kallet får blokkens klippede grenser og rad og kolonne i rutenettet.
fn each_block(r: Region, bs: usize, mut f: impl FnMut(usize, usize, Region)) {
    let (gx, gy) = (r.x0 - r.x0 % bs, r.y0 - r.y0 % bs);
    for (row, by) in (gy..r.y1).step_by(bs).enumerate() {
        for (col, bx) in (gx..r.x1).step_by(bs).enumerate() {
            let cell = Region {
                x0: bx.max(r.x0),
                y0: by.max(r.y0),
                x1: (bx + bs).min(r.x1),
                y1: (by + bs).min(r.y1),
            };
            f(col, row, cell);
        }
    }
}

/// Ekte pixelering: hver blokk får alfavektet snittfarge av pikslene under, og hver piksel
/// beholder sin egen alfa, så transparens utenfor en frihåndsform består.
pub fn pixelate(buf: &mut [u8], img_w: usize, r: Region, block: f64) {
    each_block(r, block_size(block), |_, _, c| {
        let [b, g, red, n] = block_sum(buf, img_w, c.x0, c.y0, c.x1, c.y1);
        if n == 0 {
            return;
        }
        // Summen av forhåndsmultipliserte farger delt på summen av alfa er snittet vektet med alfa.
        let straight = |v: u64| (v * 255 + n / 2) / n;
        let (sb, sg, sr) = (straight(b), straight(g), straight(red));
        for y in c.y0..c.y1 {
            let row = &mut buf[(y * img_w + c.x0) * BPP..(y * img_w + c.x1) * BPP];
            for px in row.chunks_exact_mut(BPP) {
                let a = u64::from(px[3]);
                px[0] = ((sb * a + 127) / 255).min(a) as u8;
                px[1] = ((sg * a + 127) / 255).min(a) as u8;
                px[2] = ((sr * a + 127) / 255).min(a) as u8;
            }
        }
    });
}

/// Rutenettet som et lite bilde med én piksel per blokk, til forhåndsvisning mens man drar.
/// Det koster like mye som den ekte pixeleringen å regne ut, men er langt billigere å laste
/// opp hver ramme. Delvis dekkede piksler vises med blokkens snittalfa til operasjonen er
/// brent inn.
#[derive(Debug, PartialEq, Eq)]
pub struct BlockPreview {
    pub cols: usize,
    pub rows: usize,
    /// Rutenettets øvre venstre hjørne i bildet.
    pub origin: (usize, usize),
    pub block: usize,
    pub data: Vec<u8>,
}

pub fn block_preview(src: &[u8], img_w: usize, r: Region, block: f64) -> BlockPreview {
    let bs = block_size(block);
    let origin = (r.x0 - r.x0 % bs, r.y0 - r.y0 % bs);
    let cols = (r.x1 - origin.0).div_ceil(bs);
    let rows = (r.y1 - origin.1).div_ceil(bs);
    let mut data = vec![0; cols * rows * BPP];
    each_block(r, bs, |col, row, c| {
        let count = ((c.x1 - c.x0) * (c.y1 - c.y0)) as u64;
        let sum = block_sum(src, img_w, c.x0, c.y0, c.x1, c.y1);
        let at = (row * cols + col) * BPP;
        for (out, v) in data[at..at + BPP].iter_mut().zip(sum) {
            *out = ((v + count / 2) / count) as u8;
        }
    });
    BlockPreview {
        cols,
        rows,
        origin,
        block: bs,
        data,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opaque(w: usize, h: usize, f: impl Fn(usize, usize) -> u8) -> Vec<u8> {
        let mut out = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let v = f(x, y);
                out.extend_from_slice(&[v, v, v, 255]);
            }
        }
        out
    }

    #[test]
    fn premultiply_round_trip_keeps_alpha_exactly() {
        let mut buf = vec![200, 100, 50, 128, 1, 2, 3, 0, 9, 8, 7, 255];
        let original = buf.clone();
        assert!(rgba_to_premul_bgra(&mut buf));
        assert_eq!(&buf[0..4], &[25, 50, 100, 128]);
        assert_eq!(&buf[4..8], &[0, 0, 0, 0]);
        assert_eq!(&buf[8..12], &[7, 8, 9, 255]);
        let back = premul_bgra_to_rgba(&buf);
        assert_eq!(back[3], 128);
        assert_eq!(back[7], 0);
        assert_eq!(&back[8..12], &original[8..12]);
        assert!((i32::from(back[0]) - 200).abs() <= 1);
    }

    #[test]
    fn opaque_image_reports_no_alpha() {
        let mut buf = vec![1, 2, 3, 255];
        assert!(!rgba_to_premul_bgra(&mut buf));
        assert_eq!(opaque_bgra_to_rgb(&buf), vec![1, 2, 3]);
    }

    #[test]
    fn region_clips_and_rounds_outwards() {
        let r = Region::of_rect(-3.0, 1.5, 6.2, 2.0, 10, 10).unwrap();
        assert_eq!((r.x0, r.y0, r.x1, r.y1), (0, 1, 4, 4));
        assert_eq!(Region::of_rect(11.0, 0.0, 5.0, 5.0, 10, 10), None);
        assert_eq!(Region::of_rect(2.0, 2.0, 0.0, 5.0, 10, 10), None);
    }

    #[test]
    fn pixelate_replaces_detail_with_block_mean() {
        let mut buf = opaque(4, 4, |x, _| if x % 2 == 0 { 0 } else { 200 });
        let r = Region::of_rect(0.0, 0.0, 4.0, 4.0, 4, 4).unwrap();
        pixelate(&mut buf, 4, r, 4.0);
        assert!(buf.chunks_exact(4).all(|p| p == [100, 100, 100, 255]));
    }

    #[test]
    fn pixelate_leaves_pixels_outside_the_region() {
        let mut buf = opaque(6, 2, |x, _| (x * 40) as u8);
        let before = buf.clone();
        let r = Region::of_rect(2.0, 0.0, 2.0, 2.0, 6, 2).unwrap();
        pixelate(&mut buf, 6, r, 2.0);
        assert_eq!(&buf[..8], &before[..8]);
        assert_eq!(&buf[16..24], &before[16..24]);
        assert_ne!(&buf[8..16], &before[8..16]);
    }

    #[test]
    fn grid_is_anchored_to_the_image() {
        // Samme piksel skal få samme blokk uansett hvor området begynner.
        let src = opaque(16, 1, |x, _| (x * 10) as u8);
        let mut a = src.clone();
        let mut b = src.clone();
        pixelate(
            &mut a,
            16,
            Region::of_rect(8.0, 0.0, 8.0, 1.0, 16, 1).unwrap(),
            4.0,
        );
        pixelate(
            &mut b,
            16,
            Region::of_rect(0.0, 0.0, 16.0, 1.0, 16, 1).unwrap(),
            4.0,
        );
        assert_eq!(&a[8 * 4..], &b[8 * 4..]);
    }

    #[test]
    fn pixelate_weights_by_alpha_and_keeps_transparency() {
        // Rød og dekkende ved siden av hel transparens: fargen skal ikke blandes med svart.
        let mut buf = vec![0, 0, 200, 255, 0, 0, 0, 0];
        pixelate(
            &mut buf,
            2,
            Region::of_rect(0.0, 0.0, 2.0, 1.0, 2, 1).unwrap(),
            2.0,
        );
        assert_eq!(buf, vec![0, 0, 200, 255, 0, 0, 0, 0]);
    }

    #[test]
    fn pixelate_skips_fully_transparent_blocks() {
        let mut buf = vec![0; 16];
        pixelate(
            &mut buf,
            2,
            Region::of_rect(0.0, 0.0, 2.0, 2.0, 2, 2).unwrap(),
            2.0,
        );
        assert_eq!(buf, vec![0; 16]);
    }

    #[test]
    fn small_blocks_are_clamped() {
        assert_eq!(block_size(0.2), 2);
        assert_eq!(block_size(13.6), 14);
    }

    #[test]
    fn preview_matches_the_burned_result_on_opaque_images() {
        let src = opaque(23, 17, |x, y| ((x * 31 + y * 17) % 251) as u8);
        let r = Region::of_rect(3.0, 2.0, 15.0, 11.0, 23, 17).unwrap();
        let p = block_preview(&src, 23, r, 7.0);
        assert_eq!(p.origin, (0, 0));
        assert_eq!((p.cols, p.rows), (3, 2));
        let mut burned = src.clone();
        pixelate(&mut burned, 23, r, 7.0);
        for y in r.y0..r.y1 {
            for x in r.x0..r.x1 {
                let cell = ((y - p.origin.1) / 7 * p.cols + (x - p.origin.0) / 7) * 4;
                let px = &burned[(y * 23 + x) * 4..(y * 23 + x) * 4 + 4];
                for (shown, burned) in p.data[cell..cell + 4].iter().zip(px) {
                    assert!((i32::from(*shown) - i32::from(*burned)).abs() <= 1);
                }
            }
        }
    }
}
